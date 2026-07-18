//! SR-017 / LLR-020 — multiple output definitions in a single build produce one
//! MP4 per definition — and LLR-045: `build --output <name>` restricts the run
//! to the one named definition (unknown names fail fast).

mod common;

use std::path::Path;
use std::process::Command;

/// Write the shared two-output config used by the tests in this file.
fn write_two_output_config(cfg: &Path, media: &Path, out: &Path) {
    let media_s = media.display().to_string().replace('\\', "/");
    let base_s = out.display().to_string().replace('\\', "/");
    let toml = format!(
        r#"[input]
media_root = "{media_s}"
ignore_patterns = []

[output]
base_dir = "{base_s}"

[processing]
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
    std::fs::write(cfg, toml).unwrap();
}

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
    write_two_output_config(&cfg, &media, &out);

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

// Verifies: SR-017, LLR-045 — `build --output <name>` builds ONLY the named
// output definition; the other definitions are untouched.
#[test]
fn build_output_filter_builds_only_named_definition_sr017() {
    let tmp = common::TempDir::new("multi_out_filter");
    let media = tmp.join("media");
    let out = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    common::write_png(&media.join("a.png"), 320, 240, [90, 40, 160]);

    let cfg = tmp.join("config.toml");
    write_two_output_config(&cfg, &media, &out);

    let status = Command::new(common::slideshow_bin())
        .arg("--config")
        .arg(&cfg)
        .arg("--non-interactive")
        .args(["build", "--output", "bedroom"])
        .status()
        .expect("run filtered build");
    assert!(status.success(), "filtered build should exit 0");

    let built = out.join("bedroom.mp4");
    assert!(
        std::fs::metadata(&built)
            .map(|m| m.len() > 0)
            .unwrap_or(false),
        "the named output must be built"
    );
    assert!(
        !out.join("living-room.mp4").exists(),
        "the unnamed output must NOT be built"
    );
}

// Verifies: SR-017, LLR-045 — an unknown `--output` name fails fast (non-zero,
// nothing encoded) with a message naming the available definitions.
#[test]
fn build_output_filter_unknown_name_fails_fast_sr017() {
    let tmp = common::TempDir::new("multi_out_unknown");
    let media = tmp.join("media");
    let out = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    common::write_png(&media.join("a.png"), 320, 240, [10, 40, 60]);

    let cfg = tmp.join("config.toml");
    write_two_output_config(&cfg, &media, &out);

    let result = Command::new(common::slideshow_bin())
        .arg("--config")
        .arg(&cfg)
        .arg("--non-interactive")
        .args(["build", "--output", "nope"])
        .output()
        .expect("run filtered build");
    assert!(!result.status.success(), "unknown output name must fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("nope") && stderr.contains("living-room") && stderr.contains("bedroom"),
        "error must name the bad value and the available outputs:\n{stderr}"
    );
    assert!(
        !out.join("living-room.mp4").exists() && !out.join("bedroom.mp4").exists(),
        "nothing may be encoded on an unknown output name"
    );
}
