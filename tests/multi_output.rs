//! SR-017 / LLR-020 — multiple output definitions in a single build produce one
//! MP4 per definition.

mod common;

use std::process::Command;

// Verifies: SR-017, LLR-020 — two [[outputs]] -> two distinct MP4s in one build.
#[test]
fn build_produces_one_mp4_per_output_sr017() {
    let tmp = common::TempDir::new("multi_out");
    let media = tmp.join("media");
    let out = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    common::write_png(&media.join("a.png"), 320, 240, [40, 90, 160]);

    let cfg = tmp.join("config.toml");
    let media_s = media.display().to_string().replace('\\', "/");
    let base_s = out.display().to_string().replace('\\', "/");
    let toml = format!(
        r#"[input]
media_root = "{media_s}"
ignore_patterns = []

[output]
base_dir = "{base_s}"

[processing]
temp_dir = "{base_s}/cache"
use_parallelism = true
dry_run = false
verbose = false
ffmpeg_timeout_secs = 120

[[outputs]]
name = "living-room"
width = 320
height = 240
fps = 24
pic_display_time_secs = 1.0
fade_time_secs = 0.2
max_rotation_degrees = 0.0
bulk_video_time_min = 20
quality_crf = 30
enable_audio = false

[[outputs]]
name = "bedroom"
width = 256
height = 144
fps = 24
pic_display_time_secs = 1.0
fade_time_secs = 0.2
max_rotation_degrees = 0.0
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
    assert!(status.success(), "build should exit 0");

    for name in ["living-room", "bedroom"] {
        let f = out.join(format!("{name}.mp4"));
        let meta =
            std::fs::metadata(&f).unwrap_or_else(|_| panic!("expected output {}", f.display()));
        assert!(meta.len() > 0, "{} should be non-empty", f.display());
    }
}
