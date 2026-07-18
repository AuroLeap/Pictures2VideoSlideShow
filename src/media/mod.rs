//! Media scanning and indexing: walk the input directory in parallel, classify
//! files as image/video, extract dimensions (and video duration), and apply the
//! configured ignore patterns. Probe results are served from the SR-038 probe
//! cache when the file's identity (size+mtime) is unchanged.

pub mod probe_cache;

use crate::config::InputConfig;
use crate::error::Result;
use probe_cache::{ProbeCache, ProbeEntry};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaFile {
    pub path: PathBuf,
    pub file_type: MediaType,
    pub dimensions: (u32, u32),
    pub duration_secs: Option<f32>,
    pub size_bytes: u64,
    /// File modification time (epoch ms) at scan time — with `size_bytes` the
    /// file's cache identity (SR-037 segment keys, SR-038 probe entries).
    // Implements: LLR-053, SR-037
    #[serde(default)]
    pub mtime_ms: u64,
    /// Whether the file carries a decodable audio stream (always `false` for
    /// images). Drives audio passthrough: only audio-bearing video clips
    /// contribute sound to the slideshow.
    // Implements: LLR-039, SR-032
    #[serde(default)]
    pub has_audio: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MediaType {
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "video")]
    Video,
}

/// How the scan's probe work split between the SR-038 cache and real probes —
/// the observable evidence that a warm scan of an unchanged library ran zero
/// probe subprocesses/header decodes (TC-085).
// Implements: LLR-059, SR-038
#[derive(Debug, Clone, Copy, Default)]
pub struct ProbeStats {
    /// Files whose scan result was served from a current cache entry.
    pub cache_hits: usize,
    /// Files actually probed (ffprobe / image-header decode) this scan.
    pub probed: usize,
}

#[derive(Debug, Clone)]
pub struct Album {
    pub media_files: Vec<MediaFile>,
    pub total_size: u64,
    #[allow(dead_code)] // recorded for provenance; not yet surfaced
    pub created_at: SystemTime,
    /// Cache-hit vs probed split for this scan (SR-038).
    pub probe_stats: ProbeStats,
}

pub struct MediaLoader {
    config: InputConfig,
    /// Override for the probe-cache file (tests / bench isolation). `None` =
    /// the per-user default, [`ProbeCache::path_for`]`(media_root)`.
    // Implements: LLR-058, SR-038
    probe_cache_path: Option<PathBuf>,
}

impl MediaLoader {
    pub fn new(config: InputConfig) -> Self {
        Self {
            config,
            probe_cache_path: None,
        }
    }

    /// Pin the probe-cache file to an explicit path instead of the per-user
    /// default — used by tests and the bench runner to isolate cache state.
    // Implements: LLR-058, SR-038
    pub fn with_probe_cache_path(mut self, path: PathBuf) -> Self {
        self.probe_cache_path = Some(path);
        self
    }

    /// Scan and index the input tree. The walk is collected first, then files
    /// are probed in parallel with rayon — consulting the SR-038 probe cache
    /// before spawning any probe, and refreshing it afterwards (a cache save
    /// failure is logged, never fails the scan).
    // Implements: LLR-059, LLR-058, SR-038
    pub fn scan_and_index(&self) -> Result<Album> {
        log::info!(
            "Starting media scan from: {}",
            self.config.media_root.display()
        );

        let cache_path = self
            .probe_cache_path
            .clone()
            .unwrap_or_else(|| ProbeCache::path_for(&self.config.media_root));
        let cache = ProbeCache::load(&cache_path);

        let paths: Vec<PathBuf> = walkdir::WalkDir::new(&self.config.media_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            // Ignore filtering runs BEFORE any cache lookup (SR-018 identical
            // warm and cold). Implements: LLR-021, LLR-059
            .filter(|p| !self.is_ignored(p))
            .collect();

        let mut scanned: Vec<(MediaFile, String, ProbeEntry, bool)> = paths
            .par_iter()
            .filter_map(|p| {
                let rel = self.rel_key(p);
                match process_file(p, &cache, &rel) {
                    Ok(Some(hit)) => Some(hit),
                    Ok(None) => None,
                    // Skip semantics identical warm and cold (SR-014): a
                    // failed probe is logged and skipped, never cached.
                    Err(e) => {
                        log::warn!("Skipping {}: {}", p.display(), e);
                        None
                    }
                }
            })
            .collect();

        // Stable, deterministic order (by path).
        scanned.sort_by(|a, b| a.0.path.cmp(&b.0.path));

        // Refresh the cache with every scanned file's current entry (hits
        // carried over, misses replaced) and persist atomically.
        let mut probe_stats = ProbeStats::default();
        let mut fresh = ProbeCache::default();
        let mut media_files = Vec::with_capacity(scanned.len());
        for (mf, rel, entry, was_hit) in scanned {
            if was_hit {
                probe_stats.cache_hits += 1;
            } else {
                probe_stats.probed += 1;
            }
            fresh.insert(rel, entry);
            media_files.push(mf);
        }
        if let Err(e) = fresh.save(&cache_path) {
            log::warn!("Probe cache not saved (scan unaffected): {}", e);
        }

        let total_size = media_files.iter().map(|m| m.size_bytes).sum();

        log::info!(
            "Media scan complete: {} files, {} bytes ({} cached, {} probed)",
            media_files.len(),
            total_size,
            probe_stats.cache_hits,
            probe_stats.probed,
        );

        Ok(Album {
            media_files,
            total_size,
            created_at: SystemTime::now(),
            probe_stats,
        })
    }

    /// The probe-cache key for `path`: its media_root-relative path with
    /// forward slashes (stable across scans of the same root).
    // Implements: LLR-058, SR-038
    fn rel_key(&self, path: &Path) -> String {
        path.strip_prefix(&self.config.media_root)
            .unwrap_or(path)
            .display()
            .to_string()
            .replace('\\', "/")
    }

    /// A file is ignored if any configured pattern appears (case-insensitively)
    /// anywhere in its path **relative to `media_root`** — so a pattern can
    /// exclude a whole folder (SR-018 `target={file,folder}`), while a pattern
    /// that happens to occur in the media root's own path never wipes the album.
    // Implements: SR-018, LLR-021
    fn is_ignored(&self, path: &Path) -> bool {
        let rel = path.strip_prefix(&self.config.media_root).unwrap_or(path);
        let hay = rel.to_string_lossy().to_lowercase();
        self.config
            .ignore_patterns
            .iter()
            .any(|pat| !pat.is_empty() && hay.contains(&pat.to_lowercase()))
    }
}

/// Classify by extension (cheap, never cached), then either serve the probe
/// result from a current cache entry — zero subprocesses/header decodes — or
/// probe the file and build a fresh entry. Returns
/// `(media file, cache key, entry, served-from-cache)`.
// Implements: LLR-059, SR-038
fn process_file(
    path: &Path,
    cache: &ProbeCache,
    rel: &str,
) -> Result<Option<(MediaFile, String, ProbeEntry, bool)>> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    let file_type = match extension.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "tiff" => MediaType::Image,
        "mp4" | "avi" | "mov" | "mkv" | "webm" | "flv" | "wmv" => MediaType::Video,
        _ => return Ok(None),
    };

    let metadata = std::fs::metadata(path)?;
    let size = metadata.len();
    let mtime_ms = mtime_epoch_ms(&metadata);

    // Cache hit: rebuild the scan result from the entry, probing nothing.
    if let Some(entry) = cache.lookup(rel, file_type, size, mtime_ms) {
        let mf = MediaFile {
            path: path.to_path_buf(),
            file_type,
            dimensions: entry.dims,
            duration_secs: entry.duration_secs,
            size_bytes: size,
            mtime_ms,
            has_audio: entry.has_audio,
        };
        return Ok(Some((mf, rel.to_string(), entry.clone(), true)));
    }

    // Miss (new / changed / corrupt entry): probe as always and refresh.
    let (dimensions, duration_secs, has_audio) = match file_type {
        MediaType::Image => (image::image_dimensions(path)?, None, false),
        MediaType::Video => {
            let (dims, dur) = probe_video(path)?;
            (dims, dur, probe_has_audio(path))
        }
    };

    let entry = ProbeEntry {
        size,
        mtime_ms,
        kind: file_type,
        dims: dimensions,
        duration_secs,
        has_audio,
    };
    let mf = MediaFile {
        path: path.to_path_buf(),
        file_type,
        dimensions,
        duration_secs,
        size_bytes: size,
        mtime_ms,
        has_audio,
    };
    Ok(Some((mf, rel.to_string(), entry, false)))
}

/// A file's modification time as epoch milliseconds (0 when unavailable —
/// consistently, so the identity check still behaves deterministically).
// Implements: LLR-059, SR-038
fn mtime_epoch_ms(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Whether `path` has at least one audio stream, via `ffprobe`. Used to decide
/// which video clips contribute audio (a probe failure is treated as no audio
/// so a quirky file can never break the build).
// Implements: LLR-039, SR-032
fn probe_has_audio(path: &Path) -> bool {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().contains("audio"),
        _ => false,
    }
}

/// Probe a video's dimensions and duration via `ffprobe`.
fn probe_video(path: &Path) -> Result<((u32, u32), Option<f32>)> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height:format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Ok(((0, 0), None)),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let w = lines.next().and_then(|s| s.trim().parse::<u32>().ok());
    let h = lines.next().and_then(|s| s.trim().parse::<u32>().ok());
    let dur = lines.next().and_then(|s| s.trim().parse::<f32>().ok());

    Ok(((w.unwrap_or(0), h.unwrap_or(0)), dur))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loader(patterns: &[&str]) -> MediaLoader {
        MediaLoader::new(InputConfig {
            media_root: PathBuf::from("."),
            ignore_patterns: patterns.iter().map(|s| s.to_string()).collect(),
            exception_pattern: None,
            exception_threshold: None,
            roi_db: None,
        })
    }

    // Verifies: SR-018, LLR-021 — case-insensitive substring ignore match.
    #[test]
    fn ignore_pattern_matches_case_insensitively_sr018() {
        let l = loader(&["DNP"]);
        assert!(l.is_ignored(&PathBuf::from("/a/b/Family_DNP.jpg")));
        assert!(l.is_ignored(&PathBuf::from("/a/b/family_dnp.jpg")));
    }

    // Verifies: SR-018, LLR-021 — a pattern naming a FOLDER excludes the files
    // inside it (path match, not just file-name match), while the media root's
    // own path is never matched against patterns.
    #[test]
    fn ignore_pattern_matches_folders_sr018() {
        let mut l = loader(&["private"]);
        l.config.media_root = PathBuf::from("/media/root");
        // File inside an ignored folder is excluded even though its own name
        // does not contain the pattern.
        assert!(l.is_ignored(&PathBuf::from("/media/root/Private Album/img1.jpg")));
        // The pattern occurring only in the media root itself must NOT match.
        let mut r = loader(&["root"]);
        r.config.media_root = PathBuf::from("/media/root");
        assert!(!r.is_ignored(&PathBuf::from("/media/root/holiday/img2.jpg")));
    }

    // Verifies: SR-018, LLR-021 — non-matching files are not ignored; empty
    // patterns never match.
    #[test]
    fn non_matching_files_kept_sr018() {
        let l = loader(&["DNP"]);
        assert!(!l.is_ignored(&PathBuf::from("/a/b/vacation.jpg")));
        let none = loader(&[]);
        assert!(!none.is_ignored(&PathBuf::from("/a/b/DNP.jpg")));
        let empty = loader(&[""]);
        assert!(!empty.is_ignored(&PathBuf::from("/a/b/anything.jpg")));
    }
}
