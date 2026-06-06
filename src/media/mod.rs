//! Media scanning and indexing: walk the input directory in parallel, classify
//! files as image/video, extract dimensions (and video duration), and apply the
//! configured ignore patterns.

use crate::config::InputConfig;
use crate::error::Result;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    pub path: PathBuf,
    pub file_type: MediaType,
    pub dimensions: (u32, u32),
    pub duration_secs: Option<f32>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MediaType {
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "video")]
    Video,
}

#[derive(Debug, Clone)]
pub struct Album {
    pub media_files: Vec<MediaFile>,
    pub total_size: u64,
    #[allow(dead_code)] // recorded for provenance; not yet surfaced
    pub created_at: SystemTime,
}

pub struct MediaLoader {
    config: InputConfig,
}

impl MediaLoader {
    pub fn new(config: InputConfig) -> Self {
        Self { config }
    }

    /// Scan and index the input tree. The walk is collected first, then files
    /// are probed in parallel with rayon.
    pub async fn scan_and_index(&self) -> Result<Album> {
        log::info!(
            "Starting media scan from: {}",
            self.config.media_root.display()
        );

        let paths: Vec<PathBuf> = walkdir::WalkDir::new(&self.config.media_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .filter(|p| !self.is_ignored(p))
            .collect();

        let mut media_files: Vec<MediaFile> = paths
            .par_iter()
            .filter_map(|p| match process_file(p) {
                Ok(Some(mf)) => Some(mf),
                Ok(None) => None,
                Err(e) => {
                    log::warn!("Skipping {}: {}", p.display(), e);
                    None
                }
            })
            .collect();

        // Stable, deterministic order (by path).
        media_files.sort_by(|a, b| a.path.cmp(&b.path));

        let total_size = media_files.iter().map(|m| m.size_bytes).sum();

        log::info!(
            "Media scan complete: {} files, {} bytes",
            media_files.len(),
            total_size
        );

        Ok(Album {
            media_files,
            total_size,
            created_at: SystemTime::now(),
        })
    }

    /// A file is ignored if any configured pattern appears (case-insensitively)
    /// in its file name.
    fn is_ignored(&self, path: &Path) -> bool {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        self.config
            .ignore_patterns
            .iter()
            .any(|pat| !pat.is_empty() && name.contains(&pat.to_lowercase()))
    }
}

fn process_file(path: &Path) -> Result<Option<MediaFile>> {
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

    let (dimensions, duration_secs) = match file_type {
        MediaType::Image => (image::image_dimensions(path)?, None),
        MediaType::Video => probe_video(path)?,
    };

    Ok(Some(MediaFile {
        path: path.to_path_buf(),
        file_type,
        dimensions,
        duration_secs,
        size_bytes: metadata.len(),
    }))
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
