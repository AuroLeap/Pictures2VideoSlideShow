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

/// Synthesize a tiny test video at `path` via ffmpeg's `testsrc` (silent).
/// Returns true on success so callers can give a clear failure message if
/// ffmpeg genuinely cannot synth (it should — CI invariant).
pub fn synth_video(path: &Path, seconds: u32, w: u32, h: u32, rate: u32) -> bool {
    let lavfi = format!("testsrc=duration={seconds}:size={w}x{h}:rate={rate}");
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg(&lavfi)
        .args(["-pix_fmt", "yuv420p"])
        .arg(path)
        .status();
    matches!(status, Ok(s) if s.success()) && path.exists()
}

/// Absolute path to the integration-test target binary built by cargo.
///
/// Cargo sets `CARGO_BIN_EXE_<name>` for integration tests; the bin is
/// `make_video_slideshow` (see Cargo.toml `[[bin]] name = "make_video_slideshow"`).
pub fn slideshow_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_make_video_slideshow"))
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
temp_dir = "{base}/cache"
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

/// Assert `mp4` satisfies the SR-005 frame-compatible profile via ffprobe:
/// codec h264, pix_fmt yuv420p, even dimensions, fps in 24..=30, and
/// faststart (moov atom before mdat). Shared by the encoder-permutation tests
/// (TC-068) and the fallback test (TC-067) so the profile check lives once.
// Verifies: SR-005
pub fn assert_sr005_profile(mp4: &Path) {
    let probe = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,pix_fmt,r_frame_rate",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(mp4)
        .output()
        .expect("ffprobe");
    let text = String::from_utf8_lossy(&probe.stdout);
    let mut lines = text.lines();
    let codec = lines.next().unwrap_or("").trim().to_string();
    let width: u32 = lines.next().unwrap_or("0").trim().parse().unwrap_or(0);
    let height: u32 = lines.next().unwrap_or("0").trim().parse().unwrap_or(0);
    let pix_fmt = lines.next().unwrap_or("").trim().to_string();
    let rate = lines.next().unwrap_or("").trim().to_string();
    let fps: u32 = rate.split('/').next().unwrap_or("0").parse().unwrap_or(0);

    assert_eq!(codec, "h264", "codec for {}", mp4.display());
    assert_eq!(pix_fmt, "yuv420p", "pixel format for {}", mp4.display());
    assert_eq!(width % 2, 0, "even width for {}", mp4.display());
    assert_eq!(height % 2, 0, "even height for {}", mp4.display());
    assert!(
        (24..=30).contains(&fps),
        "fps {fps} in 24..=30 for {}",
        mp4.display()
    );

    let bytes = fs::read(mp4).expect("read mp4");
    let moov = bytes.windows(4).position(|w| w == b"moov");
    let mdat = bytes.windows(4).position(|w| w == b"mdat");
    assert!(
        matches!((moov, mdat), (Some(m), Some(d)) if m < d),
        "faststart: moov ({moov:?}) should precede mdat ({mdat:?}) in {}",
        mp4.display()
    );
}

/// Deterministic per-pixel noise PNG (xorshift): adjacent output frames of a
/// Ken Burns pan differ strongly, so a one-frame drop/dup or a stale cached
/// segment shows up as a large pixel delta instead of hiding in flat color.
/// Shared by the SR-037 suites (concat seams, cache scenarios).
pub fn write_noise_png(path: &Path, w: u32, h: u32, seed: u32) {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let mut img = image::RgbImage::new(w, h);
    for p in img.pixels_mut() {
        let v = next();
        *p = image::Rgb([
            (v & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            ((v >> 16) & 0xFF) as u8,
        ]);
    }
    img.save(path).expect("save noise png");
}

/// A `MediaFile` image entry for an on-disk file, with its real size/mtime
/// identity (the SR-037 cache key inputs) and header dimensions.
pub fn image_media_file(path: &Path) -> slideshow_core::media::MediaFile {
    let meta = fs::metadata(path).expect("image metadata");
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let dims = image::image_dimensions(path).expect("image dimensions");
    slideshow_core::media::MediaFile {
        path: path.to_path_buf(),
        file_type: slideshow_core::media::MediaType::Image,
        dimensions: dims,
        duration_secs: None,
        size_bytes: meta.len(),
        mtime_ms,
        has_audio: false,
    }
}

/// Decode every frame of `mp4` to raw rgb24 buffers of `w`x`h` via ffmpeg.
/// Shared by the SR-037 equivalence suites (TC-079 concat seams, TC-081 cache
/// scenarios) so the cold-vs-warm comparison lives once.
// Verifies: SR-037 (equivalence legs)
pub fn decode_frames(mp4: &Path, w: u32, h: u32) -> Vec<Vec<u8>> {
    let out = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(mp4)
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"])
        .output()
        .expect("run ffmpeg decode");
    assert!(
        out.status.success(),
        "ffmpeg decode of {} failed: {}",
        mp4.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let frame_bytes = (w * h * 3) as usize;
    assert_eq!(
        out.stdout.len() % frame_bytes,
        0,
        "decoded byte count of {} is not a whole number of {w}x{h} frames",
        mp4.display(),
    );
    out.stdout.chunks(frame_bytes).map(|c| c.to_vec()).collect()
}

/// Container duration in seconds via ffprobe (SR-037 equivalence legs).
pub fn probe_duration(mp4: &Path) -> f64 {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(mp4)
        .output()
        .expect("run ffprobe");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("parse duration")
}

/// Mean absolute per-byte difference between two rgb24 frames (encode-noise
/// tolerant frame comparison for the SR-037 equivalence legs).
pub fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let sum: u64 = a
        .iter()
        .zip(b.iter())
        .map(|(&x, &y)| (x as i16 - y as i16).unsigned_abs() as u64)
        .sum();
    sum as f64 / a.len() as f64
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
