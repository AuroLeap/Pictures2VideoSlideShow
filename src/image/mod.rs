//! Frame generation: turn one source image into a sequence of RGB frames
//! implementing the Ken Burns pan/zoom + fade effect.
//!
//! The source image is pre-scaled once (shared via [`Arc`]) and each frame is
//! produced by cropping a window and scaling it to the output size. Frames are
//! emitted as raw `rgb24` byte buffers ready to pipe to FFmpeg.

use crate::config::OutputDef;
use crate::error::{Result, SlideshowError};
use crate::transform::{seed_from_str, ClipPlan};
use ::image::imageops::FilterType;
use ::image::{GenericImageView, Rgb, RgbImage};
use imageproc::geometric_transformations::{warp_into, Interpolation};
use rayon::prelude::*;
use std::path::Path;
use std::sync::Arc;

/// Elapsed decode (`image::open`) and prescale (resize) time for one clip
/// load — the two components of the SR-036 clip-boundary stall (PB-002).
// Implements: LLR-050, SR-036
#[derive(Debug, Clone, Copy, Default)]
pub struct LoadTimings {
    pub decode: std::time::Duration,
    pub prescale: std::time::Duration,
}

/// Renders all frames for a single image clip.
pub struct FrameRenderer {
    /// Pre-scaled source, sized so the max-zoom crop is full detail.
    prescaled: Arc<RgbImage>,
    plan: ClipPlan,
    /// How long this clip's decode/prescale took, for the stage timers.
    load_timings: LoadTimings,
}

impl FrameRenderer {
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
        // Time decode and prescale separately: together they are the serial
        // clip-boundary stall the PB-002 budget tracks (LLR-050, SR-036).
        let t = std::time::Instant::now();
        let img = ::image::open(path).map_err(SlideshowError::Image)?;
        let decode = t.elapsed();
        let (w, h) = img.dimensions();
        let seed = seed_from_str(&path.to_string_lossy());
        let plan = ClipPlan::with_focus(w, h, out, seed, focus);

        // Pre-scale once. Triangle is a good speed/quality trade-off for the
        // up/down-scale that follows per frame.
        let t = std::time::Instant::now();
        let prescaled =
            ::image::imageops::resize(&img.to_rgb8(), plan.pre_w, plan.pre_h, FilterType::Triangle);
        let prescale = t.elapsed();

        Ok(Self {
            prescaled: Arc::new(prescaled),
            plan,
            load_timings: LoadTimings { decode, prescale },
        })
    }

    /// Elapsed decode/prescale time recorded by [`FrameRenderer::load_with_focus`].
    // Implements: LLR-050, SR-036
    pub fn load_timings(&self) -> LoadTimings {
        self.load_timings
    }

    pub fn total_frames(&self) -> u32 {
        self.plan.total_frames
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
        let out_w = self.plan.out_w;
        let out_h = self.plan.out_h;
        let render_w = self.plan.render_w;
        let render_h = self.plan.render_h;

        // One sub-pixel warp does the whole pan/zoom/rotation in a single
        // resample (smooth motion, no integer-crop jitter, no double-resample
        // softening). The projection maps source -> render canvas; `warp_into`
        // inverts it and samples the source at sub-pixel coordinates.
        // Fades/dissolves are applied later by the pipeline's transition mixer.
        let proj = self.plan.projection(i);
        let mut render = RgbImage::new(render_w, render_h);
        warp_into(
            self.prescaled.as_ref(),
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
