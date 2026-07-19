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
