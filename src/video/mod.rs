//! Video passthrough: decode an input video to raw `rgb24` frames at the target
//! resolution/fps (cover-fit, like images) and stream them into the slideshow
//! encoder, so videos play inline in the sequence.

use crate::error::{Result, SlideshowError};
use crate::ffmpeg::FfmpegEncoder;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

/// Decode `path` into the running [`FfmpegEncoder`], scaling/cropping to
/// `width`x`height` and resampling to `fps`. Returns the number of frames
/// streamed. Audio is dropped (the slideshow track is silent).
pub fn stream_into(
    path: &Path,
    width: u32,
    height: u32,
    fps: u32,
    encoder: &mut FfmpegEncoder,
) -> Result<u64> {
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
        .map_err(|e| SlideshowError::Ffmpeg(format!("Failed to spawn ffmpeg decoder: {}", e)))?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| SlideshowError::Ffmpeg("ffmpeg decoder stdout unavailable".into()))?;

    let frame_bytes = (width * height * 3) as usize;
    let mut buf = vec![0u8; frame_bytes];
    let mut frames = 0u64;

    loop {
        match read_full(&mut stdout, &mut buf)? {
            // A complete frame.
            n if n == frame_bytes => {
                encoder.write_frame(&buf)?;
                frames += 1;
            }
            // EOF (clean end of stream).
            0 => break,
            // Truncated trailing frame — ignore.
            _ => break,
        }
    }

    let status = child
        .wait()
        .map_err(|e| SlideshowError::Ffmpeg(format!("Failed waiting for ffmpeg decoder: {}", e)))?;
    if !status.success() {
        return Err(SlideshowError::Ffmpeg(format!(
            "ffmpeg decoder exited with status {} for {}",
            status,
            path.display()
        )));
    }

    Ok(frames)
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
