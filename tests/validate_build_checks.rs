//! Integration tests for the pre-run prerequisite checks (SR-002, SR-016;
//! LLR-004, LLR-005, LLR-019).
//!
//! Drives the built `slideshow` binary's `validate` and `build` commands with a
//! good config and with failing path prerequisites (missing media_root, an
//! output base_dir that cannot be created because its parent is a FILE), and
//! asserts the per-check pass/fail outcomes, the named path on failure, and
//! that `build` refuses (exits non-zero) on an essential failure.
//!
//! Verifies: SR-002, SR-016, LLR-004, LLR-005, LLR-019 (TC-002, TC-003, TC-004,
//! TC-026). FFmpeg/ffprobe must be on PATH (CI invariant) so those checks pass
//! and the path checks are the discriminating ones.

mod common;

use common::{slideshow_bin, write_config, write_png, TempDir};
use std::path::Path;
use std::process::{Command, Output};

fn run(config: &Path, sub: &str) -> Output {
    Command::new(slideshow_bin())
        .arg("--config")
        .arg(config)
        .arg(sub)
        .output()
        .expect("run slideshow")
}

/// Good config: every essential runtime prerequisite passes, `validate` exits
/// zero and emits a `[PASS]` line per prereq. Verifies: SR-002, LLR-004
/// (TC-002).
#[test]
fn validate_passes_with_good_config_sr002() {
    let tmp = TempDir::new("val_ok");
    let media = tmp.join("media");
    let outdir = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    write_png(&media.join("a.png"), 32, 32, [10, 20, 30]);

    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", 64, 48);

    let out = run(&cfg, "validate");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "validate must exit zero on a good config; stdout=\n{stdout}"
    );
    // One labeled pass line per essential prerequisite (FFmpeg, ffprobe, config,
    // media root, output dir). We assert the discriminating runtime prereqs pass.
    assert!(
        stdout.contains("[PASS]"),
        "expected [PASS] lines:\n{stdout}"
    );
    assert!(
        !stdout.contains("[FAIL]"),
        "no prereq should fail on a good config:\n{stdout}"
    );
    assert!(
        stdout.contains("FFmpeg") && stdout.contains("Media root") && stdout.contains("Output"),
        "validate should report each prereq by name:\n{stdout}"
    );
}

/// Missing media_root: the media-root check fails NAMING the path, `validate`
/// exits non-zero. Verifies: SR-002, SR-016, LLR-019 (TC-003, TC-026).
#[test]
fn validate_fails_naming_missing_media_root_sr002_sr016() {
    let tmp = TempDir::new("val_nomedia");
    let media = tmp.join("does_not_exist_media");
    let outdir = tmp.join("out");
    // Deliberately do NOT create `media`.

    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", 64, 48);

    let out = run(&cfg, "validate");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        !out.status.success(),
        "validate must exit non-zero when media_root is missing:\n{stdout}"
    );
    assert!(
        stdout.contains("[FAIL]"),
        "expected a [FAIL] line:\n{stdout}"
    );
    assert!(
        stdout.contains("Media root"),
        "the failing check should be the media-root check:\n{stdout}"
    );
    // SR-016: the exact offending path must be named in plain language.
    assert!(
        stdout.contains("does_not_exist_media"),
        "the failure must name the offending path:\n{stdout}"
    );
}

/// Unusable output dir: point base_dir at a path WHOSE PARENT IS A FILE, so the
/// directory cannot be created. The output-writable check fails naming the
/// path, `validate` exits non-zero. Verifies: SR-002, SR-016, LLR-019
/// (TC-003, TC-026).
#[test]
fn validate_fails_naming_unusable_output_dir_sr002_sr016() {
    let tmp = TempDir::new("val_badout");
    let media = tmp.join("media");
    std::fs::create_dir_all(&media).unwrap();
    write_png(&media.join("a.png"), 32, 32, [1, 2, 3]);

    // A regular file that we then try to treat as a parent directory.
    let blocker = tmp.join("blocker.txt");
    std::fs::write(&blocker, b"not a directory").unwrap();
    // base_dir's parent (`blocker.txt`) is a file -> create_dir_all must fail.
    let outdir = blocker.join("nested_out");

    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", 64, 48);

    let out = run(&cfg, "validate");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        !out.status.success(),
        "validate must exit non-zero when the output dir is unusable:\n{stdout}"
    );
    assert!(
        stdout.contains("[FAIL]"),
        "expected a [FAIL] line:\n{stdout}"
    );
    assert!(
        stdout.contains("Output"),
        "the failing check should be the output-writable check:\n{stdout}"
    );
    // SR-016: the offending path is named (no panic / stack trace).
    assert!(
        stdout.contains("nested_out") || stdout.contains("blocker.txt"),
        "the failure must name the offending output path:\n{stdout}"
    );
    assert!(
        !stdout.contains("panicked") && !String::from_utf8_lossy(&out.stderr).contains("panicked"),
        "failure must be a plain message, not a panic"
    );
}

/// Build refuses on an essential failure: with a missing media_root, `build`
/// exits non-zero and produces no output file. Verifies: SR-002, LLR-005,
/// SR-016 (TC-004).
#[test]
fn build_refuses_on_essential_failure_sr002() {
    let tmp = TempDir::new("build_refuse");
    let media = tmp.join("missing_media");
    let outdir = tmp.join("out");
    std::fs::create_dir_all(&outdir).unwrap();
    // media missing on purpose.

    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", 64, 48);

    let out = run(&cfg, "build");
    assert!(
        !out.status.success(),
        "build must refuse (exit non-zero) when an essential prereq fails"
    );
    assert!(
        !outdir.join("show.mp4").exists(),
        "build must not produce an output when it refuses to start"
    );
}
