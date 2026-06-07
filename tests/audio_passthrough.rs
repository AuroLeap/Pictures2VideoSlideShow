//! SR-032 / LLR-041 — source-video audio passthrough.
//!
//! Builds with `enable_audio = true` over an album containing a synthesized
//! video *with* an audio track, and asserts the output carries an AAC audio
//! stream whose duration tracks the (crossfaded) video timeline. A second test
//! confirms an image-only album still finalizes (silently) when audio is on.

mod common;

use std::path::Path;
use std::process::Command;

/// Synthesize a tiny H.264+AAC video (`secs` long) via ffmpeg lavfi: a test
/// pattern plus a sine tone, so the clip actually carries decodable audio.
fn synth_video_with_audio(path: &Path, secs: u32) {
    let dur = secs.to_string();
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=duration={dur}:size=320x240:rate=24"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={dur}"),
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(path)
        .status()
        .expect("spawn ffmpeg to synth video");
    assert!(status.success(), "ffmpeg synth should succeed");
}

fn write_audio_cfg(cfg: &Path, media: &Path, out: &Path, enable_audio: bool) {
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
name = "frame"
width = 320
height = 240
fps = 24
pic_display_time_secs = 1.0
fade_time_secs = 0.2
max_rotation_degrees = 0.0
bulk_video_time_min = 20
quality_crf = 30
enable_audio = {enable_audio}
audio_bitrate_kbps = 128
audio_sample_rate = 48000
"#
    );
    std::fs::write(cfg, toml).unwrap();
}

fn run_build(cfg: &Path) {
    let status = Command::new(common::slideshow_bin())
        .arg("--config")
        .arg(cfg)
        .arg("--non-interactive")
        .arg("build")
        .status()
        .expect("run build");
    assert!(status.success(), "build should exit 0");
}

/// First audio stream's codec name (e.g. "aac"), or empty if none.
fn probe_audio_codec(mp4: &Path) -> String {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(mp4)
        .output()
        .expect("ffprobe audio");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn probe_format_duration(mp4: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(mp4)
        .output()
        .expect("ffprobe duration");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .unwrap_or(0.0)
}

// Verifies: SR-032, LLR-041 — with audio enabled, a video clip's audio is
// preserved as an AAC stream sized to the (crossfaded) output timeline.
#[test]
fn build_passes_through_video_audio_sr032() {
    let tmp = common::TempDir::new("audio_pass");
    let media = tmp.join("media");
    let out = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    // "a.png" sorts before "v.mp4": image clip first, then the audio video.
    common::write_png(&media.join("a.png"), 320, 240, [60, 140, 200]);
    synth_video_with_audio(&media.join("v.mp4"), 2);

    let cfg = tmp.join("config.toml");
    write_audio_cfg(&cfg, &media, &out, true);
    run_build(&cfg);

    let mp4 = out.join("frame.mp4");
    assert!(mp4.exists(), "output should exist");
    assert_eq!(
        probe_audio_codec(&mp4),
        "aac",
        "output should carry an AAC track"
    );

    // Output spans ~ image (1s) + video (2s) minus the ~0.2s crossfade overlap.
    let dur = probe_format_duration(&mp4);
    assert!(
        (1.5..4.0).contains(&dur),
        "audio output duration {dur}s should track the crossfaded timeline"
    );
}

// Verifies: SR-032, LLR-041 — enabling audio on an image-only album still
// finalizes a playable (silent) output via the promote path, not an error.
#[test]
fn audio_enabled_image_only_album_is_silent_but_succeeds_sr032() {
    let tmp = common::TempDir::new("audio_silent");
    let media = tmp.join("media");
    let out = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    common::write_png(&media.join("a.png"), 320, 240, [200, 90, 90]);
    common::write_png(&media.join("b.png"), 320, 240, [90, 200, 90]);

    let cfg = tmp.join("config.toml");
    write_audio_cfg(&cfg, &media, &out, true);
    run_build(&cfg);

    let mp4 = out.join("frame.mp4");
    assert!(
        std::fs::metadata(&mp4)
            .map(|m| m.len() > 0)
            .unwrap_or(false),
        "silent output should still be produced"
    );
    assert_eq!(
        probe_audio_codec(&mp4),
        "",
        "image-only album should have no audio stream"
    );
}
