//! Shared helpers for the OBJ3 Wave-1 hardening integration tests.
//!
//! These tests synthesize tiny PNGs (via the `image` crate, already a
//! dependency) into a unique temp directory, drive the public library API or
//! the built `slideshow` binary, and clean up after themselves. Nothing here
//! touches the developer's real media.
//!
//! This module is compiled into each integration-test crate, but any single
//! crate uses only a subset of the helpers — silence the resulting dead-code
//! lint rather than per-item gating.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A unique, self-cleaning temp directory rooted under the OS temp dir.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Create a fresh unique directory `slideshow_it_<label>_<pid>_<n>`.
    pub fn new(label: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("slideshow_it_{label}_{pid}_{n}_{nanos}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        Self { path: dir }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        self.path.join(rel)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best-effort cleanup; a leaked temp dir must never fail a test.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Write a tiny solid-color PNG of `w`x`h` to `path`. Deterministic content.
pub fn write_png(path: &Path, w: u32, h: u32, rgb: [u8; 3]) {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
    img.save(path).expect("save png");
}

/// Absolute path to the integration-test target binary built by cargo.
///
/// Cargo sets `CARGO_BIN_EXE_<name>` for integration tests; the bin is
/// `slideshow` (see Cargo.toml `[[bin]] name = "slideshow"`).
pub fn slideshow_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_slideshow"))
}

/// Write a minimal valid TOML config with a single output definition.
/// `media_root` and `base_dir` are written as absolute paths.
pub fn write_config(
    config_path: &Path,
    media_root: &Path,
    base_dir: &Path,
    output_name: &str,
    width: u32,
    height: u32,
) {
    // Use forward slashes; TOML treats backslashes as escapes inside basic
    // strings, so normalize to avoid escaping headaches on Windows.
    let media = media_root.display().to_string().replace('\\', "/");
    let base = base_dir.display().to_string().replace('\\', "/");
    let toml = format!(
        r#"[input]
media_root = "{media}"
ignore_patterns = []

[output]
base_dir = "{base}"

[processing]
use_parallelism = true
dry_run = false
verbose = false
ffmpeg_timeout_secs = 120

[[outputs]]
name = "{output_name}"
width = {width}
height = {height}
fps = 24
pic_display_time_secs = 1.0
fade_time_secs = 0.2
max_rotation_degrees = 5.0
bulk_video_time_min = 20
quality_crf = 30
enable_audio = false
"#
    );
    fs::write(config_path, toml).expect("write config");
}

/// Hash-ish fingerprint of a file: (len, mtime-nanos, full byte content hash).
/// Sufficient to prove a source file is byte-for-byte unchanged.
pub fn fingerprint(path: &Path) -> (u64, u128, u64) {
    let meta = fs::metadata(path).expect("metadata");
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let bytes = fs::read(path).expect("read file");
    // FNV-1a 64-bit content hash — deterministic, no extra deps.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in &bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    (meta.len(), mtime, h)
}

/// Snapshot fingerprints of every file under `dir` (recursively), keyed by the
/// path relative to `dir`.
pub fn snapshot_tree(dir: &Path) -> std::collections::BTreeMap<PathBuf, (u64, u128, u64)> {
    let mut map = std::collections::BTreeMap::new();
    fn walk(
        base: &Path,
        cur: &Path,
        map: &mut std::collections::BTreeMap<PathBuf, (u64, u128, u64)>,
    ) {
        for entry in fs::read_dir(cur).expect("read_dir") {
            let entry = entry.expect("dir entry");
            let p = entry.path();
            if p.is_dir() {
                walk(base, &p, map);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_path_buf();
                map.insert(rel, fingerprint(&p));
            }
        }
    }
    walk(dir, dir, &mut map);
    map
}
