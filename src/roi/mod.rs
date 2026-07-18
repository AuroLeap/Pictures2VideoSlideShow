//! Optional region-of-interest (ROI) database.
//!
//! A single JSON file maps an image's path (relative to the input media root)
//! to a focus point, which anchors the Ken Burns zoom on e.g. a face or feature
//! located by prior recognition. Without an entry an image keeps the default
//! pan/zoom. Each value is either an explicit normalized point or a normalized
//! bounding box (whose center becomes the focus):
//!
//! ```json
//! {
//!   "2015/Trip/beach.jpg": { "x": 0.42, "y": 0.55 },
//!   "2016/Party/cake.jpg": { "bbox": [0.30, 0.20, 0.25, 0.25] }
//! }
//! ```
// Implements: SR-031, LLR-038

use crate::error::{Result, SlideshowError};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// One ROI entry: either an explicit normalized focus point, or a normalized
/// bounding box `[x, y, w, h]` whose center becomes the focus point. All values
/// are fractions in `[0,1]` of image width/height.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RoiEntry {
    Point { x: f32, y: f32 },
    Bbox { bbox: [f32; 4] },
}

impl RoiEntry {
    /// Resolve to a focus point clamped to `[0,1]`.
    fn focus(&self) -> (f32, f32) {
        let (x, y) = match self {
            RoiEntry::Point { x, y } => (*x, *y),
            RoiEntry::Bbox { bbox } => (bbox[0] + bbox[2] / 2.0, bbox[1] + bbox[3] / 2.0),
        };
        (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
    }
}

/// In-memory ROI lookup keyed by normalized relative path.
#[derive(Debug, Clone, Default)]
pub struct RoiDb {
    focuses: HashMap<String, (f32, f32)>,
}

impl RoiDb {
    /// Load and parse a JSON ROI map from `path`.
    // Implements: LLR-038, SR-031
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            SlideshowError::Config(format!(
                "Failed to read ROI database {}: {}",
                path.display(),
                e
            ))
        })?;
        Self::from_json(&content)
    }

    /// Parse a JSON ROI map from a string (split out for testability).
    pub fn from_json(json: &str) -> Result<Self> {
        let raw: HashMap<String, RoiEntry> = serde_json::from_str(json)
            .map_err(|e| SlideshowError::Config(format!("ROI database JSON parse error: {}", e)))?;
        let focuses = raw
            .into_iter()
            .map(|(k, v)| (normalize_key(&k), v.focus()))
            .collect();
        Ok(Self { focuses })
    }

    /// Focus point for an image identified by its path relative to the media
    /// root. Returns `None` when the image has no ROI entry.
    pub fn focus_for(&self, rel_path: &Path) -> Option<(f32, f32)> {
        let key = normalize_key(&rel_path.to_string_lossy());
        self.focuses.get(&key).copied()
    }

    /// Number of entries loaded.
    pub fn len(&self) -> usize {
        self.focuses.len()
    }

    // Present to satisfy clippy's `len_without_is_empty`; not used by the binary.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.focuses.is_empty()
    }
}

/// Normalize a path key so Windows `\`, a leading `./`, and case differences do
/// not prevent a match against the JSON map's `/`-separated keys. (The tool is
/// Windows-targeted, where paths are case-insensitive.)
fn normalize_key(s: &str) -> String {
    let s = s.replace('\\', "/");
    let trimmed = s.strip_prefix("./").unwrap_or(&s).trim_start_matches('/');
    trimmed.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Verifies: LLR-038 — point + bbox entries parse and resolve to focuses.
    #[test]
    fn parses_point_and_bbox_entries() {
        let json = r#"{
            "a/img1.jpg": { "x": 0.4, "y": 0.6 },
            "a/img2.jpg": { "bbox": [0.2, 0.2, 0.4, 0.4] }
        }"#;
        let db = RoiDb::from_json(json).unwrap();
        assert_eq!(db.len(), 2);
        let p = db.focus_for(&PathBuf::from("a/img1.jpg")).unwrap();
        assert!((p.0 - 0.4).abs() < 1e-6 && (p.1 - 0.6).abs() < 1e-6);
        // bbox center = (0.2 + 0.4/2, 0.2 + 0.4/2) = (0.4, 0.4)
        let b = db.focus_for(&PathBuf::from("a/img2.jpg")).unwrap();
        assert!((b.0 - 0.4).abs() < 1e-6 && (b.1 - 0.4).abs() < 1e-6);
    }

    // Verifies: LLR-038 — Windows backslash paths match forward-slash JSON keys.
    #[test]
    fn matches_across_path_separators_and_case() {
        let json = r#"{ "2015/Trip/Beach.JPG": { "x": 0.5, "y": 0.5 } }"#;
        let db = RoiDb::from_json(json).unwrap();
        // Backslashes + different case still resolve.
        assert!(db
            .focus_for(&PathBuf::from(r"2015\Trip\beach.jpg"))
            .is_some());
        assert!(db.focus_for(&PathBuf::from("missing.jpg")).is_none());
    }

    // Verifies: LLR-038 — out-of-range coordinates are clamped to [0,1].
    #[test]
    fn clamps_out_of_range_coordinates() {
        let json = r#"{ "x.jpg": { "x": 1.5, "y": -0.2 } }"#;
        let db = RoiDb::from_json(json).unwrap();
        let p = db.focus_for(&PathBuf::from("x.jpg")).unwrap();
        assert_eq!(p, (1.0, 0.0));
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(RoiDb::from_json("not json").is_err());
    }

    // Verifies: LLR-038 — load() reads and parses a JSON file from disk; a
    // missing path is a plain error (not a silent empty db).
    #[test]
    fn load_reads_file_and_reports_missing() {
        let dir = std::env::temp_dir().join(format!("slideshow_roi_load_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("focus.json");
        std::fs::write(&path, r#"{ "a/img.jpg": { "x": 0.5, "y": 0.5 } }"#).unwrap();
        let db = RoiDb::load(&path).unwrap();
        assert!(!db.is_empty() && db.len() == 1);
        assert!(db.focus_for(&PathBuf::from("a/img.jpg")).is_some());
        // A non-existent path is a hard error.
        assert!(RoiDb::load(&dir.join("nope.json")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
