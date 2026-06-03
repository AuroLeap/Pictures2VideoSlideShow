//! FFmpeg coordination: spawn an encoder process and stream raw `rgb24` frames
//! to it over stdin, producing an H.264 MP4.
//!
//! Each output is encoded to a distinguishable `<name>.mp4.part` temp file and
//! atomically renamed to the final `<name>.mp4` only after ffmpeg exits
//! successfully (LLR-015, SR-011). Any failure or early drop removes the temp
//! so a killed run never leaves a complete-looking final file.

use crate::error::{Result, SlideshowError};
use crate::util::{disk_full_error, is_disk_full};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// A running FFmpeg encoder fed raw `rgb24` frames via stdin.
pub struct FfmpegEncoder {
    child: Child,
    /// The `<name>.mp4.part` file ffmpeg writes to.
    temp_path: PathBuf,
    /// The final `<name>.mp4` to promote to on success.
    final_path: PathBuf,
    /// Set once finalize/abort has run so Drop does not double-clean.
    finished: bool,
}

impl FfmpegEncoder {
    /// Spawn FFmpeg to read `width`x`height` `rgb24` frames at `fps` from stdin
    /// and encode them to a temp file alongside `output`. Call [`finish`] to
    /// atomically promote it to `output`.
    // Implements: LLR-015, SR-011
    pub fn start(output: &Path, width: u32, height: u32, fps: u32, crf: u32) -> Result<Self> {
        let final_path = output.to_path_buf();
        let temp_path = part_path(output);

        // Remove any stale temp from a prior killed run so we start clean.
        let _ = std::fs::remove_file(&temp_path);

        let child = Command::new("ffmpeg")
            .arg("-y")
            .arg("-loglevel")
            .arg("error")
            // Raw input description.
            .arg("-f")
            .arg("rawvideo")
            .arg("-pixel_format")
            .arg("rgb24")
            .arg("-video_size")
            .arg(format!("{}x{}", width, height))
            .arg("-framerate")
            .arg(fps.to_string())
            .arg("-i")
            .arg("pipe:0")
            // Output encoding.
            .arg("-an")
            .arg("-c:v")
            .arg("libx264")
            .arg("-preset")
            .arg("medium")
            .arg("-crf")
            .arg(crf.to_string())
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-movflags")
            .arg("+faststart")
            // The temp file uses a `.part` extension ffmpeg can't infer a muxer
            // from, so pin the container explicitly.
            .arg("-f")
            .arg("mp4")
            .arg(&temp_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| {
                SlideshowError::Ffmpeg(format!("Failed to spawn ffmpeg (is it on PATH?): {}", e))
            })?;

        Ok(Self {
            child,
            temp_path,
            final_path,
            finished: false,
        })
    }

    /// Write one raw `rgb24` frame to the encoder.
    pub fn write_frame(&mut self, data: &[u8]) -> Result<()> {
        let temp_path = self.temp_path.clone();
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| SlideshowError::Ffmpeg("ffmpeg stdin not available".into()))?;
        stdin.write_all(data).map_err(|e| {
            // A full output volume can surface as a write/broken-pipe failure.
            if is_disk_full(&e) {
                disk_full_error(&temp_path)
            } else {
                SlideshowError::Ffmpeg(format!("Failed writing frame to ffmpeg: {}", e))
            }
        })
    }

    /// Close stdin, wait for FFmpeg, and on success atomically rename the temp
    /// file to the final output path. On any failure the temp is removed so no
    /// complete-looking final file is left.
    // Implements: LLR-015, SR-011, SR-015
    pub fn finish(mut self) -> Result<()> {
        self.finished = true;
        // Drop stdin to signal EOF.
        drop(self.child.stdin.take());
        let status = self
            .child
            .wait()
            .map_err(|e| SlideshowError::Ffmpeg(format!("Failed waiting for ffmpeg: {}", e)))?;
        if !status.success() {
            let _ = std::fs::remove_file(&self.temp_path);
            return Err(SlideshowError::Ffmpeg(format!(
                "encoding failed for '{}': ffmpeg exited with status {}",
                self.final_path.display(),
                status
            )));
        }

        // Atomic promote temp -> final on the same volume.
        std::fs::rename(&self.temp_path, &self.final_path).map_err(|e| {
            let _ = std::fs::remove_file(&self.temp_path);
            if is_disk_full(&e) {
                disk_full_error(&self.final_path)
            } else {
                SlideshowError::Ffmpeg(format!(
                    "Failed finalizing '{}': {}",
                    self.final_path.display(),
                    e
                ))
            }
        })?;
        Ok(())
    }
}

impl Drop for FfmpegEncoder {
    fn drop(&mut self) {
        // If we never finished (early return / panic / kill), kill ffmpeg and
        // remove the temp so no partial output bearing a real name survives.
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

/// The distinguishable in-progress temp path for `output`: `<output>.part`.
// Implements: LLR-015, SR-011
fn part_path(output: &Path) -> PathBuf {
    let mut name = output.file_name().map(|n| n.to_owned()).unwrap_or_default();
    name.push(".part");
    output.with_file_name(name)
}
