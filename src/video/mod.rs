//! Video passthrough: decode an input video to raw `rgb24` frames at the target
//! resolution/fps (cover-fit, like images), one frame at a time, so it can feed
//! the same streaming pipeline (and cross-fade mixer) as image clips.

use crate::error::{Result, SlideshowError};
use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

/// Streams decoded `rgb24` frames from a video via an ffmpeg child process.
pub struct VideoFrameReader {
    child: Child,
    stdout: ChildStdout,
    frame_bytes: usize,
    finished: bool,
}

impl VideoFrameReader {
    /// Spawn ffmpeg to decode `path`, scaling/cropping to `width`x`height`
    /// (cover-fit) and resampling to `fps`. Audio is dropped.
    pub fn open(path: &Path, width: u32, height: u32, fps: u32) -> Result<Self> {
        // Cover-fit (fill then crop) to match the image Ken Burns framing, then
        // resample to the target frame rate and emit raw rgb24.
        let vf = format!(
            "scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},fps={fps}",
            w = width,
            h = height,
            fps = fps
        );

        let mut child = Command::new("ffmpeg")
            .arg("-v")
            .arg("error")
            .arg("-i")
            .arg(path)
            .arg("-an")
            .arg("-vf")
            .arg(&vf)
            .arg("-pix_fmt")
            .arg("rgb24")
            .arg("-f")
            .arg("rawvideo")
            .arg("pipe:1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                SlideshowError::Ffmpeg(format!("Failed to spawn ffmpeg decoder: {}", e))
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SlideshowError::Ffmpeg("ffmpeg decoder stdout unavailable".into()))?;

        Ok(Self {
            child,
            stdout,
            frame_bytes: (width * height * 3) as usize,
            finished: false,
        })
    }

    /// Read the next decoded frame, or `None` at end of stream.
    pub fn read_frame(&mut self) -> Result<Option<Vec<u8>>> {
        if self.finished {
            return Ok(None);
        }
        let mut buf = vec![0u8; self.frame_bytes];
        let n = read_full(&mut self.stdout, &mut buf)?;
        if n == self.frame_bytes {
            return Ok(Some(buf));
        }
        // EOF (clean, n == 0) or a truncated trailing frame: end the stream.
        self.wait()?;
        Ok(None)
    }

    fn wait(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        let status = self.child.wait().map_err(|e| {
            SlideshowError::Ffmpeg(format!("Failed waiting for ffmpeg decoder: {}", e))
        })?;
        if !status.success() {
            return Err(SlideshowError::Ffmpeg(format!(
                "ffmpeg decoder exited with status {}",
                status
            )));
        }
        Ok(())
    }
}

impl Drop for VideoFrameReader {
    fn drop(&mut self) {
        if !self.finished {
            // Source dropped before EOF (e.g. an error upstream): don't leak the
            // child process.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Read until `buf` is full or EOF. Returns the number of bytes read.
fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = reader
            .read(&mut buf[filled..])
            .map_err(|e| SlideshowError::Ffmpeg(format!("Error reading decoded frame: {}", e)))?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}
