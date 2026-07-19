//! Pipeline orchestration: wire media → frame generation → FFmpeg encoding,
//! with cross-fade (dissolve) transitions between consecutive clips.
//!
//! Two encode paths share one frame source, the streaming [`CrossfadeMixer`]:
//! the single-encoder streaming build (`--no-cache`), and the default SR-037
//! cached build — plan segments against the store, re-encode only miss runs,
//! assemble by stream-copy concat, then mux/promote. Both paths write through
//! the Phase-3 [`EncoderWriter`] thread (LLR-063), so rendering overlaps the
//! encoder pipe writes. The mixer only ever holds a rolling window of
//! `fade_frames` at each boundary, and every inter-stage buffer/channel is
//! bounded (see [`writer_channel_depth`]), so memory stays bounded regardless
//! of clip length or count — whole videos are never loaded.

mod overlap;
mod prefetch;
mod segment;
mod source;

pub use overlap::{writer_channel_depth, EncoderWriter};

use crate::cache::planner::{self, ClipFacts, SegmentPlan};
use crate::cache::store::SegmentStore;
use crate::cache::{EncodeParams, SourceIdentity};
use crate::config::{OutputDef, ProcessingConfig};
use crate::error::Result;
use crate::ffmpeg::audio::{AudioClip, AudioParams};
use crate::ffmpeg::encoder_args::EncoderSettings;
use crate::ffmpeg::FfmpegEncoder;
use crate::image::backend::{self, BackendSelection, RenderBackend};
use crate::image::{gpu, ClipRenderer, FrameRenderer, LoadedClip};
use crate::media::{Album, MediaFile, MediaType};
use crate::roi::RoiDb;
use crate::transport::FrameTransport;
use crate::util::ensure_dir_exists;
use crate::util::estimate::{
    estimate_output_bytes, human_bytes, is_oversize, OVERSIZE_THRESHOLD_BYTES,
};
use crate::util::timing::{StageSnapshot, StageTimings};
use crate::video::VideoFrameReader;
use prefetch::{ClipPrefetcher, PrefetchJob};
use segment::SegmentedEncoderSink;
use source::{FrameSource, ImageFrameSource, VideoFrameSource};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// Where the mixer's output frames go: the streaming encoder, the
/// segment-rolling sink of the SR-037 segmented build, or the [`EncoderWriter`]
/// thread wrapping either (LLR-063). `roll` is invoked at each transition
/// midpoint (a segment boundary); single-file sinks ignore it, which keeps the
/// streaming path byte-identical. Frames pass by value so the overlapped path
/// can move them into its channel without a copy.
// Implements: LLR-056, LLR-063, SR-037
pub trait FrameSink {
    /// Write one raw `rgb24` output frame.
    fn write(&mut self, frame: Vec<u8>) -> Result<()>;
    /// Transition-midpoint marker (plan §4 segment boundary). Default: no-op.
    fn roll(&mut self) -> Result<()> {
        Ok(())
    }
}

/// The streaming single-encoder sink: every frame goes to one FFmpeg process.
impl FrameSink for FfmpegEncoder {
    fn write(&mut self, frame: Vec<u8>) -> Result<()> {
        self.write_frame(&frame)
    }
}

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
    /// Cached-build evidence (SR-037): `(segments reused, segments
    /// re-encoded)`. `None` on the streaming (`--no-cache`) path.
    // Implements: LLR-055, SR-037
    pub cache: Option<(usize, usize)>,
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
    /// Render-backend selection, resolved and reported once per build
    /// (LLR-072; the adapter probe itself is cached process-wide, LLR-068).
    // Implements: LLR-072, SR-039
    render: std::cell::OnceCell<BackendSelection>,
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
            render: std::cell::OnceCell::new(),
        }
    }

    /// The render-backend selection for this build: resolved on first use
    /// (`cpu` never touches wgpu), reported exactly once — the backend-in-use
    /// line mirroring LLR-048's encoder line — with an explicit-`gpu` degrade
    /// additionally warned with the probe reason (SR-039 fallback leg).
    // Implements: LLR-068, LLR-072, SR-039
    fn render_selection(&self) -> &BackendSelection {
        self.render.get_or_init(|| {
            let sel = backend::select_for(&self.processing.render_backend);
            if let (Some(reason), "gpu") = (
                sel.fallback_reason.as_deref(),
                self.processing.render_backend.as_str(),
            ) {
                log::warn!("{reason}");
            }
            log::info!("Rendering with {}", sel.describe());
            sel
        })
    }

    /// Wrap a prefetched clip in the selected backend's renderer (LLR-066):
    /// GPU when selected, the process is not degraded (LLR-072), and the
    /// clip's prescale fits the device texture limit; otherwise the CPU
    /// reference renderer.
    // Implements: LLR-066, LLR-072, SR-039
    fn clip_renderer(&self, clip: LoadedClip) -> Box<dyn ClipRenderer> {
        if self.render_selection().backend == RenderBackend::Gpu && !gpu::degraded() {
            match gpu::context() {
                Ok(ctx) if ctx.fits_texture(&clip.plan) => {
                    return Box::new(gpu::GpuRenderer::new(ctx, clip));
                }
                Ok(_) => log::debug!(
                    "clip prescale exceeds the GPU texture limit; rendering this clip on cpu"
                ),
                // Unreachable when selection said Gpu; defensive.
                Err(e) => log::debug!("GPU context unavailable ({}); rendering on cpu", e.reason),
            }
        }
        Box::new(FrameRenderer::new(clip))
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
    pub fn execute(&self) -> Result<BuildSummary> {
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
        // Backend-in-use line, once per build (LLR-072, SR-039 reporting).
        self.render_selection();
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

        // One bounded depth sizes both the render batches and the
        // mixer->writer channel; it replaces the deleted RANGE_MEMORY_BUDGET
        // as the in-flight-frame memory cap (PB-005). Implements: LLR-063
        let frame_bytes = (width * height * 3) as usize;
        let depth = writer_channel_depth(output_def.fade_frames() as usize, frame_bytes);
        let batch = depth as u32;

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
                transport: FrameTransport::Rgb24,
            },
            // Inactivity watchdog: abort a wedged ffmpeg after this many seconds
            // of no frame writes (0 disables). SR-013 / LLR-008.
            self.processing.ffmpeg_timeout_secs,
        )?;

        // Phase-3 overlap (LLR-063): the writer thread owns ffmpeg stdin;
        // the mixer only ever blocks on the bounded channel (back-pressure),
        // which emit() times as the encode-write stall.
        let writer = EncoderWriter::start(encoder, depth, Arc::clone(&stage_timings));
        let mut mixer = CrossfadeMixer::new(
            writer,
            output_def.fade_frames() as usize,
            Arc::clone(&stage_timings),
            FrameTransport::Rgb24,
            (width * height) as usize,
        );

        let total = media.len();
        let start = Instant::now();
        let outcome = self.drive_clips(output_def, media, &mut mixer, batch, &stage_timings)?;
        let (produced_total, audio_clips) = (outcome.produced, outcome.audio);

        // All inputs were skipped/unusable for this output: abort without
        // finalizing so no complete-looking (empty) `<name>.mp4` is produced.
        // Dropping the mixer drops the writer (joins its thread), which drops
        // the encoder, whose Drop removes the `.part` (LLR-064).
        // Implements: LLR-017, SR-014
        if produced_total == 0 {
            log::warn!(
                "No frames produced for '{}' (every input skipped/unusable); no output written",
                output_def.name
            );
            drop(mixer);
            return Ok(None);
        }

        let (emitted, writer) = mixer.finish()?;
        // Join re-raises any writer-thread failure with serial semantics
        // (LLR-064) and hands the encoder back for finalize.
        let encoder = writer.join()?;
        let part = encoder.finish_to_part()?;
        // finish_to_part waits for ffmpeg to exit: the encoder's wall time.
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
            cache: None,
        }))
    }

    /// Drive every media item of one output through `mixer`: prefetch image
    /// clips in the background (LLR-060), join each at its own item, skip and
    /// record failures (SR-014), and capture audio-bearing clips' output start
    /// frames (LLR-040). Zero `produced` means every input was skipped and
    /// nothing may finalize. Shared verbatim by the streaming and segmented
    /// encode paths so the two builds emit an identical frame sequence
    /// (SR-022 determinism); the cached path additionally reads the per-clip
    /// actual frame counts (LLR-055).
    // Implements: LLR-017, LLR-060, LLR-040, SR-014, SR-032, SR-036
    fn drive_clips<S: FrameSink>(
        &self,
        output_def: &OutputDef,
        media: &[&MediaFile],
        mixer: &mut CrossfadeMixer<S>,
        batch: u32,
        stage_timings: &Arc<StageTimings>,
    ) -> Result<DriveOutcome> {
        let (width, height) = output_def.even_dims();
        let total = media.len();
        // Source frames contributed across all clips for this output; if zero
        // (every input skipped/unusable) we must NOT finalize an empty file.
        // Implements: LLR-017, SR-014
        let mut produced_total: u64 = 0;

        // Per-clip actual source frame counts, aligned with `media`; `None`
        // marks a skipped clip. The cached path records these as the exact
        // layout inputs (LLR-055).
        let mut clip_frames: Vec<Option<u64>> = vec![None; media.len()];

        // Audio-bearing video clips and their output start frames, for the
        // optional audio passthrough mux after the silent video is built.
        // Implements: SR-032, LLR-040
        let mut audio_clips: Vec<AudioClip> = Vec::new();

        // Background prefetch of every image clip, in media order, with the
        // Ken Burns focus resolved here (main thread, ROI db) so the worker
        // only decodes+prescales. The loop below joins each result at its item,
        // timing the blocking wait as the PB-002 boundary stall.
        // Implements: LLR-060, SR-014, SR-036
        let jobs: Vec<PrefetchJob> = media
            .iter()
            .enumerate()
            .filter(|(_, m)| m.file_type == MediaType::Image)
            .map(|(index, m)| PrefetchJob {
                index,
                path: m.path.clone(),
                focus: self.focus_for(&m.path),
            })
            .collect();
        let mut prefetcher = ClipPrefetcher::spawn(jobs, output_def.clone());

        for (idx, item) in media.iter().enumerate() {
            let name = item
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            let mut src: Box<dyn FrameSource> = match item.file_type {
                MediaType::Image => {
                    // Join the prefetched load for THIS item: the worker feeds
                    // results in media order, so the received index always
                    // matches. Only the blocking wait is boundary dead time
                    // (PB-002); decode/prescale accrue their raw overlapped
                    // time below. Implements: LLR-060, LLR-050, SR-036
                    let wait = Instant::now();
                    let (job_idx, result) = prefetcher
                        .next()
                        .expect("prefetcher yields one result per image item");
                    stage_timings.add_stall(wait.elapsed());
                    debug_assert_eq!(job_idx, idx, "prefetch results must arrive in media order");
                    match result {
                        Ok(clip) => {
                            let lt = clip.timings;
                            stage_timings.add_decode(lt.decode);
                            stage_timings.add_prescale(lt.prescale);
                            stage_timings.add_clip();
                            // Wrap the backend-blind clip in the selected
                            // backend's renderer here — never on the prefetch
                            // worker. Implements: LLR-066, SR-039
                            Box::new(ImageFrameSource::new(
                                self.clip_renderer(clip),
                                batch,
                                Arc::clone(stage_timings),
                            ))
                        }
                        // A failed prefetch surfaces here, at its own item, and
                        // routes through the same skip path as a serial load
                        // failure. Implements: LLR-060, LLR-016, LLR-017, SR-014
                        Err(e) => {
                            log::warn!("Skipping image {}: {}", item.path.display(), e);
                            self.record_skip(&item.path, e.to_string());
                            continue;
                        }
                    }
                }
                MediaType::Video => {
                    match VideoFrameReader::open(
                        &item.path,
                        width,
                        height,
                        output_def.fps,
                        FrameTransport::Rgb24,
                    ) {
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
            let produced = mixer.add_clip(src.as_mut())?;
            clip_frames[idx] = Some(produced);
            if output_def.enable_audio && item.file_type == MediaType::Video && item.has_audio {
                audio_clips.push(AudioClip {
                    source: item.path.clone(),
                    start_frame,
                });
            }
            produced_total += produced;
            log::info!(
                "  [{}/{}] {} {} ({} frames)",
                idx + 1,
                total,
                match item.file_type {
                    MediaType::Image => "[img]",
                    MediaType::Video => "[vid]",
                },
                name,
                produced,
            );
        }

        Ok(DriveOutcome {
            produced: produced_total,
            audio: audio_clips,
            clip_frames,
        })
    }

    /// Run every output through the SR-037 cache-aware segmented path against
    /// one shared `store` (keys embed the output parameters, so outputs
    /// coexist), then LRU-prune the store to the configured cap and persist
    /// its index. The default `build` path; `--no-cache` uses
    /// [`Self::execute`] instead (neither reads nor writes the store).
    // Implements: LLR-054, LLR-055, SR-037, SR-014
    pub fn execute_cached(&self, store: &mut SegmentStore) -> Result<BuildSummary> {
        ensure_dir_exists(&self.output_dir)?;
        let mut written = Vec::new();
        for output_def in &self.outputs {
            if let Some(build) = self.execute_segmented(output_def, store)? {
                let size_bytes = std::fs::metadata(&build.output)
                    .map(|m| m.len())
                    .unwrap_or(0);
                written.push(WrittenOutput {
                    path: build.output,
                    size_bytes,
                    cache: Some((build.reused, build.re_encoded)),
                });
            }
        }
        // Bound the cache (SR-037) and persist the index; a save failure is
        // logged, never a build failure — the outputs are already promoted.
        let cap_bytes = (self.processing.segment_cache_gb * 1e9) as u64;
        store.prune_to(cap_bytes);
        if let Err(e) = store.save() {
            log::warn!("Segment cache index not saved (build unaffected): {}", e);
        }
        Ok(BuildSummary {
            written,
            skipped: self.skipped.borrow().clone(),
            timings: self.timings.borrow().clone(),
        })
    }

    /// The per-clip planner facts for the current effective media list: cache
    /// identity, applied Ken Burns focus, best-known frame count (exact for
    /// images and previously-encoded videos; probed-duration estimate
    /// otherwise), and audio presence.
    // Implements: LLR-055, SR-037
    fn clip_facts(
        &self,
        output_def: &OutputDef,
        media: &[&MediaFile],
        store: &mut SegmentStore,
    ) -> Vec<ClipFacts> {
        let image_frames = output_def.total_frames() as u64;
        media
            .iter()
            .map(|m| {
                let identity = SourceIdentity::of(m);
                let is_image = m.file_type == MediaType::Image;
                let (frames, frames_exact) = if is_image {
                    (image_frames, true)
                } else {
                    match store.clip_frames(&identity, output_def.fps) {
                        Some(f) => (f, true),
                        None => (
                            planner::estimate_video_frames(m.duration_secs, output_def.fps),
                            false,
                        ),
                    }
                };
                ClipFacts {
                    focus: if is_image {
                        self.focus_for(&m.path)
                    } else {
                        None
                    },
                    identity,
                    frames,
                    frames_exact,
                    has_audio: !is_image && m.has_audio,
                }
            })
            .collect()
    }

    /// Build one output via the SR-037 segment cache: plan hit/miss against
    /// `store`, encode only the miss runs (each through the mixer +
    /// [`SegmentedEncoderSink`], so frames are identical to the streaming
    /// build — SR-022), assemble hits + fresh segments by stream-copy concat
    /// (LLR-056), map audio delays through the concat timeline into the
    /// unchanged mux (LLR-057/LLR-040/LLR-041), and promote atomically
    /// (SR-011). A clip that fails mid-run is skipped and the album re-planned
    /// without it (SR-014 — its neighbors' keys change, so exactly the
    /// affected segments re-encode). Returns `None` when there is no media or
    /// every input was skipped.
    // Implements: LLR-053, LLR-054, LLR-055, LLR-056, LLR-057, SR-037, SR-011, SR-013, SR-014, SR-032
    pub fn execute_segmented(
        &self,
        output_def: &OutputDef,
        store: &mut SegmentStore,
    ) -> Result<Option<SegmentedBuild>> {
        ensure_dir_exists(&self.output_dir)?;
        let mut media: Vec<&MediaFile> = self.album.media_files.iter().collect();
        if media.is_empty() {
            log::warn!("No media to process for output '{}'", output_def.name);
            return Ok(None);
        }

        let (width, height) = output_def.even_dims();
        let out_path = self.output_dir.join(format!("{}.mp4", output_def.name));
        // Same encoder resolution as the streaming path (SR-034 fallback); the
        // *resolved* encoder enters every segment key (LLR-053).
        let selection = crate::ffmpeg::probe::selection_for(
            self.processing.ffmpeg_path.as_deref(),
            &output_def.encoder,
        );
        if let Some(reason) = &selection.fallback_reason {
            log::warn!("Output '{}': {}", output_def.name, reason);
        }
        // Backend-in-use line, once per build (LLR-072, SR-039 reporting).
        self.render_selection();
        log::info!(
            "Encoding '{}' (cached segments) -> {} ({}x{} @ {}fps, crf {}, encoder {})",
            output_def.name,
            out_path.display(),
            width,
            height,
            output_def.fps,
            output_def.quality_crf,
            selection.describe(),
        );
        self.warn_if_estimated_oversize(output_def, &media);

        let params = EncodeParams::of(
            output_def,
            selection.encoder.codec_name(),
            FrameTransport::Rgb24,
        );
        let engine = crate::cache::engine_version();
        let enc = EncoderSettings {
            choice: selection.encoder,
            crf: output_def.quality_crf,
            x264_preset: output_def.x264_preset.clone(),
            transport: FrameTransport::Rgb24,
        };
        // Same bounded depth as the streaming path (LLR-063): sizes render
        // batches and each run's mixer->writer channel.
        let fade = output_def.fade_frames() as usize;
        let frame_bytes = (width * height * 3) as usize;
        let batch = writer_channel_depth(fade, frame_bytes) as u32;
        let stage_timings = Arc::new(StageTimings::new());
        let start = Instant::now();

        // Keys committed (re-encoded) during THIS build — the honest
        // re-encode count for the summary (TC-081 evidence).
        let mut committed: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Plan → encode misses → re-plan until everything is a hit. Each
        // iteration either commits segments, records exact frame counts, or
        // removes a skipped clip, so the loop converges; the bound fails
        // loudly rather than spinning (SR-014 fail-loudly spirit).
        let mut iterations = 0usize;
        let (plan, facts) = loop {
            iterations += 1;
            if iterations > 2 * self.album.media_files.len() + 4 {
                return Err(crate::error::SlideshowError::Processing(format!(
                    "segment planning for '{}' did not converge; re-run with --no-cache",
                    output_def.name
                )));
            }
            if media.is_empty() {
                log::warn!(
                    "No frames produced for '{}' (every input skipped/unusable); no output written",
                    output_def.name
                );
                return Ok(None);
            }
            let facts = self.clip_facts(output_def, &media, store);
            let plan = planner::plan_segments(&facts, fade, &params, &engine, |k| store.lookup(k));
            log::debug!(
                "Output '{}': plan {} segments, {} hits, {} misses (SR-037)",
                output_def.name,
                plan.segments.len(),
                plan.hits(),
                plan.misses(),
            );
            let miss_runs = miss_runs(&plan);
            if miss_runs.is_empty() {
                break (plan, facts);
            }
            for run in miss_runs {
                match self.encode_run(
                    output_def,
                    &media,
                    &facts,
                    &plan,
                    run,
                    store,
                    &enc,
                    &params,
                    &engine,
                    batch,
                    &stage_timings,
                    &mut committed,
                )? {
                    RunOutcome::Committed => {}
                    // Short-circuit the remaining runs — their plan is stale.
                    RunOutcome::Replan => break,
                    RunOutcome::Skipped(paths) => {
                        media.retain(|m| !paths.contains(&m.path));
                        break;
                    }
                }
            }
            // Loop again unconditionally: the committed segments (and recorded
            // actual counts) turn this plan's misses into hits, and the final
            // all-hit plan is the assembly truth.
        };

        // Assemble: every planned segment is now a validated store hit.
        let mut segments = Vec::with_capacity(plan.segments.len());
        let mut boundaries = Vec::new();
        let mut total_frames: u64 = 0;
        for seg in &plan.segments {
            let (path, frames) = seg
                .stored
                .clone()
                .expect("all-hit plan: every segment has a stored file");
            if total_frames > 0 {
                boundaries.push(total_frames);
            }
            total_frames += frames;
            segments.push(path);
        }
        let reused = plan
            .segments
            .iter()
            .filter(|s| !committed.contains(&s.key))
            .count();
        let re_encoded = plan.segments.len() - reused;

        let part = crate::ffmpeg::concat::concat_segments(
            &segments,
            &out_path,
            self.processing.ffmpeg_timeout_secs,
        )?;

        // Audio timeline (LLR-057): audio-bearing clips' output start frames
        // come from the plan's frame offsets — the streaming path's
        // mixer.emitted() capture (LLR-040) mapped through the concat plan —
        // and feed the existing delay_ms/mux_audio unchanged (LLR-041), on
        // the assembled .part before the atomic promote (SR-011).
        // Implements: LLR-057, SR-032, SR-011
        let audio_clips = if output_def.enable_audio {
            if facts.iter().any(|f| f.has_audio && !f.frames_exact) {
                // Only possible when a clip_frames record was pruned while its
                // segment survived — starts could drift by ±1 frame until the
                // next re-encode records them again.
                log::debug!(
                    "Output '{}': some audio start frames derive from estimated clip lengths",
                    output_def.name
                );
            }
            plan.audio_clips(&facts)
        } else {
            Vec::new()
        };
        if output_def.enable_audio && !audio_clips.is_empty() {
            let audio_params = AudioParams {
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
                total_frames,
                &audio_params,
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
        // Same end-of-output frames/s line as the streaming path (TC-072
        // contract), plus the SR-037 reuse evidence (TC-081).
        log::info!(
            "Finished '{}': {} items, {} frames in {:.1}s ({:.1} frames/s) — segments: {} reused, {} re-encoded (SR-037)",
            output_def.name,
            media.len(),
            total_frames,
            elapsed.as_secs_f64(),
            total_frames as f64 / elapsed.as_secs_f64().max(0.001),
            reused,
            re_encoded,
        );
        stage_timings.log_summary(&output_def.name);
        self.timings.borrow_mut().push(OutputTimings {
            name: output_def.name.clone(),
            frames: total_frames,
            stages: stage_timings.snapshot(),
        });

        Ok(Some(SegmentedBuild {
            output: out_path,
            segments,
            boundaries,
            frames: total_frames,
            reused,
            re_encoded,
        }))
    }

    /// Encode one contiguous run of planned miss segments: render the run's
    /// member clips plus the adjacent context clip on each side (their
    /// overlapping transition frames are part of the segments), roll at the
    /// mixer-signalled midpoints, and commit each non-context segment into the
    /// store under its key. Frame identity with the streaming build follows
    /// from the shared mixer/renderer and per-path determinism (SR-022).
    // Implements: LLR-055, LLR-056, SR-037, SR-014, SR-022
    #[allow(clippy::too_many_arguments)] // internal step fn: the run context is wide by nature
    fn encode_run(
        &self,
        output_def: &OutputDef,
        media: &[&MediaFile],
        facts: &[ClipFacts],
        plan: &SegmentPlan,
        run: std::ops::Range<usize>,
        store: &mut SegmentStore,
        enc: &EncoderSettings,
        params: &EncodeParams,
        engine: &str,
        batch: u32,
        stage_timings: &Arc<StageTimings>,
        committed: &mut std::collections::HashSet<String>,
    ) -> Result<RunOutcome> {
        let fade = output_def.fade_frames() as usize;
        let first_clip = plan.segments[run.start].members.start;
        let last_clip = plan.segments[run.end - 1].members.end;
        // A non-first segment always opens at a transition midpoint, so the
        // previous clip's tail frames are inside it — include that clip as
        // context (and symmetrically on the right). Context partials are
        // discarded after the run.
        let ctx_left = first_clip > 0;
        let ctx_right = last_clip < media.len();
        let sub_start = first_clip - usize::from(ctx_left);
        let sub_end = last_clip + usize::from(ctx_right);
        let sub = &media[sub_start..sub_end];

        let staging = store.staging_dir()?;
        // Best-effort staging cleanup on every exit path.
        struct StagingGuard(PathBuf);
        impl Drop for StagingGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _guard = StagingGuard(staging.clone());

        let (width, height) = output_def.even_dims();
        let sink = SegmentedEncoderSink::start(
            &staging,
            &output_def.name,
            width,
            height,
            output_def.fps,
            enc.clone(),
            self.processing.ffmpeg_timeout_secs,
        )?;
        // The segmented sink goes through the same writer thread as the
        // streaming path (LLR-063); rolls queue in-order with the frames so
        // segment boundaries land on exactly the serial frame.
        let writer = EncoderWriter::start(sink, batch as usize, Arc::clone(stage_timings));
        let mut mixer = CrossfadeMixer::new(
            writer,
            fade,
            Arc::clone(stage_timings),
            FrameTransport::Rgb24,
            (width * height) as usize,
        );
        let outcome = self.drive_clips(output_def, sub, &mut mixer, batch, stage_timings)?;

        // Record every actual video frame count first — even if we replan,
        // exact counts make the next plan's layout exact (LLR-055).
        for (m, cf) in sub.iter().zip(&outcome.clip_frames) {
            if let (MediaType::Video, Some(f)) = (m.file_type, cf) {
                store.record_clip_frames(&SourceIdentity::of(m), output_def.fps, *f);
            }
        }

        let skipped: Vec<PathBuf> = sub
            .iter()
            .zip(&outcome.clip_frames)
            .filter(|(_, cf)| cf.is_none())
            .map(|(m, _)| m.path.clone())
            .collect();
        if !skipped.is_empty() {
            // drive_clips already logged + recorded the skips (SR-014); the
            // caller drops the clips and re-plans — the neighbors' keys change
            // so exactly the affected segments re-encode.
            drop(mixer);
            return Ok(RunOutcome::Skipped(skipped));
        }

        let (_emitted, writer) = mixer.finish()?;
        // Join re-raises any writer-thread failure (LLR-064) before the run's
        // segments are trusted.
        let sink = writer.join()?;
        let (seg_paths, _sink_boundaries) = sink.finish_all()?;

        // Ground truth for this run: the layout over the ACTUAL counts. Its
        // groups must match what the sink produced; each non-context group is
        // a full-album segment (transition math is local to a clip and its
        // neighbors, all present here) and is committed under its key.
        let actual: Vec<u64> = outcome.clip_frames.iter().map(|c| c.unwrap_or(0)).collect();
        let sub_layout = planner::clip_layout(&actual, fade);
        let groups = planner::segment_groups(sub.len(), &sub_layout);
        if groups.len() != seg_paths.len() {
            log::warn!(
                "Output '{}': segment run produced {} segments where {} were expected; re-planning",
                output_def.name,
                seg_paths.len(),
                groups.len()
            );
            return Ok(RunOutcome::Replan);
        }
        let mut seg_starts: Vec<u64> = vec![0];
        seg_starts.extend(sub_layout.boundaries.iter().map(|&(_, b)| b));

        let mut committed_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        for (gi, g) in groups.iter().enumerate() {
            let contains_ctx = (ctx_left && g.start == 0) || (ctx_right && g.end == sub.len());
            if contains_ctx {
                continue; // context partial — not a full-album segment
            }
            let album_members = (sub_start + g.start)..(sub_start + g.end);
            let key = planner::group_key(facts, &album_members, params, engine);
            let end = seg_starts.get(gi + 1).copied().unwrap_or(sub_layout.total);
            store.commit(&key, &seg_paths[gi], end - seg_starts[gi])?;
            committed.insert(key);
            committed_ranges.push(album_members);
        }

        // If a frame-count estimate was wrong enough to change the grouping
        // (e.g. a video turned out shorter than the transition), the committed
        // set differs from the planned one — re-plan with the now-exact
        // counts; the committed segments are picked up as hits.
        let planned_ranges: Vec<std::ops::Range<usize>> = run
            .clone()
            .map(|si| plan.segments[si].members.clone())
            .collect();
        if committed_ranges != planned_ranges {
            log::info!(
                "Output '{}': actual clip lengths changed the segment grouping; re-planning",
                output_def.name
            );
            return Ok(RunOutcome::Replan);
        }
        Ok(RunOutcome::Committed)
    }
}

/// Outcome of driving one media list through the mixer (see
/// [`FrameGenerationPipeline::drive_clips`]).
// Implements: LLR-017, LLR-040, LLR-055, SR-014
struct DriveOutcome {
    /// Source frames contributed across all clips (0 = everything skipped).
    produced: u64,
    /// Audio-bearing clips with their mixer-captured start frames (LLR-040).
    audio: Vec<AudioClip>,
    /// Per-clip actual source frame counts (`None` = skipped), aligned with
    /// the input media slice.
    clip_frames: Vec<Option<u64>>,
}

/// How one miss-run encode ended (see
/// [`FrameGenerationPipeline::encode_run`]).
enum RunOutcome {
    /// Every planned segment of the run is committed to the store.
    Committed,
    /// Structure diverged from the plan (now-recorded exact counts fix it) —
    /// plan again.
    Replan,
    /// These clips failed to load/decode (SR-014): drop them and plan again.
    Skipped(Vec<PathBuf>),
}

/// Contiguous runs of miss segments in `plan`, as segment-index ranges —
/// adjacent misses share one encode run (and its context clips).
// Implements: LLR-055, SR-037
fn miss_runs(plan: &SegmentPlan) -> Vec<std::ops::Range<usize>> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (i, seg) in plan.segments.iter().enumerate() {
        match (seg.hit, start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                runs.push(s..i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push(s..plan.segments.len());
    }
    runs
}

/// What a segmented build produced: the promoted output, the ordered segment
/// files it was assembled from, the output frame index where each segment
/// after the first begins (the transition midpoints), the total frames, and
/// the reuse split (SR-037 warm-build evidence).
// Implements: LLR-055, LLR-056, SR-037
// lib-API: fields read by tests/concat_seam.rs and tests/segment_cache.rs;
// the binary consumes only the summary counts via execute_cached.
#[allow(dead_code)]
#[derive(Debug)]
pub struct SegmentedBuild {
    pub output: PathBuf,
    pub segments: Vec<PathBuf>,
    pub boundaries: Vec<u64>,
    pub frames: u64,
    /// Segments served from the cache this build.
    pub reused: usize,
    /// Segments encoded this build.
    pub re_encoded: usize,
}

/// Drive the real [`CrossfadeMixer`] over synthetic fixed-length clips and
/// capture its observable frame arithmetic — the ground truth the pure
/// `cache::planner::clip_layout` is verified against (TC-076/TC-077), so the
/// planner and mixer cannot drift apart.
// Implements: LLR-055, LLR-057, SR-037 (test support)
#[cfg(test)]
pub(crate) struct MixerProbe {
    /// `mixer.emitted()` before each clip — the LLR-040 start frames.
    pub starts: Vec<u64>,
    /// Output frame index at each roll (transition midpoint).
    pub rolls: Vec<u64>,
    /// Total frames emitted (finish included).
    pub total: u64,
}

#[cfg(test)]
pub(crate) fn mixer_test_probe(counts: &[u64], fade: usize) -> MixerProbe {
    struct CountSink {
        written: u64,
        rolls: Vec<u64>,
    }
    impl FrameSink for CountSink {
        fn write(&mut self, _frame: Vec<u8>) -> Result<()> {
            self.written += 1;
            Ok(())
        }
        fn roll(&mut self) -> Result<()> {
            self.rolls.push(self.written);
            Ok(())
        }
    }
    struct FixedSource {
        left: u64,
    }
    impl FrameSource for FixedSource {
        fn next_frame(&mut self) -> Result<Option<Vec<u8>>> {
            if self.left == 0 {
                return Ok(None);
            }
            self.left -= 1;
            Ok(Some(vec![0u8; 3]))
        }
    }

    let sink = CountSink {
        written: 0,
        rolls: Vec::new(),
    };
    // The probe counts frames only — transport/luma_len are inert here.
    let mut mixer = CrossfadeMixer::new(
        sink,
        fade,
        Arc::new(StageTimings::new()),
        FrameTransport::Rgb24,
        1,
    );
    let mut starts = Vec::with_capacity(counts.len());
    for &c in counts {
        starts.push(mixer.emitted());
        mixer
            .add_clip(&mut FixedSource { left: c })
            .expect("counting sink cannot fail");
    }
    let (total, sink) = mixer.finish().expect("counting sink cannot fail");
    MixerProbe {
        starts,
        rolls: sink.rolls,
        total,
    }
}

/// Streams clips into a [`FrameSink`], dissolving the tail of each clip into
/// the head of the next over `n` frames. The first clip fades in from black
/// and the last fades out to black. At each transition midpoint the sink's
/// `roll` is signalled — the plan §4 segment boundary (no-op when streaming).
struct CrossfadeMixer<S: FrameSink> {
    sink: S,
    n: usize,
    /// The held tail (last <= n frames) of the previously emitted clip, awaiting
    /// the next clip to dissolve into. Empty before the first clip.
    prev_tail: Vec<Vec<u8>>,
    emitted: u64,
    /// Per-output stage timers (blend and encode-write stall). SR-036.
    timings: Arc<StageTimings>,
    /// The run's frame transport, plumbed in explicitly (never guessed from
    /// buffer sizes) so black-fades scale toward the right per-plane black
    /// (LLR-077); dissolves stay format-blind per-byte lerps.
    // Implements: LLR-077, SR-040
    transport: FrameTransport,
    /// Y-plane length (`w*h`) — where the U/V planes begin in a yuv frame.
    luma_len: usize,
}

impl<S: FrameSink> CrossfadeMixer<S> {
    fn new(
        sink: S,
        n: usize,
        timings: Arc<StageTimings>,
        transport: FrameTransport,
        luma_len: usize,
    ) -> Self {
        Self {
            sink,
            n,
            prev_tail: Vec::new(),
            emitted: 0,
            timings,
            transport,
            luma_len,
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
        let out = scale(a, t, self.transport, self.luma_len);
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
            // Dissolve overlapping frames; surplus head frames (this clip's
            // head ran longer than the previous tail — a short previous clip)
            // are ordinary body, consumed by value with no clone (LLR-062).
            for (k, f) in head.into_iter().enumerate() {
                if k < m {
                    // Transition midpoint: the frame at floor(m/2) opens the
                    // next segment ("second half of the incoming transition"
                    // belongs to the new clip's segment, plan §4). No-op for
                    // the streaming sink. Implements: LLR-056, SR-037
                    if k == m / 2 {
                        self.sink.roll()?;
                    }
                    let t = (k + 1) as f32 / (m + 1) as f32;
                    let frame = self.timed_blend(&prev[k], &f, t);
                    self.emit(frame)?;
                } else {
                    self.emit(f)?;
                }
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

    /// Fade out the final clip's tail to black and hand back
    /// `(emitted_frames, sink)`. The caller finalizes the sink — flushing the
    /// streaming encoder to its `.part` (then promoting or muxing audio), or
    /// closing the last segment and concat-assembling.
    // Implements: LLR-041, SR-011
    fn finish(mut self) -> Result<(u64, S)> {
        let prev = std::mem::take(&mut self.prev_tail);
        let len = prev.len();
        for (k, f) in prev.iter().enumerate() {
            let t = 1.0 - (k + 1) as f32 / (len + 1) as f32;
            let frame = self.timed_scale(f, t);
            self.emit(frame)?;
        }
        Ok((self.emitted, self.sink))
    }

    fn emit(&mut self, frame: Vec<u8>) -> Result<()> {
        // Time the sink write: the time the mixer blocks on the encode side.
        // Through the EncoderWriter this is the bounded-channel back-pressure
        // wait (the raw pipe time accrues on the writer thread as pipe-write);
        // for a direct sink it is the pipe write itself. Same meaning either
        // way: dead time the pipeline spends blocked on the encoder
        // (LLR-050, LLR-063, SR-036).
        let s = Instant::now();
        self.sink.write(frame)?;
        self.timings.add_write(s.elapsed());
        self.emitted += 1;
        Ok(())
    }
}

/// Quantize a blend factor `t` in `[0,1]` to a fixed-point weight in `0..=256`
/// (8.8 fixed point; 256 = fully the second frame). Quantization moves
/// transition bytes at most ±1 LSB vs the f32 reference — motion parameters
/// (SR-022 determinism) are untouched.
// Implements: LLR-062, SR-036
fn blend_weight(t: f32) -> u16 {
    (t.clamp(0.0, 1.0) * 256.0).round() as u16
}

/// Linear cross-dissolve: `out = a*(1-t) + b*t` per channel byte, computed in
/// u16 fixed point (`(a*(256-w) + b*w + 128) >> 8`). The max intermediate is
/// `255*256 + 128 = 65408`, which fits u16; `+128` rounds to nearest.
// Implements: LLR-062, SR-036
fn blend(a: &[u8], b: &[u8], t: f32) -> Vec<u8> {
    let w = blend_weight(t);
    let inv = 256 - w;
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| ((x as u16 * inv + y as u16 * w + 128) >> 8) as u8)
        .collect()
}

/// The one fixed-point fade kernel (LLR-077): `out = x*t + target*(1-t)` as
/// `(x*w + target*(256-w) + 128) >> 8`. `target = 0` reproduces the
/// pre-SR-040 scale-toward-black byte for byte; the max intermediate is
/// `255*256 + 128 = 65408` (since `target <= 255`), which fits u16.
// Implements: LLR-062, LLR-077, SR-036, SR-040
#[inline]
fn fade_byte(x: u8, w: u16, target: u16) -> u8 {
    ((x as u16 * w + target * (256 - w) + 128) >> 8) as u8
}

/// Format-aware brightness scale toward/from black — the ONE fade-to-black
/// implementation (LLR-077): `Rgb24` scales every byte toward 0
/// (byte-identical to the pre-SR-040 form, the `b = 0` special case of
/// [`blend`]); `Yuv420p` scales each plane toward its black level — Y toward
/// [`Y_BLACK`] (16, limited-range black) and U/V toward [`UV_NEUTRAL`] (128),
/// so a fully-faded frame has U = V = 128 exactly and never tints
/// green/purple (SR-040 chroma-neutral-fade leg). `luma_len` is the Y-plane
/// length (`w*h`); ignored on rgb.
// Implements: LLR-062, LLR-077, SR-036, SR-040
fn scale(a: &[u8], t: f32, transport: FrameTransport, luma_len: usize) -> Vec<u8> {
    let w = blend_weight(t);
    match transport {
        FrameTransport::Rgb24 => a.iter().map(|&x| fade_byte(x, w, 0)).collect(),
        FrameTransport::Yuv420p => {
            use crate::image::yuv::{UV_NEUTRAL, Y_BLACK};
            let luma = luma_len.min(a.len());
            let mut out = Vec::with_capacity(a.len());
            out.extend(a[..luma].iter().map(|&x| fade_byte(x, w, Y_BLACK as u16)));
            out.extend(
                a[luma..]
                    .iter()
                    .map(|&x| fade_byte(x, w, UV_NEUTRAL as u16)),
            );
            out
        }
    }
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
            segment_cache_gb: 20.0,
            render_backend: "cpu".into(),
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
            probe_stats: Default::default(),
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

    // Verifies: SR-022, SR-036, LLR-062 (TC-088) — the u16 fixed-point
    // blend/scale match an f32 reference within +-1 LSB on every channel over
    // boundary and interior weights/pixels, and the weight endpoints are exact
    // (w=0 returns the first frame byte-for-byte, w=256 the second).
    #[test]
    fn fixed_point_blend_matches_f32_within_one_lsb_sr022() {
        // Representative pixel bytes (TC-088 Parameters) plus both frames'
        // values crossed, so a/b pairs cover the corners and mid-range.
        let px: [u8; 6] = [0, 1, 127, 128, 254, 255];
        let a: Vec<u8> = px.iter().flat_map(|&x| px.iter().map(move |_| x)).collect();
        let b: Vec<u8> = px.iter().flat_map(|_| px.iter().copied()).collect();

        // Boundary and interior weights (TC-088: {0,1,127,128,255,max}) as the
        // t each weight quantizes from, plus arbitrary ts between lattice
        // points to exercise the quantization itself.
        let ts: Vec<f32> = [0u16, 1, 127, 128, 255, 256]
            .iter()
            .map(|&w| w as f32 / 256.0)
            .chain([0.1234f32, 0.337, 0.5001, 0.9999])
            .collect();

        for &t in &ts {
            let w = blend_weight(t) as i32;
            assert!((0..=256).contains(&w), "weight {w} out of range for t={t}");

            let got = blend(&a, &b, t);
            let scaled = scale(&a, t, FrameTransport::Rgb24, a.len());
            for i in 0..a.len() {
                // f32 reference (the pre-LLR-062 implementation).
                let reference = (a[i] as f32 * (1.0 - t) + b[i] as f32 * t)
                    .round()
                    .clamp(0.0, 255.0);
                let diff = (got[i] as f32 - reference).abs();
                assert!(
                    diff <= 1.0,
                    "blend a={} b={} t={t}: got {} vs f32 ref {} (>1 LSB)",
                    a[i],
                    b[i],
                    got[i],
                    reference
                );
                let sref = (a[i] as f32 * t).round().clamp(0.0, 255.0);
                assert!(
                    (scaled[i] as f32 - sref).abs() <= 1.0,
                    "scale a={} t={t}: got {} vs f32 ref {sref} (>1 LSB)",
                    a[i],
                    scaled[i]
                );
                // The exact fixed-point contract from LLR-062.
                let expect =
                    ((a[i] as u16 * (256 - w as u16) + b[i] as u16 * w as u16 + 128) >> 8) as u8;
                assert_eq!(
                    got[i], expect,
                    "blend must equal the u16 fixed-point formula (a={} b={} w={w})",
                    a[i], b[i]
                );
            }
        }

        // Endpoint exactness: zero weight returns the first frame exactly,
        // full weight the second.
        assert_eq!(blend(&a, &b, 0.0), a, "w=0 must return frame a exactly");
        assert_eq!(blend(&a, &b, 1.0), b, "w=256 must return frame b exactly");
        assert_eq!(
            scale(&a, 1.0, FrameTransport::Rgb24, a.len()),
            a,
            "scale w=256 must be identity"
        );
        assert_eq!(
            scale(&a, 0.0, FrameTransport::Rgb24, a.len()),
            vec![0u8; a.len()],
            "scale w=0 must be black"
        );
    }

    /// Fade weights spanning the full range: the TC-111 lattice points plus
    /// the exact endpoints.
    fn fade_weights() -> Vec<f32> {
        [0u16, 1, 64, 127, 128, 192, 255, 256]
            .iter()
            .map(|&w| w as f32 / 256.0)
            .collect()
    }

    // Verifies: SR-040, LLR-077 (TC-111) — the ONE format-aware
    // scale-to-black fn on the pure CI-safe seam (the LLR-082 recorded
    // chroma assertion): on Yuv420p an achromatic source keeps U = V = 128
    // (+-1) at EVERY fade weight while Y moves monotonically to Y_BLACK = 16
    // at full fade; a saturated-chroma source moves U/V monotonically TOWARD
    // 128 — never toward 0 (green/purple tint) — with U = V = 128 exactly and
    // Y = 16 at full fade. Dissolve (blend) stays per-byte and plane-agnostic,
    // guarded by TC-088 above — referenced, not duplicated.
    #[test]
    fn yuv_scale_to_black_chroma_neutral_sr040() {
        use crate::image::yuv::{UV_NEUTRAL, Y_BLACK};
        let luma_len = 16usize; // 8x4-ish planar toy frame: 16 Y + 4 U + 4 V
        let frame = |y: u8, u: u8, v: u8| -> Vec<u8> {
            let mut f = vec![y; luma_len];
            f.extend(vec![u; luma_len / 4]);
            f.extend(vec![v; luma_len / 4]);
            f
        };

        // (a) Achromatic source: chroma pinned at neutral for every weight.
        let gray = frame(200, UV_NEUTRAL, UV_NEUTRAL);
        let mut prev_y: Option<u8> = None;
        for &t in &fade_weights() {
            let out = scale(&gray, t, FrameTransport::Yuv420p, luma_len);
            assert_eq!(out.len(), gray.len());
            for &b in &out[luma_len..] {
                assert!(
                    (b as i16 - UV_NEUTRAL as i16).abs() <= 1,
                    "achromatic fade must keep U/V at 128+-1 (weight {t}): got {b}"
                );
            }
            let y = out[0];
            if let Some(p) = prev_y {
                assert!(y >= p, "Y must move monotonically with fade weight");
            }
            prev_y = Some(y);
            if t == 0.0 {
                assert!(
                    out[..luma_len].iter().all(|&b| b == Y_BLACK),
                    "full fade must land Y exactly on limited-range black (16)"
                );
            }
        }

        // (b) Saturated chroma: U/V move monotonically TOWARD 128, never
        // toward 0, and land exactly on 128 at full fade.
        let saturated = frame(100, 240, 54);
        let mut prev_dev: Option<(i16, i16)> = None;
        for &t in &fade_weights() {
            let out = scale(&saturated, t, FrameTransport::Yuv420p, luma_len);
            let u = out[luma_len];
            let v = out[luma_len + luma_len / 4];
            assert!(
                (128..=240).contains(&u) && (54..=128).contains(&v),
                "chroma must stay between source and neutral (weight {t}): u={u} v={v}"
            );
            let dev = (
                (u as i16 - UV_NEUTRAL as i16).abs(),
                (v as i16 - UV_NEUTRAL as i16).abs(),
            );
            if let Some(p) = prev_dev {
                assert!(
                    dev.0 >= p.0 && dev.1 >= p.1,
                    "U/V must move monotonically toward 128 as weight falls"
                );
            }
            prev_dev = Some(dev);
            if t == 0.0 {
                assert_eq!((u, v), (UV_NEUTRAL, UV_NEUTRAL), "full fade: U=V=128 exact");
                assert!(
                    out[..luma_len].iter().all(|&b| b == Y_BLACK),
                    "full fade: Y=16"
                );
            }
        }
    }

    // Verifies: SR-040, LLR-077 (TC-111) — the Rgb24 arm of the one scale fn
    // is byte-identical to the pre-change fixed-point form
    // `(x*w + 128) >> 8` at every weight (the TC-088 +-1-LSB contract is
    // asserted above and unchanged); `luma_len` has no effect on rgb frames.
    #[test]
    fn rgb_scale_to_black_unchanged_sr040() {
        let a: Vec<u8> = (0..=255u8).collect();
        for &t in &fade_weights() {
            let w = blend_weight(t);
            let expect: Vec<u8> = a
                .iter()
                .map(|&x| ((x as u16 * w + 128) >> 8) as u8)
                .collect();
            assert_eq!(
                scale(&a, t, FrameTransport::Rgb24, a.len()),
                expect,
                "rgb scale must equal the pre-SR-040 fixed-point form (t={t})"
            );
            // A nonsense luma_len must not change rgb output (rgb ignores it).
            assert_eq!(scale(&a, t, FrameTransport::Rgb24, 1), expect);
        }
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
