//! Pipeline orchestration: wire media → frame generation → FFmpeg encoding,
//! with cross-fade (dissolve) transitions between consecutive clips.
//!
//! For each output definition we open one FFmpeg encoder and drive every clip
//! (image or video) through a streaming [`CrossfadeMixer`]. The mixer only ever
//! holds a rolling window of `fade_frames` at each boundary, so memory stays
//! bounded regardless of clip length or count — whole videos are never loaded.

mod source;

use crate::config::{OutputDef, ProcessingConfig};
use crate::error::Result;
use crate::ffmpeg::FfmpegEncoder;
use crate::image::FrameRenderer;
use crate::media::{Album, MediaFile, MediaType};
use crate::util::ensure_dir_exists;
use crate::video::VideoFrameReader;
use source::{FrameSource, ImageFrameSource, VideoFrameSource};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

/// Soft cap on in-flight frame bytes for image render batches.
const RANGE_MEMORY_BUDGET: usize = 64 * 1024 * 1024;

pub struct FrameGenerationPipeline {
    album: Album,
    outputs: Vec<OutputDef>,
    #[allow(dead_code)]
    processing: ProcessingConfig,
    output_dir: PathBuf,
}

impl FrameGenerationPipeline {
    pub fn new(
        album: Album,
        outputs: Vec<OutputDef>,
        processing: ProcessingConfig,
        output_dir: PathBuf,
    ) -> Self {
        Self {
            album,
            outputs,
            processing,
            output_dir,
        }
    }

    pub async fn execute(&self) -> Result<()> {
        ensure_dir_exists(&self.output_dir)?;

        let media: Vec<_> = self.album.media_files.iter().collect();

        for output_def in &self.outputs {
            self.encode_output(output_def, &media)?;
        }

        Ok(())
    }

    fn encode_output(&self, output_def: &OutputDef, media: &[&MediaFile]) -> Result<()> {
        let out_path = self.output_dir.join(format!("{}.mp4", output_def.name));
        log::info!(
            "Encoding '{}' -> {} ({}x{} @ {}fps, crf {}, {}-frame crossfade)",
            output_def.name,
            out_path.display(),
            output_def.width,
            output_def.height,
            output_def.fps,
            output_def.quality_crf,
            output_def.fade_frames(),
        );

        if media.is_empty() {
            log::warn!("No media to process for output '{}'", output_def.name);
            return Ok(());
        }

        let frame_bytes = (output_def.width * output_def.height * 3) as usize;
        let batch = (RANGE_MEMORY_BUDGET / frame_bytes.max(1)).max(1) as u32;

        let encoder = FfmpegEncoder::start(
            &out_path,
            output_def.width,
            output_def.height,
            output_def.fps,
            output_def.quality_crf,
        )?;

        let mut mixer = CrossfadeMixer::new(encoder, output_def.fade_frames() as usize);

        let total = media.len();
        let start = Instant::now();

        for (idx, item) in media.iter().enumerate() {
            let name = item
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            let mut src: Box<dyn FrameSource> = match item.file_type {
                MediaType::Image => match FrameRenderer::load(&item.path, output_def) {
                    Ok(r) => Box::new(ImageFrameSource::new(r, batch)),
                    Err(e) => {
                        log::warn!("Skipping image {}: {}", item.path.display(), e);
                        continue;
                    }
                },
                MediaType::Video => match VideoFrameReader::open(
                    &item.path,
                    output_def.width,
                    output_def.height,
                    output_def.fps,
                ) {
                    Ok(rd) => Box::new(VideoFrameSource::new(rd)),
                    Err(e) => {
                        log::warn!("Skipping video {}: {}", item.path.display(), e);
                        continue;
                    }
                },
            };

            let clip_frames = mixer.add_clip(src.as_mut())?;
            log::info!(
                "  [{}/{}] {} {} ({} frames)",
                idx + 1,
                total,
                match item.file_type {
                    MediaType::Image => "[img]",
                    MediaType::Video => "[vid]",
                },
                name,
                clip_frames,
            );
        }

        let emitted = mixer.finish()?;

        let elapsed = start.elapsed();
        log::info!(
            "Finished '{}': {} items, {} frames in {:.1}s ({:.1} frames/s)",
            output_def.name,
            total,
            emitted,
            elapsed.as_secs_f64(),
            emitted as f64 / elapsed.as_secs_f64().max(0.001),
        );

        Ok(())
    }
}

/// Streams clips into the encoder, dissolving the tail of each clip into the
/// head of the next over `n` frames. The first clip fades in from black and the
/// last fades out to black.
struct CrossfadeMixer {
    encoder: FfmpegEncoder,
    n: usize,
    /// The held tail (last <= n frames) of the previously emitted clip, awaiting
    /// the next clip to dissolve into. Empty before the first clip.
    prev_tail: Vec<Vec<u8>>,
    emitted: u64,
}

impl CrossfadeMixer {
    fn new(encoder: FfmpegEncoder, n: usize) -> Self {
        Self {
            encoder,
            n,
            prev_tail: Vec::new(),
            emitted: 0,
        }
    }

    /// Drive one clip through the mixer. Returns the number of source frames
    /// the clip produced.
    fn add_clip(&mut self, src: &mut dyn FrameSource) -> Result<u64> {
        let n = self.n;
        let mut produced: u64 = 0;

        // 1. Read up to `n` head frames for the incoming transition.
        let mut head: Vec<Vec<u8>> = Vec::with_capacity(n);
        for _ in 0..n {
            match src.next_frame()? {
                Some(f) => {
                    produced += 1;
                    head.push(f);
                }
                None => break,
            }
        }

        // 2. Transition into this clip.
        if self.prev_tail.is_empty() {
            // First clip overall: fade in from black across the head.
            let len = head.len().max(1);
            for (k, f) in head.iter().enumerate() {
                let t = (k + 1) as f32 / len as f32;
                self.emit(scale(f, t))?;
            }
        } else {
            let prev = std::mem::take(&mut self.prev_tail);
            let m = prev.len().min(head.len());
            // Dissolve overlapping frames.
            for k in 0..m {
                let t = (k + 1) as f32 / (m + 1) as f32;
                self.emit(blend(&prev[k], &head[k], t))?;
            }
            // If this clip's head ran longer than the previous tail (a short
            // previous clip), emit the surplus head frames as ordinary body.
            for f in head.iter().skip(m) {
                self.emit(f.clone())?;
            }
            // (If the previous tail was longer than this head, the leftover
            // tail frames are dropped — only happens for sub-transition clips.)
        }

        // 3. Stream the body, holding the last `n` frames as the new tail.
        let mut ring: VecDeque<Vec<u8>> = VecDeque::with_capacity(n + 1);
        while let Some(f) = src.next_frame()? {
            produced += 1;
            ring.push_back(f);
            if ring.len() > n {
                let body = ring.pop_front().unwrap();
                self.emit(body)?;
            }
        }
        self.prev_tail = ring.into_iter().collect();

        Ok(produced)
    }

    /// Fade out the final clip's tail to black, flush the encoder, and return
    /// the total number of frames emitted.
    fn finish(mut self) -> Result<u64> {
        let prev = std::mem::take(&mut self.prev_tail);
        let len = prev.len();
        for (k, f) in prev.iter().enumerate() {
            let t = 1.0 - (k + 1) as f32 / (len + 1) as f32;
            self.emit(scale(f, t))?;
        }
        let emitted = self.emitted;
        self.encoder.finish()?;
        Ok(emitted)
    }

    fn emit(&mut self, frame: Vec<u8>) -> Result<()> {
        self.encoder.write_frame(&frame)?;
        self.emitted += 1;
        Ok(())
    }
}

/// Linear cross-dissolve: `out = a*(1-t) + b*t` per channel byte.
fn blend(a: &[u8], b: &[u8], t: f32) -> Vec<u8> {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| (x as f32 * inv + y as f32 * t).round().clamp(0.0, 255.0) as u8)
        .collect()
}

/// Brightness scale toward/from black: `out = a*t`.
fn scale(a: &[u8], t: f32) -> Vec<u8> {
    let t = t.clamp(0.0, 1.0);
    a.iter()
        .map(|&x| (x as f32 * t).round().clamp(0.0, 255.0) as u8)
        .collect()
}
