//! SR-039 render-backend integration legs (TC-099/TC-100/TC-101 + the
//! TC-102 GPU Demonstration leg): auto-fallback equivalence, explicit-gpu
//! fallback warning, and the backend-in-use reporting line.
//!
//! Host-independent forcing (CI has no GPU; this dev box has one): the child
//! build runs with `WGPU_BACKEND=gl` — wgpu's own backend-override convention
//! — and the engine compiles only DX12/Vulkan (LLR-074 feature trim), so the
//! adapter probe finds nothing and the SR-039 fallback paths run identically
//! on a GPU-less CI runner and a GPU dev box. The `#[ignore]`d TC-102 leg is
//! the inverse: it needs a real adapter and runs via `-- --ignored`.

mod common;

use std::path::Path;
use std::process::Command;

/// Write a config whose `[processing]` carries `render_backend` (`""` omits
/// the field — the SR-039 default-to-auto leg).
fn write_config(cfg: &Path, media: &Path, base: &Path, render_backend: &str) {
    let media_s = media.display().to_string().replace('\\', "/");
    let base_s = base.display().to_string().replace('\\', "/");
    let backend_line = if render_backend.is_empty() {
        String::new()
    } else {
        format!("render_backend = \"{render_backend}\"")
    };
    let toml = format!(
        r#"[input]
media_root = "{media_s}"
ignore_patterns = []

[output]
base_dir = "{base_s}"

[processing]
temp_dir = "{base_s}/cache"
{backend_line}

[[outputs]]
name = "clip"
width = 320
height = 240
fps = 24
pic_display_time_secs = 1.0
fade_time_secs = 0.2
max_rotation_degrees = 5.0
bulk_video_time_min = 20
quality_crf = 30
enable_audio = false
"#
    );
    std::fs::write(cfg, toml).expect("write config");
}

/// Run one `build --no-cache` (deterministic streaming path, SR-022) with an
/// optional `WGPU_BACKEND` override; returns (exit ok, combined output).
fn run_build(cfg: &Path, wgpu_backend: Option<&str>) -> (bool, String) {
    let mut cmd = Command::new(common::slideshow_bin());
    cmd.arg("--config")
        .arg(cfg)
        .arg("--non-interactive")
        .arg("--no-cache")
        .arg("build")
        .env_remove("WGPU_BACKEND");
    if let Some(b) = wgpu_backend {
        cmd.env("WGPU_BACKEND", b);
    }
    let output = cmd.output().expect("run build");
    let all = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), all)
}

/// Seed a small deterministic media dir (noise so pixel drift is visible).
fn seed_media(media: &Path) {
    std::fs::create_dir_all(media).unwrap();
    common::write_noise_png(&media.join("a.png"), 400, 300, 11);
    common::write_noise_png(&media.join("b.png"), 400, 300, 22);
}

/// Count backend-in-use report lines, case-insensitively.
fn backend_lines(all: &str) -> usize {
    all.to_lowercase().matches("rendering with ").count()
}

// Verifies: SR-039, LLR-068, LLR-072 (TC-099) — render_backend=auto on a
// host with no usable wgpu adapter: the run logs the CPU-fallback reason,
// completes exit 0, the output passes the SR-005 ffprobe checks, and the MP4
// is byte-identical to an explicit render_backend=cpu build of the same
// corpus on the same host (auto-fallback and explicit cpu resolve to the
// same renderer; SR-022 within-backend determinism makes them comparable).
#[test]
fn auto_without_adapter_falls_back_and_matches_cpu_sr039() {
    let tmp = common::TempDir::new("rbauto");
    let media = tmp.join("media");
    seed_media(&media);

    // auto (field omitted -> SR-039 default) with the adapter forced absent.
    let out_auto = tmp.join("out_auto");
    std::fs::create_dir_all(&out_auto).unwrap();
    let cfg_auto = tmp.join("auto.toml");
    write_config(&cfg_auto, &media, &out_auto, "auto");
    let (ok, all) = run_build(&cfg_auto, Some("gl"));
    assert!(ok, "auto build must exit 0 with no adapter; output:\n{all}");
    let lower = all.to_lowercase();
    assert!(
        lower.contains("rendering with cpu (fallback:") && lower.contains("adapter"),
        "auto fallback must log the reason:\n{all}"
    );

    // Explicit cpu (the probe is never consulted, LLR-068).
    let out_cpu = tmp.join("out_cpu");
    std::fs::create_dir_all(&out_cpu).unwrap();
    let cfg_cpu = tmp.join("cpu.toml");
    write_config(&cfg_cpu, &media, &out_cpu, "cpu");
    let (ok, all_cpu) = run_build(&cfg_cpu, None);
    assert!(ok, "cpu build must exit 0; output:\n{all_cpu}");

    // Both resolve to the same renderer -> byte-identical outputs.
    let mp4_auto = out_auto.join("clip.mp4");
    let mp4_cpu = out_cpu.join("clip.mp4");
    common::assert_sr005_profile(&mp4_auto);
    assert_eq!(
        std::fs::read(&mp4_auto).expect("read auto mp4"),
        std::fs::read(&mp4_cpu).expect("read cpu mp4"),
        "auto-fallback and explicit cpu must produce byte-identical MP4s"
    );
}

// Verifies: SR-039, LLR-068, LLR-072 (TC-100) — render_backend=gpu explicitly
// selected on a host whose adapter probe finds none: the run logs a WARN
// naming the gpu backend and the probe reason, falls back to the CPU
// renderer, completes exit 0, and the output passes SR-005 — a build never
// fails solely for a missing adapter. Mirrors TC-067.
#[test]
fn explicit_gpu_without_adapter_warns_and_falls_back_sr039() {
    let tmp = common::TempDir::new("rbgpu");
    let media = tmp.join("media");
    seed_media(&media);
    let out = tmp.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let cfg = tmp.join("gpu.toml");
    write_config(&cfg, &media, &out, "gpu");

    let (ok, all) = run_build(&cfg, Some("gl"));
    assert!(
        ok,
        "explicit gpu with no adapter must still exit 0; output:\n{all}"
    );
    // The warning names the requested backend and the probe outcome.
    assert!(all.contains("WARN"), "fallback is a warning:\n{all}");
    assert!(
        all.contains("render_backend=gpu"),
        "warn names the gpu backend:\n{all}"
    );
    assert!(
        all.to_lowercase().contains("adapter"),
        "warn names the probe reason:\n{all}"
    );
    // The run reports the renderer actually used (cpu fallback form).
    assert!(
        all.to_lowercase().contains("rendering with cpu (fallback:"),
        "reports the backend in use:\n{all}"
    );
    common::assert_sr005_profile(&out.join("clip.mp4"));
}

// Verifies: SR-039, LLR-072 (TC-101) — exactly one backend-in-use line per
// build, reporting the render backend actually used (the CI cpu form here;
// the gpu form is asserted on hardware by the TC-102 leg below), mirroring
// the SR-034 encoder-in-use line.
#[test]
fn build_reports_render_backend_in_use_sr039() {
    let tmp = common::TempDir::new("rbline");
    let media = tmp.join("media");
    seed_media(&media);
    let out = tmp.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let cfg = tmp.join("default.toml");
    // Field omitted: the default-auto leg (TC-096/TC-099 cover the value).
    write_config(&cfg, &media, &out, "");

    let (ok, all) = run_build(&cfg, Some("gl"));
    assert!(ok, "build must exit 0; output:\n{all}");
    assert_eq!(
        backend_lines(&all),
        1,
        "exactly one backend-in-use line per build:\n{all}"
    );
    assert!(
        all.to_lowercase().contains("rendering with cpu"),
        "the line reports the cpu form on a no-adapter host:\n{all}"
    );
}

// Verifies: SR-040, LLR-075, LLR-080 (TC-113) — a cpu-backend build and an
// auto-fallback build both select the rgb24 transport through the LLR-075
// home and say so alongside the LLR-072 backend-in-use line. The byte-level
// invariance itself is carried by the existing suites passing with ZERO test
// edits — TC-099 (auto-fallback MP4 byte-identical to explicit cpu), TC-098,
// TC-010, TC-018, TC-021, TC-022/023, TC-057/058/059, TC-089 — cited, not
// duplicated; this test adds only the transport-reported assertion.
#[test]
fn cpu_backend_keeps_rgb24_transport_sr040() {
    let tmp = common::TempDir::new("rbrgb24");
    let media = tmp.join("media");
    seed_media(&media);

    for (label, backend, wgpu) in [
        ("explicit cpu", "cpu", None),
        ("auto-fallback", "auto", Some("gl")),
    ] {
        let out = tmp.join(&format!("out_{backend}"));
        std::fs::create_dir_all(&out).unwrap();
        let cfg = tmp.join(&format!("{backend}.toml"));
        write_config(&cfg, &media, &out, backend);
        let (ok, all) = run_build(&cfg, wgpu);
        assert!(ok, "{label} build must exit 0; output:\n{all}");
        let lower = all.to_lowercase();
        assert!(
            lower.contains("rendering with cpu"),
            "{label}: cpu backend reported:\n{all}"
        );
        assert!(
            lower.contains("(rgb24 transport)"),
            "{label}: the rgb24 transport must be named on the backend-in-use line:\n{all}"
        );
        assert!(
            !lower.contains("yuv420p transport"),
            "{label}: a cpu run must never select the yuv transport:\n{all}"
        );
    }
}

/// Deterministic grayscale-noise PNG (r=g=b): full luma structure, zero
/// chroma — the achromatic fade content the SR-040 chroma-neutral assertions
/// need (any chroma seen in its fade window is conversion/fade tint).
fn write_gray_noise_png(path: &Path, w: u32, h: u32, seed: u32) {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let mut img = image::RgbImage::new(w, h);
    for p in img.pixels_mut() {
        let v = (next() & 0xFF) as u8;
        *p = image::Rgb([v, v, v]);
    }
    img.save(path).expect("save gray noise png");
}

/// Smooth full-range ramp PNG (no modulo wraps): resample-stable
/// cross-backend content whose gentle chroma slopes keep the KNOWN
/// subsample-filter difference (our 2x2 box vs swscale's default — the
/// Round-9b tolerance NOTE, measured ~0.15/255 decoded on this content)
/// out of the colorimetry measurement, while a matrix/range mistake still
/// shifts it by several units (a deliberate BT.709 conversion measures
/// ~6.4/255 against the swscale referee on the same content).
fn write_gradient_png(path: &Path, w: u32, h: u32, phase: u32) {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([
            ((x + phase).min(w - 1) * 255 / (w - 1)) as u8,
            ((y + phase).min(h - 1) * 255 / (h - 1)) as u8,
            ((x + y) * 255 / (w + h - 2)) as u8,
        ])
    });
    img.save(path).expect("save gradient png");
}

/// Decode every frame of `mp4` as rawvideo yuv420p (no rgb conversion).
fn decode_yuv_frames(mp4: &Path, w: u32, h: u32) -> Vec<Vec<u8>> {
    let out = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(mp4)
        .args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "pipe:1"])
        .output()
        .expect("run ffmpeg yuv decode");
    assert!(out.status.success(), "yuv decode of {}", mp4.display());
    let frame_bytes = (w * h * 3 / 2) as usize;
    assert_eq!(out.stdout.len() % frame_bytes, 0, "whole yuv frames");
    out.stdout.chunks(frame_bytes).map(|c| c.to_vec()).collect()
}

// Verifies: SR-040, LLR-076 (TC-115, Release leg) — a render_backend=gpu
// build completes reporting gpu in use WITH the yuv420p transport named:
// every frame reached the encoder as w*h*3/2 planar yuv420p (a wrong size
// would break the rawvideo demuxer and fail the build; the tightly-packed
// buffer-to-buffer readback and rgb-only strip_readback are the LLR-076
// design realized in src/image/gpu.rs), and ffprobe of the output passes the
// SR-005 checks identically to an rgb24-transport build. The
// unchanged-semantics leg (SR-032, SR-011/013/014/015 suites unmodified with
// gpu active) is the full `cargo test --all` run on this GPU host.
#[test]
#[ignore = "Release tier (TC-115): needs a GPU adapter"]
fn gpu_yuv_build_passes_profile_sr040() {
    let tmp = common::TempDir::new("rbyuv");
    let media = tmp.join("media");
    seed_media(&media);
    let out = tmp.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let cfg = tmp.join("gpu.toml");
    write_config(&cfg, &media, &out, "gpu");

    let (ok, all) = run_build(&cfg, None);
    assert!(ok, "gpu yuv build must exit 0; output:\n{all}");
    let lower = all.to_lowercase();
    assert!(
        lower.contains("rendering with gpu ("),
        "gpu backend reported:\n{all}"
    );
    assert!(
        lower.contains("(yuv420p transport)"),
        "the yuv420p transport must be named:\n{all}"
    );
    common::assert_sr005_profile(&out.join("clip.mp4"));
}

/// The colorimetry-reference config: identical to [`write_config`] except a
/// transparent encode (crf 10), so the decoded comparison measures the
/// transport colorimetry instead of CRF-30 quantization divergence — two
/// slightly-different inputs quantized at CRF 30 differ by ~the quantization
/// step (measured ~5/255 mean), drowning any matrix signal either way.
fn write_colorimetry_config(cfg: &Path, media: &Path, base: &Path, render_backend: &str) {
    let media_s = media.display().to_string().replace('\\', "/");
    let base_s = base.display().to_string().replace('\\', "/");
    let toml = format!(
        r#"[input]
media_root = "{media_s}"
ignore_patterns = []

[output]
base_dir = "{base_s}"

[processing]
temp_dir = "{base_s}/cache"
render_backend = "{render_backend}"

[[outputs]]
name = "clip"
width = 320
height = 240
fps = 24
pic_display_time_secs = 1.0
fade_time_secs = 0.2
max_rotation_degrees = 5.0
bulk_video_time_min = 20
quality_crf = 10
enable_audio = false
"#
    );
    std::fs::write(cfg, toml).expect("write config");
}

// Verifies: SR-040, SR-039, LLR-082 (TC-116, Release leg) — the colorimetry
// leg per the LLR-082 recorded comparison-space call: build the reference
// mixed album once with render_backend=cpu (rgb24 transport) and once with
// render_backend=gpu (yuv420p transport), decode BOTH finished MP4s back to
// rgb24 with the SAME ffmpeg invocation (common::decode_frames — ffmpeg's
// historical swscale conversion is the independent referee), and compare
// per-frame via the one diff home under the TC-103 epsilon (1.0 of 255). A
// shader matrix/range diverging from the historical rgb24->FFmpeg conversion
// (full-vs-limited would shift ~18/255, 601-vs-709 several units on
// saturated content) shifts the decoded frames and trips this.
#[test]
#[ignore = "Release tier (TC-116): needs a GPU adapter"]
fn gpu_yuv_vs_cpu_colorimetry_sr040() {
    let tmp = common::TempDir::new("rbcolor");
    let media = tmp.join("media");
    std::fs::create_dir_all(&media).unwrap();
    write_gradient_png(&media.join("a.png"), 400, 300, 0);
    write_gradient_png(&media.join("b.png"), 400, 300, 97);
    // Smooth animated-gradient video (same rationale as the ramp PNGs: the
    // testsrc color bars' sharp chroma edges measure the known subsample
    // -filter divergence, not the matrix; matrix errors shift smooth content
    // just as hard). testsrc structure itself stays covered by TC-050/TC-114.
    // Muted colors: the video leg's residual otherwise measures the REMOVED
    // rgb round-trip's chroma double-filtering (the LLR-078 improvement,
    // explicitly tolerance-based, never bit-claimed) rather than the matrix.
    let ok = std::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("gradients=size=320x240:rate=24:duration=2:c0=0x606870:c1=0x907868:n=2")
        .args(["-pix_fmt", "yuv420p"])
        .arg(media.join("c.mp4"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "ffmpeg must synthesize the gradients video");

    let mut mp4s = Vec::new();
    for backend in ["cpu", "gpu"] {
        let out = tmp.join(&format!("out_{backend}"));
        std::fs::create_dir_all(&out).unwrap();
        let cfg = tmp.join(&format!("{backend}.toml"));
        write_colorimetry_config(&cfg, &media, &out, backend);
        let (ok, all) = run_build(&cfg, None);
        assert!(ok, "{backend} build must exit 0; output:\n{all}");
        mp4s.push(out.join("clip.mp4"));
    }

    let cpu_frames = common::decode_frames(&mp4s[0], 320, 240);
    let gpu_frames = common::decode_frames(&mp4s[1], 320, 240);
    assert_eq!(
        cpu_frames.len(),
        gpu_frames.len(),
        "both builds must emit the same frame count"
    );
    let diffs: Vec<f64> = cpu_frames
        .iter()
        .zip(&gpu_frames)
        .map(|(c, g)| common::mean_abs_diff(c, g))
        .collect();
    eprintln!("per-frame diffs: {diffs:.3?}");
    // Diagnostic: signed per-frame mean (colorimetry error = systematic
    // shift; encoder quantization noise = ~zero-mean).
    let signed: Vec<f64> = cpu_frames
        .iter()
        .zip(&gpu_frames)
        .map(|(c, g)| {
            c.iter()
                .zip(g.iter())
                .map(|(&a, &b)| b as f64 - a as f64)
                .sum::<f64>()
                / c.len() as f64
        })
        .collect();
    eprintln!("per-frame signed means: {signed:.3?}");
    let mut worst = 0.0f64;
    for (i, d) in diffs.iter().enumerate() {
        worst = worst.max(*d);
        assert!(
            *d <= 1.0,
            "frame {i}: decoded mean abs diff {d:.4} > TC-103 epsilon 1.0 — \
             yuv matrix/range does not match the historical conversion"
        );
    }
    eprintln!("gpu_yuv_vs_cpu_colorimetry_sr040: worst per-frame mean abs diff = {worst:.4}");
}

// Verifies: SR-040, LLR-076, LLR-077, LLR-078 (TC-117, Release leg) — the
// @pairwise-mandated corner yuv420p x mixed x black-fade: a gpu build of a
// mixed album whose black-fade windows are achromatic (grayscale first clip,
// uniform-gray VIDEO last clip — so the fade seam itself is what's measured),
// decoded as rawvideo yuv420p with NO rgb conversion. Every decoded fade
// frame keeps U and V at 128 +-1 at every fade weight (chroma trending
// toward 0 = green/purple tint = fail) while Y trends toward 16; the videos
// were decoded directly to yuv420p (LLR-078 debug line, --verbose) and
// exactly one transport served the whole run (single backend-in-use line
// naming yuv420p). The device-lost clause extends TC-104 by reference: that
// manual procedure re-run in yuv mode asserts post-degrade CPU frames arrive
// converted (LLR-079).
#[test]
#[ignore = "Release tier (TC-117): needs a GPU adapter"]
fn gpu_yuv_mixed_blackfade_chroma_neutral_sr040() {
    let tmp = common::TempDir::new("rbfade");
    let media = tmp.join("media");
    std::fs::create_dir_all(&media).unwrap();
    write_gray_noise_png(&media.join("01_gray.png"), 400, 300, 33);
    common::write_noise_png(&media.join("02_color.png"), 400, 300, 44);
    assert!(
        common::synth_video(&media.join("03_pattern.mp4"), 1, 320, 240, 24),
        "testsrc video"
    );
    let gray_lavfi = "color=c=gray:duration=1:size=320x240:rate=24";
    let ok = std::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", gray_lavfi])
        .args(["-pix_fmt", "yuv420p"])
        .arg(media.join("04_gray.mp4"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "gray video synth");

    let out = tmp.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let cfg = tmp.join("gpu.toml");
    write_config(&cfg, &media, &out, "gpu");

    // --verbose surfaces the LLR-078 per-video decode line.
    let output = Command::new(common::slideshow_bin())
        .arg("--config")
        .arg(&cfg)
        .arg("--non-interactive")
        .arg("--no-cache")
        .arg("--verbose")
        .arg("build")
        .env_remove("WGPU_BACKEND")
        .output()
        .expect("run build");
    let all = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "gpu build must exit 0:\n{all}");
    let lower = all.to_lowercase();
    assert!(lower.contains("rendering with gpu ("), "{all}");
    assert_eq!(
        backend_lines(&all),
        1,
        "one backend/transport selection for the whole run:\n{all}"
    );
    assert!(
        lower.contains("(yuv420p transport)"),
        "one yuv420p transport for the run:\n{all}"
    );
    // LLR-078: both videos decoded directly to yuv420p — no rgb round-trip.
    assert_eq!(
        lower.matches("as yuv420p rawvideo").count(),
        2,
        "both videos must decode directly to yuv420p:\n{all}"
    );
    assert!(
        !lower.contains("as rgb24 rawvideo"),
        "no video may take the rgb decode path in a yuv run:\n{all}"
    );

    // Decode the finished MP4's fade windows as yuv420p (no rgb conversion).
    let frames = decode_yuv_frames(&out.join("clip.mp4"), 320, 240);
    let fade = 5usize; // fade_frames() = round(0.2 * 24)
    assert!(frames.len() > 2 * fade, "enough frames to hold both fades");
    let y_len = 320 * 240;
    let mean_y = |f: &[u8]| -> f64 {
        f[..y_len].iter().map(|&b| b as u64).sum::<u64>() as f64 / y_len as f64
    };

    let fade_in = &frames[..fade];
    let fade_out = &frames[frames.len() - fade..];
    for (window, name) in [(fade_in, "fade-in"), (fade_out, "fade-out")] {
        for (i, f) in window.iter().enumerate() {
            for (&b, plane) in f[y_len..y_len + y_len / 4]
                .iter()
                .map(|b| (b, "U"))
                .chain(f[y_len + y_len / 4..].iter().map(|b| (b, "V")))
            {
                assert!(
                    (b as i16 - 128).abs() <= 1,
                    "{name} frame {i}: {plane} byte {b} off neutral 128 +-1 (tint!)"
                );
            }
        }
    }
    // Y trends: rising out of black across the fade-in, falling into black
    // across the fade-out, with the darkest frames near limited-range black.
    assert!(
        mean_y(&fade_in[0]) < mean_y(&fade_in[fade - 1]),
        "fade-in Y must rise: {} .. {}",
        mean_y(&fade_in[0]),
        mean_y(&fade_in[fade - 1])
    );
    assert!(
        mean_y(&fade_out[fade - 1]) < mean_y(&fade_out[0]),
        "fade-out Y must fall: {} .. {}",
        mean_y(&fade_out[0]),
        mean_y(&fade_out[fade - 1])
    );
    assert!(
        mean_y(&fade_in[0]) < 60.0 && mean_y(&fade_out[fade - 1]) < 60.0,
        "darkest fade frames must sit near limited-range black (16): {} / {}",
        mean_y(&fade_in[0]),
        mean_y(&fade_out[fade - 1])
    );
    eprintln!(
        "gpu_yuv_mixed_blackfade_chroma_neutral_sr040: fade-in Y {:.1}->{:.1}, fade-out Y {:.1}->{:.1}",
        mean_y(&fade_in[0]),
        mean_y(&fade_in[fade - 1]),
        mean_y(&fade_out[0]),
        mean_y(&fade_out[fade - 1])
    );
}

// Verifies: SR-039, SR-022, LLR-069, LLR-070, LLR-071 (TC-102, Release leg) —
// on wgpu-capable hardware: two render_backend=gpu builds of the same corpus
// and config both report gpu (adapter name) in use, pass SR-005, and are
// byte-identical (within-backend determinism, GPU leg). Identical run-to-run
// bytes also demonstrate the readback ring emits in-order frames with the
// unchanged rgb24 shape under backpressure (LLR-071 fold-in).
#[test]
#[ignore = "Release tier (TC-102): needs a GPU adapter"]
fn gpu_build_reports_adapter_and_is_deterministic_sr039() {
    let tmp = common::TempDir::new("rbdemo");
    let media = tmp.join("media");
    seed_media(&media);

    let mut mp4s = Vec::new();
    for run in 0..2 {
        let out = tmp.join(&format!("out_{run}"));
        std::fs::create_dir_all(&out).unwrap();
        let cfg = tmp.join(&format!("gpu_{run}.toml"));
        write_config(&cfg, &media, &out, "gpu");
        let (ok, all) = run_build(&cfg, None);
        assert!(ok, "gpu build {run} must exit 0; output:\n{all}");
        assert!(
            all.to_lowercase().contains("rendering with gpu ("),
            "gpu build {run} must report the adapter in use:\n{all}"
        );
        let mp4 = out.join("clip.mp4");
        common::assert_sr005_profile(&mp4);
        mp4s.push(std::fs::read(&mp4).expect("read gpu mp4"));
    }
    assert_eq!(
        mp4s[0], mp4s[1],
        "two gpu builds of identical media+config must be byte-identical (SR-022 GPU leg)"
    );
}
