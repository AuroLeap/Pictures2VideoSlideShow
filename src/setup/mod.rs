//! First-run setup building blocks: the FFmpeg dependency self-check (resolve →
//! offline fallback → gated auto-fetch) and interaction gating. The first-run
//! GUI wizard (SR-026) builds on these.

pub mod ffmpeg_fetch;
pub mod interaction;

use crate::config::Config;
use crate::error::{Result, SlideshowError};
use crate::ffmpeg::resolve;
use std::path::PathBuf;

/// Ensure an FFmpeg executable is available, honoring the SR-027 resolution
/// order and the offline / use-existing fallback. When FFmpeg is missing, a
/// gated auto-fetch is attempted ONLY in an interactive session; automation
/// (no TTY / `--non-interactive`) gets a clear, actionable error instead of a
/// hang.
// Implements: SR-027, SR-028, LLR-031, LLR-032, LLR-033
pub fn ensure_ffmpeg(config: &Config, non_interactive: bool) -> Result<PathBuf> {
    let cache = ffmpeg_fetch::cache_dir();
    if let Some(p) = resolve::resolve(config.processing.ffmpeg_path.as_deref(), &cache) {
        return Ok(p);
    }

    if interaction::is_interactive(non_interactive) {
        // Interactive: attempt the (pin-gated) auto-fetch into the per-user cache.
        match ffmpeg_fetch::fetch_ffmpeg(&cache) {
            Ok(p) => Ok(p),
            Err(e) => Err(SlideshowError::Processing(format!(
                "FFmpeg not found and auto-fetch is unavailable: {} \
                 Install FFmpeg and add it to PATH, or set `ffmpeg_path` in your config.",
                e
            ))),
        }
    } else {
        Err(SlideshowError::Processing(
            "FFmpeg not found. Install FFmpeg and add it to PATH, or set `ffmpeg_path` in your \
             config. (Auto-fetch only runs in an interactive session.)"
                .into(),
        ))
    }
}
