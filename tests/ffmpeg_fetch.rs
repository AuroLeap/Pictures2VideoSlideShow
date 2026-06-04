//! SR-027/SR-029 — auto-fetch the pinned FFmpeg, integrity-verify it, extract a
//! runnable binary. Network + ~90 MB download, so it is #[ignore]d by default;
//! run with `cargo test --test ffmpeg_fetch -- --ignored`.

mod common;

use std::process::Command;

// Verifies: SR-027/SR-029 — pinned download is checksum-verified and yields a
// working ffmpeg.exe (and ffprobe.exe) in the cache dir.
#[test]
#[ignore = "network: downloads the pinned FFmpeg release (~90 MB)"]
fn fetch_downloads_verifies_and_extracts_ffmpeg_sr027() {
    let tmp = common::TempDir::new("ffmpeg_fetch");
    let cache = tmp.join("cache");

    let ffmpeg = slideshow_core::setup::ffmpeg_fetch::fetch_ffmpeg(&cache)
        .expect("fetch should download + verify + extract");
    assert!(ffmpeg.exists(), "ffmpeg.exe should exist in the cache");

    // The integrity-gated, extracted binary must actually run.
    let status = Command::new(&ffmpeg)
        .arg("-version")
        .status()
        .expect("run fetched ffmpeg");
    assert!(status.success(), "fetched ffmpeg -version should succeed");

    // ffprobe travels with it.
    assert!(
        cache.join("ffprobe.exe").exists(),
        "ffprobe.exe should be placed beside ffmpeg.exe"
    );
}
