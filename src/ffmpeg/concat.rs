//! Stream-copy concat assembly of per-clip segments into one MP4 (SR-037):
//! `ffmpeg -f concat -safe 0 -c copy` over an ordered segment list, writing
//! the same `<name>.mp4.part` temp the streaming encoder uses so the mux /
//! atomic-promote consumers (SR-011) take over unchanged. Lives in
//! `src/ffmpeg` to reuse the module-private watchdog decision (`timed_out`,
//! SR-013) and part-path convention.

// lib-API: the SR-037 segmented path, exercised via tests/concat_seam.rs; the
// binary wires it up with the warm-build planner (LLR-055, Round 5b).
#![allow(dead_code)]

use super::{now_ms, part_path, timed_out};
use crate::error::{Result, SlideshowError};
use crate::util::{disk_full_error, is_disk_full};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// How often the concat watchdog polls the child and the growing `.part`
/// (progress = file growth; concat has no stdin frame writes to observe).
const WATCH_POLL: Duration = Duration::from_millis(100);

/// Assemble `segments` (ordered, self-contained MPEG-TS files from
/// [`super::FfmpegEncoder::start_segment`]) into `final_output`'s `.part`
/// file by stream-copy concat — no re-encode — and return the `.part` path.
///
/// Contract:
/// - Inputs: `segments` — the ordered segment paths (≥ 1); `final_output` —
///   the final `<name>.mp4` this part is destined for (used for the part path
///   and error naming only — this function never creates the final name;
///   callers promote via [`super::promote`], SR-011); `timeout_secs` —
///   inactivity watchdog like the encoder's (`0` disables, SR-013).
/// - The MP4 container flags (`-movflags +faststart`) are applied here, so
///   the assembled output carries the SR-005 profile.
/// - Failure modes: an empty segment list, an unwritable list file (disk-full
///   mapped per SR-015), a non-zero ffmpeg exit, or a watchdog timeout — each
///   surfaces as a named error with the `.part` and list file removed, never
///   a false success (SR-013).
// Implements: LLR-056, SR-037, SR-011, SR-013, SR-015
pub fn concat_segments(
    segments: &[PathBuf],
    final_output: &Path,
    timeout_secs: u64,
) -> Result<PathBuf> {
    let output_name = final_output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| final_output.display().to_string());
    if segments.is_empty() {
        return Err(SlideshowError::Ffmpeg(format!(
            "concat assembly for '{}': no segments to assemble",
            output_name
        )));
    }

    let part = part_path(final_output);
    // Start clean: a stale part from a prior killed run must not survive.
    let _ = std::fs::remove_file(&part);

    // The concat demuxer's input list, next to the part file; removed on every
    // exit path. Write failures map through the SR-015 disk-full detection.
    let list = final_output.with_file_name(format!("{}.concat.txt", output_name));
    write_concat_list(&list, segments)?;

    let spawned = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(&list)
        .args(["-c", "copy", "-movflags", "+faststart", "-f", "mp4"])
        .arg(&part)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&list);
            return Err(SlideshowError::Ffmpeg(format!(
                "Failed to spawn ffmpeg for concat of '{}' (is it on PATH?): {}",
                output_name, e
            )));
        }
    };

    // Inactivity watchdog (SR-013): progress is the part file growing. Poll
    // cheaply; if the size stalls past the timeout, kill ffmpeg and fail
    // loudly naming the output — never hang indefinitely.
    let mut last_activity = now_ms();
    let mut last_size = 0u64;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                cleanup(&part, &list);
                return Err(SlideshowError::Ffmpeg(format!(
                    "Failed waiting for concat ffmpeg for '{}': {}",
                    output_name, e
                )));
            }
        }
        let size = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if size != last_size {
            last_size = size;
            last_activity = now_ms();
        }
        if timed_out(last_activity, now_ms(), timeout_secs) {
            let _ = child.kill();
            let _ = child.wait();
            cleanup(&part, &list);
            return Err(SlideshowError::Ffmpeg(format!(
                "concat assembly timed out for '{}': no ffmpeg progress within the inactivity timeout; aborted",
                output_name
            )));
        }
        std::thread::sleep(WATCH_POLL);
    };

    let _ = std::fs::remove_file(&list);
    if !status.success() {
        let _ = std::fs::remove_file(&part);
        return Err(SlideshowError::Ffmpeg(format!(
            "concat assembly failed for '{}': ffmpeg exited with status {}",
            output_name, status
        )));
    }
    Ok(part)
}

/// Write the concat-demuxer list file: one `file '<abs path>'` line per
/// segment, forward slashes, single quotes escaped per the demuxer's rules.
// Implements: LLR-056, SR-015
fn write_concat_list(list: &Path, segments: &[PathBuf]) -> Result<()> {
    let mut body = String::new();
    for seg in segments {
        body.push_str(&format!("file '{}'\n", concat_escape(seg)));
    }
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(list)?;
        f.write_all(body.as_bytes())
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(list);
        if is_disk_full(&e) {
            disk_full_error(list)
        } else {
            SlideshowError::Ffmpeg(format!(
                "Failed writing concat list '{}': {}",
                list.display(),
                e
            ))
        }
    })
}

/// A path as the concat demuxer wants it inside `file '...'`: forward slashes
/// (Windows-safe, no TOML/shell-style backslash surprises) and embedded single
/// quotes closed-escaped-reopened (`'\''`).
fn concat_escape(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "/")
        .replace('\'', r"'\''")
}

/// Best-effort removal of the in-progress artifacts on a failure path, so no
/// complete-looking part or stale list survives (SR-011 spirit).
fn cleanup(part: &Path, list: &Path) {
    let _ = std::fs::remove_file(part);
    let _ = std::fs::remove_file(list);
}

#[cfg(test)]
mod tests {
    use super::concat_escape;
    use std::path::Path;

    // Verifies: LLR-056, SR-037 — list-file path quoting: backslashes become
    // forward slashes and single quotes are escaped for the concat demuxer.
    #[test]
    fn concat_escape_quotes_windows_paths_sr037() {
        assert_eq!(
            concat_escape(Path::new(r"C:\tmp\seg s_0001.ts")),
            "C:/tmp/seg s_0001.ts"
        );
        assert_eq!(
            concat_escape(Path::new("/a/o'brien.ts")),
            r"/a/o'\''brien.ts"
        );
    }
}
