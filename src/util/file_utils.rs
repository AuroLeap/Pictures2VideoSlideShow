//! Filesystem helpers: directory creation/writability checks, the disk-full
//! (ENOSPC) detection used to produce plain-language errors, and the per-user
//! application cache root shared by the FFmpeg fetch and the probe cache.

use crate::error::{Result, SlideshowError};
use std::io;
use std::path::{Path, PathBuf};

/// Per-user application cache root (no admin/elevation needed):
/// `%LOCALAPPDATA%\make_video_slideshow` on Windows, falling back to
/// `%APPDATA%` then the OS temp dir. Subdirectories: `ffmpeg` (LLR-031
/// auto-fetch) and `probe-cache` (LLR-058 scan cache) — never under
/// `media_root` (SR-010) or beside outputs.
// Implements: LLR-031, LLR-058, SR-027, SR-038
pub fn app_cache_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    base.join("make_video_slideshow")
}

/// Lowercase hex encoding of `bytes` (shared by the FFmpeg checksum gate and
/// the probe-cache file naming).
// Implements: LLR-034, LLR-058
pub fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Ensure `path` exists as a directory, creating it if missing.
///
/// On failure the error names the exact path and whether it was a
/// missing-parent / permission problem, in plain language (no panic/stack
/// trace).
// Implements: LLR-019, SR-016
pub fn ensure_dir_exists(path: &Path) -> Result<()> {
    if path.is_dir() {
        return ensure_writable_dir(path);
    }
    if path.exists() {
        // Exists but is not a directory.
        return Err(SlideshowError::InvalidConfig(format!(
            "Output path '{}' exists but is not a directory",
            path.display()
        )));
    }
    std::fs::create_dir_all(path).map_err(|e| {
        if is_disk_full(&e) {
            disk_full_error(path)
        } else {
            SlideshowError::InvalidConfig(format!(
                "Cannot create output directory '{}': {}",
                path.display(),
                e
            ))
        }
    })?;
    ensure_writable_dir(path)
}

/// Verify a directory is writable by probing it with a temporary file. Names
/// the exact path and the problem on failure.
// Implements: LLR-019, SR-016
pub fn ensure_writable_dir(path: &Path) -> Result<()> {
    let probe = path.join(".slideshow_write_test");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e) if is_disk_full(&e) => Err(disk_full_error(path)),
        Err(e) => Err(SlideshowError::InvalidConfig(format!(
            "Output directory '{}' is not writable: {}",
            path.display(),
            e
        ))),
    }
}

/// Plain-language out-of-disk-space error naming the affected path.
// Implements: LLR-018, SR-015
pub fn disk_full_error(path: &Path) -> SlideshowError {
    SlideshowError::Ffmpeg(format!("out of disk space writing {}", path.display()))
}

/// Best-effort classification of an out-of-space write failure.
///
/// Maps Windows `ERROR_DISK_FULL` (raw OS error 112) and POSIX `ENOSPC`
/// (errno 28) to true.
// Implements: LLR-018, SR-015
pub fn is_disk_full(err: &io::Error) -> bool {
    #[cfg(windows)]
    const ERROR_DISK_FULL: i32 = 112;
    #[cfg(not(windows))]
    const ENOSPC: i32 = 28;

    match err.raw_os_error() {
        #[cfg(windows)]
        Some(ERROR_DISK_FULL) => true,
        #[cfg(not(windows))]
        Some(ENOSPC) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies: LLR-018, SR-015
    #[test]
    fn is_disk_full_detects_platform_code_sr015() {
        #[cfg(windows)]
        let full = io::Error::from_raw_os_error(112);
        #[cfg(not(windows))]
        let full = io::Error::from_raw_os_error(28);
        assert!(is_disk_full(&full));
    }

    // Verifies: LLR-018, SR-015
    #[test]
    fn is_disk_full_rejects_other_errors_sr015() {
        let not_found = io::Error::from(io::ErrorKind::NotFound);
        assert!(!is_disk_full(&not_found));
        let other = io::Error::from_raw_os_error(5); // access denied / EIO
        assert!(!is_disk_full(&other));
    }
}
