//! Integration tests for the Phase-3 overlapped pipeline (LLR-063/LLR-064):
//! an `EncoderWriter` thread owns the ffmpeg stdin behind a bounded channel,
//! and its failures must reach the driving loop with the exact serial-path
//! semantics (non-zero exit, no false success, `.part` cleaned, no deadlock).
//!
//! Verifies: SR-011, SR-013, SR-015, SR-036, LLR-063, LLR-064 (TC-089, TC-090)
//!
//! Drives the public `slideshow_core::pipeline::{EncoderWriter, FrameSink}`
//! API plus the built binary for the end-to-end equivalence leg. FFmpeg must
//! be on PATH (CI invariant). The behavioral regression guards TC-089 cites
//! (TC-018 atomic finalize, TC-021 watchdog decision, TC-022/023 skip,
//! TC-057..059 audio) live in their own suites, referenced not duplicated.

mod common;

use common::{
    assert_sr005_profile, decode_frames, probe_duration, slideshow_bin, write_config,
    write_noise_png, TempDir,
};
use slideshow_core::error::{Result, SlideshowError};
use slideshow_core::ffmpeg::encoder_args::EncoderSettings;
use slideshow_core::ffmpeg::FfmpegEncoder;
use slideshow_core::pipeline::{writer_channel_depth, EncoderWriter, FrameSink};
use slideshow_core::util::disk_full_error;
use slideshow_core::util::timing::StageTimings;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

const W: u32 = 64;
const H: u32 = 48;
const FPS: u32 = 24;
const CRF: u32 = 30;
const FRAME_BYTES: usize = (W * H * 3) as usize;

fn part_of(mp4: &Path) -> std::path::PathBuf {
    let mut name = mp4.file_name().unwrap().to_owned();
    name.push(".part");
    mp4.with_file_name(name)
}

/// Deterministic per-frame noise so a dropped/duplicated/reordered frame in
/// the writer thread shows up as a large pixel delta, not a hidden no-op.
fn noise_frame(seed: u32) -> Vec<u8> {
    let mut state = seed | 1;
    (0..FRAME_BYTES)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state & 0xFF) as u8
        })
        .collect()
}

/// Verifies: SR-036, SR-011, LLR-063 (TC-089, lib leg) — the identical frame
/// sequence encoded serially (mixer thread writes ffmpeg stdin directly) and
/// through the `EncoderWriter` thread produces matching outputs: same frame
/// count, same duration, and identical decoded frames (same encoder, same
/// input, same host => deterministic encode).
#[test]
fn overlapped_build_output_equals_serial_sr036() {
    let tmp = TempDir::new("overlap_equal");
    let frames: Vec<Vec<u8>> = (0..60).map(|i| noise_frame(0xC0FFEE + i)).collect();

    // Serial reference: the pre-Phase-3 shape — write_frame on this thread.
    let serial_out = tmp.join("serial.mp4");
    let mut enc =
        FfmpegEncoder::start(&serial_out, W, H, FPS, &EncoderSettings::software(CRF), 120)
            .expect("start serial encoder");
    for f in &frames {
        enc.write_frame(f).expect("serial write");
    }
    enc.finish().expect("serial finish");

    // Overlapped: same frames through the bounded channel + writer thread.
    let overlap_out = tmp.join("overlap.mp4");
    let enc = FfmpegEncoder::start(
        &overlap_out,
        W,
        H,
        FPS,
        &EncoderSettings::software(CRF),
        120,
    )
    .expect("start overlapped encoder");
    let depth = writer_channel_depth(5, FRAME_BYTES);
    let mut writer = EncoderWriter::start(enc, depth, Arc::new(StageTimings::new()));
    for f in &frames {
        writer.write(f.clone()).expect("channel write");
    }
    let enc = writer
        .join()
        .expect("writer thread must hand the encoder back");
    enc.finish().expect("overlapped finish");

    let serial = decode_frames(&serial_out, W, H);
    let overlapped = decode_frames(&overlap_out, W, H);
    assert_eq!(
        serial.len(),
        overlapped.len(),
        "overlap must not drop or duplicate frames"
    );
    assert_eq!(
        serial.len(),
        frames.len(),
        "one output frame per input frame"
    );
    for (i, (a, b)) in serial.iter().zip(overlapped.iter()).enumerate() {
        assert_eq!(
            a, b,
            "decoded frame {i} differs between serial and overlapped"
        );
    }
    let (da, db) = (probe_duration(&serial_out), probe_duration(&overlap_out));
    assert!(
        (da - db).abs() < 0.5,
        "durations must match: serial {da}s vs overlapped {db}s"
    );
}

/// Verifies: SR-036, SR-005, SR-011, LLR-063 (TC-089, end-to-end leg) — the
/// default (segmented) and `--no-cache` (streaming) builds, both through the
/// overlapped stage graph, agree with each other and with the mixer's frame
/// arithmetic (`n*clip_frames - (n-1)*fade`), and keep the SR-005 profile.
#[test]
fn overlapped_binary_paths_agree_sr036() {
    let tmp = TempDir::new("overlap_e2e");
    let media = tmp.join("media");
    std::fs::create_dir_all(&media).unwrap();
    for (i, seed) in [11u32, 22, 33].iter().enumerate() {
        write_noise_png(&media.join(format!("img_{i}.png")), 96, 72, *seed);
    }

    let mut outputs = Vec::new();
    for (label, extra) in [("cached", None), ("stream", Some("--no-cache"))] {
        let outdir = tmp.join(&format!("out_{label}"));
        std::fs::create_dir_all(&outdir).unwrap();
        let cfg = tmp.join(&format!("cfg_{label}.toml"));
        write_config(&cfg, &media, &outdir, "show", W, H);
        let mut cmd = std::process::Command::new(slideshow_bin());
        cmd.arg("--config").arg(&cfg).arg("--non-interactive");
        if let Some(flag) = extra {
            cmd.arg(flag);
        }
        cmd.arg("build");
        let out = cmd.output().expect("run build");
        assert!(
            out.status.success(),
            "{label} build must exit zero; stderr=\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mp4 = outdir.join("show.mp4");
        assert!(mp4.exists(), "{label} build must produce show.mp4");
        assert!(
            !part_of(&mp4).exists(),
            "{label} build must leave no .part behind"
        );
        assert_sr005_profile(&mp4);
        outputs.push(decode_frames(&mp4, W, H).len());
    }

    // write_config pins fps=24, pic=1.0s (24 frames/clip), fade=0.2s (round
    // to 5 fade frames): 3 clips => 3*24 - 2*5 = 62 output frames.
    let expected = 3 * 24 - 2 * 5;
    assert_eq!(
        outputs[0], expected,
        "cached-path frame count must match the mixer arithmetic"
    );
    assert_eq!(
        outputs[0], outputs[1],
        "cached and streaming overlapped paths must emit identical frame counts"
    );
}

/// Verifies: SR-013, SR-011, LLR-064 (TC-090, ffmpeg-nonzero-exit leg) — a
/// stream ffmpeg rejects (misaligned garbage, the atomic_finalize failure
/// mode) must never become a false success through the writer thread: the
/// failure surfaces at the send loop, at `join`, or at `finish`, and cleanup
/// leaves NO final mp4 and NO `.part`. (The mid-stream broken-pipe leg is
/// exercised by `writer_watchdog_kill_reraised_with_cleanup_sr013`, where
/// the watchdog really kills ffmpeg under the writer.)
#[test]
fn writer_thread_error_propagates_and_cleans_sr013() {
    let tmp = TempDir::new("overlap_fail");
    let out = tmp.join("clip.mp4");

    let enc = FfmpegEncoder::start(&out, W, H, FPS, &EncoderSettings::software(CRF), 120)
        .expect("start ffmpeg");
    let mut writer = EncoderWriter::start(enc, 4, Arc::new(StageTimings::new()));

    let start = Instant::now();
    // A lone short/misaligned buffer then EOF: the rawvideo decoder cannot
    // produce a frame and ffmpeg exits non-zero (the same deterministic
    // failure the serial atomic_finalize suite uses) — here written on the
    // writer thread. The pipe write itself succeeds, so the non-zero exit
    // must surface through join (LLR-064) + finish exactly as serially.
    let mut send_error: Option<SlideshowError> = None;
    if let Err(e) = writer.write(vec![1u8, 2, 3, 4, 5]) {
        send_error = Some(e);
    }
    let result = match send_error {
        // ffmpeg died before the write drained: the writer-thread error was
        // re-raised to the sender via channel disconnect (no deadlock).
        Some(e) => {
            drop(writer); // early abort: Drop must join cleanly
            Err(e)
        }
        None => writer.join().and_then(|enc| enc.finish()),
    };
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "failure must surface promptly, not after a drain-forever stall"
    );
    let err = result.expect_err("a rejected stream must never report success");
    let msg = err.to_string();
    assert!(
        msg.contains("ffmpeg") || msg.contains("FFmpeg"),
        "the real error must be re-raised verbatim, got: {msg}"
    );

    assert!(!out.exists(), "a failed encode must leave no final mp4");
    assert!(
        !part_of(&out).exists(),
        "a failed encode must clean up its .part"
    );
}

/// A sink that fails with a chosen error after `ok_writes` frames — the
/// injection point for the disk-full leg (a real ENOSPC cannot be provoked
/// in CI; the mapping itself is unit-tested in util::file_utils).
struct FailingSink {
    ok_writes: usize,
    written: usize,
    err: Option<SlideshowError>,
}

impl FrameSink for FailingSink {
    fn write(&mut self, _frame: Vec<u8>) -> Result<()> {
        if self.written < self.ok_writes {
            self.written += 1;
            return Ok(());
        }
        Err(self.err.take().expect("failing sink consulted once"))
    }
}

/// Verifies: SR-015, LLR-064 (TC-090, disk-full leg) — a disk-full
/// `SlideshowError` raised on the writer thread is re-raised to the sender
/// as the SAME plain-language error (LLR-018 mapping preserved across the
/// thread boundary), within `depth + O(1)` further sends.
#[test]
fn writer_disk_full_error_reraised_verbatim_sr015() {
    let sink = FailingSink {
        ok_writes: 3,
        written: 0,
        err: Some(disk_full_error(Path::new("out/show.mp4.part"))),
    };
    let depth = 2;
    let mut writer = EncoderWriter::start(sink, depth, Arc::new(StageTimings::new()));

    let mut error: Option<SlideshowError> = None;
    let mut sends_after_ok = 0usize;
    for i in 0..1_000 {
        match writer.write(noise_frame(i as u32)) {
            Ok(()) => {
                if i >= 3 {
                    sends_after_ok += 1;
                }
            }
            Err(e) => {
                error = Some(e);
                break;
            }
        }
    }
    let err = error.expect("disk-full must surface to the sender");
    assert!(
        err.to_string().contains("out of disk space"),
        "the LLR-018 disk-full message must survive the thread boundary, got: {err}"
    );
    // Bounded-time unblock: at most the in-flight channel depth (+1 being
    // handed to the sink) may still send Ok after the sink has failed.
    assert!(
        sends_after_ok <= depth + 2,
        "sender must unblock via channel disconnect promptly, not after {sends_after_ok} sends"
    );
}

/// Verifies: SR-013, SR-011, LLR-064 (TC-090, watchdog-kill leg) — the
/// SR-013 inactivity watchdog still guards the encode when writes happen on
/// the writer thread: a 1 s timeout with no frames kills ffmpeg, the next
/// writes fail promptly, and no final mp4 / `.part` survives.
#[test]
fn writer_watchdog_kill_reraised_with_cleanup_sr013() {
    let tmp = TempDir::new("overlap_watchdog");
    let out = tmp.join("clip.mp4");

    let enc = FfmpegEncoder::start(&out, W, H, FPS, &EncoderSettings::software(CRF), 1)
        .expect("start ffmpeg with 1s inactivity timeout");
    let mut writer = EncoderWriter::start(enc, 4, Arc::new(StageTimings::new()));
    writer.write(noise_frame(1)).expect("first frame writes");

    // Starve the encoder past the timeout: the watchdog (~1 s wake) kills it.
    std::thread::sleep(Duration::from_millis(2_500));

    let start = Instant::now();
    let mut failed = false;
    for i in 0..10_000 {
        if writer.write(noise_frame(2 + i)).is_err() {
            failed = true;
            break;
        }
    }
    assert!(
        failed,
        "writes after a watchdog kill must fail (no silent success)"
    );
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "failure must surface promptly after the kill"
    );

    drop(writer);
    assert!(
        !out.exists(),
        "a watchdog-killed encode must leave no final mp4"
    );
    assert!(
        !part_of(&out).exists(),
        "a watchdog-killed encode must remove its .part"
    );
}

/// Verifies: SR-011, LLR-064 (TC-090, early-abort leg) — dropping the writer
/// mid-stream (the `produced_total == 0` abort path and any panic unwind)
/// joins the thread promptly and the encoder Drop removes the `.part`,
/// leaving no final file.
#[test]
fn writer_drop_mid_stream_unblocks_and_cleans_sr011() {
    let tmp = TempDir::new("overlap_drop");
    let out = tmp.join("clip.mp4");

    let enc = FfmpegEncoder::start(&out, W, H, FPS, &EncoderSettings::software(CRF), 120)
        .expect("start ffmpeg");
    let mut writer = EncoderWriter::start(enc, 4, Arc::new(StageTimings::new()));
    for i in 0..8 {
        writer.write(noise_frame(100 + i)).expect("write");
    }
    let start = Instant::now();
    drop(writer); // no join(): models an abort before finalize
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "Drop must join the writer thread promptly"
    );
    assert!(!out.exists(), "an aborted encode must leave no final mp4");
    assert!(
        !part_of(&out).exists(),
        "an aborted encode must remove its .part temp"
    );
}
