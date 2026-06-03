//! Integration tests that exercise the video decode path (`src/video`,
//! `VideoFrameReader`) and the SR-004 build completion summary.
//!
//! A tiny test video is synthesized with ffmpeg (`testsrc`) alongside a
//! synthesized PNG into a temp media dir; the built `slideshow` binary then
//! builds an MP4 over the mixed image+video album. This is the only test that
//! actually decodes a video, so it covers `VideoFrameReader::open`/`read_frame`.
//!
//! Verifies: SR-004 (completion summary), and the video passthrough path used
//! by SR-005/SR-017. (TC-008, TC-009, TC-050.) FFmpeg/ffprobe must be on PATH
//! (CI invariant).

mod common;

use common::{slideshow_bin, write_config, write_png, TempDir};
use std::path::Path;
use std::process::{Command, Output};

/// Synthesize a tiny test video at `path`. Returns true on success so the test
/// can give a clear skip message if ffmpeg genuinely cannot synth (it should).
fn synth_video(path: &Path, seconds: u32, w: u32, h: u32, rate: u32) -> bool {
    let lavfi = format!("testsrc=duration={seconds}:size={w}x{h}:rate={rate}");
    let status = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg(&lavfi)
        .args(["-pix_fmt", "yuv420p"])
        .arg(path)
        .status();
    matches!(status, Ok(s) if s.success()) && path.exists()
}

fn run_build(config: &Path) -> Output {
    Command::new(slideshow_bin())
        .arg("--config")
        .arg(config)
        .arg("build")
        .output()
        .expect("run slideshow build")
}

/// Video decode path + SR-004 summary: a media dir containing a synthesized
/// `.mp4` and a `.png` builds to one output MP4. Exercises `VideoFrameReader`
/// end-to-end and asserts the completion summary lists the written output path
/// with a size and a skipped-count line.
/// Verifies: SR-004, video passthrough (TC-008, TC-009, TC-050).
#[test]
fn build_decodes_video_and_emits_summary_sr004() {
    let tmp = TempDir::new("video_build");
    let media = tmp.join("media");
    let outdir = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&outdir).unwrap();

    // A still image and a short synthesized video, mixed in one album.
    write_png(&media.join("a.png"), 96, 72, [200, 60, 60]);
    let vid = media.join("clip.mp4");
    if !synth_video(&vid, 1, 320, 240, 10) {
        panic!("ffmpeg could not synthesize the test video (required on PATH for this test)");
    }

    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", 64, 48);

    let out = run_build(&cfg);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "build over image+video should exit zero; stdout=\n{stdout}\nstderr=\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The video path produced a real output that the image-only path could not
    // have produced on its own — assert the MP4 exists and is non-trivial.
    let mp4 = outdir.join("show.mp4");
    assert!(mp4.exists(), "build must produce show.mp4:\n{stdout}");
    assert!(
        std::fs::metadata(&mp4).unwrap().len() > 0,
        "output mp4 must be non-empty"
    );

    // SR-004 completion summary: the summary block lists the written output
    // path WITH a size, and reports the skipped count.
    assert!(
        stdout.contains("=== Build Summary ==="),
        "summary block must be printed:\n{stdout}"
    );
    assert!(
        stdout.contains("Outputs written: 1"),
        "summary must report one written output:\n{stdout}"
    );
    assert!(
        stdout.contains("show.mp4"),
        "summary must name the written output path:\n{stdout}"
    );
    // Each written line is `  <path> (<size>)`; a human-readable size in
    // parentheses (e.g. "KB"/"MB"/"B") proves a size is shown.
    assert!(
        stdout.contains("show.mp4 (")
            && (stdout.contains("KB)") || stdout.contains("MB)") || stdout.contains("B)")),
        "summary must list the output with a size:\n{stdout}"
    );
    assert!(
        stdout.contains("Inputs skipped: 0"),
        "summary must report the skipped count (0 here):\n{stdout}"
    );
}
