//! Integration tests for crash-safe / atomic finalize (SR-011, SR-015; LLR-015).
//!
//! Verifies that a build/encode that fails partway leaves NO final `<name>.mp4`
//! (only absence, or a `.part` that was cleaned up), and that the success path
//! DOES produce `<name>.mp4` and leaves no `.part` behind.
//!
//! Verifies: SR-011, SR-015, LLR-015 (TC-018, TC-025)
//!
//! Drives the public `slideshow_core::ffmpeg::FfmpegEncoder` API (the encoder is
//! the single owner of the temp -> atomic-rename finalize) plus the built
//! binary for the end-to-end success path. FFmpeg must be on PATH (CI invariant).

mod common;

use common::{slideshow_bin, write_config, write_png, TempDir};
use slideshow_core::ffmpeg::FfmpegEncoder;
use std::path::Path;

const W: u32 = 64;
const H: u32 = 48;
const FPS: u32 = 24;
const CRF: u32 = 30;

fn part_of(mp4: &Path) -> std::path::PathBuf {
    let mut name = mp4.file_name().unwrap().to_owned();
    name.push(".part");
    mp4.with_file_name(name)
}

fn solid_frame() -> Vec<u8> {
    vec![0u8; (W * H * 3) as usize]
}

/// Success path (encoder API): a clean encode produces `<name>.mp4` and leaves
/// no `.part`. Verifies: SR-011, LLR-015 (TC-018 success leg).
#[test]
fn finish_produces_final_mp4_and_no_part_sr011() {
    let tmp = TempDir::new("atomic_ok");
    let out = tmp.join("clip.mp4");

    let mut enc = FfmpegEncoder::start(&out, W, H, FPS, CRF, 120).expect("start ffmpeg");
    for _ in 0..FPS {
        enc.write_frame(&solid_frame()).expect("write frame");
    }
    enc.finish().expect("finish should succeed");

    assert!(out.exists(), "final <name>.mp4 must exist on success");
    assert!(
        !part_of(&out).exists(),
        "no .part artifact may remain after a successful finalize"
    );
    // Sanity: a non-trivial file was written.
    assert!(
        std::fs::metadata(&out).unwrap().len() > 0,
        "final mp4 should be non-empty"
    );
}

/// Failure path (encoder API): feeding ffmpeg a truncated/garbage stream makes
/// it exit non-zero; `finish()` must return Err and leave NEITHER the final
/// `<name>.mp4` NOR a `.part`. Verifies: SR-011, SR-015, LLR-015 (TC-018/025).
#[test]
fn failed_encode_leaves_no_final_and_cleans_part_sr011_sr015() {
    let tmp = TempDir::new("atomic_fail");
    let out = tmp.join("clip.mp4");

    // Declare a frame size of W*H*3 but feed ffmpeg a short, misaligned buffer
    // so the rawvideo demuxer/encoder cannot produce a valid stream and ffmpeg
    // exits non-zero (a deterministic encode failure, no flaky timing).
    let mut enc = FfmpegEncoder::start(&out, W, H, FPS, CRF, 120).expect("start ffmpeg");
    let _ = enc.write_frame(&[1u8, 2, 3, 4, 5]); // far too few bytes for one frame
    let result = enc.finish();

    assert!(
        result.is_err(),
        "finish() must report an error when ffmpeg fails (no false success)"
    );
    assert!(
        !out.exists(),
        "a failed encode must NOT leave a complete-looking final <name>.mp4"
    );
    assert!(
        !part_of(&out).exists(),
        "a failed encode must clean up its .part temp"
    );
}

/// Abort/Drop path (encoder API): dropping the encoder mid-encode without
/// calling finish() must kill ffmpeg and remove the `.part`, leaving no final
/// file — modeling a killed/crashed run. Verifies: SR-011, LLR-015 (TC-018).
#[test]
fn dropped_encoder_leaves_no_final_or_part_sr011() {
    let tmp = TempDir::new("atomic_drop");
    let out = tmp.join("clip.mp4");

    {
        let mut enc = FfmpegEncoder::start(&out, W, H, FPS, CRF, 120).expect("start ffmpeg");
        enc.write_frame(&solid_frame()).expect("write one frame");
        // Drop without finish(): models an abort before finalize.
    }

    assert!(
        !out.exists(),
        "an aborted (dropped) encode must leave no final <name>.mp4"
    );
    assert!(
        !part_of(&out).exists(),
        "an aborted (dropped) encode must remove its .part temp"
    );
}

/// End-to-end success via the built binary: `build` over tiny PNGs produces
/// `<name>.mp4`, leaves no `.part`, and exits zero. Verifies: SR-011, LLR-015,
/// LLR-017 (TC-018 success leg, end-to-end).
#[test]
fn build_success_produces_mp4_no_part_sr011() {
    let tmp = TempDir::new("atomic_build");
    let media = tmp.join("media");
    let outdir = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&outdir).unwrap();
    write_png(&media.join("a.png"), 96, 72, [200, 50, 50]);
    write_png(&media.join("b.png"), 96, 72, [50, 200, 50]);

    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", W, H);

    let status = std::process::Command::new(slideshow_bin())
        .arg("--config")
        .arg(&cfg)
        .arg("build")
        .status()
        .expect("run slideshow build");

    assert!(status.success(), "build should exit zero on a healthy run");
    let mp4 = outdir.join("show.mp4");
    assert!(mp4.exists(), "build must produce <name>.mp4");
    assert!(
        !part_of(&mp4).exists(),
        "a successful build must leave no .part artifact"
    );
}
