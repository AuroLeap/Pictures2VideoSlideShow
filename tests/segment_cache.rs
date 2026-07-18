//! SR-037 segment-cache scenario matrix (TC-081) and the CLI cache-flag legs
//! of TC-078: warm builds re-encode only misses, evidenced by the
//! reused/re-encoded counts the build surfaces, with every output equivalent
//! to its cold build (frame count, duration, SR-005 profile).
//!
//! TC-081's `scenario x cache-flag` Permutations are `@pairwise`: with the
//! substantive interactions concentrated on `cache-flag=default` (the flags
//! bypass or empty the cache regardless of scenario), the pairing runs every
//! scenario against `default` in `warm_scenarios_reencode_only_misses_sr037`
//! and pairs the `no-cache` / `clear-cache` levels with the cold and
//! warm-unchanged scenarios through the real binary in
//! `no_cache_and_clear_cache_flags_sr037` (which also covers TC-078's flag
//! and store-root legs; the store's round-trip/LRU/corrupt unit legs live in
//! `src/cache/store.rs`).
//!
//! Verifies: SR-037, LLR-053, LLR-054, LLR-055. (TC-081, TC-078.)
//! FFmpeg/ffprobe must be on PATH (CI invariant).

mod common;

use common::{
    assert_sr005_profile, decode_frames, image_media_file, probe_duration, slideshow_bin,
    snapshot_tree, write_config, write_noise_png, write_png, TempDir,
};
use slideshow_core::cache::store::SegmentStore;
use slideshow_core::config::{OutputDef, ProcessingConfig};
use slideshow_core::media::{Album, MediaFile};
use slideshow_core::pipeline::FrameGenerationPipeline;
use std::path::Path;

const SRC_W: u32 = 640;
const SRC_H: u32 = 480;
const OUT_W: u32 = 320;
const OUT_H: u32 = 240;
const FPS: u32 = 24;
/// pic 1.0s @ 24fps -> 24 frames/clip; fade 0.25s -> 6 overlap frames.
const CLIP_FRAMES: u64 = 24;
const FADE_FRAMES: u64 = 6;

fn output_def(crf: u32) -> OutputDef {
    OutputDef {
        name: "cachetest".into(),
        width: OUT_W,
        height: OUT_H,
        fps: FPS,
        pic_display_time_secs: 1.0,
        fade_time_secs: 0.25,
        max_rotation_degrees: 0.0,
        bulk_video_time_min: 20,
        quality_crf: crf,
        enable_audio: false,
        audio_bitrate_kbps: 192,
        audio_sample_rate: 48_000,
        encoder: "software".into(),
        x264_preset: "veryfast".into(),
        zoom_amount: 0.12,
        ken_burns: true,
    }
}

fn processing() -> ProcessingConfig {
    ProcessingConfig {
        temp_dir: None,
        max_workers: None,
        use_parallelism: true,
        dry_run: false,
        verbose: false,
        ffmpeg_timeout_secs: 120,
        ffmpeg_path: None,
        default_focus: None,
        segment_cache_gb: 20.0,
    }
}

/// Expected total output frames for `n` full-length clips: each clip emits
/// `CLIP_FRAMES - FADE_FRAMES` (its tail merges into the next transition) and
/// the final tail fades out (see `cache::planner::clip_layout`).
fn expected_frames(n: u64) -> u64 {
    n * (CLIP_FRAMES - FADE_FRAMES) + FADE_FRAMES
}

/// An album over the given noise-image files (writing any that are missing),
/// in slice order.
fn album_of(media: &Path, names_seeds: &[(&str, u32)]) -> Album {
    std::fs::create_dir_all(media).unwrap();
    let files: Vec<MediaFile> = names_seeds
        .iter()
        .map(|(name, seed)| {
            let p = media.join(name);
            if !p.exists() {
                write_noise_png(&p, SRC_W, SRC_H, *seed);
            }
            image_media_file(&p)
        })
        .collect();
    Album {
        media_files: files,
        total_size: 0,
        created_at: std::time::SystemTime::now(),
        probe_stats: Default::default(),
    }
}

/// Build `album` through the cache-aware path against `store`, asserting the
/// SR-005 profile plus the expected duration on the produced file.
fn cached_build(
    tmp: &TempDir,
    media: &Path,
    album: Album,
    def: &OutputDef,
    store: &mut SegmentStore,
    out_sub: &str,
) -> slideshow_core::pipeline::SegmentedBuild {
    let pipe = FrameGenerationPipeline::new(
        album,
        vec![def.clone()],
        processing(),
        tmp.join(out_sub),
        media.to_path_buf(),
        None,
    );
    let build = pipe
        .execute_segmented(def, store)
        .expect("cached build")
        .expect("output produced");
    assert_sr005_profile(&build.output);
    let dur = probe_duration(&build.output);
    let expected = build.frames as f64 / FPS as f64;
    assert!(
        (dur - expected).abs() < 1.0 / FPS as f64,
        "duration {dur:.4}s vs expected {expected:.4}s"
    );
    build
}

/// Verifies: SR-037, LLR-053, LLR-054, LLR-055 (TC-081, cache-flag=default
/// legs) — cold populates the cache; warm-unchanged re-encodes 0 segments
/// (all reused, output equivalent); added-photos re-encodes only the new clip
/// plus its two transition neighbors; removed-photo only the removed clip's
/// neighbors; changed-settings invalidates every segment; a corrupted cache
/// entry is re-encoded and the build succeeds. Reuse evidence is the
/// reused/re-encoded counts the build surfaces (mirrored into the summary).
#[test]
fn warm_scenarios_reencode_only_misses_sr037() {
    let tmp = TempDir::new("segcache_scen");
    let media = tmp.join("media");
    let mut store = SegmentStore::open(&tmp.join("store")).expect("open store");
    let def = output_def(18);

    let base: Vec<(&str, u32)> = vec![
        ("img_0.png", 0xA11CE),
        ("img_1.png", 0xB0B5_1DE5),
        ("img_2.png", 0xCAFE_F00D),
        ("img_3.png", 0xD00D_5EED),
        ("img_4.png", 0x5EED_BEEF),
    ];

    // --- cold: empty cache, everything encoded, cache populated.
    let album = album_of(&media, &base);
    let cold = cached_build(&tmp, &media, album, &def, &mut store, "out_cold");
    assert_eq!((cold.reused, cold.re_encoded), (0, 5), "cold = all miss");
    assert_eq!(cold.frames, expected_frames(5));
    let cold_frames = decode_frames(&cold.output, OUT_W, OUT_H);
    assert_eq!(cold_frames.len() as u64, cold.frames);

    // --- warm-unchanged: all segments reused, output equivalent to cold.
    let album = album_of(&media, &base);
    let warm = cached_build(&tmp, &media, album, &def, &mut store, "out_warm");
    assert_eq!(
        (warm.reused, warm.re_encoded),
        (5, 0),
        "warm-unchanged must re-encode nothing (SR-037)"
    );
    assert_eq!(warm.frames, expected_frames(5));
    let warm_frames = decode_frames(&warm.output, OUT_W, OUT_H);
    assert_eq!(
        warm_frames, cold_frames,
        "warm-unchanged output is identical (same segments, stream-copy)"
    );

    // --- added-photos: insert a photo between img_1 and img_2 — only the new
    // clip and its two neighbors (whose neighbor identity changed) re-encode.
    let mut added = base.clone();
    added.insert(2, ("img_1x.png", 0xADDED));
    let album = album_of(&media, &added);
    let b = cached_build(&tmp, &media, album, &def, &mut store, "out_added");
    assert_eq!(
        (b.reused, b.re_encoded),
        (3, 3),
        "added photo re-encodes new + 2 neighbors only"
    );
    assert_eq!(b.frames, expected_frames(6));

    // --- removed-photo: remove img_2 — only its former neighbors (img_1x,
    // img_3) re-key/re-encode; the untouched clips are served.
    let mut removed = added.clone();
    removed.remove(3); // img_2
    let album = album_of(&media, &removed);
    let b = cached_build(&tmp, &media, album, &def, &mut store, "out_removed");
    assert_eq!(
        (b.reused, b.re_encoded),
        (3, 2),
        "removed photo re-encodes its two neighbors only"
    );
    assert_eq!(b.frames, expected_frames(5));

    // --- changed-settings: an encode-relevant parameter change (quality)
    // invalidates every segment of the output — full miss, never stale.
    let def_crf = output_def(28);
    let album = album_of(&media, &removed);
    let crf_build = cached_build(&tmp, &media, album, &def_crf, &mut store, "out_crf");
    assert_eq!(
        (crf_build.reused, crf_build.re_encoded),
        (0, 5),
        "changed settings invalidate all segments"
    );

    // --- corrupted-cache-entry: truncate a segment THIS plan serves — it
    // becomes a miss (quiet self-heal), the rest are served, and the build
    // succeeds with an equivalent output.
    std::fs::write(&crf_build.segments[2], b"truncated").unwrap();
    let album = album_of(&media, &removed);
    let b = cached_build(&tmp, &media, album, &def_crf, &mut store, "out_corrupt");
    assert_eq!(
        (b.reused, b.re_encoded),
        (4, 1),
        "corrupt entry is exactly one quiet miss, self-healed"
    );
    assert_eq!(b.frames, expected_frames(5));
    assert!(b.output.exists());
}

/// Count the "segments: X reused, Y re-encoded" evidence lines in build
/// stdout, returning the summed counts (one line per output).
fn parse_cache_summary(stdout: &str) -> Option<(u64, u64)> {
    let mut found = None;
    for line in stdout.lines() {
        if let Some(idx) = line.find("segments: ") {
            let rest = &line[idx + "segments: ".len()..];
            let mut nums = rest
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<u64>().unwrap());
            let (r, e) = (nums.next()?, nums.next()?);
            let (ar, ae) = found.unwrap_or((0, 0));
            found = Some((ar + r, ae + e));
        }
    }
    found
}

/// Verifies: SR-037, LLR-054 (TC-078 flag + root legs; TC-081 no-cache /
/// clear-cache pairings) — through the real binary: the default build uses
/// the cache rooted under the configured `temp_dir` and reports reuse in the
/// summary; `--no-cache` neither reads nor writes the store (bytes on disk
/// untouched, no reuse evidence line); `--clear-cache` empties the store and
/// proceeds (cold re-encode repopulates it).
#[test]
fn no_cache_and_clear_cache_flags_sr037() {
    let tmp = TempDir::new("segcache_flags");
    let media = tmp.join("media");
    std::fs::create_dir_all(&media).unwrap();
    for i in 0..4u32 {
        write_png(
            &media.join(format!("img_{i}.png")),
            96,
            72,
            [40 + i as u8, 90, 130],
        );
    }
    let out_dir = tmp.join("out");
    let config = tmp.join("config.toml");
    // write_config pins temp_dir = <out>/cache, so the store root must appear
    // at <out>/cache/segment-cache (TC-078 root leg).
    write_config(&config, &media, &out_dir, "show", 128, 96);
    let store_dir = out_dir.join("cache").join("segment-cache");

    let run = |extra: &[&str]| {
        let mut cmd = std::process::Command::new(slideshow_bin());
        cmd.arg("--config").arg(&config).arg("--non-interactive");
        cmd.args(extra);
        cmd.arg("build");
        let out = cmd.output().expect("run build");
        assert!(
            out.status.success(),
            "build failed: {}\n{}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // Cold default build: populates the store, reports 4 re-encodes.
    let stdout = run(&[]);
    assert_eq!(
        parse_cache_summary(&stdout),
        Some((0, 4)),
        "cold build summary evidence: {stdout}"
    );
    assert!(store_dir.is_dir(), "store rooted under temp_dir (LLR-054)");
    let ts_count = std::fs::read_dir(&store_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "ts")
        })
        .count();
    assert_eq!(ts_count, 4, "one committed segment per clip");

    // Warm default build: everything reused.
    let stdout = run(&[]);
    assert_eq!(
        parse_cache_summary(&stdout),
        Some((4, 0)),
        "warm build reuses all segments: {stdout}"
    );

    // --no-cache: builds fine, no reuse evidence line, and the store is
    // byte-for-byte untouched (neither read-bumped nor written).
    let before = snapshot_tree(&store_dir);
    let stdout = run(&["--no-cache"]);
    assert_eq!(
        parse_cache_summary(&stdout),
        None,
        "--no-cache must not report cache evidence: {stdout}"
    );
    assert_eq!(
        snapshot_tree(&store_dir),
        before,
        "--no-cache must neither read-bump nor write the store"
    );
    assert!(out_dir.join("show.mp4").exists());

    // --clear-cache: store emptied, build proceeds cold and repopulates.
    let stdout = run(&["--clear-cache"]);
    assert_eq!(
        parse_cache_summary(&stdout),
        Some((0, 4)),
        "--clear-cache forces a cold re-encode: {stdout}"
    );
}
