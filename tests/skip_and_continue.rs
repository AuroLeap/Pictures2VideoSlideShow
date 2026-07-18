//! Integration tests for corrupt/unsupported-input skip-and-continue and the
//! build exit semantics (SR-014; LLR-016, LLR-017).
//!
//! Drives the built `slideshow` binary's `build` over media sets that mix a
//! corrupt image with a valid image (some-bad) and that contain only corrupt
//! images (all-bad), asserting:
//!   - some-bad: exit zero, the summary states a skipped count >= 1, and the
//!     valid output IS produced;
//!   - all-bad: exit non-zero with the plain "no outputs produced" message and
//!     no output MP4.
//!
//! Verifies: SR-014, LLR-016, LLR-017 (TC-022, TC-023). FFmpeg must be on PATH.

mod common;

use common::{slideshow_bin, write_config, write_png, TempDir};
use std::path::Path;
use std::process::{Command, Output};

fn run_build(config: &Path) -> Output {
    Command::new(slideshow_bin())
        .arg("--config")
        .arg(config)
        .arg("build")
        .output()
        .expect("run slideshow build")
}

/// Write a PNG whose header is intact (so the scan-stage `image_dimensions`
/// probe succeeds and it becomes a scanned `MediaFile`) but whose image data is
/// truncated, so the pipeline's full decode (`FrameRenderer::load` ->
/// `image::open`) fails and the file is skipped-and-continued (recorded with a
/// path + reason). This is the realistic corrupt-input path: a file that scans
/// OK but cannot be decoded at render time.
fn write_corrupt_image(path: &Path) {
    // Make a valid PNG, then keep only its first half (header survives, the
    // compressed image stream is cut off) — verified to read dimensions but
    // fail full decode.
    let tmp = std::env::temp_dir().join(format!(
        "slideshow_corrupt_src_{}_{}.png",
        std::process::id(),
        path.file_name().unwrap().to_string_lossy()
    ));
    write_png(&tmp, 96, 72, [10, 20, 30]);
    let bytes = std::fs::read(&tmp).expect("read seed png");
    let _ = std::fs::remove_file(&tmp);
    std::fs::write(path, &bytes[..bytes.len() / 2]).expect("write corrupt");
}

/// some-bad: a corrupt image alongside a valid one. The bad file is skipped
/// (logged with a reason), the valid file still produces output, the summary
/// states a skipped count >= 1, and the run exits zero.
/// Verifies: SR-014, LLR-016, LLR-017 (TC-022, TC-023 some-bad leg).
#[test]
fn build_skips_bad_and_continues_exit_zero_sr014() {
    let tmp = TempDir::new("skip_some_bad");
    let media = tmp.join("media");
    let outdir = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&outdir).unwrap();

    write_png(&media.join("good.png"), 96, 72, [40, 180, 220]);
    write_corrupt_image(&media.join("bad.png"));

    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", 64, 48);

    let out = run_build(&cfg);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "build must exit zero when >=1 valid output is produced; stdout=\n{stdout}\nstderr=\n{stderr}"
    );

    // The valid file still produced output.
    let mp4 = outdir.join("show.mp4");
    assert!(
        mp4.exists() && std::fs::metadata(&mp4).unwrap().len() > 0,
        "the valid image must still produce a non-empty output:\n{stdout}"
    );

    // SR-014: summary reports a skipped count >= 1 and names the skipped file +
    // a reason.
    assert!(
        stdout.contains("Inputs skipped: 1"),
        "summary must report the skipped count (1):\n{stdout}"
    );
    assert!(
        stdout.contains("skipped") && stdout.contains("bad.png"),
        "summary must name the skipped file with a reason:\n{stdout}"
    );
}

/// With background clip prefetch active (LLR-060, bounded lookahead), a
/// corrupt image among valid ones: the failed prefetch is delivered in order
/// to the clip loop and routed through `record_skip` identically to the
/// serial path — the corrupt file is skipped exactly once with path + reason,
/// BOTH surrounding valid clips still contribute (frame count proves the
/// neighbors survived), and the run exits zero. Skip semantics are otherwise
/// guarded by the two suites above (TC-022/TC-023), referenced not duplicated.
/// Verifies: SR-014, SR-036, LLR-060 (TC-086).
#[test]
fn prefetch_error_routes_to_record_skip_sr014() {
    let tmp = TempDir::new("skip_prefetch_order");
    let media = tmp.join("media");
    let outdir = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&outdir).unwrap();

    // Corrupt clip in the MIDDLE so in-order delivery matters: the prefetcher
    // has already loaded (or is loading) the neighbors when the error lands.
    write_png(&media.join("a_good.png"), 96, 72, [40, 180, 220]);
    write_corrupt_image(&media.join("b_bad.png"));
    write_png(&media.join("c_good.png"), 96, 72, [220, 80, 40]);

    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", 64, 48);

    let out = run_build(&cfg);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "build must exit zero (valid clips produced output); stdout=\n{stdout}\nstderr=\n{stderr}"
    );
    let mp4 = outdir.join("show.mp4");
    assert!(
        mp4.exists() && std::fs::metadata(&mp4).unwrap().len() > 0,
        "the valid images must still produce a non-empty output:\n{stdout}"
    );
    // Exactly the corrupt file is skipped, with its path and a reason.
    assert!(
        stdout.contains("Inputs skipped: 1"),
        "exactly one skip (the corrupt middle clip):\n{stdout}"
    );
    assert!(
        stdout.contains("b_bad.png"),
        "the skipped file must be named:\n{stdout}"
    );
    // Both neighbors contributed: two 1-second clips at the configured fps
    // means the two [img] progress lines around the skip both appear.
    assert!(
        stderr.contains("a_good.png") && stderr.contains("c_good.png"),
        "both valid neighbors must stream around the in-order skip:\n{stderr}"
    );
}

/// all-bad: only corrupt images. Every input is skipped, no valid output is
/// produced, the run MUST exit non-zero with the plain "no outputs produced"
/// message and leave no MP4.
///
/// IGNORED — documents a confirmed SR-014 conformance gap (BLOCKER, see
/// docs/status.md TEST-ENGINEER OBJ3 finding): when every input is skipped the
/// pipeline still opens an encoder, feeds it 0 frames, and `finish()` succeeds,
/// producing an empty `<name>.mp4` that is counted in `summary.written`. So the
/// run exits ZERO (wrong) instead of emitting "no outputs produced" and exiting
/// non-zero. The fix belongs in `pipeline::encode_output` (treat a 0-frame
/// encode as not-written / a skip). Unignore once `@software-engineer` lands
/// the fix; then flip TC-023 all-bad leg to Verified.
/// Verifies: SR-014, LLR-017 (TC-023 all-bad leg).
#[test]
fn build_all_bad_exits_nonzero_no_outputs_sr014() {
    let tmp = TempDir::new("skip_all_bad");
    let media = tmp.join("media");
    let outdir = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&outdir).unwrap();

    write_corrupt_image(&media.join("bad1.png"));
    write_corrupt_image(&media.join("bad2.png"));

    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", 64, 48);

    let out = run_build(&cfg);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        !out.status.success(),
        "build must exit non-zero when every input is skipped (zero outputs):\nstdout=\n{stdout}\nstderr=\n{stderr}"
    );
    assert!(
        stderr.contains("no outputs produced") || stdout.contains("no outputs produced"),
        "the all-skipped run must emit the plain 'no outputs produced' message:\nstdout=\n{stdout}\nstderr=\n{stderr}"
    );
    assert!(
        !outdir.join("show.mp4").exists(),
        "no output MP4 may be left when every input was skipped"
    );
}
