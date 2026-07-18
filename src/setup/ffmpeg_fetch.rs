//! FFmpeg acquisition: integrity-verified download to a per-user directory, with
//! a strict "never run an unverified binary" guarantee. FFmpeg is fetched at
//! runtime (never bundled/redistributed); the offline / use-existing-FFmpeg
//! fallback is handled by [`crate::ffmpeg::resolve`].
// Implements: LLR-031, LLR-034, SR-027, SR-029

use crate::error::{Result, SlideshowError};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Pinned FFmpeg download: a specific, immutable GitHub release asset (the
/// gyan.dev Windows "essentials" build linked from ffmpeg.org) and its verified
/// SHA-256 (computed from the downloaded asset). The integrity gate in
/// [`fetch_ffmpeg`] runs BEFORE extraction so a tampered/corrupt download is
/// never extracted or run. To bump the version: change both constants together
/// (URL is immutable per tag; recompute the SHA-256 of the new asset).
// Implements: SR-027, SR-029
const PINNED_URL: &str =
    "https://github.com/GyanD/codexffmpeg/releases/download/7.1/ffmpeg-7.1-essentials_build.zip";
const PINNED_SHA256: &str = "fa7d4d7e795db0e2503f49f105f46ed5852386f0cfdd819899be3b65ebde24fc";

/// Per-user cache directory for an auto-fetched FFmpeg (no admin/elevation):
/// `%LOCALAPPDATA%\make_video_slideshow\ffmpeg` on Windows (the shared
/// [`crate::util::file_utils::app_cache_root`]).
// Implements: LLR-031, SR-027
pub fn cache_dir() -> PathBuf {
    crate::util::file_utils::app_cache_root().join("ffmpeg")
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

use crate::util::file_utils::hex_lower as hex_encode;

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

    // Reuse an already-fetched copy.
    let target = dest_dir.join("ffmpeg.exe");
    if target.exists() {
        return Ok(target);
    }

    let archive = dest_dir.join("ffmpeg-pinned.zip");
    download_via_powershell(PINNED_URL, &archive)?;
    // Integrity gate BEFORE extraction: a mismatch aborts, leaving nothing run.
    verify_checksum(&archive, PINNED_SHA256)?;

    let extract_dir = dest_dir.join("extracted");
    let _ = std::fs::remove_dir_all(&extract_dir);
    extract_zip_via_powershell(&archive, &extract_dir)?;

    // The build nests the binaries under `<pkg>/bin/`; locate them robustly.
    let ffmpeg_src = find_file(&extract_dir, "ffmpeg.exe").ok_or_else(|| {
        SlideshowError::Processing("ffmpeg.exe not found in the downloaded archive".into())
    })?;
    std::fs::copy(&ffmpeg_src, &target).map_err(|e| {
        SlideshowError::Processing(format!("cannot place ffmpeg.exe in cache: {}", e))
    })?;
    // ffprobe travels with ffmpeg; place it beside so the cache dir is complete.
    if let Some(ffprobe_src) = find_file(&extract_dir, "ffprobe.exe") {
        let _ = std::fs::copy(&ffprobe_src, dest_dir.join("ffprobe.exe"));
    }

    // Best-effort cleanup of the archive + extraction scratch.
    let _ = std::fs::remove_file(&archive);
    let _ = std::fs::remove_dir_all(&extract_dir);

    Ok(target)
}

/// Extract a zip via PowerShell `Expand-Archive` (no extra runtime deps).
// Implements: LLR-031, SR-027
fn extract_zip_via_powershell(archive: &Path, dest: &Path) -> Result<()> {
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            archive.display(),
            dest.display()
        ))
        .status()
        .map_err(|e| SlideshowError::Processing(format!("failed to launch extractor: {}", e)))?;
    if !status.success() {
        return Err(SlideshowError::Processing(
            "failed to extract the downloaded FFmpeg archive".into(),
        ));
    }
    Ok(())
}

/// Recursively find a file by (case-insensitive) name under `root`.
fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Some(f) = entry.file_name().to_str() {
                if f.eq_ignore_ascii_case(name) {
                    return Some(entry.into_path());
                }
            }
        }
    }
    None
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

    // Verifies: SR-027/SR-029 — a verified pin is configured (https URL + 64-hex
    // sha256) so auto-fetch is enabled and integrity-gated. (The actual network
    // download is exercised by the #[ignore]d integration test.)
    #[test]
    fn pinned_release_is_configured_sr027() {
        assert!(
            PINNED_URL.starts_with("https://"),
            "pinned URL must be https"
        );
        assert_eq!(PINNED_SHA256.len(), 64, "sha256 is 64 hex chars");
        assert!(
            PINNED_SHA256.chars().all(|c| c.is_ascii_hexdigit()),
            "sha256 is hex"
        );
    }
}
