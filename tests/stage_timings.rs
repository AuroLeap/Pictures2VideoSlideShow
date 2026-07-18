//! Integration tests for the SR-036 per-stage build timings (LLR-050): a
//! `--verbose` build emits one timing line per stage per output (decode,
//! prescale, render, blend, encode-write-stall, ffmpeg-wall); a non-verbose
//! build's output is unchanged apart from the existing end-of-output frames/s
//! line. FFmpeg must be on PATH (CI invariant).
//!
//! Verifies: SR-036, LLR-050. (TC-072.)

mod common;

use common::{slideshow_bin, write_config, write_png, TempDir};
use std::process::Output;

/// The six SR-036 stages, as named in the `--verbose` summary lines.
const STAGES: [&str; 6] = [
    "decode",
    "prescale",
    "render",
    "blend",
    "encode-write-stall",
    "ffmpeg-wall",
];

/// Build a tiny two-image album (so a cross-dissolve — the blend stage —
/// actually runs) through the built binary, with or without `--verbose`.
fn run_build(tmp: &TempDir, verbose: bool) -> Output {
    let media = tmp.join("media");
    let outdir = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&outdir).unwrap();
    write_png(&media.join("a.png"), 96, 72, [200, 40, 40]);
    write_png(&media.join("b.png"), 96, 72, [40, 200, 40]);
    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", 64, 48);

    let mut cmd = std::process::Command::new(slideshow_bin());
    cmd.arg("--config").arg(&cfg);
    if verbose {
        cmd.arg("--verbose");
    }
    cmd.arg("--non-interactive").arg("build");
    cmd.output().expect("run build")
}

/// Verifies: SR-036, LLR-050 (TC-072) — a `--verbose` build emits exactly one
/// timing line per stage for the single configured output, each naming the
/// stage with an elapsed time.
#[test]
fn verbose_emits_one_line_per_stage_sr036() {
    let tmp = TempDir::new("stage_verbose");
    let out = run_build(&tmp, true);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "verbose build must exit zero; stderr=\n{stderr}"
    );
    for stage in STAGES {
        let needle = format!("stage {stage}:");
        assert_eq!(
            stderr.matches(needle.as_str()).count(),
            1,
            "expected exactly one '{needle}' line for the one output; stderr=\n{stderr}"
        );
    }
    // Each line reports an elapsed time (the summary formats milliseconds).
    assert!(
        stderr.contains(" ms"),
        "stage lines must carry an elapsed time in ms; stderr=\n{stderr}"
    );
}

/// Verifies: SR-036, LLR-050 (TC-072) — without `--verbose` no stage line
/// appears; the existing end-of-output frames/s line is unchanged.
#[test]
fn nonverbose_output_unchanged_sr036() {
    let tmp = TempDir::new("stage_nonverbose");
    let out = run_build(&tmp, false);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "build must exit zero; stderr=\n{stderr}"
    );
    for stage in STAGES {
        let needle = format!("stage {stage}:");
        assert!(
            !stderr.contains(needle.as_str()) && !stdout.contains(needle.as_str()),
            "non-verbose output must not contain '{needle}'; stderr=\n{stderr}"
        );
    }
    assert!(
        stderr.contains("frames/s"),
        "the existing end-of-output frames/s line must remain; stderr=\n{stderr}"
    );
}
