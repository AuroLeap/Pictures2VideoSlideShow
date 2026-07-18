//! Media-scan probe cache (SR-038): JSON persistence of per-file probe
//! results keyed by file identity (path + size + mtime), so a warm rescan of
//! an unchanged library serves scan results with zero ffprobe subprocesses or
//! image-header decodes. One cache file per `media_root`, in the per-user
//! cache dir — never under `media_root` (SR-010) or beside outputs.

use crate::error::{Result, SlideshowError};
use crate::media::MediaType;
use crate::util::file_utils::{app_cache_root, hex_lower};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One cached probe result. `size`/`mtime_ms` are the identity the entry was
/// probed at; the rest is everything a [`crate::media::MediaFile`] needs, so
/// a hit rebuilds the scan result without touching the file's contents.
// Implements: LLR-058, SR-038
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeEntry {
    /// File size in bytes at probe time.
    pub size: u64,
    /// File modification time, epoch milliseconds, at probe time.
    pub mtime_ms: u64,
    /// Image or video (revalidated against the extension on lookup).
    pub kind: MediaType,
    /// Pixel dimensions (width, height).
    pub dims: (u32, u32),
    /// Video duration in seconds (`None` for images/unprobeable).
    pub duration_secs: Option<f32>,
    /// Whether a decodable audio stream exists (always false for images).
    pub has_audio: bool,
}

/// Pure hit decision (the TC-083 matrix): a cached entry is current only when
/// BOTH the size and the mtime match the file's present identity — either
/// changing forces a re-probe (SR-038 changed-size/changed-mtime legs).
// Implements: LLR-059, SR-038
pub fn entry_current(entry: &ProbeEntry, size: u64, mtime_ms: u64) -> bool {
    entry.size == size && entry.mtime_ms == mtime_ms
}

/// The per-`media_root` probe cache: relative path (forward slashes) →
/// [`ProbeEntry`]. `BTreeMap` keeps the JSON stable/diffable.
// Implements: LLR-058, SR-038
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProbeCache {
    entries: BTreeMap<String, ProbeEntry>,
}

impl ProbeCache {
    /// The cache file for `media_root`: `<app cache root>/probe-cache/
    /// <sha256(root)[..16]>.json`. Keyed by a hash of the (canonicalized when
    /// possible) root path so distinct libraries never share a file and the
    /// name is filesystem-safe.
    // Implements: LLR-058, SR-038
    pub fn path_for(media_root: &Path) -> PathBuf {
        let canon = std::fs::canonicalize(media_root).unwrap_or_else(|_| media_root.to_path_buf());
        let mut hasher = Sha256::new();
        hasher.update(canon.to_string_lossy().as_bytes());
        let digest = hasher.finalize();
        app_cache_root()
            .join("probe-cache")
            .join(format!("{}.json", &hex_lower(&digest)[..16]))
    }

    /// Load the cache at `path`. A missing, unreadable, or corrupt file
    /// (including corrupt entries — JSON is parsed whole) yields an empty
    /// cache: a cold scan, never an error (SR-038 corrupt-entry leg).
    // Implements: LLR-058, SR-038
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Atomically persist the cache: write `<path>.tmp`, then rename over
    /// `path` (replace-existing on Windows too), so a killed save never
    /// leaves a corrupt cache — at worst the old file survives.
    // Implements: LLR-058, SR-038
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                SlideshowError::Processing(format!(
                    "cannot create probe-cache dir {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string(self)
            .map_err(|e| SlideshowError::Processing(format!("probe-cache serialize: {}", e)))?;
        std::fs::write(&tmp, body)
            .and_then(|_| std::fs::rename(&tmp, path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                SlideshowError::Processing(format!(
                    "cannot write probe cache {}: {}",
                    path.display(),
                    e
                ))
            })
    }

    /// The current entry for `rel`, or `None` when absent, stale
    /// ([`entry_current`]), or of the wrong kind (a corrupt/misclassified
    /// entry is a miss, never trusted).
    // Implements: LLR-059, SR-038
    pub fn lookup(
        &self,
        rel: &str,
        kind: MediaType,
        size: u64,
        mtime_ms: u64,
    ) -> Option<&ProbeEntry> {
        self.entries
            .get(rel)
            .filter(|e| e.kind == kind && entry_current(e, size, mtime_ms))
    }

    /// Record `entry` for `rel` (insert or replace).
    pub fn insert(&mut self, rel: String, entry: ProbeEntry) {
        self.entries.insert(rel, entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(size: u64, mtime_ms: u64) -> ProbeEntry {
        ProbeEntry {
            size,
            mtime_ms,
            kind: MediaType::Image,
            dims: (96, 72),
            duration_secs: None,
            has_audio: false,
        }
    }

    // Verifies: SR-038, LLR-059 (TC-083) — the pure hit decision across the
    // match matrix: both-match -> hit; size-differs, mtime-differs, and
    // both-differ -> re-probe.
    #[test]
    fn entry_current_requires_size_and_mtime_sr038() {
        let e = entry(1000, 5000);
        assert!(entry_current(&e, 1000, 5000), "both match -> hit");
        assert!(!entry_current(&e, 1001, 5000), "size differs -> miss");
        assert!(!entry_current(&e, 1000, 5001), "mtime differs -> miss");
        assert!(!entry_current(&e, 999, 4999), "both differ -> miss");
    }

    // Verifies: SR-038, LLR-059 — lookup layers the kind revalidation on top
    // of entry_current: a wrong-kind (corrupt/misclassified) entry is a miss.
    #[test]
    fn lookup_rejects_wrong_kind_sr038() {
        let mut c = ProbeCache::default();
        c.insert("a.png".into(), entry(10, 20));
        assert!(c.lookup("a.png", MediaType::Image, 10, 20).is_some());
        assert!(c.lookup("a.png", MediaType::Video, 10, 20).is_none());
        assert!(c.lookup("a.png", MediaType::Image, 11, 20).is_none());
        assert!(c.lookup("missing.png", MediaType::Image, 10, 20).is_none());
    }

    // Verifies: SR-038, LLR-058 (TC-084, location leg) — the cache file lives
    // under the per-user cache root (never under media_root), is keyed by a
    // hash of the root path, and distinct roots get distinct files.
    #[test]
    fn path_for_is_per_user_and_keyed_by_root_sr038() {
        let a = ProbeCache::path_for(Path::new("C:/lib/photos"));
        let b = ProbeCache::path_for(Path::new("C:/lib/other"));
        assert_ne!(a, b, "distinct roots -> distinct cache files");
        assert!(a.starts_with(app_cache_root()), "under the per-user root");
        assert!(!a.starts_with("C:/lib/photos"), "never under media_root");
        assert_eq!(a.extension().unwrap(), "json");
    }
}
