//! First-run setup building blocks: the FFmpeg dependency self-check (resolve →
//! offline fallback → gated auto-fetch) and interaction gating. The first-run
//! GUI wizard (SR-026) builds on these.

pub mod config_builder;
pub mod ffmpeg_fetch;
pub mod interaction;
pub mod wizard;

use crate::config::Config;
use crate::error::{Result, SlideshowError};
use crate::ffmpeg::resolve;
use std::path::{Path, PathBuf};

/// Run the first-run GUI wizard, build a valid config from the collected values,
/// and write it to `path` (creating parent dirs). Used when there is no config
/// and the session is interactive (gated by [`interaction::should_prompt`]).
// Implements: LLR-029, LLR-030, SR-026, SR-030
pub fn run_first_run_setup(path: &Path) -> Result<()> {
    let values = wizard::run_wizard()?;
    let config = config_builder::build_config(&values);
    let toml = config_builder::to_toml(&config)?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, toml).map_err(|e| {
        SlideshowError::Config(format!("cannot write config {}: {}", path.display(), e))
    })?;
    log::info!("Setup complete — wrote config to {}", path.display());
    Ok(())
}

/// Ensure an FFmpeg executable is available, honoring the SR-027 resolution
/// order and the offline / use-existing fallback. When FFmpeg is missing, a
/// gated auto-fetch is attempted ONLY in an interactive session; automation
/// (no TTY / `--non-interactive`) gets a clear, actionable error instead of a
/// hang.
// Implements: SR-027, SR-028, LLR-031, LLR-032, LLR-033
pub fn ensure_ffmpeg(config: &Config, non_interactive: bool) -> Result<PathBuf> {
    let cache = ffmpeg_fetch::cache_dir();
    if let Some(p) = resolve::resolve(config.processing.ffmpeg_path.as_deref(), &cache) {
        prepend_dir_to_path(&p);
        return Ok(p);
    }

    if interaction::is_interactive(non_interactive) {
        // Interactive: attempt the (pin-gated, integrity-verified) auto-fetch.
        match ffmpeg_fetch::fetch_ffmpeg(&cache) {
            Ok(p) => {
                prepend_dir_to_path(&p);
                Ok(p)
            }
            Err(e) => Err(SlideshowError::Processing(format!(
                "FFmpeg not found and auto-fetch failed: {} \
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

/// Prepend the resolved FFmpeg's directory to this process's `PATH` so the
/// literal `ffmpeg`/`ffprobe` spawns in the encoder, video reader, and prober
/// pick up a configured or auto-fetched build (not just one already on PATH).
/// A bare `ffmpeg` (already-on-PATH source) has no parent dir and is skipped.
// Implements: SR-027
fn prepend_dir_to_path(ffmpeg: &Path) {
    let Some(dir) = ffmpeg.parent() else { return };
    if dir.as_os_str().is_empty() {
        return; // resolved from PATH already
    }
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(std::env::split_paths(&current));
    if let Ok(joined) = std::env::join_paths(paths) {
        std::env::set_var("PATH", joined);
    }
}
