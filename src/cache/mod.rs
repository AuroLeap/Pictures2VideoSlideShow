//! Segment cache (SR-037): pure cache-key derivation for per-clip encoded
//! segments, keyed by source identity, encode-relevant output parameters,
//! transition-neighbor identity on each side, and engine version — so a warm
//! rebuild re-encodes only what actually changed (plus its two neighbors).

pub mod planner;
pub mod store;

use crate::config::OutputDef;
use crate::media::MediaFile;
use crate::util::file_utils::hex_lower;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

/// Bump to invalidate every cached segment when the segment format or the
/// frame-generation semantics change without a crate-version bump (SR-037
/// engine-version key part; the crate version is folded in alongside).
// Implements: LLR-053, SR-037
pub const CACHE_FORMAT_VERSION: u32 = 1;

/// The engine identity folded into every segment key: crate version plus the
/// bumpable cache-format constant.
// Implements: LLR-053, SR-037
pub fn engine_version() -> String {
    format!("{}+f{}", env!("CARGO_PKG_VERSION"), CACHE_FORMAT_VERSION)
}

/// Identity of one source media file for cache purposes: absolute path (which
/// is also the Ken Burns seed input, `transform::seed_from_str` — SR-022),
/// byte size, and mtime in epoch ms. Any of the three changing means the
/// clip's pixels may change, so it must re-key.
// Implements: LLR-053, SR-037
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIdentity {
    pub path: String,
    pub size: u64,
    pub mtime_ms: u64,
}

impl SourceIdentity {
    /// Identity of a scanned media file (mtime captured at scan time).
    pub fn of(file: &MediaFile) -> Self {
        Self {
            path: file.path.to_string_lossy().into_owned(),
            size: file.size_bytes,
            mtime_ms: file.mtime_ms,
        }
    }

    /// Append this identity to a canonical key string.
    fn write_to(&self, out: &mut String) {
        let _ = write!(out, "{}\x1f{}\x1f{}", self.path, self.size, self.mtime_ms);
    }
}

/// The encode-relevant [`OutputDef`] facts that enter every segment key —
/// everything that changes output pixels or the encoded bitstream. Derived
/// once per output via [`EncodeParams::of`]; `encoder` is the **resolved**
/// encoder actually used (LLR-047 fallback applied), so a machine gaining or
/// losing a hardware encoder re-keys rather than mixing bitstreams.
// Implements: LLR-053, SR-037
#[derive(Debug, Clone, PartialEq)]
pub struct EncodeParams {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub quality_crf: u32,
    pub pic_display_time_secs: f32,
    pub fade_time_secs: f32,
    pub max_rotation_degrees: f32,
    pub zoom_amount: f32,
    pub ken_burns: bool,
    /// Resolved encoder name (SR-034 selection after fallback).
    pub encoder: String,
    /// libx264 preset (SR-035); software path only but always keyed for
    /// simplicity — a preset change on a hardware output re-keys harmlessly.
    pub x264_preset: String,
}

impl EncodeParams {
    /// Extract the key-relevant facts from an output definition and the
    /// resolved encoder choice. Uses [`OutputDef::even_dims`] — the dims that
    /// actually reach ffmpeg (SR-006).
    pub fn of(def: &OutputDef, resolved_encoder: &str) -> Self {
        let (width, height) = def.even_dims();
        Self {
            width,
            height,
            fps: def.fps,
            quality_crf: def.quality_crf,
            pic_display_time_secs: def.pic_display_time_secs,
            fade_time_secs: def.fade_time_secs,
            max_rotation_degrees: def.max_rotation_degrees,
            zoom_amount: def.zoom_amount,
            ken_burns: def.ken_burns,
            encoder: resolved_encoder.to_string(),
            x264_preset: def.x264_preset.clone(),
        }
    }
}

/// Everything that determines one clip-segment's bytes (the SR-037 key tuple):
/// the clip's source identity and applied ROI focus, both transition-neighbor
/// identities (`None` at album edges), the encode parameters, and the engine
/// version ([`engine_version`] in production; a parameter so sensitivity is
/// unit-testable).
// Implements: LLR-053, SR-037
#[derive(Debug, Clone)]
pub struct SegmentKeyInput<'a> {
    pub source: &'a SourceIdentity,
    /// The Ken Burns focus actually applied (ROI entry or configured default;
    /// `None` = default two-point pan). Changes pixels, so it re-keys.
    pub focus: Option<(f32, f32)>,
    pub left: Option<&'a SourceIdentity>,
    pub right: Option<&'a SourceIdentity>,
    pub params: &'a EncodeParams,
    pub engine: &'a str,
}

/// Derive the cache key for one clip's segment: SHA-256 hex over a canonical,
/// field-tagged serialization of [`SegmentKeyInput`]. Pure — identical input
/// yields an identical key; changing any tuple member yields a different key,
/// bounding invalidation to inserted items plus their two neighbors (SR-037).
// Implements: LLR-053, SR-037
pub fn segment_key(input: &SegmentKeyInput) -> String {
    let mut s = String::with_capacity(256);
    let _ = write!(s, "v={}|src=", input.engine);
    input.source.write_to(&mut s);
    let _ = write!(s, "|focus=");
    match input.focus {
        // `{}` on f32 prints the shortest round-trip form — stable per value.
        Some((x, y)) => {
            let _ = write!(s, "{x},{y}");
        }
        None => s.push('-'),
    }
    for (tag, neighbor) in [("|L=", input.left), ("|R=", input.right)] {
        s.push_str(tag);
        match neighbor {
            Some(n) => n.write_to(&mut s),
            None => s.push('-'),
        }
    }
    let p = input.params;
    let _ = write!(
        s,
        "|dim={}x{}|fps={}|crf={}|pic={}|fade={}|rot={}|zoom={}|kb={}|enc={}|preset={}",
        p.width,
        p.height,
        p.fps,
        p.quality_crf,
        p.pic_display_time_secs,
        p.fade_time_secs,
        p.max_rotation_degrees,
        p.zoom_amount,
        p.ken_burns,
        p.encoder,
        p.x264_preset,
    );
    hash_hex(s.as_bytes())
}

/// Key for a segment that (exceptionally) spans several clips — clips shorter
/// than the transition merge into their successor's segment because the mixer
/// never rolls on an empty tail (see `cache::planner`). The combined key
/// hashes the member keys in order; a single member returns its key unchanged,
/// so the common one-clip case is exactly [`segment_key`].
// Implements: LLR-053, LLR-055, SR-037
pub fn combine_keys(member_keys: &[String]) -> String {
    match member_keys {
        [one] => one.clone(),
        many => hash_hex(many.join("\x1e").as_bytes()),
    }
}

/// Identity key for the per-clip actual-frame-count record in the store index:
/// a video's decoded frame count depends on its content identity and the
/// output fps only (not dims/quality).
// Implements: LLR-055, SR-037
pub fn clip_frames_key(id: &SourceIdentity, fps: u32) -> String {
    let mut s = String::new();
    id.write_to(&mut s);
    let _ = write!(s, "\x1f{fps}");
    hash_hex(s.as_bytes())
}

/// SHA-256 of `bytes` as lowercase hex (shared by every cache key form).
fn hash_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_lower(&hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(path: &str, size: u64, mtime_ms: u64) -> SourceIdentity {
        SourceIdentity {
            path: path.into(),
            size,
            mtime_ms,
        }
    }

    fn params() -> EncodeParams {
        EncodeParams {
            width: 1920,
            height: 1080,
            fps: 30,
            quality_crf: 28,
            pic_display_time_secs: 6.0,
            fade_time_secs: 0.5,
            max_rotation_degrees: 15.0,
            zoom_amount: 0.12,
            ken_burns: true,
            encoder: "libx264".into(),
            x264_preset: "medium".into(),
        }
    }

    fn key_of(
        source: &SourceIdentity,
        focus: Option<(f32, f32)>,
        left: Option<&SourceIdentity>,
        right: Option<&SourceIdentity>,
        params: &EncodeParams,
        engine: &str,
    ) -> String {
        segment_key(&SegmentKeyInput {
            source,
            focus,
            left,
            right,
            params,
            engine,
        })
    }

    // Verifies: SR-037, LLR-053 (TC-075) — identical input yields an identical
    // key (pure, deterministic across calls).
    #[test]
    fn segment_key_stable_for_identical_input_sr037() {
        let src = identity("C:/lib/a.jpg", 1000, 111);
        let l = identity("C:/lib/prev.jpg", 900, 100);
        let r = identity("C:/lib/next.jpg", 1100, 120);
        let p = params();
        let k1 = key_of(&src, Some((0.5, 0.5)), Some(&l), Some(&r), &p, "0.1.0+f1");
        let k2 = key_of(&src, Some((0.5, 0.5)), Some(&l), Some(&r), &p, "0.1.0+f1");
        assert_eq!(k1, k2, "same input must yield the same key");
        assert_eq!(k1.len(), 64, "sha256 hex");
    }

    // Verifies: SR-037, LLR-053 (TC-075) — changing ANY member of the key
    // tuple (source identity, each encode-relevant OutputDef field, focus,
    // either neighbor identity incl. edge None vs Some, engine version) yields
    // a different key, bounding invalidation to inserted items + 2 neighbors.
    #[test]
    fn segment_key_changes_on_each_field_sr037() {
        let src = identity("C:/lib/a.jpg", 1000, 111);
        let l = identity("C:/lib/prev.jpg", 900, 100);
        let r = identity("C:/lib/next.jpg", 1100, 120);
        let p = params();
        let base = key_of(&src, Some((0.5, 0.5)), Some(&l), Some(&r), &p, "0.1.0+f1");

        // Source identity variations (TC-075: source-path/size/mtime).
        for varied in [
            identity("C:/lib/b.jpg", 1000, 111),
            identity("C:/lib/a.jpg", 1001, 111),
            identity("C:/lib/a.jpg", 1000, 112),
        ] {
            let k = key_of(
                &varied,
                Some((0.5, 0.5)),
                Some(&l),
                Some(&r),
                &p,
                "0.1.0+f1",
            );
            assert_ne!(k, base, "source change must re-key: {varied:?}");
        }

        // Focus variations (ROI focus actually applied changes pixels).
        for focus in [Some((0.25f32, 0.5f32)), Some((0.5, 0.75)), None] {
            let k = key_of(&src, focus, Some(&l), Some(&r), &p, "0.1.0+f1");
            assert_ne!(k, base, "focus change must re-key: {focus:?}");
        }

        // Neighbor variations: changed identity and edge None on each side.
        let other = identity("C:/lib/new.jpg", 5, 5);
        for (left, right) in [
            (Some(&other), Some(&r)),
            (None, Some(&r)),
            (Some(&l), Some(&other)),
            (Some(&l), None),
        ] {
            let k = key_of(&src, Some((0.5, 0.5)), left, right, &p, "0.1.0+f1");
            assert_ne!(k, base, "neighbor change must re-key: {left:?}/{right:?}");
        }

        // Each encode-relevant parameter (TC-075 vary set).
        type Vary = Box<dyn Fn(&mut EncodeParams)>;
        let variations: Vec<Vary> = vec![
            Box::new(|p| p.width = 1280),
            Box::new(|p| p.height = 720),
            Box::new(|p| p.fps = 24),
            Box::new(|p| p.quality_crf = 23),
            Box::new(|p| p.pic_display_time_secs = 5.0),
            Box::new(|p| p.fade_time_secs = 0.25),
            Box::new(|p| p.max_rotation_degrees = 0.0),
            Box::new(|p| p.zoom_amount = 0.2),
            Box::new(|p| p.ken_burns = false),
            Box::new(|p| p.encoder = "h264_nvenc".into()),
            Box::new(|p| p.x264_preset = "veryfast".into()),
        ];
        for (i, vary) in variations.iter().enumerate() {
            let mut varied = params();
            vary(&mut varied);
            assert_ne!(varied, p, "variation {i} must actually change a field");
            let k = key_of(
                &src,
                Some((0.5, 0.5)),
                Some(&l),
                Some(&r),
                &varied,
                "0.1.0+f1",
            );
            assert_ne!(k, base, "param variation {i} must re-key");
        }

        // Engine version (crate version or CACHE_FORMAT_VERSION bump).
        let k = key_of(&src, Some((0.5, 0.5)), Some(&l), Some(&r), &p, "0.1.0+f2");
        assert_ne!(k, base, "engine version change must re-key");
    }

    // Verifies: SR-037, LLR-053, LLR-055 — the multi-clip combined key is the
    // member key itself for one member (common case) and order-sensitive for
    // several; the engine_version string carries the bumpable format const.
    #[test]
    fn combine_keys_and_engine_version_sr037() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        assert_eq!(combine_keys(std::slice::from_ref(&a)), a);
        let ab = combine_keys(&[a.clone(), b.clone()]);
        let ba = combine_keys(&[b, a]);
        assert_ne!(ab, ba, "member order must matter");
        assert_eq!(ab.len(), 64);

        let v = engine_version();
        assert!(v.contains(env!("CARGO_PKG_VERSION")));
        assert!(v.ends_with(&format!("+f{CACHE_FORMAT_VERSION}")));
    }

    // Verifies: SR-037, LLR-055 — clip frame counts are keyed by content
    // identity AND output fps (a 24fps and 30fps decode differ).
    #[test]
    fn clip_frames_key_includes_fps_sr037() {
        let id = identity("C:/lib/v.mp4", 10, 20);
        assert_ne!(clip_frames_key(&id, 24), clip_frames_key(&id, 30));
        assert_eq!(clip_frames_key(&id, 30), clip_frames_key(&id, 30));
    }
}
