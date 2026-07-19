//! Single home for rgb-to-yuv colorimetry (SR-040): the BT.601 limited-range
//! coefficient set shared by the CPU converter and the GPU compute pass (which
//! receives it as uniforms — `render.wgsl` carries no copied constants), the
//! per-plane black levels the LLR-077 fade consumes, and the CPU
//! `rgb24`-to-planar-`yuv420p` converter used by the device-lost degrade and
//! texture-limit CPU fallback paths so exactly one transport reaches the
//! encoder per run.

use crate::image::{ClipRenderer, FrameRenderer, LoadTimings, LoadedClip};
use crate::transport::FrameTransport;

/// Limited-range luma black level (Y for RGB black). The fade scales Y toward
/// this, never 0 (LLR-077).
// Implements: LLR-079, SR-040
pub const Y_BLACK: u8 = 16;

/// Chroma neutral value (U and V for any gray, black included). The fade
/// scales U/V toward this — chroma trending toward 0 tints green/purple
/// (SR-040 chroma-neutral-fade leg).
// Implements: LLR-079, SR-040
pub const UV_NEUTRAL: u8 = 128;

/// One rgb-to-yuv coefficient set: per-plane RGB rows plus the additive
/// offsets. The single source both the CPU converter and the GPU compute-pass
/// uniforms (`gpu::YuvUniforms`) derive from — the coefficients exist nowhere
/// else (TC-109).
// Implements: LLR-079, SR-040
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YuvMatrix {
    /// Y row: `Y = y_offset + y.rgb` (inputs 0..=255).
    pub y: [f32; 3],
    /// U (Cb) row: `U = uv_offset + u.rgb`.
    pub u: [f32; 3],
    /// V (Cr) row: `V = uv_offset + v.rgb`.
    pub v: [f32; 3],
    pub y_offset: f32,
    pub uv_offset: f32,
}

/// BT.601 **limited range** (Y 16..=235, chroma centered on 128) — pinned
/// because ffmpeg swscale's default rgb24->yuv420p conversion is BT.601
/// limited at every resolution, and the SR-040 colorimetry leg compares
/// against that historical conversion (LLR-076 recorded call). Rows are the
/// ITU coefficients Kr=0.299/Kg=0.587/Kb=0.114 scaled by 219/255 (luma) and
/// 224/255 (chroma).
// Implements: LLR-079, LLR-076, SR-040
pub const BT601_LIMITED: YuvMatrix = YuvMatrix {
    y: [0.256_788, 0.504_129, 0.097_906],
    u: [-0.148_223, -0.290_993, 0.439_216],
    v: [0.439_216, -0.367_788, -0.071_427],
    y_offset: Y_BLACK as f32,
    uv_offset: UV_NEUTRAL as f32,
};

/// Convert one interleaved `rgb24` frame to planar `yuv420p` in FFmpeg
/// rawvideo order (Y `w*h`, then U, then V quarter planes) using
/// [`BT601_LIMITED`] in integer fixed point. Chroma is the plain 2x2 average
/// (the same subsampling the LLR-076 compute pass applies, so residual vs the
/// shader is rounding only). `w`/`h` must be even (`even_dims`, LLR-010).
// Implements: LLR-079, SR-040
pub fn rgb_to_yuv420p(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    debug_assert!(
        w.is_multiple_of(2) && h.is_multiple_of(2),
        "even dims required: {w}x{h}"
    );
    debug_assert_eq!(rgb.len(), w * h * 3, "rgb24 buffer must match {w}x{h}");
    let out_len = FrameTransport::Yuv420p.frame_bytes(w as u32, h as u32);
    let mut out = vec![0u8; out_len];
    let (y_plane, chroma) = out.split_at_mut(w * h);
    let (u_plane, v_plane) = chroma.split_at_mut(w * h / 4);

    // 16.16 fixed-point rows derived from the ONE f32 home above — never a
    // second copy of the coefficients. `+ HALF >> 16` rounds to nearest
    // (arithmetic shift keeps negative chroma sums correct).
    #[inline]
    fn fx(c: f32) -> i32 {
        (c * 65536.0).round() as i32
    }
    let m = &BT601_LIMITED;
    let [yr, yg, yb] = m.y.map(fx);
    let [ur, ug, ub] = m.u.map(fx);
    let [vr, vg, vb] = m.v.map(fx);
    const HALF: i32 = 1 << 15;
    // Rounding term for chroma: 16.16 coefficients times a 4-pixel sum, so
    // the total shift is 18 bits.
    const HALF4: i32 = 1 << 17;

    // Two source rows per iteration (4:2:0: one chroma row per 2 luma rows);
    // per-pixel work is branch-free integer math, autovectorizer-friendly.
    let half_w = w / 2;
    for cy in 0..h / 2 {
        let top = &rgb[2 * cy * w * 3..(2 * cy + 1) * w * 3];
        let bot = &rgb[(2 * cy + 1) * w * 3..(2 * cy + 2) * w * 3];
        for cx in 0..half_w {
            // The 2x2 block's four pixels: two from each source row.
            let mut sum = [0i32; 3];
            for (row_off, row) in [(0usize, top), (1, bot)] {
                for k in 0..2 {
                    let p = (2 * cx + k) * 3;
                    let (r, g, b) = (row[p] as i32, row[p + 1] as i32, row[p + 2] as i32);
                    sum[0] += r;
                    sum[1] += g;
                    sum[2] += b;
                    let y = ((yr * r + yg * g + yb * b + HALF) >> 16) + Y_BLACK as i32;
                    y_plane[(2 * cy + row_off) * w + 2 * cx + k] = y as u8;
                }
            }
            // Chroma from the 2x2 rgb sum (the matrix is affine, so averaging
            // rgb before it equals averaging converted values — the same 2x2
            // average the LLR-076 compute pass applies).
            let u = ((ur * sum[0] + ug * sum[1] + ub * sum[2] + HALF4) >> 18) + UV_NEUTRAL as i32;
            let v = ((vr * sum[0] + vg * sum[1] + vb * sum[2] + HALF4) >> 18) + UV_NEUTRAL as i32;
            u_plane[cy * half_w + cx] = u as u8;
            v_plane[cy * half_w + cx] = v as u8;
        }
    }
    out
}

/// The CPU [`ClipRenderer`] for one run transport: a plain [`FrameRenderer`]
/// in rgb mode, or the same renderer behind a per-frame [`rgb_to_yuv420p`]
/// conversion in yuv mode — the LLR-079 consumer that keeps a texture-limit
/// or post-degrade CPU clip on the run's single transport (SR-040
/// one-format-per-run leg).
// Implements: LLR-079, SR-040
pub fn cpu_clip_renderer(clip: LoadedClip, transport: FrameTransport) -> Box<dyn ClipRenderer> {
    match transport {
        FrameTransport::Rgb24 => Box::new(FrameRenderer::new(clip)),
        FrameTransport::Yuv420p => Box::new(YuvCpuRenderer::new(clip)),
    }
}

/// A CPU frame renderer whose `rgb24` output is converted to the run's
/// planar `yuv420p` transport frame by frame.
struct YuvCpuRenderer {
    inner: FrameRenderer,
    out_w: usize,
    out_h: usize,
}

impl YuvCpuRenderer {
    fn new(clip: LoadedClip) -> Self {
        let (out_w, out_h) = (clip.plan.out_w as usize, clip.plan.out_h as usize);
        Self {
            inner: FrameRenderer::new(clip),
            out_w,
            out_h,
        }
    }
}

impl ClipRenderer for YuvCpuRenderer {
    fn total_frames(&self) -> u32 {
        self.inner.total_frames()
    }

    fn render_range(&mut self, start: u32, end: u32) -> Vec<Vec<u8>> {
        self.inner
            .render_range(start, end)
            .iter()
            .map(|f| rgb_to_yuv420p(f, self.out_w, self.out_h))
            .collect()
    }

    fn load_timings(&self) -> LoadTimings {
        self.inner.load_timings()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w`x`h` frame of one solid color.
    fn uniform_frame(w: usize, h: usize, rgb: [u8; 3]) -> Vec<u8> {
        rgb.iter().copied().cycle().take(w * h * 3).collect()
    }

    // Verifies: SR-040, LLR-079, LLR-076 (TC-109) — every colorimetry
    // constant lives in ONE home: the shader-uniform payload is built from
    // BT601_LIMITED and round-trips equal, and the fade's black levels
    // (Y_BLACK=16, U/V=UV_NEUTRAL=128) are the same symbols the matrix
    // offsets carry.
    #[test]
    fn bt601_constants_single_home_roundtrip_sr040() {
        let m = &BT601_LIMITED;
        let u = crate::image::gpu::YuvUniforms::for_output(320, 200);
        assert_eq!(u.y_row, [m.y[0], m.y[1], m.y[2], m.y_offset]);
        assert_eq!(u.u_row, [m.u[0], m.u[1], m.u[2], m.uv_offset]);
        assert_eq!(u.v_row, [m.v[0], m.v[1], m.v[2], m.uv_offset]);
        assert_eq!([u.size[0], u.size[1]], [320, 200]);

        // The LLR-077 fade consumes exactly these values (pinned).
        assert_eq!(Y_BLACK, 16);
        assert_eq!(UV_NEUTRAL, 128);
        assert_eq!(m.y_offset, Y_BLACK as f32);
        assert_eq!(m.uv_offset, UV_NEUTRAL as f32);

        // Each chroma row sums to ~0: any gray input yields exactly the
        // neutral 128 (the property the fade and TC-114 gray leg rely on).
        for row in [m.u, m.v] {
            assert!(row.iter().sum::<f32>().abs() < 1e-5, "{row:?}");
        }
    }

    // Verifies: SR-040, LLR-079, LLR-076 (TC-109) — render.wgsl carries NO
    // copied BT.601 (or mistaken BT.709) coefficients and no hard-coded
    // 16/128 offsets: the compute pass receives them as uniforms from this
    // module, so a constant can never drift between the CPU and GPU paths.
    #[test]
    fn wgsl_carries_no_copied_coefficients_sr040() {
        let wgsl = include_str!("render.wgsl");

        // The uniform path must exist (positive check first, so this test
        // fails loudly if the entry point is renamed rather than silently
        // scanning the wrong shader).
        assert!(wgsl.contains("cs_yuv420p"), "yuv compute entry point");
        assert!(wgsl.contains("YuvUniforms"), "uniform-fed coefficients");

        // No coefficient of the one home may appear as a literal — checked in
        // both shortest-roundtrip and 3-decimal textbook form.
        let m = &BT601_LIMITED;
        let mut forbidden: Vec<String> = Vec::new();
        for c in m.y.iter().chain(&m.u).chain(&m.v) {
            forbidden.push(format!("{}", c.abs()));
            forbidden.push(format!("{:.3}", c.abs()));
        }
        // Textbook full-range BT.601 and BT.709 forms (a wrong-matrix copy
        // must trip this too), plus the limited-range offsets.
        for s in [
            "0.299", "0.587", "0.114", "0.2126", "0.7152", "0.0722", "16.0", "128.0",
        ] {
            forbidden.push(s.to_string());
        }
        for s in forbidden {
            assert!(
                !wgsl.contains(&s),
                "render.wgsl must not carry the copied constant '{s}' (TC-109)"
            );
        }
    }

    // Verifies: SR-040, LLR-079 (TC-110) — the converter pins the BT.601
    // limited-range values on known colors: black/white exact, primaries
    // within +-1 rounding; planar Y-U-V layout and w*h*3/2 length from the
    // LLR-075 home.
    #[test]
    fn rgb_to_yuv420p_known_colors_sr040() {
        let (w, h) = (8usize, 4usize);
        let y_len = w * h;
        let q = y_len / 4;

        let cases: [([u8; 3], [u8; 3], bool); 5] = [
            ([0, 0, 0], [16, 128, 128], true),        // black, exact
            ([255, 255, 255], [235, 128, 128], true), // white, exact
            ([255, 0, 0], [81, 90, 240], false),      // red +-1
            ([0, 255, 0], [145, 54, 34], false),      // green +-1
            ([0, 0, 255], [41, 240, 110], false),     // blue +-1
        ];
        for (rgb, [ey, eu, ev], exact) in cases {
            let out = rgb_to_yuv420p(&uniform_frame(w, h, rgb), w, h);
            assert_eq!(
                out.len(),
                FrameTransport::Yuv420p.frame_bytes(w as u32, h as u32),
                "length from the LLR-075 home for {rgb:?}"
            );
            let tol = if exact { 0i16 } else { 1 };
            let (y_plane, chroma) = out.split_at(y_len);
            let (u_plane, v_plane) = chroma.split_at(q);
            for (plane, expect, name) in
                [(y_plane, ey, "Y"), (u_plane, eu, "U"), (v_plane, ev, "V")]
            {
                for &b in plane {
                    assert!(
                        (b as i16 - expect as i16).abs() <= tol,
                        "{name} of {rgb:?}: got {b}, want {expect} +-{tol}"
                    );
                }
            }
        }

        // Planar ORDER (Y then U then V): a red top-left 2x2 block on black
        // puts U~90 at the U plane's first byte and V~240 at the V plane's
        // first byte — swapped planes would show 240 first.
        let mut frame = uniform_frame(w, h, [0, 0, 0]);
        for (x, y) in [(0usize, 0usize), (1, 0), (0, 1), (1, 1)] {
            frame[(y * w + x) * 3] = 255; // R
        }
        let out = rgb_to_yuv420p(&frame, w, h);
        assert!(
            (out[y_len] as i16 - 90).abs() <= 1,
            "first U byte ~90 for red block, got {}",
            out[y_len]
        );
        assert!(
            (out[y_len + q] as i16 - 240).abs() <= 1,
            "first V byte ~240 for red block, got {}",
            out[y_len + q]
        );
    }
}
