//! FFmpeg acquisition: integrity-verified download to a per-user directory, with
//! a strict "never run an unverified binary" guarantee. FFmpeg is fetched at
//! runtime (never bundled/redistributed); the offline / use-existing-FFmpeg
//! fallback is handled by [`crate::ffmpeg::resolve`].
// Implements: LLR-031, LLR-034, SR-027, SR-029

use crate::error::{Result, SlideshowError};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Pinned FFmpeg download. These MUST be set to a specific, verified release
/// (URL + its SHA-256) before auto-fetch is enabled. They are intentionally
/// empty so the build never downloads or runs an *unverified* binary: until a
/// maintainer pins a checksum, [`fetch_ffmpeg`] refuses and the user falls back
/// to an existing FFmpeg (PATH or `ffmpeg_path` in config).
// Implements: SR-027, SR-029
const PINNED_URL: &str = "";
const PINNED_SHA256: &str = "";

/// Per-user cache directory for an auto-fetched FFmpeg (no admin/elevation):
/// `%LOCALAPPDATA%\make_video_slideshow\ffmpeg` on Windows.
// Implements: LLR-031, SR-027
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    base.join("make_video_slideshow").join("ffmpeg")
}

/// Verify a file's SHA-256 against `expected_hex` (case-insensitive). Returns an
/// error on mismatch so a tampered/corrupt download is never executed.
// Implements: LLR-034, SR-029
pub fn verify_checksum(path: &Path, expected_hex: &str) -> Result<()> {
    let bytes = std::fs::read(path).map_err(|e| {
        SlideshowError::Processing(format!("cannot read {}: {}", path.display(), e))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let got = hasher.finalize();
    let got_hex = hex_encode(&got);
    if got_hex.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        Err(SlideshowError::Processing(format!(
            "FFmpeg integrity check failed for {}: expected sha256 {}, got {}",
            path.display(),
            expected_hex,
            got_hex
        )))
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Fetch FFmpeg into `dest_dir`, verifying its integrity before it is usable.
///
/// Refuses (returns an error pointing at the offline fallback) until a verified
/// `PINNED_URL`/`PINNED_SHA256` is set, so an unverified binary is never run.
/// When pinned, downloads via PowerShell `Invoke-WebRequest` (no extra runtime
/// deps), then [`verify_checksum`] gates use. Demonstration-verified (network).
// Implements: LLR-031, LLR-034, SR-027, SR-029
pub fn fetch_ffmpeg(dest_dir: &Path) -> Result<PathBuf> {
    if PINNED_URL.is_empty() || PINNED_SHA256.is_empty() {
        return Err(SlideshowError::Processing(
            "automatic FFmpeg download is not yet enabled (no pinned, checksum-verified release). \
             Install FFmpeg and add it to PATH, or set `ffmpeg_path` in your config."
                .into(),
        ));
    }
    std::fs::create_dir_all(dest_dir).map_err(|e| {
        SlideshowError::Processing(format!(
            "cannot create FFmpeg cache dir {}: {}",
            dest_dir.display(),
            e
        ))
    })?;
    let archive = dest_dir.join("ffmpeg-download.bin");
    download_via_powershell(PINNED_URL, &archive)?;
    // Integrity gate BEFORE any use/extraction.
    verify_checksum(&archive, PINNED_SHA256)?;
    // NOTE: extraction of the verified archive to `ffmpeg.exe` is performed here
    // once a real archive layout is pinned; returns the extracted binary path.
    Ok(dest_dir.join("ffmpeg.exe"))
}

/// Download `url` to `dest` using PowerShell's `Invoke-WebRequest` (TLS via the
/// OS). Kept isolated so the rest of the module is unit-testable without network.
// Implements: LLR-031, SR-027
fn download_via_powershell(url: &str, dest: &Path) -> Result<()> {
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!(
            "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -Uri '{}' -OutFile '{}'",
            url,
            dest.display()
        ))
        .status()
        .map_err(|e| SlideshowError::Processing(format!("failed to launch downloader: {}", e)))?;
    if !status.success() {
        return Err(SlideshowError::Processing(format!(
            "download failed for {} (are you online?)",
            url
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies: SR-029, LLR-034 — checksum matches a known SHA-256.
    #[test]
    fn verify_checksum_accepts_correct_hash_sr029() {
        // sha256("abc") — a well-known fixed vector.
        let dir = std::env::temp_dir().join("mvs_checksum_ok");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("abc.txt");
        std::fs::write(&f, b"abc").unwrap();
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify_checksum(&f, expected).is_ok());
        // Case-insensitive.
        assert!(verify_checksum(&f, &expected.to_uppercase()).is_ok());
        let _ = std::fs::remove_file(&f);
    }

    // Verifies: SR-029, LLR-034 — a wrong hash is rejected (no execution).
    #[test]
    fn verify_checksum_rejects_wrong_hash_sr029() {
        let dir = std::env::temp_dir().join("mvs_checksum_bad");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("abc.txt");
        std::fs::write(&f, b"abc").unwrap();
        let wrong = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(verify_checksum(&f, wrong).is_err());
        let _ = std::fs::remove_file(&f);
    }

    // Verifies: SR-027 — auto-fetch refuses (no unverified binary) until pinned.
    #[test]
    fn fetch_refuses_without_pinned_release_sr027() {
        let dir = std::env::temp_dir().join("mvs_fetch_disabled");
        let err = fetch_ffmpeg(&dir).unwrap_err();
        assert!(err.to_string().contains("not yet enabled"));
    }
}
