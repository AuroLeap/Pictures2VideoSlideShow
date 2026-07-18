//! Integration tests for the SR-038 media-scan probe cache: persistence
//! (missing/corrupt cache file starts cold, atomic tmp+rename save, per-user
//! location keyed by media_root — TC-084) and scan integration (a warm scan
//! of an unchanged library probes nothing and equals the cold scan; new,
//! changed, and corrupt entries are re-probed — TC-085). Skip/ignore
//! semantics stay guarded by the existing TC-022/TC-023/TC-028 suites,
//! referenced not duplicated.
//!
//! Verifies: SR-038, LLR-058, LLR-059. (TC-084, TC-085.) FFmpeg/ffprobe must
//! be on PATH (CI invariant).

mod common;

use common::{synth_video, write_png, TempDir};
use slideshow_core::config::InputConfig;
use slideshow_core::media::{Album, MediaLoader, MediaType};
use std::path::Path;

/// Scan `media_root` with the probe cache pinned to `cache` (isolated from
/// the developer's real per-user cache dir).
fn scan(media_root: &Path, cache: &Path) -> Album {
    let loader = MediaLoader::new(InputConfig {
        media_root: media_root.to_path_buf(),
        ignore_patterns: vec![],
        exception_pattern: None,
        exception_threshold: None,
        roi_db: None,
    })
    .with_probe_cache_path(cache.to_path_buf());
    futures::executor::block_on(loader.scan_and_index()).expect("scan")
}

/// A three-file library: two images and one silent synthesized video.
fn write_library(media: &Path) {
    std::fs::create_dir_all(media).unwrap();
    write_png(&media.join("a.png"), 96, 72, [200, 40, 40]);
    write_png(&media.join("b.png"), 120, 80, [40, 200, 40]);
    assert!(
        synth_video(&media.join("clip.mp4"), 1, 320, 240, 10),
        "ffmpeg could not synthesize the test video (required on PATH)"
    );
}

/// Verifies: SR-038, LLR-059, LLR-058 (TC-085) — a warm scan of an unchanged
/// image+video library serves every entry from the cache (zero probes, all
/// hits) and yields exactly the cold scan's result.
#[test]
fn warm_scan_reprobes_zero_unchanged_sr038() {
    let tmp = TempDir::new("probe_warm");
    let media = tmp.join("media");
    let cache = tmp.join("cache/probe.json");
    write_library(&media);

    let cold = scan(&media, &cache);
    assert_eq!(cold.media_files.len(), 3);
    assert_eq!(cold.probe_stats.probed, 3, "cold scan probes everything");
    assert_eq!(cold.probe_stats.cache_hits, 0);

    let warm = scan(&media, &cache);
    assert_eq!(
        warm.probe_stats.probed, 0,
        "unchanged warm scan must run zero probe subprocesses/header decodes"
    );
    assert_eq!(warm.probe_stats.cache_hits, 3, "every entry served cached");
    // Same result as cold: items, kinds, dims, durations, audio.
    assert_eq!(warm.media_files, cold.media_files);
    let vid = warm
        .media_files
        .iter()
        .find(|m| m.file_type == MediaType::Video)
        .expect("video present");
    assert_eq!(vid.dimensions, (320, 240));
    assert!(vid.duration_secs.is_some(), "cached duration preserved");
}

/// Verifies: SR-038, LLR-059 (TC-085) — a new file is probed and cached; a
/// changed file (size+mtime) is re-probed and its entry replaced (fresh dims
/// visible); unchanged files stay cache-served.
#[test]
fn changed_and_new_files_reprobed_sr038() {
    let tmp = TempDir::new("probe_changed");
    let media = tmp.join("media");
    let cache = tmp.join("cache/probe.json");
    write_library(&media);
    let cold = scan(&media, &cache);
    assert_eq!(cold.probe_stats.probed, 3);

    // Change one image's content (different dims => different size) and add a
    // brand-new image.
    write_png(&media.join("a.png"), 200, 150, [10, 10, 10]);
    write_png(&media.join("new.png"), 64, 48, [1, 2, 3]);

    let warm = scan(&media, &cache);
    assert_eq!(warm.media_files.len(), 4);
    assert_eq!(
        warm.probe_stats.probed, 2,
        "exactly the changed and the new file are re-probed"
    );
    assert_eq!(warm.probe_stats.cache_hits, 2, "the rest stay cached");
    let a = warm
        .media_files
        .iter()
        .find(|m| m.path.ends_with("a.png"))
        .expect("a.png present");
    assert_eq!(
        a.dimensions,
        (200, 150),
        "replaced entry carries fresh dims"
    );
}

/// Verifies: SR-038, LLR-058 (TC-084) — a missing cache file starts cold and
/// a corrupt cache file is discarded (cold scan, no error); after either, the
/// rewritten cache serves the next scan warm.
#[test]
fn missing_or_corrupt_cache_starts_cold_sr038() {
    let tmp = TempDir::new("probe_corrupt");
    let media = tmp.join("media");
    let cache = tmp.join("cache/probe.json");
    write_library(&media);

    // Missing cache file: cold behavior, never an error.
    let first = scan(&media, &cache);
    assert_eq!(first.probe_stats.probed, 3);

    // Corrupt cache file: discarded, scan proceeds cold and repairs the cache.
    std::fs::write(&cache, b"{ not valid json !!!").unwrap();
    let after_corrupt = scan(&media, &cache);
    assert_eq!(
        after_corrupt.probe_stats.probed, 3,
        "corrupt cache is discarded and everything re-probed, without failing"
    );
    let repaired = scan(&media, &cache);
    assert_eq!(repaired.probe_stats.cache_hits, 3, "cache was rewritten");
}

/// Verifies: SR-038, LLR-058 (TC-084) — save is atomic (tmp + rename): after
/// a scan the cache file parses as JSON and no `.tmp` sibling survives; a
/// second scan overwrites it in place.
#[test]
fn save_is_atomic_tmp_rename_sr038() {
    let tmp = TempDir::new("probe_atomic");
    let media = tmp.join("media");
    let cache_dir = tmp.join("cache");
    let cache = cache_dir.join("probe.json");
    write_library(&media);

    scan(&media, &cache);
    assert!(cache.exists(), "cache written after scan");
    let text = std::fs::read_to_string(&cache).unwrap();
    serde_json::from_str::<serde_json::Value>(&text).expect("cache is valid JSON");
    let tmp_leftovers: Vec<_> = std::fs::read_dir(&cache_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().map(|e| e == "tmp").unwrap_or(false))
        .collect();
    assert!(
        tmp_leftovers.is_empty(),
        "no .tmp survives the rename: {tmp_leftovers:?}"
    );

    // Overwrite-in-place on the next scan (rename over existing works).
    scan(&media, &cache);
    assert!(cache.exists());
}
