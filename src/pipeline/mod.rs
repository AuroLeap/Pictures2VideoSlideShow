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
use crate::util::estimate::{
    estimate_output_bytes, human_bytes, is_oversize, OVERSIZE_THRESHOLD_BYTES,
};
use crate::video::VideoFrameReader;
use source::{FrameSource, ImageFrameSource, VideoFrameSource};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

/// Soft cap on in-flight frame bytes for image render batches.
const RANGE_MEMORY_BUDGET: usize = 64 * 1024 * 1024;

/// One input that could not be turned into frames, with the reason.
// Implements: LLR-017, SR-014
#[derive(Debug, Clone)]
pub struct SkippedInput {
    pub path: PathBuf,
    pub reason: String,
}

/// One successfully written output and its final on-disk size.
// Implements: LLR-023, LLR-024, SR-004
#[derive(Debug, Clone)]
pub struct WrittenOutput {
    pub path: PathBuf,
    pub size_bytes: u64,
}

/// Aggregated result of a build, used to drive the completion summary and the
/// process exit status.
// Implements: LLR-017, LLR-023, LLR-024, SR-004, SR-014
#[derive(Debug, Default)]
pub struct BuildSummary {
    pub written: Vec<WrittenOutput>,
    pub skipped: Vec<SkippedInput>,
}

impl BuildSummary {
    /// Final outputs whose size exceeds the ~3.5 GB FAT32 threshold.
    // Implements: LLR-013, SR-009
    pub fn oversize_outputs(&self) -> Vec<&WrittenOutput> {
        self.written
            .iter()
            .filter(|o| is_oversize(o.size_bytes))
            .collect()
    }
}

pub struct FrameGenerationPipeline {
    album: Album,
    outputs: Vec<OutputDef>,
    processing: ProcessingConfig,
    output_dir: PathBuf,
    /// Inputs skipped across all outputs (deduped by path) for the summary.
    skipped: RefCell<Vec<SkippedInput>>,
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
            skipped: RefCell::new(Vec::new()),
        }
    }

    /// Run every output definition, returning a [`BuildSummary`] of written
    /// files and skipped inputs. The directory check names the exact path on
    /// failure (LLR-019).
    // Implements: LLR-017, LLR-023, LLR-024, SR-004, SR-014
    pub async fn execute(&self) -> Result<BuildSummary> {
        ensure_dir_exists(&self.output_dir)?;

        let media: Vec<_> = self.album.media_files.iter().collect();

        let mut written = Vec::new();
        for output_def in &self.outputs {
            if let Some(out) = self.encode_output(output_def, &media)? {
                written.push(out);
            }
        }

        Ok(BuildSummary {
            written,
            skipped: self.skipped.borrow().clone(),
        })
    }

    /// Record a skipped input once (deduped by path) for the summary.
    // Implements: LLR-017, SR-014
    fn record_skip(&self, path: &std::path::Path, reason: String) {
        let mut skips = self.skipped.borrow_mut();
        if !skips.iter().any(|s| s.path == path) {
            skips.push(SkippedInput {
                path: path.to_path_buf(),
                reason,
            });
        }
    }

    /// Pre-run oversize warning for this output, using the documented duration
    /// formula (sum of clip durations minus transition overlaps).
    // Implements: LLR-013, SR-009
    fn warn_if_estimated_oversize(&self, output_def: &OutputDef, media: &[&MediaFile]) {
        let fade = output_def.fade_time_secs.max(0.0) as f64;
        let transitions = media.len().saturating_sub(1) as f64;
        let raw: f64 = media
            .iter()
            .map(|m| match m.file_type {
                MediaType::Image => output_def.pic_display_time_secs as f64,
                MediaType::Video => m.duration_secs.unwrap_or(0.0) as f64,
            })
            .sum();
        let duration = (raw - transitions * fade).max(0.0);
        let est = estimate_output_bytes(duration, output_def.quality_crf);
        if is_oversize(est) {
            log::warn!(
                "Output '{}' estimated at ~{} (> {} FAT32 limit); consider splitting, fewer/shorter clips, or a higher CRF",
                output_def.name,
                human_bytes(est),
                human_bytes(OVERSIZE_THRESHOLD_BYTES),
            );
        }
    }

    fn encode_output(
        &self,
        output_def: &OutputDef,
        media: &[&MediaFile],
    ) -> Result<Option<WrittenOutput>> {
        let out_path = self.output_dir.join(format!("{}.mp4", output_def.name));
        // Normalize to even dimensions before anything reaches ffmpeg.
        // Implements: LLR-010, SR-006
        let (width, height) = output_def.even_dims();
        log::info!(
            "Encoding '{}' -> {} ({}x{} @ {}fps, crf {}, {}-frame crossfade)",
            output_def.name,
            out_path.display(),
            width,
            height,
            output_def.fps,
            output_def.quality_crf,
            output_def.fade_frames(),
        );

        if media.is_empty() {
            log::warn!("No media to process for output '{}'", output_def.name);
            return Ok(None);
        }

        self.warn_if_estimated_oversize(output_def, media);

        let frame_bytes = (width * height * 3) as usize;
        let batch = (RANGE_MEMORY_BUDGET / frame_bytes.max(1)).max(1) as u32;

        let encoder = FfmpegEncoder::start(
            &out_path,
            width,
            height,
            output_def.fps,
            output_def.quality_crf,
            // Inactivity watchdog: abort a wedged ffmpeg after this many seconds
            // of no frame writes (0 disables). SR-013 / LLR-008.
            self.processing.ffmpeg_timeout_secs,
        )?;

        let mut mixer = CrossfadeMixer::new(encoder, output_def.fade_frames() as usize);

        let total = media.len();
        let start = Instant::now();
        // Source frames contributed across all clips for this output; if zero
        // (every input skipped/unusable) we must NOT finalize an empty file.
        // Implements: LLR-017, SR-014
        let mut produced_total: u64 = 0;

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
                        self.record_skip(&item.path, e.to_string());
                        continue;
                    }
                },
                MediaType::Video => {
                    match VideoFrameReader::open(&item.path, width, height, output_def.fps) {
                        Ok(rd) => Box::new(VideoFrameSource::new(rd)),
                        Err(e) => {
                            log::warn!("Skipping video {}: {}", item.path.display(), e);
                            self.record_skip(&item.path, e.to_string());
                            continue;
                        }
                    }
                }
            };

            let clip_frames = mixer.add_clip(src.as_mut())?;
            produced_total += clip_frames;
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

        // All inputs were skipped/unusable for this output: abort without
        // finalizing so no complete-looking (empty) `<name>.mp4` is produced.
        // Dropping the mixer drops the encoder, whose Drop removes the `.part`.
        // Implements: LLR-017, SR-014
        if produced_total == 0 {
            log::warn!(
                "No frames produced for '{}' (every input skipped/unusable); no output written",
                output_def.name
            );
            drop(mixer);
            return Ok(None);
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

        let size_bytes = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
        Ok(Some(WrittenOutput {
            path: out_path,
            size_bytes,
        }))
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
