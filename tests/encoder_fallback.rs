//! SR-034 fallback + reporting legs (TC-067): a hardware-encoder request on a
//! host whose FFmpeg offers no hardware encoder must fall back to libx264 with
//! a logged reason, complete with exit zero, and still produce an SR-005
//! profile output — never fail the build.
//!
//! Host-independent forcing: the config's `ffmpeg_path` points at a fake
//! FFmpeg wrapper whose `-encoders` listing contains only libx264, so every
//! hardware candidate probes Absent deterministically (the probe consults the
//! *resolved* FFmpeg, LLR-046). The build's actual encode still runs the real
//! `ffmpeg` from PATH — the fake is deliberately not named `ffmpeg`.

mod common;

use std::path::Path;
use std::process::Command;

/// Write the fake FFmpeg wrapper: answers `-version` (so resolution accepts
/// it) and `-encoders` (libx264 only); exits 0 for anything else.
#[cfg(windows)]
fn write_fake_ffmpeg(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("fake_ffmpeg.cmd");
    let script = "@echo off\r\n\
        echo %* | findstr /C:\"-encoders\" >nul\r\n\
        if not errorlevel 1 (\r\n\
        echo Encoders:\r\n\
        echo  V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC\r\n\
        exit /b 0\r\n\
        )\r\n\
        echo ffmpeg version 0.0-fake-no-hardware\r\n\
        exit /b 0\r\n";
    std::fs::write(&path, script).expect("write fake ffmpeg");
    path
}

#[cfg(unix)]
fn write_fake_ffmpeg(dir: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake_ffmpeg.sh");
    let script = "#!/bin/sh\n\
        case \"$*\" in *-encoders*)\n\
        echo 'Encoders:'\n\
        echo ' V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC'\n\
        exit 0;;\n\
        esac\n\
        echo 'ffmpeg version 0.0-fake-no-hardware'\n\
        exit 0\n";
    std::fs::write(&path, script).expect("write fake ffmpeg");
    let mut perms = std::fs::metadata(&path).expect("meta").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).expect("chmod");
    path
}

// Verifies: SR-034, LLR-046, LLR-048 (TC-067) — encoder=auto with no working
// hardware encoder: fallback WARN names the candidates and reason, the run
// reports libx264 in use, exits zero, and the output passes SR-005.
#[test]
fn auto_without_hardware_falls_back_and_reports_sr034() {
    let tmp = common::TempDir::new("encfallback");
    let media = tmp.join("media");
    let out = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    common::write_png(&media.join("a.png"), 320, 240, [40, 120, 220]);

    let fake = write_fake_ffmpeg(tmp.path());
    let media_s = media.display().to_string().replace('\\', "/");
    let base_s = out.display().to_string().replace('\\', "/");
    let fake_s = fake.display().to_string().replace('\\', "/");
    let toml = format!(
        r#"[input]
media_root = "{media_s}"
ignore_patterns = []

[output]
base_dir = "{base_s}"

[processing]
temp_dir = "{base_s}/cache"
ffmpeg_path = "{fake_s}"

[[outputs]]
name = "fallback"
width = 320
height = 240
fps = 24
pic_display_time_secs = 1.0
fade_time_secs = 0.2
max_rotation_degrees = 0.0
bulk_video_time_min = 20
quality_crf = 30
enable_audio = false
encoder = "auto"
"#
    );
    let cfg = tmp.join("config.toml");
    std::fs::write(&cfg, toml).unwrap();

    let output = Command::new(common::slideshow_bin())
        .arg("--config")
        .arg(&cfg)
        .arg("--non-interactive")
        .arg("build")
        .output()
        .expect("run build");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let all = format!("{stdout}\n{stderr}");

    // Never a build failure for a missing hardware encoder (SR-034).
    assert!(
        output.status.success(),
        "build must exit 0 despite no hardware encoder; output:\n{all}"
    );
    // Fallback WARN names the probed encoder(s) and the reason.
    assert!(all.contains("h264_nvenc"), "warn names the encoder:\n{all}");
    assert!(
        all.to_lowercase().contains("falling back"),
        "warn states the fallback:\n{all}"
    );
    // The run reports the encoder actually used (libx264 fallback).
    assert!(all.contains("libx264"), "reports encoder in use:\n{all}");

    // The fallback-encoded output still satisfies the SR-005 profile.
    common::assert_sr005_profile(&out.join("fallback.mp4"));
}
