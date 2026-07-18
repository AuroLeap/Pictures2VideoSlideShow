//! SR-005 (frame-compatible encode profile) and SR-008 (CRF controls size).

mod common;

use std::path::Path;
use std::process::Command;

/// Write a config with one output at the given crf/dims.
fn write_cfg(cfg: &Path, media: &Path, out: &Path, name: &str, w: u32, h: u32, crf: u32) {
    write_cfg_enc(cfg, media, out, name, w, h, crf, None, None);
}

/// [`write_cfg`] plus optional SR-034/SR-035 `encoder` / `x264_preset` fields
/// (omitted lines keep the serde defaults: software / medium).
#[allow(clippy::too_many_arguments)]
fn write_cfg_enc(
    cfg: &Path,
    media: &Path,
    out: &Path,
    name: &str,
    w: u32,
    h: u32,
    crf: u32,
    encoder: Option<&str>,
    x264_preset: Option<&str>,
) {
    let media_s = media.display().to_string().replace('\\', "/");
    let base_s = out.display().to_string().replace('\\', "/");
    let enc_line = encoder
        .map(|e| format!("encoder = \"{e}\"\n"))
        .unwrap_or_default();
    let preset_line = x264_preset
        .map(|p| format!("x264_preset = \"{p}\"\n"))
        .unwrap_or_default();
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
name = "{name}"
width = {w}
height = {h}
fps = 24
pic_display_time_secs = 1.0
fade_time_secs = 0.2
max_rotation_degrees = 8.0
bulk_video_time_min = 20
quality_crf = {crf}
enable_audio = false
{enc_line}{preset_line}"#
    );
    std::fs::write(cfg, toml).unwrap();
}

/// A detailed pseudo-random PNG so compression (and thus CRF) actually matters.
fn write_noise_png(path: &Path, w: u32, h: u32) {
    let mut img = image::RgbImage::new(w, h);
    let mut s: u32 = 0x1234_5678;
    for p in img.pixels_mut() {
        // xorshift for cheap, deterministic noise.
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        *p = image::Rgb([
            (s & 0xff) as u8,
            ((s >> 8) & 0xff) as u8,
            ((s >> 16) & 0xff) as u8,
        ]);
    }
    img.save(path).expect("save noise png");
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

// Verifies: SR-005, LLR-009 — default-profile output is H.264/yuv420p, even
// dims, fps in 24-30, and faststart (moov before mdat).
#[test]
fn output_is_frame_compatible_profile_sr005() {
    let tmp = common::TempDir::new("profile");
    let media = tmp.join("media");
    let out = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    common::write_png(&media.join("a.png"), 320, 240, [200, 60, 60]);

    let cfg = tmp.join("config.toml");
    write_cfg(&cfg, &media, &out, "frame", 320, 240, 30);
    run_build(&cfg);

    let mp4 = out.join("frame.mp4");
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,pix_fmt,r_frame_rate",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(&mp4)
        .output()
        .expect("ffprobe");
    let text = String::from_utf8_lossy(&probe.stdout);
    let mut lines = text.lines();
    let codec = lines.next().unwrap_or("").trim();
    let width: u32 = lines.next().unwrap_or("0").trim().parse().unwrap_or(0);
    let height: u32 = lines.next().unwrap_or("0").trim().parse().unwrap_or(0);
    let pix_fmt = lines.next().unwrap_or("").trim();
    let rate = lines.next().unwrap_or("").trim();
    let fps: u32 = rate.split('/').next().unwrap_or("0").parse().unwrap_or(0);

    assert_eq!(codec, "h264", "codec");
    assert_eq!(pix_fmt, "yuv420p", "pixel format");
    assert_eq!(width % 2, 0, "even width");
    assert_eq!(height % 2, 0, "even height");
    assert!((24..=30).contains(&fps), "fps {fps} in 24..=30");

    // Faststart: the moov atom must precede mdat.
    let bytes = std::fs::read(&mp4).unwrap();
    let moov = bytes.windows(4).position(|w| w == b"moov");
    let mdat = bytes.windows(4).position(|w| w == b"mdat");
    assert!(
        matches!((moov, mdat), (Some(m), Some(d)) if m < d),
        "faststart: moov ({moov:?}) should precede mdat ({mdat:?})"
    );
}

// Verifies: SR-008, LLR-012 — lower CRF yields a larger file on identical input.
#[test]
fn lower_crf_yields_larger_output_sr008() {
    let tmp = common::TempDir::new("crf");
    let media = tmp.join("media");
    std::fs::create_dir_all(&media).unwrap();
    write_noise_png(&media.join("noise.png"), 320, 240);

    let out_lo = tmp.join("out_lo"); // low CRF = high quality = big
    let out_hi = tmp.join("out_hi"); // high CRF = low quality = small
    std::fs::create_dir_all(&out_lo).unwrap();
    std::fs::create_dir_all(&out_hi).unwrap();

    let cfg_lo = tmp.join("lo.toml");
    let cfg_hi = tmp.join("hi.toml");
    write_cfg(&cfg_lo, &media, &out_lo, "frame", 320, 240, 12);
    write_cfg(&cfg_hi, &media, &out_hi, "frame", 320, 240, 44);
    run_build(&cfg_lo);
    run_build(&cfg_hi);

    let size_lo = std::fs::metadata(out_lo.join("frame.mp4")).unwrap().len();
    let size_hi = std::fs::metadata(out_hi.join("frame.mp4")).unwrap().len();
    assert!(
        size_lo > size_hi,
        "crf12 ({size_lo} bytes) should exceed crf44 ({size_hi} bytes)"
    );
}

/// Build once with the given encoder/preset, assert exit 0, return combined
/// stdout+stderr, leaving `<out>/<name>.mp4` for profile checks.
fn run_build_capture(cfg: &Path) -> String {
    let output = Command::new(common::slideshow_bin())
        .arg("--config")
        .arg(cfg)
        .arg("--non-interactive")
        .arg("build")
        .output()
        .expect("run build");
    assert!(output.status.success(), "build should exit 0");
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

// Verifies: SR-035, SR-034, LLR-049, LLR-045 (TC-068 software leg) — explicit
// encoder=software with x264_preset at its boundary values (veryfast, slow)
// plus unset (default medium) keeps the SR-005 profile, and the per-output
// encoding line reports the encoder in use (libx264).
#[test]
fn software_presets_keep_frame_profile_sr035() {
    let tmp = common::TempDir::new("preset_profile");
    let media = tmp.join("media");
    std::fs::create_dir_all(&media).unwrap();
    common::write_png(&media.join("a.png"), 320, 240, [60, 180, 90]);

    for (label, preset) in [
        ("unset", None),
        ("veryfast", Some("veryfast")),
        ("slow", Some("slow")),
    ] {
        let out = tmp.join(&format!("out_{label}"));
        std::fs::create_dir_all(&out).unwrap();
        let cfg = tmp.join(&format!("{label}.toml"));
        write_cfg_enc(
            &cfg,
            &media,
            &out,
            "frame",
            320,
            240,
            30,
            Some("software"),
            preset,
        );
        let log = run_build_capture(&cfg);
        assert!(
            log.contains("libx264"),
            "[{label}] encoding line reports the encoder in use:\n{log}"
        );
        common::assert_sr005_profile(&out.join("frame.mp4"));
    }
}

// Verifies: SR-034, LLR-045 (TC-068 hardware legs) — every hardware encoder
// choice still yields an SR-005-profile output (either via the GPU encoder or
// the probe-driven libx264 fallback, both legal under SR-034).
#[test]
#[ignore = "Release tier (TC-068 hardware leg): exercises h264_nvenc/h264_qsv/h264_amf; run locally on GPU hosts with: cargo test --test encode_profile -- --ignored"]
fn hardware_encoders_keep_frame_profile_sr034() {
    let tmp = common::TempDir::new("hw_profile");
    let media = tmp.join("media");
    std::fs::create_dir_all(&media).unwrap();
    common::write_png(&media.join("a.png"), 320, 240, [200, 120, 40]);

    for enc in ["h264_nvenc", "h264_qsv", "h264_amf"] {
        let out = tmp.join(&format!("out_{enc}"));
        std::fs::create_dir_all(&out).unwrap();
        let cfg = tmp.join(&format!("{enc}.toml"));
        write_cfg_enc(&cfg, &media, &out, "frame", 320, 240, 30, Some(enc), None);
        let log = run_build_capture(&cfg);
        assert!(
            log.contains(enc) || log.contains("libx264"),
            "[{enc}] reports the encoder in use (selected or fallback):\n{log}"
        );
        common::assert_sr005_profile(&out.join("frame.mp4"));
    }
}
