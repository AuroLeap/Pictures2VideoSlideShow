//! Integration tests driving the remaining CLI subcommands (`stats`, `bench`)
//! end-to-end through the built binary, so `main.rs::run_stats`/`run_bench` are
//! exercised. `validate`/`build` are covered by the other integration tests.
//!
//! Verifies: SR-004 reporting surface (stats/bench output). FFmpeg/ffprobe must
//! be on PATH (CI invariant). (TC-051.)

mod common;

use common::{slideshow_bin, write_config, write_png, TempDir};
use std::path::Path;
use std::process::{Command, Output};

fn run(config: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(slideshow_bin());
    cmd.arg("--config").arg(config);
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("run slideshow")
}

fn setup() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new("cli_arms");
    let media = tmp.join("media");
    let outdir = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&outdir).unwrap();
    write_png(&media.join("a.png"), 96, 72, [120, 120, 200]);

    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", 64, 48);
    (tmp, cfg)
}

/// `stats`: scans the album and prints the statistics block. Exits zero.
/// Exercises `main.rs::run_stats`.
#[test]
fn stats_reports_album_statistics() {
    let (_tmp, cfg) = setup();
    let out = run(&cfg, &["stats"]);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "stats must exit zero; stdout=\n{stdout}\nstderr=\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("=== Project Statistics ==="),
        "stats must print the statistics block:\n{stdout}"
    );
    assert!(
        stdout.contains("Media files: 1"),
        "stats must report the scanned media count:\n{stdout}"
    );
    assert!(
        stdout.contains("Output definitions: 1"),
        "stats must report the output-definition count:\n{stdout}"
    );
}

/// Verifies: SR-036, LLR-051, LLR-052 — automated smoke of `bench --full`: a
/// real tiny-corpus build that writes cwd-relative `docs/test/perf-metrics.json`
/// keyed by PB-ID, with PB-004 honestly absent until SR-037 lands. The
/// canonical Release-tier bench with PB-comparable numbers is TC-074
/// (`Scripts/bench.ps1` on the dev box).
#[test]
fn bench_full_writes_pb_metrics_sr036() {
    let (tmp, cfg) = setup();
    // A second image so a clip boundary (PB-002's subject) actually occurs.
    write_png(&tmp.join("media").join("b.png"), 96, 72, [10, 200, 50]);

    let mut cmd = Command::new(slideshow_bin());
    cmd.current_dir(tmp.path()); // metrics land under <cwd>/docs/test
    cmd.arg("--config").arg(&cfg).arg("--non-interactive");
    cmd.args(["bench", "--full", "--corpus"])
        .arg(tmp.join("media"));
    let out = cmd.output().expect("run bench --full");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "bench --full must exit zero; stdout=\n{stdout}\nstderr=\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("PB-001"),
        "bench --full must report the PB numbers:\n{stdout}"
    );

    let metrics = tmp.join("docs").join("test").join("perf-metrics.json");
    let text = std::fs::read_to_string(&metrics).expect("bench must write perf-metrics.json");
    let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let obj = json.as_object().expect("flat {PB-ID: number} object");
    for pb in ["PB-001", "PB-002", "PB-003"] {
        assert!(
            obj.get(pb).is_some_and(|v| v.is_number()),
            "{pb} must be a numeric entry:\n{text}"
        );
    }
    if cfg!(any(windows, target_os = "linux")) {
        assert!(
            obj.get("PB-005").is_some_and(|v| v.is_number()),
            "PB-005 (peak working set) must be measured on this platform:\n{text}"
        );
    }
    assert!(
        obj.get("PB-004").is_none(),
        "PB-004 must be omitted until the segment cache (SR-037) exists:\n{text}"
    );
}

/// `bench --images 1`: renders one image's frames and prints the benchmark
/// block. Exits zero. Exercises `main.rs::run_bench`.
#[test]
fn bench_renders_and_reports_throughput() {
    let (_tmp, cfg) = setup();
    let out = run(&cfg, &["bench", "--images", "1"]);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "bench must exit zero; stdout=\n{stdout}\nstderr=\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("=== Benchmark Results ==="),
        "bench must print the benchmark block:\n{stdout}"
    );
    assert!(
        stdout.contains("frames/image") && stdout.contains("frames/sec"),
        "bench must report frame throughput:\n{stdout}"
    );
    assert!(
        stdout.contains("Projected for 1 images"),
        "bench must report the projection for the requested image count:\n{stdout}"
    );
}
