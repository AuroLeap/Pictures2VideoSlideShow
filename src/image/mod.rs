//! Frame generation: turn one source image into a sequence of RGB frames
//! implementing the Ken Burns pan/zoom + fade effect.
//!
//! The source image is pre-scaled once (shared via [`Arc`]) and each frame is
//! produced by cropping a window and scaling it to the output size. Frames are
//! emitted as raw `rgb24` byte buffers ready to pipe to FFmpeg.

pub mod backend;
pub mod gpu;
pub mod yuv;

use crate::config::OutputDef;
use crate::error::{Result, SlideshowError};
use crate::transform::{seed_from_str, ClipPlan};
use ::image::{GenericImageView, Rgb, RgbImage};
use fast_image_resize as fir;
use imageproc::geometric_transformations::{warp_into, Interpolation};
use rayon::prelude::*;
use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

thread_local! {
    /// One SIMD resizer per rayon worker: `fir::Resizer` reuses internal
    /// buffers across resizes but is not `Sync`, so sharing per-thread keeps
    /// the reuse without locking.
    static RESIZER: RefCell<fir::Resizer> = RefCell::new(fir::Resizer::new());
}

/// SIMD bilinear resize of an rgb24 buffer, optionally from a float crop
/// window `(left, top, width, height)` of the source (sub-pixel cropping is
/// how the fast path pans/zooms without a warp). SSE4.1/AVX2 are runtime
/// detected by `fast_image_resize`.
// Implements: LLR-061, SR-036
fn simd_resize(
    src: &RgbImage,
    crop: Option<(f32, f32, f32, f32)>,
    dst_w: u32,
    dst_h: u32,
) -> RgbImage {
    let src_view = fir::images::ImageRef::new(
        src.width(),
        src.height(),
        src.as_raw(),
        fir::PixelType::U8x3,
    )
    .expect("rgb24 buffer matches dimensions");
    let mut dst = fir::images::Image::new(dst_w, dst_h, fir::PixelType::U8x3);
    let mut opts = fir::ResizeOptions::new()
        .resize_alg(fir::ResizeAlg::Convolution(fir::FilterType::Bilinear));
    if let Some((l, t, w, h)) = crop {
        opts = opts.crop(l as f64, t as f64, w as f64, h as f64);
    }
    RESIZER.with(|r| {
        r.borrow_mut()
            .resize(&src_view, &mut dst, &opts)
            .expect("resize rgb24")
    });
    RgbImage::from_raw(dst_w, dst_h, dst.into_vec()).expect("resized buffer matches dimensions")
}

/// Elapsed decode (`image::open`) and prescale (resize) time for one clip
/// load — raw work time, accrued into the SR-036 stage totals. Since the
/// background prefetch (LLR-060) this overlaps the build; the PB-002 boundary
/// stall is the clip loop's blocking wait, timed separately.
// Implements: LLR-050, SR-036
#[derive(Debug, Clone, Copy, Default)]
pub struct LoadTimings {
    pub decode: std::time::Duration,
    pub prescale: std::time::Duration,
}

/// One decoded + prescaled image clip, backend-blind: exactly what either
/// renderer needs (the shared prescaled pixels, the motion plan, and the load
/// timings). Produced by the prefetch worker; the clip loop wraps it in the
/// selected backend's [`ClipRenderer`], so GPU objects never live on the
/// prefetch thread. Prescale deliberately stays CPU-side (LLR-066 recorded
/// call): both backends then sample identical source pixels.
// Implements: LLR-066, SR-039
#[derive(Clone)]
pub struct LoadedClip {
    /// Pre-scaled source, sized so the max-zoom crop is full detail.
    pub prescaled: Arc<RgbImage>,
    pub plan: ClipPlan,
    /// How long this clip's decode/prescale took, for the stage timers.
    pub timings: LoadTimings,
}

impl LoadedClip {
    /// Decode `path`, plan its Ken Burns motion (seeded from the path, with an
    /// optional fixed focus point in normalized `[0,1]` image coordinates),
    /// and pre-scale once. `None` focus keeps the default pan.
    // Implements: LLR-037, LLR-066, SR-031, SR-039
    pub fn load(path: &Path, out: &OutputDef, focus: Option<(f32, f32)>) -> Result<Self> {
        // Time decode and prescale separately (LLR-050, SR-036): raw stage
        // totals; the PB-002 boundary stall is the caller's blocking wait.
        let t = std::time::Instant::now();
        let img = ::image::open(path).map_err(SlideshowError::Image)?;
        let decode = t.elapsed();
        let (w, h) = img.dimensions();
        let seed = seed_from_str(&path.to_string_lossy());
        let plan = ClipPlan::with_focus(w, h, out, seed, focus);

        // Pre-scale once, via the SIMD resizer (bilinear ≈ the former Triangle
        // filter as a speed/quality trade-off for the per-frame scale that
        // follows). Implements: LLR-061, SR-036
        let t = std::time::Instant::now();
        let prescaled = simd_resize(&img.to_rgb8(), None, plan.pre_w, plan.pre_h);
        let prescale = t.elapsed();

        Ok(Self {
            prescaled: Arc::new(prescaled),
            plan,
            timings: LoadTimings { decode, prescale },
        })
    }
}

/// A per-clip frame renderer behind which the CPU and GPU backends are
/// interchangeable: [`FrameRenderer`] is the CPU implementation (and the
/// permanent correctness reference); `GpuRenderer` the wgpu one. Frames are
/// raw `rgb24` buffers, shape-identical across backends, so everything
/// downstream of the [`crate::pipeline`] frame source stays backend-blind.
// Implements: LLR-066, SR-039
pub trait ClipRenderer: Send {
    /// Total frames this clip will produce (`ClipPlan::total_frames`).
    fn total_frames(&self) -> u32;
    /// Render the contiguous frame range `[start, end)`, in frame order.
    fn render_range(&mut self, start: u32, end: u32) -> Vec<Vec<u8>>;
    /// Elapsed decode/prescale time recorded at load, for the stage timers.
    fn load_timings(&self) -> LoadTimings;
}

/// Mean absolute per-channel difference between two equal-length `rgb24`
/// buffers — the single cross-backend comparison hook (unit tests,
/// integration tests, and the bench comparison leg all share it). The
/// tolerance epsilon lives in the verifying TC row, not here (one home).
/// Panics on unequal lengths (caller bug); empty buffers compare as `0.0`.
// Implements: LLR-073, SR-039
pub fn frame_mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len(), "frame buffers must be equal-length");
    if a.is_empty() {
        return 0.0;
    }
    let sum: u64 = a
        .iter()
        .zip(b.iter())
        .map(|(&x, &y)| (x as i16 - y as i16).unsigned_abs() as u64)
        .sum();
    sum as f64 / a.len() as f64
}

/// Renders all frames for a single image clip on the CPU (the SR-039
/// reference backend and permanent fallback).
pub struct FrameRenderer {
    clip: LoadedClip,
}

impl FrameRenderer {
    /// Wrap an already-loaded clip (the prefetcher's output).
    // Implements: LLR-066, SR-039
    pub fn new(clip: LoadedClip) -> Self {
        Self { clip }
    }

    /// Load `path`, pre-scale it according to `plan`, and prepare to render.
    pub fn load(path: &Path, out: &OutputDef) -> Result<Self> {
        Self::load_with_focus(path, out, None)
    }

    /// Like [`FrameRenderer::load`], but anchors the Ken Burns zoom on an
    /// optional fixed focus point (normalized `[0,1]` image coordinates) — e.g.
    /// a region of interest from prior recognition. `None` keeps the default pan.
    // Implements: LLR-037, SR-031
    pub fn load_with_focus(
        path: &Path,
        out: &OutputDef,
        focus: Option<(f32, f32)>,
    ) -> Result<Self> {
        Ok(Self::new(LoadedClip::load(path, out, focus)?))
    }

    /// Elapsed decode/prescale time recorded by [`LoadedClip::load`].
    // Implements: LLR-050, SR-036
    pub fn load_timings(&self) -> LoadTimings {
        self.clip.timings
    }

    pub fn total_frames(&self) -> u32 {
        self.clip.plan.total_frames
    }

    /// True when frames are produced by the SIMD crop+resize fast path (the
    /// projection is axis-aligned, i.e. rotation is off) rather than the
    /// generic warp. Exposed for the TC-087 path-selection assertion.
    // Implements: LLR-061, SR-036
    #[allow(dead_code)] // consumed by the TC-087 unit test + lib callers only
    pub fn uses_fast_path(&self) -> bool {
        self.clip.plan.is_axis_aligned()
    }

    /// Render a contiguous range of frames in parallel, returning raw `rgb24`
    /// buffers in frame order. Rendering in ranges bounds peak memory.
    pub fn render_range(&self, start: u32, end: u32) -> Vec<Vec<u8>> {
        (start..end)
            .into_par_iter()
            .map(|i| self.render_frame(i))
            .collect()
    }

    /// Render a single frame to a raw `rgb24` buffer (`out_w * out_h * 3`).
    fn render_frame(&self, i: u32) -> Vec<u8> {
        let plan = &self.clip.plan;
        let out_w = plan.out_w;
        let out_h = plan.out_h;
        let render_w = plan.render_w;
        let render_h = plan.render_h;

        // Fast path (rotation off, the common default): the projection is an
        // axis-aligned scale+translate, so the frame is exactly "SIMD-resize
        // the float crop window to the output" — same window as the warp path
        // (ClipPlan::crop_window), several times faster than a generic warp.
        // Implements: LLR-061, SR-036
        if plan.is_axis_aligned() {
            let crop = plan.crop_window(i);
            return simd_resize(self.clip.prescaled.as_ref(), Some(crop), out_w, out_h).into_raw();
        }

        // One sub-pixel warp does the whole pan/zoom/rotation in a single
        // resample (smooth motion, no integer-crop jitter, no double-resample
        // softening). The projection maps source -> render canvas; `warp_into`
        // inverts it and samples the source at sub-pixel coordinates.
        // Fades/dissolves are applied later by the pipeline's transition mixer.
        let proj = plan.projection(i);
        let mut render = RgbImage::new(render_w, render_h);
        warp_into(
            self.clip.prescaled.as_ref(),
            &proj,
            Interpolation::Bilinear,
            Rgb([0, 0, 0]),
            &mut render,
        );

        // Center-crop the constant rotation margin to the output size. The crop
        // offset is identical every frame, so it contributes no motion.
        let frame = if render_w == out_w && render_h == out_h {
            render
        } else {
            let off_x = (render_w - out_w) / 2;
            let off_y = (render_h - out_h) / 2;
            ::image::imageops::crop_imm(&render, off_x, off_y, out_w, out_h).to_image()
        };

        frame.into_raw()
    }
}

/// The CPU implementation of the backend-blind renderer contract. Delegates
/// to the inherent methods so the trait extraction changes no pixels
/// (byte-identity guarded by TC-098).
// Implements: LLR-066, SR-039
impl ClipRenderer for FrameRenderer {
    fn total_frames(&self) -> u32 {
        FrameRenderer::total_frames(self)
    }

    fn render_range(&mut self, start: u32, end: u32) -> Vec<Vec<u8>> {
        FrameRenderer::render_range(self, start, end)
    }

    fn load_timings(&self) -> LoadTimings {
        FrameRenderer::load_timings(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OutputDef;

    fn out_def() -> OutputDef {
        OutputDef {
            name: "t".into(),
            width: 320,
            height: 200,
            fps: 30,
            pic_display_time_secs: 1.0,
            fade_time_secs: 0.1,
            max_rotation_degrees: 0.0,
            bulk_video_time_min: 20,
            quality_crf: 28,
            enable_audio: false,
            audio_bitrate_kbps: 192,
            audio_sample_rate: 48_000,
            encoder: "software".into(),
            x264_preset: "medium".into(),
            zoom_amount: 0.12,
            ken_burns: true,
        }
    }

    // Verifies: SR-022, SR-036, LLR-061 (TC-087) — the SIMD resize fast path
    // is selected iff rotation is off; both paths emit frames of the exact
    // expected byte size; and the same path-seeded clip renders identically
    // across loads (plan-level determinism within the implementation —
    // bit-exactness vs the generic warp resampler is NOT claimed).
    #[test]
    fn rotation_off_takes_simd_fast_path_sr022() {
        let dir = std::env::temp_dir().join("slideshow_test_simd_path");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grad.png");
        let img = RgbImage::from_fn(640, 480, |x, y| {
            ::image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
        });
        img.save(&path).unwrap();

        // Rotation off (the common default): axis-aligned projection -> SIMD.
        let flat = out_def();
        let fast = FrameRenderer::load(&path, &flat).unwrap();
        assert!(
            fast.uses_fast_path(),
            "rotation off must select the SIMD resize fast path"
        );

        // Rotation on: generic warp_into path.
        let mut rot = out_def();
        rot.max_rotation_degrees = 8.0;
        let warp = FrameRenderer::load(&path, &rot).unwrap();
        assert!(
            !warp.uses_fast_path(),
            "rotation on must keep the warp_into path"
        );

        // Both paths emit frames of the exact rgb24 byte size on every frame.
        for r in [&fast, &warp] {
            for f in r.render_range(0, 3) {
                assert_eq!(f.len() as u32, flat.width * flat.height * 3);
            }
        }

        // Same seed (path) -> identical output within the implementation.
        let again = FrameRenderer::load(&path, &flat).unwrap();
        assert_eq!(
            fast.render_range(0, 2),
            again.render_range(0, 2),
            "same path-seeded clip must render identically across loads"
        );
    }

    // Verifies: SR-039, LLR-073 (TC-097) — the pure comparison hook at its
    // boundaries: identical buffers -> 0.0; a single-channel delta of d over
    // n bytes -> exactly d/n; all-0 vs all-255 -> 255.0 (degenerate max);
    // symmetric in its arguments. The unit leg the Release tolerance TC
    // (TC-103) asserts against.
    #[test]
    fn frame_mean_abs_diff_known_deltas_sr039() {
        // Identical buffers -> exactly 0.0.
        let a = vec![7u8, 130, 255, 0];
        assert_eq!(frame_mean_abs_diff(&a, &a), 0.0);

        // Single-channel delta of d over n bytes -> exactly d/n.
        let mut b = a.clone();
        b[1] = 130 + 40; // d = 40, n = 4
        assert_eq!(frame_mean_abs_diff(&a, &b), 40.0 / 4.0);
        // Symmetry.
        assert_eq!(frame_mean_abs_diff(&b, &a), 40.0 / 4.0);

        // Degenerate max: all-0 vs all-255 -> 255.0.
        let zeros = vec![0u8; 12];
        let maxed = vec![255u8; 12];
        assert_eq!(frame_mean_abs_diff(&zeros, &maxed), 255.0);
        assert_eq!(frame_mean_abs_diff(&maxed, &zeros), 255.0);
    }

    // Verifies: SR-039, LLR-066 (TC-098) — rendering the same path-seeded
    // LoadedClip through Box<dyn ClipRenderer> (the FrameRenderer CPU impl
    // behind the trait) and directly through FrameRenderer is byte-identical:
    // the trait extraction changes no pixels. The SR-039 CPU-path invariance
    // leg additionally requires the pre-existing determinism/render suites to
    // pass UNMODIFIED (TC-029/030/052/087/088 — cited, not duplicated).
    #[test]
    fn clip_renderer_trait_matches_frame_renderer_sr039() {
        let dir = std::env::temp_dir().join("slideshow_test_trait_eq");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grad.png");
        let img = RgbImage::from_fn(640, 480, |x, y| {
            ::image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x * y) % 256) as u8])
        });
        img.save(&path).unwrap();

        // Rotation on: exercises the warp path (the GPU-relevant projection).
        let mut out = out_def();
        out.max_rotation_degrees = 8.0;
        let clip = LoadedClip::load(&path, &out, None).unwrap();

        let direct = FrameRenderer::new(clip.clone());
        let mut boxed: Box<dyn ClipRenderer> = Box::new(FrameRenderer::new(clip));
        assert_eq!(direct.total_frames(), boxed.total_frames());

        let total = direct.total_frames().min(6);
        let via_trait = boxed.render_range(0, total);
        let direct_frames = direct.render_range(0, total);
        assert_eq!(
            via_trait, direct_frames,
            "trait-dispatched CPU rendering must be byte-identical to direct FrameRenderer"
        );
        assert_eq!(via_trait.len(), total as usize);
    }

    #[test]
    fn frames_have_expected_raw_size() {
        // Synthesize a small image on disk.
        let dir = std::env::temp_dir().join("slideshow_test_frames");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("solid.png");
        let img = RgbImage::from_pixel(640, 480, ::image::Rgb([10, 120, 200]));
        img.save(&path).unwrap();

        let out = out_def();
        let renderer = FrameRenderer::load(&path, &out).unwrap();
        let frames = renderer.render_range(0, 3);
        assert_eq!(frames.len(), 3);
        for f in &frames {
            assert_eq!(f.len() as u32, out.width * out.height * 3);
        }
    }
}
