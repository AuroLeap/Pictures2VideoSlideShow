//! FFmpeg resolution order (SR-027): an explicitly configured `ffmpeg_path`
//! first, then `ffmpeg` on `PATH`, then a per-user cached download. This is the
//! offline / use-existing-FFmpeg fallback — no network is required when FFmpeg
//! is already present.
// Implements: LLR-032, SR-027

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Which source a resolved FFmpeg came from, in priority order.
#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    Configured,
    Path,
    Cached,
}

/// Pure priority decision given which candidates are usable.
// Implements: LLR-032, SR-027
pub fn pick(configured_ok: bool, on_path: bool, cached_ok: bool) -> Option<Source> {
    if configured_ok {
        Some(Source::Configured)
    } else if on_path {
        Some(Source::Path)
    } else if cached_ok {
        Some(Source::Cached)
    } else {
        None
    }
}

/// True if `<prog> -version` runs cleanly.
fn program_runs(prog: &Path) -> bool {
    Command::new(prog)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Resolve the FFmpeg executable per the SR-027 order. `configured` is the
/// optional `ffmpeg_path` from config; `cache_dir` is the per-user download dir.
/// Returns `None` only when FFmpeg is available from none of the three sources.
// Implements: LLR-032, SR-027
pub fn resolve(configured: Option<&Path>, cache_dir: &Path) -> Option<PathBuf> {
    let configured_ok = configured.map(program_runs).unwrap_or(false);
    let on_path = program_runs(Path::new("ffmpeg"));
    let cached = cache_dir.join("ffmpeg.exe");
    let cached_ok = cached.exists() && program_runs(&cached);

    match pick(configured_ok, on_path, cached_ok)? {
        Source::Configured => Some(
            configured
                .expect("configured_ok implies Some")
                .to_path_buf(),
        ),
        Source::Path => Some(PathBuf::from("ffmpeg")),
        Source::Cached => Some(cached),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies: SR-027, LLR-032 — configured beats PATH beats cache.
    #[test]
    fn pick_follows_priority_order_sr027() {
        assert_eq!(pick(true, true, true), Some(Source::Configured));
        assert_eq!(pick(false, true, true), Some(Source::Path));
        assert_eq!(pick(false, false, true), Some(Source::Cached));
    }

    // Verifies: SR-027, LLR-032 — none available -> None (caller falls back/fetches).
    #[test]
    fn pick_none_when_no_source_sr027() {
        assert_eq!(pick(false, false, false), None);
    }
}
