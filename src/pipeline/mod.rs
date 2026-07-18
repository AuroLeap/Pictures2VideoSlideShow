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
use crate::ffmpeg::audio::{AudioClip, AudioParams};
use crate::ffmpeg::encoder_args::EncoderSettings;
use crate::ffmpeg::FfmpegEncoder;
use crate::image::FrameRenderer;
use crate::media::{Album, MediaFile, MediaType};
use crate::roi::RoiDb;
use crate::util::ensure_dir_exists;
use crate::util::estimate::{
    estimate_output_bytes, human_bytes, is_oversize, OVERSIZE_THRESHOLD_BYTES,
};
use crate::util::timing::{StageSnapshot, StageTimings};
use crate::video::VideoFrameReader;
use source::{FrameSource, ImageFrameSource, VideoFrameSource};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
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

/// Per-output stage totals and frame count, captured for the `--verbose`
/// summary and read by the bench runner (LLR-052).
// Implements: LLR-050, SR-036
#[derive(Debug, Clone)]
pub struct OutputTimings {
    pub name: String,
    /// Output frames emitted to the encoder (transitions included).
    pub frames: u64,
    pub stages: StageSnapshot,
}

/// Aggregated result of a build, used to drive the completion summary and the
/// process exit status.
// Implements: LLR-017, LLR-023, LLR-024, SR-004, SR-014
#[derive(Debug, Default)]
pub struct BuildSummary {
    pub written: Vec<WrittenOutput>,
    pub skipped: Vec<SkippedInput>,
    /// One entry per encoded output, in build order (SR-036).
    pub timings: Vec<OutputTimings>,
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
    /// Input media root, used to key ROI lookups by relative path.
    media_root: PathBuf,
    /// Optional region-of-interest database for per-image Ken Burns focus.
    // Implements: SR-031, LLR-038
    roi: Option<RoiDb>,
    /// Inputs skipped across all outputs (deduped by path) for the summary.
    skipped: RefCell<Vec<SkippedInput>>,
    /// Per-output stage timings collected during encode (SR-036).
    timings: RefCell<Vec<OutputTimings>>,
}

impl FrameGenerationPipeline {
    pub fn new(
        album: Album,
        outputs: Vec<OutputDef>,
        processing: ProcessingConfig,
        output_dir: PathBuf,
        media_root: PathBuf,
        roi: Option<RoiDb>,
    ) -> Self {
        Self {
            album,
            outputs,
            processing,
            output_dir,
            media_root,
            roi,
            skipped: RefCell::new(Vec::new()),
            timings: RefCell::new(Vec::new()),
        }
    }

    /// Resolve the Ken Burns focus for an image: its ROI-database entry (keyed
    /// by path relative to the media root) if present, else the configured
    /// `default_focus`. `None` keeps the default two-point pan.
    // Implements: SR-031, LLR-037, LLR-038
    fn focus_for(&self, path: &std::path::Path) -> Option<(f32, f32)> {
        if let Some(db) = &self.roi {
            let rel = path.strip_prefix(&self.media_root).unwrap_or(path);
            if let Some(f) = db.focus_for(rel) {
                return Some(f);
            }
        }
        self.processing.default_focus.map(|[x, y]| (x, y))
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
            timings: self.timings.borrow().clone(),
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

        // Resolve the encoder actually used (probe + fallback decision); a
        // hardware request that cannot be honored degrades to software with a
        // logged reason — never a build failure.
        // Implements: SR-034, LLR-046, LLR-048
        let selection = crate::ffmpeg::probe::selection_for(
            self.processing.ffmpeg_path.as_deref(),
            &output_def.encoder,
        );
        if let Some(reason) = &selection.fallback_reason {
            log::warn!("Output '{}': {}", output_def.name, reason);
        }
        log::info!(
            "Encoding '{}' -> {} ({}x{} @ {}fps, crf {}, {}-frame crossfade, encoder {})",
            output_def.name,
            out_path.display(),
            width,
            height,
            output_def.fps,
            output_def.quality_crf,
            output_def.fade_frames(),
            // `h264_nvenc` or `libx264 (fallback: <reason>)` per LLR-048.
            selection.describe(),
        );

        if media.is_empty() {
            log::warn!("No media to process for output '{}'", output_def.name);
            return Ok(None);
        }

        self.warn_if_estimated_oversize(output_def, media);

        let frame_bytes = (width * height * 3) as usize;
        let batch = (RANGE_MEMORY_BUDGET / frame_bytes.max(1)).max(1) as u32;

        // Per-output stage timers (SR-036): shared with the image sources
        // (render) and the mixer (blend, encode-write stall); decode/prescale
        // are read from each clip's load timings below.
        // Implements: LLR-050, SR-036
        let stage_timings = Arc::new(StageTimings::new());
        let enc_start = Instant::now();

        let encoder = FfmpegEncoder::start(
            &out_path,
            width,
            height,
            output_def.fps,
            // Implements: SR-034, SR-035, LLR-045, LLR-049
            &EncoderSettings {
                choice: selection.encoder,
                crf: output_def.quality_crf,
                x264_preset: output_def.x264_preset.clone(),
            },
            // Inactivity watchdog: abort a wedged ffmpeg after this many seconds
            // of no frame writes (0 disables). SR-013 / LLR-008.
            self.processing.ffmpeg_timeout_secs,
        )?;

        let mut mixer = CrossfadeMixer::new(
            encoder,
            output_def.fade_frames() as usize,
            Arc::clone(&stage_timings),
        );

        let total = media.len();
        let start = Instant::now();
        // Source frames contributed across all clips for this output; if zero
        // (every input skipped/unusable) we must NOT finalize an empty file.
        // Implements: LLR-017, SR-014
        let mut produced_total: u64 = 0;

        // Audio-bearing video clips and their output start frames, for the
        // optional audio passthrough mux after the silent video is built.
        // Implements: SR-032, LLR-040
        let mut audio_clips: Vec<AudioClip> = Vec::new();

        for (idx, item) in media.iter().enumerate() {
            let name = item
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            let mut src: Box<dyn FrameSource> = match item.file_type {
                MediaType::Image => match FrameRenderer::load_with_focus(
                    &item.path,
                    output_def,
                    self.focus_for(&item.path),
                ) {
                    Ok(r) => {
                        // The serial decode+prescale stall at this clip
                        // boundary (PB-002). Implements: LLR-050, SR-036
                        let lt = r.load_timings();
                        stage_timings.add_decode(lt.decode);
                        stage_timings.add_prescale(lt.prescale);
                        stage_timings.add_clip();
                        Box::new(ImageFrameSource::new(r, batch, Arc::clone(&stage_timings)))
                    }
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

            // Capture this clip's output start frame BEFORE mixing it in; for an
            // audio-bearing video that frame is the clip's audio delay (LLR-040).
            let start_frame = mixer.emitted();
            let clip_frames = mixer.add_clip(src.as_mut())?;
            if output_def.enable_audio && item.file_type == MediaType::Video && item.has_audio {
                audio_clips.push(AudioClip {
                    source: item.path.clone(),
                    start_frame,
                });
            }
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

        let (emitted, part) = mixer.finish()?;
        // finish() waits for ffmpeg to exit, so this is the encoder's wall time.
        // Implements: LLR-050, SR-036
        stage_timings.set_ffmpeg_wall(enc_start.elapsed());

        // Finalize: mux source-video audio onto the silent video when enabled and
        // there is audio to add, otherwise atomically promote the silent video.
        // Both paths make `<name>.mp4` appear atomically (SR-011).
        // Implements: SR-032, LLR-041
        if output_def.enable_audio && !audio_clips.is_empty() {
            let params = AudioParams {
                bitrate_kbps: output_def.audio_bitrate_kbps,
                sample_rate: output_def.audio_sample_rate,
            };
            log::info!(
                "Muxing audio from {} video clip(s) into '{}'",
                audio_clips.len(),
                output_def.name
            );
            crate::ffmpeg::audio::mux_audio(
                &part,
                &out_path,
                &audio_clips,
                output_def.fps,
                emitted,
                &params,
            )?;
        } else {
            if output_def.enable_audio {
                log::info!(
                    "Audio enabled for '{}' but no audio-bearing video clips found; output is silent",
                    output_def.name
                );
            }
            crate::ffmpeg::promote(&part, &out_path)?;
        }

        let elapsed = start.elapsed();
        log::info!(
            "Finished '{}': {} items, {} frames in {:.1}s ({:.1} frames/s)",
            output_def.name,
            total,
            emitted,
            elapsed.as_secs_f64(),
            emitted as f64 / elapsed.as_secs_f64().max(0.001),
        );
        // Per-stage totals: debug lines under --verbose, and a snapshot the
        // bench runner reads from the summary. Implements: LLR-050, SR-036
        stage_timings.log_summary(&output_def.name);
        self.timings.borrow_mut().push(OutputTimings {
            name: output_def.name.clone(),
            frames: emitted,
            stages: stage_timings.snapshot(),
        });

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
    /// Per-output stage timers (blend and encode-write stall). SR-036.
    timings: Arc<StageTimings>,
}

impl CrossfadeMixer {
    fn new(encoder: FfmpegEncoder, n: usize, timings: Arc<StageTimings>) -> Self {
        Self {
            encoder,
            n,
            prev_tail: Vec::new(),
            emitted: 0,
            timings,
        }
    }

    /// [`blend`] with the elapsed time accumulated into the blend stage.
    // Implements: LLR-050, SR-036
    fn timed_blend(&self, a: &[u8], b: &[u8], t: f32) -> Vec<u8> {
        let s = Instant::now();
        let out = blend(a, b, t);
        self.timings.add_blend(s.elapsed());
        out
    }

    /// [`scale`] with the elapsed time accumulated into the blend stage.
    // Implements: LLR-050, SR-036
    fn timed_scale(&self, a: &[u8], t: f32) -> Vec<u8> {
        let s = Instant::now();
        let out = scale(a, t);
        self.timings.add_blend(s.elapsed());
        out
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
                let frame = self.timed_scale(f, t);
                self.emit(frame)?;
            }
        } else {
            let prev = std::mem::take(&mut self.prev_tail);
            let m = prev.len().min(head.len());
            // Dissolve overlapping frames.
            for k in 0..m {
                let t = (k + 1) as f32 / (m + 1) as f32;
                let frame = self.timed_blend(&prev[k], &head[k], t);
                self.emit(frame)?;
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

    /// Output frames emitted so far. Read before each `add_clip` to learn the
    /// clip's output start frame (its audio delay for passthrough).
    // Implements: LLR-040, SR-032
    fn emitted(&self) -> u64 {
        self.emitted
    }

    /// Fade out the final clip's tail to black, flush the encoder to its
    /// validated `.part`, and return `(emitted_frames, part_path)`. The pipeline
    /// finalizes — promoting the silent part directly, or muxing audio first.
    // Implements: LLR-041, SR-011
    fn finish(mut self) -> Result<(u64, PathBuf)> {
        let prev = std::mem::take(&mut self.prev_tail);
        let len = prev.len();
        for (k, f) in prev.iter().enumerate() {
            let t = 1.0 - (k + 1) as f32 / (len + 1) as f32;
            let frame = self.timed_scale(f, t);
            self.emit(frame)?;
        }
        let emitted = self.emitted;
        let part = self.encoder.finish_to_part()?;
        Ok((emitted, part))
    }

    fn emit(&mut self, frame: Vec<u8>) -> Result<()> {
        // Time the stdin write: when ffmpeg's input buffer is full this is the
        // encoder-write stall the pipeline blocks on (LLR-050, SR-036).
        let s = Instant::now();
        self.encoder.write_frame(&frame)?;
        self.timings.add_write(s.elapsed());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn processing(default_focus: Option<[f32; 2]>) -> ProcessingConfig {
        ProcessingConfig {
            temp_dir: None,
            max_workers: None,
            use_parallelism: false,
            dry_run: false,
            verbose: false,
            ffmpeg_timeout_secs: 0,
            ffmpeg_path: None,
            default_focus,
        }
    }

    fn pipeline(
        media_root: &str,
        roi: Option<RoiDb>,
        default_focus: Option<[f32; 2]>,
    ) -> FrameGenerationPipeline {
        let album = Album {
            media_files: Vec::new(),
            total_size: 0,
            created_at: SystemTime::UNIX_EPOCH,
        };
        FrameGenerationPipeline::new(
            album,
            Vec::new(),
            processing(default_focus),
            PathBuf::from("out"),
            PathBuf::from(media_root),
            roi,
        )
    }

    // Verifies: SR-031, LLR-038 — an ROI entry (keyed by path relative to the
    // media root) drives the focus and takes precedence over the default.
    #[test]
    fn roi_entry_overrides_default_focus() {
        let db = RoiDb::from_json(r#"{ "a/img.jpg": { "x": 0.25, "y": 0.75 } }"#).unwrap();
        let p = pipeline("root", Some(db), Some([0.5, 0.5]));
        // Absolute path under the media root resolves via its relative key.
        let f = p.focus_for(&PathBuf::from("root/a/img.jpg")).unwrap();
        assert!((f.0 - 0.25).abs() < 1e-6 && (f.1 - 0.75).abs() < 1e-6);
    }

    // Verifies: SR-031, LLR-037 — images without an ROI entry fall back to the
    // configured default focus, and `None` keeps the default pan.
    #[test]
    fn falls_back_to_default_then_none() {
        let db = RoiDb::from_json(r#"{ "a/img.jpg": { "x": 0.1, "y": 0.1 } }"#).unwrap();
        let p = pipeline("root", Some(db), Some([0.5, 0.5]));
        // Miss -> default focus.
        assert_eq!(
            p.focus_for(&PathBuf::from("root/other.jpg")),
            Some((0.5, 0.5))
        );
        // No ROI db and no default -> None (default pan).
        let p2 = pipeline("root", None, None);
        assert_eq!(p2.focus_for(&PathBuf::from("root/a/img.jpg")), None);
    }
}
