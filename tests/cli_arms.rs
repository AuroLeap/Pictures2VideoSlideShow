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
