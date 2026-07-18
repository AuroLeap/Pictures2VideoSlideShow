//! Concat-seam spike (TC-079) plus the concat failure-routing legs of TC-080:
//! the same small fixed corpus is built (a) cold through today's streaming
//! single-encoder pipeline and (b) as per-clip segments encoded separately and
//! assembled with a stream-copy `ffmpeg -f concat`, then the two outputs are
//! compared frame-for-frame. This is the plan §4 mandatory risk burn-down that
//! gates the SR-037 segment cache: identical frame count and duration, the
//! SR-005 profile on the assembled file, and clean seams (no dropped or
//! duplicated frames) at every segment join.
//!
//! Verifies: SR-037, SR-005, LLR-056. (TC-079, TC-080.) FFmpeg/ffprobe must be
//! on PATH (CI invariant).

mod common;

use common::{assert_sr005_profile, TempDir};
use slideshow_core::config::{OutputDef, ProcessingConfig};
use slideshow_core::ffmpeg::concat::concat_segments;
use slideshow_core::media::{Album, MediaFile, MediaType};
use slideshow_core::pipeline::FrameGenerationPipeline;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Source image size: larger than the output so the Ken Burns crop resamples
/// real texture instead of upscaling flat color.
const SRC_W: u32 = 640;
const SRC_H: u32 = 480;
/// Output frame size (even, SR-005) — small enough for a fast CI encode.
const OUT_W: u32 = 320;
const OUT_H: u32 = 240;
const FPS: u32 = 24;

/// The one output definition both builds share. Noise corpus + crf 18 keeps
/// encode noise well below the frame-to-frame signal so seam misalignment
/// (a dropped/duplicated frame) is unambiguously detectable.
fn spike_output(name: &str) -> OutputDef {
    OutputDef {
        name: name.into(),
        width: OUT_W,
        height: OUT_H,
        fps: FPS,
        pic_display_time_secs: 1.0,
        fade_time_secs: 0.25,
        max_rotation_degrees: 0.0,
        bulk_video_time_min: 20,
        quality_crf: 18,
        enable_audio: false,
        audio_bitrate_kbps: 192,
        audio_sample_rate: 48_000,
        encoder: "software".into(),
        x264_preset: "veryfast".into(),
        zoom_amount: 0.12,
        ken_burns: true,
    }
}

fn processing() -> ProcessingConfig {
    ProcessingConfig {
        temp_dir: None,
        max_workers: None,
        use_parallelism: true,
        dry_run: false,
        verbose: false,
        ffmpeg_timeout_secs: 120,
        ffmpeg_path: None,
        default_focus: None,
    }
}

/// Deterministic per-pixel noise PNG (xorshift), so adjacent output frames of
/// the Ken Burns pan differ strongly — a one-frame drop/dup at a seam then
/// shows up as a large pixel delta instead of hiding in flat color.
fn write_noise_png(path: &Path, seed: u32) {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let mut img = image::RgbImage::new(SRC_W, SRC_H);
    for p in img.pixels_mut() {
        let v = next();
        *p = image::Rgb([
            (v & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            ((v >> 16) & 0xFF) as u8,
        ]);
    }
    img.save(path).expect("save noise png");
}

/// A four-image album over `media` dir (writes the corpus on first call).
fn build_album(media: &Path) -> Album {
    std::fs::create_dir_all(media).unwrap();
    let mut files = Vec::new();
    for (i, seed) in [0xA11CE_u32, 0xB0B5_1DE5, 0xCAFE_F00D, 0xD00D_5EED]
        .iter()
        .enumerate()
    {
        let p = media.join(format!("img_{i}.png"));
        write_noise_png(&p, *seed);
        files.push(MediaFile {
            path: p.clone(),
            file_type: MediaType::Image,
            dimensions: (SRC_W, SRC_H),
            duration_secs: None,
            size_bytes: std::fs::metadata(&p).unwrap().len(),
            has_audio: false,
        });
    }
    Album {
        media_files: files,
        total_size: 0,
        created_at: std::time::SystemTime::now(),
    }
}

/// Decode every frame of `mp4` to raw rgb24 buffers via ffmpeg.
fn decode_frames(mp4: &Path) -> Vec<Vec<u8>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(mp4)
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"])
        .output()
        .expect("run ffmpeg decode");
    assert!(
        out.status.success(),
        "ffmpeg decode of {} failed: {}",
        mp4.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let frame_bytes = (OUT_W * OUT_H * 3) as usize;
    assert_eq!(
        out.stdout.len() % frame_bytes,
        0,
        "decoded byte count of {} is not a whole number of {}x{} frames",
        mp4.display(),
        OUT_W,
        OUT_H
    );
    out.stdout.chunks(frame_bytes).map(|c| c.to_vec()).collect()
}

/// Container duration in seconds via ffprobe.
fn probe_duration(mp4: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(mp4)
        .output()
        .expect("run ffprobe");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("parse duration")
}

/// Mean absolute per-byte difference between two rgb24 frames.
fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let sum: u64 = a
        .iter()
        .zip(b.iter())
        .map(|(&x, &y)| (x as i16 - y as i16).unsigned_abs() as u64)
        .sum();
    sum as f64 / a.len() as f64
}

/// Verifies: SR-037, SR-005, LLR-056 (TC-079) — the segmented+concat build of
/// a fixed corpus equals the cold streaming build: identical total frame
/// count, identical duration, SR-005 profile on the assembled output, and at
/// every segment join a ±3-frame window whose frames match the cold build's
/// same-index frames (encode-noise tolerance) and match them better than any
/// neighboring frame (no dropped/duplicated frame at the seam).
#[test]
fn segmented_concat_build_equals_cold_build_sr037() {
    let tmp = TempDir::new("concat_seam");
    let media = tmp.join("media");
    let album = build_album(&media);
    let def = spike_output("seam");

    // (a) Cold build through today's single-encoder streaming pipeline.
    let cold_dir = tmp.join("cold");
    let cold_pipe = FrameGenerationPipeline::new(
        album.clone(),
        vec![def.clone()],
        processing(),
        cold_dir.clone(),
        media.clone(),
        None,
    );
    let summary = futures::executor::block_on(cold_pipe.execute()).expect("cold build");
    assert_eq!(summary.written.len(), 1, "cold build writes one output");
    let cold_mp4 = summary.written[0].path.clone();

    // (b) The same corpus as independently-encoded per-clip segments,
    // assembled by stream-copy concat.
    let seg_out_dir = tmp.join("segmented");
    let seg_dir = tmp.join("segments");
    let seg_pipe = FrameGenerationPipeline::new(
        album,
        vec![def.clone()],
        processing(),
        seg_out_dir.clone(),
        media.clone(),
        None,
    );
    let build = seg_pipe
        .execute_segmented(&def, &seg_dir)
        .expect("segmented build")
        .expect("segmented build produced an output");

    // One segment per clip, one boundary per transition, all promoted files real.
    assert_eq!(build.segments.len(), 4, "one segment per clip");
    assert_eq!(build.boundaries.len(), 3, "one boundary per transition");
    assert!(
        build.boundaries.windows(2).all(|w| w[0] < w[1]),
        "boundaries strictly increasing: {:?}",
        build.boundaries
    );
    for seg in &build.segments {
        assert!(seg.exists(), "segment file exists: {}", seg.display());
    }

    // SR-005 profile holds on the concat-assembled output.
    assert_sr005_profile(&build.output);

    // Identical duration (within one frame period) and identical frame count.
    let d_cold = probe_duration(&cold_mp4);
    let d_asm = probe_duration(&build.output);
    assert!(
        (d_cold - d_asm).abs() < 1.0 / FPS as f64,
        "durations differ: cold {d_cold:.4}s vs assembled {d_asm:.4}s"
    );

    let cold = decode_frames(&cold_mp4);
    let asm = decode_frames(&build.output);
    assert_eq!(
        cold.len(),
        asm.len(),
        "total decoded frame count must be identical"
    );
    assert_eq!(
        asm.len() as u64,
        build.frames,
        "assembled frame count matches the frames the mixer emitted"
    );

    // Seam integrity: around every join, each assembled frame matches the cold
    // build's same-index frame within encode-noise tolerance, and matches it
    // strictly better than the cold build's neighboring frames — a dropped or
    // duplicated frame at the seam would shift the best match by one index.
    const MAX_MEAN_DIFF: f64 = 10.0; // mean |Δ|/byte from two independent lossy encodes
    for &b in &build.boundaries {
        let b = b as usize;
        let lo = b.saturating_sub(3);
        let hi = (b + 3).min(cold.len());
        for i in lo..hi {
            let aligned = mean_abs_diff(&asm[i], &cold[i]);
            assert!(
                aligned < MAX_MEAN_DIFF,
                "join@{b}: frame {i} differs from cold by mean {aligned:.2} (> {MAX_MEAN_DIFF})"
            );
            if i > 0 {
                let prev = mean_abs_diff(&asm[i], &cold[i - 1]);
                assert!(
                    aligned <= prev + 0.5,
                    "join@{b}: frame {i} matches cold[{}] ({prev:.2}) better than cold[{i}] \
                     ({aligned:.2}) — duplicated frame at the seam",
                    i - 1
                );
            }
            if i + 1 < cold.len() {
                let next = mean_abs_diff(&asm[i], &cold[i + 1]);
                assert!(
                    aligned <= next + 0.5,
                    "join@{b}: frame {i} matches cold[{}] ({next:.2}) better than cold[{i}] \
                     ({aligned:.2}) — dropped frame at the seam",
                    i + 1
                );
            }
        }
    }
}

/// Verifies: SR-037, SR-011, SR-013, SR-015, LLR-056 (TC-080, failure-routing
/// legs) — a failing concat (unreadable segment) surfaces as a named error
/// with no final `<name>.mp4` and no leftover `.part`/list file, and an empty
/// segment list is rejected up front rather than invoking ffmpeg.
#[test]
fn concat_failure_names_error_and_leaves_no_final_sr037() {
    let tmp = TempDir::new("concat_fail");
    let final_out = tmp.join("show.mp4");

    // Empty segment list: rejected loudly, nothing written.
    let err = concat_segments(&[], &final_out, 30).expect_err("empty list must fail");
    assert!(
        err.to_string().contains("show.mp4"),
        "error names the output: {err}"
    );

    // A garbage segment: ffmpeg exits non-zero; the error names the output and
    // neither the final name nor any in-progress artifact survives.
    let bad = tmp.join("bad.ts");
    std::fs::write(&bad, b"not a transport stream").unwrap();
    let err = concat_segments(&[bad], &final_out, 30).expect_err("garbage segment must fail");
    assert!(
        err.to_string().contains("show.mp4"),
        "error names the output: {err}"
    );
    assert!(!final_out.exists(), "no final output may appear (SR-011)");
    let leftovers: Vec<PathBuf> = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy().into_owned();
            n.ends_with(".part") || n.contains(".concat")
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "no .part or concat list may be left behind: {leftovers:?}"
    );
}
