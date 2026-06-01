//! Transform calculations for the Ken Burns (pan + zoom) slideshow effect.
//!
//! For each image we build a [`ClipPlan`] that, for every frame index, yields a
//! crop window (in pre-scaled source pixels) and a brightness multiplier (for
//! fade in/out). The renderer crops that window and scales it to the output
//! resolution, producing smooth motion without per-frame full re-scales.

use crate::config::OutputDef;

/// Smoothstep easing: slow in, slow out. `t` is clamped to `[0, 1]`.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Deterministic per-image pseudo-randomness derived from a 64-bit seed, so a
/// given image always animates the same way (reproducible output).
struct SeededRng {
    state: u64,
}

impl SeededRng {
    fn new(seed: u64) -> Self {
        // Avoid a zero state.
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// SplitMix64.
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform `f32` in `[0, 1)`.
    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

/// FNV-1a hash of a string, used to seed per-image randomness from its path.
pub fn seed_from_str(s: &str) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for b in s.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

/// A crop rectangle in pre-scaled source pixels.
#[derive(Debug, Clone, Copy)]
pub struct CropWindow {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// A fully-resolved animation plan for a single image clip.
#[derive(Debug, Clone)]
pub struct ClipPlan {
    pub total_frames: u32,
    pub fade_frames: u32,
    /// Final output frame size.
    pub out_w: u32,
    pub out_h: u32,
    /// Intermediate render size (>= output) that leaves a margin so rotation
    /// never exposes black corners. Equals the output size when rotation is off.
    pub render_w: u32,
    pub render_h: u32,
    /// Pre-scaled source size the renderer should scale the source image to.
    pub pre_w: u32,
    pub pre_h: u32,
    /// Max zoom factor (the pre-scale is sized so this zoom shows full detail).
    zmax: f32,
    /// Zoom factor at the start and end of the clip (each >= 1.0).
    z0: f32,
    z1: f32,
    /// Pan focus fractions [0,1] for start and end (top-left of crop window).
    f0: (f32, f32),
    f1: (f32, f32),
    /// Rotation in degrees at the start and end of the clip.
    rot0: f32,
    rot1: f32,
}

impl ClipPlan {
    /// Build a plan for an image of `(img_w, img_h)` rendered with `out`.
    /// `seed` makes the random zoom direction / pan endpoints reproducible.
    pub fn new(img_w: u32, img_h: u32, out: &OutputDef, seed: u64) -> Self {
        let out_w = out.width;
        let out_h = out.height;
        let total_frames = out.total_frames();
        let fade_frames = out.fade_frames();

        // Scale needed for the source to fully cover the output canvas.
        let cover = (out_w as f32 / img_w as f32).max(out_h as f32 / img_h as f32);

        let (z0, z1, f0, f1, rot0, rot1) = if out.ken_burns {
            let mut rng = SeededRng::new(seed);
            let amount = out.zoom_amount.max(0.0);
            // Randomize zoom direction.
            let (z0, z1) = if rng.next_bool() {
                (1.0, 1.0 + amount) // zoom in
            } else {
                (1.0 + amount, 1.0) // zoom out
            };
            // Random but distinct-ish pan endpoints.
            let f0 = (rng.next_f32(), rng.next_f32());
            let f1 = (rng.next_f32(), rng.next_f32());
            // Subtle rotation: a random tilt drifting toward (closer to) level.
            let max_rot = out.max_rotation_degrees.max(0.0);
            let sign = if rng.next_bool() { 1.0 } else { -1.0 };
            let rot0 = sign * max_rot;
            let rot1 = sign * max_rot * 0.4;
            (z0, z1, f0, f1, rot0, rot1)
        } else {
            // Static, centered.
            (1.0, 1.0, (0.5, 0.5), (0.5, 0.5), 0.0, 0.0)
        };

        // Rotation margin: how much larger than the output we must render so
        // that rotating by up to `max_rotation_degrees` never shows black
        // corners. Derived from the rotated-rectangle cover requirement.
        let margin = {
            let max_abs = rot0.abs().max(rot1.abs());
            if max_abs <= f32::EPSILON {
                1.0
            } else {
                let r = max_abs.to_radians();
                let (s, c) = (r.sin().abs(), r.cos().abs());
                let mw = c + (out_h as f32 / out_w as f32) * s;
                let mh = c + (out_w as f32 / out_h as f32) * s;
                mw.max(mh) * 1.01 // small safety factor
            }
        };

        let render_w = ((out_w as f32 * margin).round() as u32).max(out_w);
        let render_h = ((out_h as f32 * margin).round() as u32).max(out_h);

        let zmax = z0.max(z1);
        let pre_w = (img_w as f32 * cover * zmax * margin)
            .round()
            .max(render_w as f32) as u32;
        let pre_h = (img_h as f32 * cover * zmax * margin)
            .round()
            .max(render_h as f32) as u32;

        Self {
            total_frames,
            fade_frames,
            out_w,
            out_h,
            render_w,
            render_h,
            pre_w,
            pre_h,
            zmax,
            z0,
            z1,
            f0,
            f1,
            rot0,
            rot1,
        }
    }

    /// Rotation in degrees for frame `i` (eased).
    pub fn rotation_deg(&self, i: u32) -> f32 {
        if (self.rot0 - self.rot1).abs() <= f32::EPSILON && self.rot0.abs() <= f32::EPSILON {
            return 0.0;
        }
        let t = if self.total_frames <= 1 {
            0.0
        } else {
            i as f32 / (self.total_frames - 1) as f32
        };
        let e = smoothstep(t);
        self.rot0 + (self.rot1 - self.rot0) * e
    }

    /// Crop window in pre-scaled pixels for frame `i`.
    pub fn window(&self, i: u32) -> CropWindow {
        let t = if self.total_frames <= 1 {
            0.0
        } else {
            i as f32 / (self.total_frames - 1) as f32
        };
        let e = smoothstep(t);

        let zoom = self.z0 + (self.z1 - self.z0) * e;
        // Visible region size in pre-scaled pixels. At zoom == zmax this equals
        // the render size (full detail); smaller zoom shows a larger region.
        let cw = (self.render_w as f32 * self.zmax / zoom).round() as u32;
        let ch = (self.render_h as f32 * self.zmax / zoom).round() as u32;
        let cw = cw.min(self.pre_w).max(1);
        let ch = ch.min(self.pre_h).max(1);

        let max_x = self.pre_w.saturating_sub(cw);
        let max_y = self.pre_h.saturating_sub(ch);

        let fx = self.f0.0 + (self.f1.0 - self.f0.0) * e;
        let fy = self.f0.1 + (self.f1.1 - self.f0.1) * e;

        let x = (max_x as f32 * fx).round() as u32;
        let y = (max_y as f32 * fy).round() as u32;

        CropWindow {
            x: x.min(max_x),
            y: y.min(max_y),
            w: cw,
            h: ch,
        }
    }

    /// Brightness multiplier in `[0, 1]` for fade in/out on frame `i`.
    pub fn brightness(&self, i: u32) -> f32 {
        if self.fade_frames == 0 {
            return 1.0;
        }
        let fade = self.fade_frames;
        // Fade in.
        if i < fade {
            return (i + 1) as f32 / fade as f32;
        }
        // Fade out.
        if i >= self.total_frames.saturating_sub(fade) {
            let remaining = self.total_frames.saturating_sub(i);
            return remaining as f32 / fade as f32;
        }
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_output() -> OutputDef {
        OutputDef {
            name: "t".into(),
            width: 1440,
            height: 900,
            fps: 30,
            pic_display_time_secs: 6.0,
            fade_time_secs: 0.5,
            max_rotation_degrees: 0.0,
            bulk_video_time_min: 20,
            quality_crf: 28,
            enable_audio: false,
            zoom_amount: 0.12,
            ken_burns: true,
        }
    }

    #[test]
    fn frame_count_matches_timing() {
        let out = test_output();
        assert_eq!(out.total_frames(), 180);
        assert_eq!(out.fade_frames(), 15);
    }

    #[test]
    fn windows_fit_inside_prescaled_image() {
        let out = test_output();
        let plan = ClipPlan::new(1920, 1280, &out, 12345);
        for i in 0..plan.total_frames {
            let w = plan.window(i);
            assert!(w.x + w.w <= plan.pre_w, "frame {i} x overflow");
            assert!(w.y + w.h <= plan.pre_h, "frame {i} y overflow");
            assert!(w.w > 0 && w.h > 0);
        }
    }

    #[test]
    fn prescaled_covers_output_at_max_zoom() {
        let out = test_output();
        let plan = ClipPlan::new(1230, 1920, &out, 7);
        assert!(plan.pre_w >= out.width);
        assert!(plan.pre_h >= out.height);
    }

    #[test]
    fn brightness_fades_from_zero_to_full() {
        let out = test_output();
        let plan = ClipPlan::new(1920, 1280, &out, 1);
        assert!(plan.brightness(0) < 1.0);
        assert!((plan.brightness(plan.total_frames / 2) - 1.0).abs() < f32::EPSILON);
        assert!(plan.brightness(plan.total_frames - 1) < 1.0);
    }

    #[test]
    fn aspect_ratio_of_window_matches_output() {
        let out = test_output();
        let plan = ClipPlan::new(1920, 1280, &out, 99);
        let w = plan.window(0);
        let win_ar = w.w as f32 / w.h as f32;
        let out_ar = out.width as f32 / out.height as f32;
        // Rounding to integer pixels introduces a small error.
        assert!((win_ar - out_ar).abs() < 0.02, "ar {win_ar} vs {out_ar}");
    }
}
