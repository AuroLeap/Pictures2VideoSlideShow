//! SR-040 VideoFrameReader yuv-decode leg (TC-114): CI-safe and adapter-less
//! — the reader's transport mode is driven directly, no GPU involved. The
//! rgb-mode reader itself is unchanged and stays covered by the TC-050 suite
//! (tests/video_build.rs), referenced not duplicated.

mod common;

use slideshow_core::transport::FrameTransport;
use slideshow_core::video::VideoFrameReader;
use std::path::Path;

/// Synthesize a uniform mid-gray video via lavfi `color` (silent).
fn synth_gray_video(path: &Path, seconds: u32, w: u32, h: u32, rate: u32) -> bool {
    let lavfi = format!("color=c=gray:duration={seconds}:size={w}x{h}:rate={rate}");
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg(&lavfi)
        .args(["-pix_fmt", "yuv420p"])
        .arg(path)
        .status();
    matches!(status, Ok(s) if s.success()) && path.exists()
}

/// Drain a reader, asserting every frame is exactly `expect_bytes` long.
fn read_all(mut rd: VideoFrameReader, expect_bytes: usize) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    while let Some(f) = rd.read_frame().expect("read frame") {
        assert_eq!(f.len(), expect_bytes, "frame {} size", frames.len());
        frames.push(f);
    }
    frames
}

// Verifies: SR-040, LLR-078 (TC-114) — yuv decode mode over synthesized
// sources: every frame is exactly w*h*3/2 bytes (the LLR-075 home), the frame
// count equals the rgb-mode read of the same source (the cover-fit/fps filter
// chain and EOF semantics are transport-independent), the testsrc Y plane is
// non-degenerate, and a uniform-gray source's U/V quarter planes sit at the
// neutral 128 +-2 — no rgb round-trip anywhere in the yuv path.
#[test]
fn yuv_decode_mode_frame_size_and_planes_sr040() {
    let tmp = common::TempDir::new("vyuv");
    let (w, h, fps) = (320u32, 240u32, 24u32);
    let y_len = (w * h) as usize;
    let q = y_len / 4;
    let yuv_bytes = FrameTransport::Yuv420p.frame_bytes(w, h);
    let rgb_bytes = FrameTransport::Rgb24.frame_bytes(w, h);

    // testsrc pattern: structure in the Y plane.
    let pattern = tmp.join("pattern.mp4");
    assert!(
        common::synth_video(&pattern, 2, 320, 240, 24),
        "ffmpeg must synthesize the testsrc video"
    );
    let yuv_frames = read_all(
        VideoFrameReader::open(&pattern, w, h, fps, FrameTransport::Yuv420p).expect("open yuv"),
        yuv_bytes,
    );
    let rgb_frames = read_all(
        VideoFrameReader::open(&pattern, w, h, fps, FrameTransport::Rgb24).expect("open rgb"),
        rgb_bytes,
    );
    assert!(!yuv_frames.is_empty(), "testsrc must decode frames");
    assert_eq!(
        yuv_frames.len(),
        rgb_frames.len(),
        "frame count must match across decode modes"
    );
    // Y-plane stats non-degenerate: not all-black, not all-white.
    let y = &yuv_frames[yuv_frames.len() / 2][..y_len];
    let (min, max) = y
        .iter()
        .fold((255u8, 0u8), |(lo, hi), &b| (lo.min(b), hi.max(b)));
    assert!(
        min < 64 && max > 192,
        "testsrc Y plane must span dark..bright, got {min}..{max}"
    );

    // Uniform gray: chroma planes at the neutral value.
    let gray = tmp.join("gray.mp4");
    assert!(
        synth_gray_video(&gray, 1, 320, 240, 24),
        "ffmpeg must synthesize the gray video"
    );
    let gray_frames = read_all(
        VideoFrameReader::open(&gray, w, h, fps, FrameTransport::Yuv420p).expect("open gray"),
        yuv_bytes,
    );
    assert!(!gray_frames.is_empty());
    for f in &gray_frames {
        for (&b, name) in f[y_len..y_len + q]
            .iter()
            .map(|b| (b, "U"))
            .chain(f[y_len + q..].iter().map(|b| (b, "V")))
        {
            assert!(
                (b as i16 - 128).abs() <= 2,
                "{name} plane of a gray source must be 128 +-2, got {b}"
            );
        }
    }
}
