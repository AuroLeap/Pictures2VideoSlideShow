//! Audio passthrough: mux source-video audio onto the (silent) slideshow video.
//!
//! The video pipeline produces a silent `.part` whose timeline is crossfade-
//! compressed (clips overlap by `fade_frames`). Each video clip's **output start
//! frame** — captured from the mixer — converts directly to an `adelay`, so its
//! audio lands exactly under its frames. We delay each clip's audio, mix them,
//! and encode AAC in one ffmpeg pass, then atomically promote to the final path.
//! Images contribute no audio (silence is just the gaps between delayed clips).
// Implements: SR-032, LLR-040, LLR-041

use crate::error::{Result, SlideshowError};
use crate::ffmpeg::promote;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One audio-bearing video clip and where it lands on the output timeline.
#[derive(Debug, Clone)]
pub struct AudioClip {
    /// Source video file (carries the audio stream).
    pub source: PathBuf,
    /// Output frame index where this clip's first frame is emitted.
    pub start_frame: u64,
}

/// AAC encode parameters for the muxed audio track.
#[derive(Debug, Clone, Copy)]
pub struct AudioParams {
    pub bitrate_kbps: u32,
    pub sample_rate: u32,
}

/// Output-timeline delay (ms) for a clip starting at `start_frame` at `fps`.
/// Pure so the sync math can be unit-tested without ffmpeg.
// Implements: LLR-040, SR-032
pub fn delay_ms(start_frame: u64, fps: u32) -> u64 {
    if fps == 0 {
        return 0;
    }
    // Round to the nearest millisecond.
    (start_frame * 1000 + (fps as u64 / 2)) / fps as u64
}

/// The `<final>.apart` temp the audio pass writes before atomic promotion.
fn apart_path(final_path: &Path) -> PathBuf {
    let mut name = final_path
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_default();
    name.push(".apart");
    final_path.with_file_name(name)
}

/// Mux the audio of `clips` onto the silent video at `part_path`, producing
/// `final_path` atomically. `total_frames`/`fps` set the exact output duration
/// so the audio track matches the video length. The silent `.part` is removed
/// once consumed. On any failure no `final_path` is left (SR-011).
// Implements: SR-032, LLR-041
pub fn mux_audio(
    part_path: &Path,
    final_path: &Path,
    clips: &[AudioClip],
    fps: u32,
    total_frames: u64,
    params: &AudioParams,
) -> Result<()> {
    // No audio-bearing clips: nothing to mux, just finalize the silent video.
    if clips.is_empty() {
        return promote(part_path, final_path);
    }

    let duration = total_frames as f64 / fps.max(1) as f64;
    let apart = apart_path(final_path);
    let _ = std::fs::remove_file(&apart); // clear any stale temp

    // Build the per-clip delay chains and the mix.
    let mut filter = String::new();
    for (k, clip) in clips.iter().enumerate() {
        // Input 0 is the silent video; clip inputs start at 1.
        let in_idx = k + 1;
        let ms = delay_ms(clip.start_frame, fps);
        filter.push_str(&format!(
            "[{in_idx}:a]aresample=async=1,adelay={ms}:all=1[a{k}];"
        ));
    }
    if clips.len() == 1 {
        filter.push_str("[a0]apad[aout]");
    } else {
        for k in 0..clips.len() {
            filter.push_str(&format!("[a{k}]"));
        }
        filter.push_str(&format!(
            "amix=inputs={}:normalize=0:dropout_transition=0,apad[aout]",
            clips.len()
        ));
    }

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y").arg("-loglevel").arg("error");
    cmd.arg("-i").arg(part_path);
    for clip in clips {
        cmd.arg("-i").arg(&clip.source);
    }
    cmd.arg("-filter_complex")
        .arg(&filter)
        .arg("-map")
        .arg("0:v")
        .arg("-map")
        .arg("[aout]")
        .arg("-c:v")
        .arg("copy")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg(format!("{}k", params.bitrate_kbps))
        .arg("-ar")
        .arg(params.sample_rate.to_string())
        // Pin the output length to the video so the track can't run long/short.
        .arg("-t")
        .arg(format!("{:.6}", duration))
        .arg("-movflags")
        .arg("+faststart")
        .arg("-f")
        .arg("mp4")
        .arg(&apart);

    let status = cmd.status().map_err(|e| {
        SlideshowError::Ffmpeg(format!("Failed to spawn ffmpeg for audio mux: {}", e))
    })?;

    if !status.success() {
        let _ = std::fs::remove_file(&apart);
        let _ = std::fs::remove_file(part_path);
        return Err(SlideshowError::Ffmpeg(format!(
            "audio mux failed for '{}': ffmpeg exited with status {}",
            final_path.display(),
            status
        )));
    }

    // Audio video built: promote it atomically and drop the silent intermediate.
    promote(&apart, final_path)?;
    let _ = std::fs::remove_file(part_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies: LLR-040, SR-032 — output-start frame converts to the right delay.
    #[test]
    fn delay_ms_maps_start_frame_to_milliseconds() {
        // 0 frames -> 0 ms regardless of fps.
        assert_eq!(delay_ms(0, 30), 0);
        // 30 frames @ 30fps = 1.000 s.
        assert_eq!(delay_ms(30, 30), 1000);
        // 45 frames @ 30fps = 1.500 s.
        assert_eq!(delay_ms(45, 30), 1500);
        // 24 frames @ 24fps = 1.000 s.
        assert_eq!(delay_ms(24, 24), 1000);
        // Rounding to nearest ms: 1 frame @ 24fps = 41.66.. -> 42 ms.
        assert_eq!(delay_ms(1, 24), 42);
        // Degenerate fps never panics.
        assert_eq!(delay_ms(10, 0), 0);
    }
}
