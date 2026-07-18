//! SR-031 / LLR-038 — a build with an `roi_db` configured loads the JSON focus
//! database and produces output, exercising the end-to-end ROI-focus path
//! (main.rs ROI load -> pipeline per-image focus -> FrameRenderer::load_with_focus).

mod common;

use std::process::Command;

// Verifies: SR-031, LLR-038 — build with a configured ROI database (one point,
// one bbox, plus an unlisted image that falls back to default_focus) succeeds
// and writes a non-empty MP4.
#[test]
fn build_with_roi_db_focuses_and_succeeds_sr031() {
    let tmp = common::TempDir::new("roi_focus");
    let media = tmp.join("media");
    let sub = media.join("2015");
    let out = tmp.join("out");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    // One image gets a point ROI, one a bbox ROI, one has no entry (default).
    common::write_png(&sub.join("face.png"), 320, 240, [200, 120, 60]);
    common::write_png(&sub.join("group.png"), 320, 240, [60, 120, 200]);
    common::write_png(&media.join("scenery.png"), 320, 240, [60, 200, 120]);

    // ROI database keyed by path relative to media_root (forward slashes).
    let roi = tmp.join("focus.json");
    std::fs::write(
        &roi,
        r#"{
            "2015/face.png": { "x": 0.40, "y": 0.35 },
            "2015/group.png": { "bbox": [0.20, 0.20, 0.30, 0.30] }
        }"#,
    )
    .unwrap();

    let cfg = tmp.join("config.toml");
    let media_s = media.display().to_string().replace('\\', "/");
    let base_s = out.display().to_string().replace('\\', "/");
    let roi_s = roi.display().to_string().replace('\\', "/");
    let toml = format!(
        r#"[input]
media_root = "{media_s}"
ignore_patterns = []
roi_db = "{roi_s}"

[output]
base_dir = "{base_s}"

[processing]
temp_dir = "{base_s}/cache"
use_parallelism = true
dry_run = false
verbose = false
ffmpeg_timeout_secs = 120
default_focus = [0.5, 0.4]

[[outputs]]
name = "frame"
width = 320
height = 240
fps = 24
pic_display_time_secs = 1.0
fade_time_secs = 0.2
max_rotation_degrees = 6.0
bulk_video_time_min = 20
quality_crf = 30
enable_audio = false
"#
    );
    std::fs::write(&cfg, toml).unwrap();

    let status = Command::new(common::slideshow_bin())
        .args(["--config"])
        .arg(&cfg)
        .arg("--non-interactive")
        .arg("build")
        .status()
        .expect("run build");
    assert!(status.success(), "build with roi_db should exit 0");

    let f = out.join("frame.mp4");
    let meta = std::fs::metadata(&f).unwrap_or_else(|_| panic!("expected output {}", f.display()));
    assert!(meta.len() > 0, "{} should be non-empty", f.display());
}
