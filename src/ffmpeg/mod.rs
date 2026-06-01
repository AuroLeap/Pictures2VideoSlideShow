//! FFmpeg coordination: spawn an encoder process and stream raw `rgb24` frames
//! to it over stdin, producing an H.264 MP4.

use crate::error::{Result, SlideshowError};
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// A running FFmpeg encoder fed raw `rgb24` frames via stdin.
pub struct FfmpegEncoder {
    child: Child,
}

impl FfmpegEncoder {
    /// Spawn FFmpeg to read `width`x`height` `rgb24` frames at `fps` from stdin
    /// and encode them to `output` (H.264, `crf` quality).
    pub fn start(output: &Path, width: u32, height: u32, fps: u32, crf: u32) -> Result<Self> {
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
            .arg(output)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| {
                SlideshowError::Ffmpeg(format!("Failed to spawn ffmpeg (is it on PATH?): {}", e))
            })?;

        Ok(Self { child })
    }

    /// Write one raw `rgb24` frame to the encoder.
    pub fn write_frame(&mut self, data: &[u8]) -> Result<()> {
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| SlideshowError::Ffmpeg("ffmpeg stdin not available".into()))?;
        stdin
            .write_all(data)
            .map_err(|e| SlideshowError::Ffmpeg(format!("Failed writing frame to ffmpeg: {}", e)))
    }

    /// Close stdin and wait for FFmpeg to finish encoding.
    pub fn finish(mut self) -> Result<()> {
        // Drop stdin to signal EOF.
        drop(self.child.stdin.take());
        let status = self
            .child
            .wait()
            .map_err(|e| SlideshowError::Ffmpeg(format!("Failed waiting for ffmpeg: {}", e)))?;
        if !status.success() {
            return Err(SlideshowError::Ffmpeg(format!(
                "ffmpeg exited with status {}",
                status
            )));
        }
        Ok(())
    }
}
