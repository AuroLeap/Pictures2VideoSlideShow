//! Transform calculations for the Ken Burns (pan + zoom) slideshow effect.
//!
//! For each image we build a [`ClipPlan`] that, for every frame index, yields a
//! floating-point [`Projection`] mapping the pre-scaled source into the render
//! canvas. The renderer feeds that projection to a single sub-pixel warp, so the
//! pan/zoom/rotation advance smoothly frame-to-frame instead of snapping to whole
//! pixels (the jitter the integer-crop approach produced).

use crate::config::OutputDef;
use imageproc::geometric_transformations::Projection;

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

/// A fully-resolved animation plan for a single image clip.
#[derive(Debug, Clone)]
pub struct ClipPlan {
    pub total_frames: u32,
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
    // Convenience wrapper over `with_focus`; the binary always supplies a focus.
    #[allow(dead_code)]
    pub fn new(img_w: u32, img_h: u32, out: &OutputDef, seed: u64) -> Self {
        Self::with_focus(img_w, img_h, out, seed, None)
    }

    /// Like [`ClipPlan::new`], but with an optional fixed focus point in
    /// normalized `[0,1]` image coordinates. When `Some`, the zoom is anchored
    /// to that point for the whole clip (no pan) — so a region of interest
    /// (e.g. a face from prior recognition) stays put as the image zooms.
    /// When `None`, the default two-point pan is used.
    // Implements: LLR-037, SR-031
    pub fn with_focus(
        img_w: u32,
        img_h: u32,
        out: &OutputDef,
        seed: u64,
        focus: Option<(f32, f32)>,
    ) -> Self {
        let out_w = out.width;
        let out_h = out.height;
        let total_frames = out.total_frames();

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
            // Subtle rotation: a random tilt drifting toward (closer to) level.
            let max_rot = out.max_rotation_degrees.max(0.0);
            let sign = if rng.next_bool() { 1.0 } else { -1.0 };
            let rot0 = sign * max_rot;
            let rot1 = sign * max_rot * 0.4;
            // Draw the default pan endpoints unconditionally so the RNG stream
            // (and thus rotation above) is unaffected by whether a focus is set.
            let p0 = (rng.next_f32(), rng.next_f32());
            let p1 = (rng.next_f32(), rng.next_f32());
            let (f0, f1) = match focus {
                // Fixed focus: anchor both endpoints on the ROI -> no pan.
                Some((fx, fy)) => {
                    let f = (fx.clamp(0.0, 1.0), fy.clamp(0.0, 1.0));
                    (f, f)
                }
                // Default: pan between two distinct random points.
                None => (p0, p1),
            };
            (z0, z1, f0, f1, rot0, rot1)
        } else {
            // Static: centered, or on the focus point when one is given.
            let f = match focus {
                Some((fx, fy)) => (fx.clamp(0.0, 1.0), fy.clamp(0.0, 1.0)),
                None => (0.5, 0.5),
            };
            (1.0, 1.0, f, f, 0.0, 0.0)
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

    /// Eased interpolation parameter for frame `i` in `[0,1]`.
    fn eased(&self, i: u32) -> f32 {
        let t = if self.total_frames <= 1 {
            0.0
        } else {
            i as f32 / (self.total_frames - 1) as f32
        };
        smoothstep(t)
    }

    /// Floating-point projection mapping the pre-scaled source into the render
    /// canvas (`render_w` x `render_h`) for frame `i`. Composes the zoom/pan
    /// (anchored on the focus fraction) with a rotation about the render center.
    /// The renderer warps the source through this with sub-pixel interpolation,
    /// then center-crops the constant rotation margin to the output size.
    // Implements: LLR-036, SR-031
    pub fn projection(&self, i: u32) -> Projection {
        let e = self.eased(i);

        let zoom = self.z0 + (self.z1 - self.z0) * e;
        // Visible region size in pre-scaled pixels. At zoom == zmax this equals
        // the render size (full detail); smaller zoom shows a larger region.
        let cw_f = (self.render_w as f32 * self.zmax / zoom).min(self.pre_w as f32);
        let ch_f = (self.render_h as f32 * self.zmax / zoom).min(self.pre_h as f32);
        // Uniform source -> render scale: maps the crop window onto the canvas.
        let s = zoom / self.zmax;

        // Focus fraction (constant for a fixed focus, interpolated for a pan).
        let fx = self.f0.0 + (self.f1.0 - self.f0.0) * e;
        let fy = self.f0.1 + (self.f1.1 - self.f0.1) * e;

        // Float crop origin in source pixels; anchoring the focus point at
        // source fraction `f` keeps it at render fraction `f` for all zooms.
        let cl = (self.pre_w as f32 - cw_f).max(0.0) * fx;
        let ct = (self.pre_h as f32 - ch_f).max(0.0) * fy;

        // Source -> render (pre-rotation): translate the crop origin to (0,0)
        // then scale onto the canvas.
        let m1 = Projection::scale(s, s) * Projection::translate(-cl, -ct);

        let theta = self.rotation_deg(i).to_radians();
        if theta.abs() <= f32::EPSILON {
            m1
        } else {
            // Rotate about the render center so the constant center-crop margin
            // keeps black corners out (matching the prior coverage guarantee).
            let cx = self.render_w as f32 / 2.0;
            let cy = self.render_h as f32 / 2.0;
            Projection::translate(cx, cy)
                * Projection::rotate(theta)
                * Projection::translate(-cx, -cy)
                * m1
        }
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

    /// Map a source point through a plan's frame projection into render space.
    fn map(plan: &ClipPlan, i: u32, p: (f32, f32)) -> (f32, f32) {
        plan.projection(i) * p
    }

    // Verifies: LLR-036 — the output region (the center crop kept after the
    // rotation margin is discarded) samples real source content on every frame,
    // i.e. no black corners even with rotation.
    #[test]
    fn output_region_stays_inside_source() {
        let mut out = test_output();
        out.max_rotation_degrees = 8.0;
        let plan = ClipPlan::new(1920, 1280, &out, 12345);
        // The renderer warps to the render canvas then center-crops the margin
        // to the output size; only that output rectangle must map to content.
        let off_x = (plan.render_w - plan.out_w) as f32 / 2.0;
        let off_y = (plan.render_h - plan.out_h) as f32 / 2.0;
        for i in 0..plan.total_frames {
            let inv = plan.projection(i).invert();
            let corners = [
                (off_x, off_y),
                (off_x + plan.out_w as f32, off_y),
                (off_x, off_y + plan.out_h as f32),
                (off_x + plan.out_w as f32, off_y + plan.out_h as f32),
            ];
            for c in corners {
                let (sx, sy) = inv * c;
                // Allow a sub-pixel tolerance for float/rounding slack.
                assert!(
                    sx >= -1.0 && sx <= plan.pre_w as f32 + 1.0,
                    "frame {i} corner {c:?} -> sx {sx} out of [0,{}]",
                    plan.pre_w
                );
                assert!(
                    sy >= -1.0 && sy <= plan.pre_h as f32 + 1.0,
                    "frame {i} corner {c:?} -> sy {sy} out of [0,{}]",
                    plan.pre_h
                );
            }
        }
    }

    #[test]
    fn prescaled_covers_output_at_max_zoom() {
        let out = test_output();
        let plan = ClipPlan::new(1230, 1920, &out, 7);
        assert!(plan.pre_w >= out.width);
        assert!(plan.pre_h >= out.height);
    }

    // Verifies: LLR-037, SR-031 — a fixed focus keeps the region of interest at a
    // constant screen fraction across the whole clip (regression for focus drift).
    #[test]
    fn fixed_focus_holds_a_constant_screen_fraction() {
        let mut out = test_output();
        out.max_rotation_degrees = 0.0; // isolate pan/zoom from rotation
        let focus = (0.3f32, 0.7f32);
        let plan = ClipPlan::with_focus(1920, 1280, &out, 99, Some(focus));
        // The source point at fraction `focus` should map to render fraction
        // `focus` on every frame.
        let fsrc = (focus.0 * plan.pre_w as f32, focus.1 * plan.pre_h as f32);
        let expect = (
            focus.0 * plan.render_w as f32,
            focus.1 * plan.render_h as f32,
        );
        for i in 0..plan.total_frames {
            let (rx, ry) = map(&plan, i, fsrc);
            assert!(
                (rx - expect.0).abs() < 1.0,
                "frame {i} fx {rx} vs {}",
                expect.0
            );
            assert!(
                (ry - expect.1).abs() < 1.0,
                "frame {i} fy {ry} vs {}",
                expect.1
            );
        }
    }

    // Verifies: LLR-036 — motion is sub-pixel smooth: the per-frame change in
    // where the source maps never jumps (no integer stair-stepping), and at least
    // one transition is a genuine fraction of a pixel.
    #[test]
    fn motion_is_subpixel_smooth() {
        let out = test_output();
        let plan = ClipPlan::new(1920, 1280, &out, 2024);
        let probe = (plan.pre_w as f32 * 0.5, plan.pre_h as f32 * 0.5);
        let mut prev = map(&plan, 0, probe);
        let mut saw_fractional = false;
        for i in 1..plan.total_frames {
            let cur = map(&plan, i, probe);
            let dx = (cur.0 - prev.0).abs();
            let dy = (cur.1 - prev.1).abs();
            // Smoothstep easing keeps per-frame motion small and bounded.
            assert!(dx < 30.0 && dy < 30.0, "frame {i} jump dx {dx} dy {dy}");
            if dx.fract() > 1e-3 || dy.fract() > 1e-3 {
                saw_fractional = true;
            }
            prev = cur;
        }
        assert!(
            saw_fractional,
            "expected sub-pixel motion, got only integer steps"
        );
    }
}
