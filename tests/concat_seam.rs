//! Concat-seam spike (TC-079) plus the concat failure-routing legs of TC-080:
//! the same small fixed corpus is built (a) cold through today's streaming
//! single-encoder pipeline and (b) as per-clip segments encoded separately and
//! assembled with a stream-copy `ffmpeg -f concat`, then the two outputs are
//! compared frame-for-frame. This is the plan §4 risk burn-down that gates the
//! SR-037 segment cache: identical frame count and duration, the SR-005
//! profile on the assembled file, and clean seams (no dropped or duplicated
//! frames) at every segment join. The segmented build here runs against a
//! fresh (empty) SegmentStore, i.e. the all-miss cold path; the warm/cache
//! scenarios are tests/segment_cache.rs (TC-081).
//!
//! Verifies: SR-037, SR-005, LLR-056. (TC-079, TC-080.) FFmpeg/ffprobe must be
//! on PATH (CI invariant).

mod common;

use common::{
    assert_sr005_profile, decode_frames, image_media_file, mean_abs_diff, probe_duration,
    write_noise_png, TempDir,
};
use slideshow_core::cache::store::SegmentStore;
use slideshow_core::config::{OutputDef, ProcessingConfig};
use slideshow_core::ffmpeg::concat::concat_segments;
use slideshow_core::media::Album;
use slideshow_core::pipeline::FrameGenerationPipeline;
use std::path::{Path, PathBuf};

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
        segment_cache_gb: 20.0,
    }
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
        write_noise_png(&p, SRC_W, SRC_H, *seed);
        files.push(image_media_file(&p));
    }
    Album {
        media_files: files,
        total_size: 0,
        created_at: std::time::SystemTime::now(),
        probe_stats: Default::default(),
    }
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
    let summary = cold_pipe.execute().expect("cold build");
    assert_eq!(summary.written.len(), 1, "cold build writes one output");
    let cold_mp4 = summary.written[0].path.clone();

    // (b) The same corpus as independently-encoded per-clip segments against
    // an empty store (all-miss cold cached build), assembled by stream-copy
    // concat.
    let seg_out_dir = tmp.join("segmented");
    let mut store = SegmentStore::open(&tmp.join("store")).expect("open store");
    let seg_pipe = FrameGenerationPipeline::new(
        album,
        vec![def.clone()],
        processing(),
        seg_out_dir.clone(),
        media.clone(),
        None,
    );
    let build = seg_pipe
        .execute_segmented(&def, &mut store)
        .expect("segmented build")
        .expect("segmented build produced an output");

    // One segment per clip, one boundary per transition, all committed
    // segments real; a cold (empty-cache) build reuses nothing (SR-037 cold
    // scenario evidence).
    assert_eq!(build.segments.len(), 4, "one segment per clip");
    assert_eq!(build.boundaries.len(), 3, "one boundary per transition");
    assert!(
        build.boundaries.windows(2).all(|w| w[0] < w[1]),
        "boundaries strictly increasing: {:?}",
        build.boundaries
    );
    assert_eq!((build.reused, build.re_encoded), (0, 4), "cold = all miss");
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

    let cold = decode_frames(&cold_mp4, OUT_W, OUT_H);
    let asm = decode_frames(&build.output, OUT_W, OUT_H);
    assert_eq!(
        cold.len(),
        asm.len(),
        "total decoded frame count must be identical"
    );
    assert_eq!(
        asm.len() as u64,
        build.frames,
        "assembled frame count matches the planned emission"
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
