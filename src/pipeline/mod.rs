//! Pipeline orchestration: wire media → frame generation → FFmpeg encoding.
//!
//! For each output definition we open one FFmpeg encoder and stream every
//! image's frames into it, producing a single slideshow video. Frames are
//! rendered in bounded-size ranges (parallel via rayon) so peak memory stays
//! low regardless of clip length.

use crate::config::{OutputDef, ProcessingConfig};
use crate::error::Result;
use crate::ffmpeg::FfmpegEncoder;
use crate::image::FrameRenderer;
use crate::media::{Album, MediaType};
use crate::util::ensure_dir_exists;
use std::path::PathBuf;
use std::time::Instant;

/// Soft cap on in-flight frame bytes per render range.
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

    fn encode_output(
        &self,
        output_def: &OutputDef,
        media: &[&crate::media::MediaFile],
    ) -> Result<()> {
        let out_path = self.output_dir.join(format!("{}.mp4", output_def.name));
        log::info!(
            "Encoding '{}' -> {} ({}x{} @ {}fps, crf {})",
            output_def.name,
            out_path.display(),
            output_def.width,
            output_def.height,
            output_def.fps,
            output_def.quality_crf,
        );

        if media.is_empty() {
            log::warn!("No media to process for output '{}'", output_def.name);
            return Ok(());
        }

        let frame_bytes = (output_def.width * output_def.height * 3) as usize;
        let range_size = (RANGE_MEMORY_BUDGET / frame_bytes.max(1)).max(1) as u32;

        let mut encoder = FfmpegEncoder::start(
            &out_path,
            output_def.width,
            output_def.height,
            output_def.fps,
            output_def.quality_crf,
        )?;

        let total = media.len();
        let start = Instant::now();
        let mut total_frames: u64 = 0;

        for (idx, item) in media.iter().enumerate() {
            let name = item
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            let frames = match item.file_type {
                MediaType::Image => {
                    let renderer = match FrameRenderer::load(&item.path, output_def) {
                        Ok(r) => r,
                        Err(e) => {
                            log::warn!("Skipping {}: {}", item.path.display(), e);
                            continue;
                        }
                    };
                    let n = renderer.total_frames();
                    let mut next = 0;
                    while next < n {
                        let end = (next + range_size).min(n);
                        let batch = renderer.render_range(next, end);
                        for frame in &batch {
                            encoder.write_frame(frame)?;
                        }
                        next = end;
                    }
                    n as u64
                }
                MediaType::Video => match crate::video::stream_into(
                    &item.path,
                    output_def.width,
                    output_def.height,
                    output_def.fps,
                    &mut encoder,
                ) {
                    Ok(n) => n,
                    Err(e) => {
                        log::warn!("Skipping video {}: {}", item.path.display(), e);
                        continue;
                    }
                },
            };

            total_frames += frames;
            log::info!(
                "  [{}/{}] {} {} ({} frames)",
                idx + 1,
                total,
                match item.file_type {
                    MediaType::Image => "[img]",
                    MediaType::Video => "[vid]",
                },
                name,
                frames,
            );
        }

        encoder.finish()?;

        let elapsed = start.elapsed();
        log::info!(
            "Finished '{}': {} items, {} frames in {:.1}s ({:.1} frames/s)",
            output_def.name,
            total,
            total_frames,
            elapsed.as_secs_f64(),
            total_frames as f64 / elapsed.as_secs_f64().max(0.001),
        );

        Ok(())
    }
}
