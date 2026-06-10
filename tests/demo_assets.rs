//! SR-033 / LLR-044 (with SR-003, SR-031) — guard that the shipped demo kit is a
//! *valid configuration*: the bundled `demo/demo_config.toml` parses and passes
//! semantic validation, and the bundled `demo/focus.json` loads as a region-of-
//! interest database with the worked point + bbox entries. If anyone edits the
//! demo into an invalid state, CI fails here instead of an end user hitting it.
//! (TC-061.)

use slideshow_core::config::Config;
use slideshow_core::roi::RoiDb;
use std::path::{Path, PathBuf};

fn demo_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("demo")
}

// Verifies: SR-033, LLR-044, SR-003 — the demo config loads and validates, and
// carries the frame-friendly demo output definition (even dims, CRF 28, audio on).
#[test]
fn demo_config_loads_and_validates_sr033() {
    let cfg_path = demo_dir().join("demo_config.toml");
    let config = Config::from_file(&cfg_path)
        .unwrap_or_else(|e| panic!("demo_config.toml should parse: {e}"));
    config
        .validate()
        .unwrap_or_else(|e| panic!("demo_config.toml should pass validation: {e}"));

    assert_eq!(config.outputs.len(), 1, "demo ships exactly one frame output");
    let out = &config.outputs[0];
    assert_eq!(out.name, "demo-1280x800");
    assert_eq!((out.width, out.height), (1280, 800));
    assert!(
        out.width % 2 == 0 && out.height % 2 == 0,
        "yuv420p H.264 needs even dimensions"
    );
    assert_eq!(out.quality_crf, 28, "documented frame-friendly default");
    assert!(out.enable_audio, "demo enables source-video audio passthrough");

    // The config points at the bundled focus database next to it.
    assert_eq!(
        config.input.roi_db.as_deref(),
        Some(Path::new("focus.json")),
        "demo config references the bundled focus.json"
    );
}

// Verifies: SR-033, LLR-044, SR-031 — the demo focus database loads with the
// worked point + bbox entries keyed by the downloaded image filenames.
#[test]
fn demo_focus_db_loads_point_and_bbox_sr033() {
    let db = RoiDb::load(&demo_dir().join("focus.json"))
        .unwrap_or_else(|e| panic!("demo focus.json should load: {e}"));
    assert_eq!(db.len(), 2, "demo focus.json has a point + a bbox entry");

    // The point entry resolves to its coordinates.
    let cat = db
        .focus_for(Path::new("cat.jpg"))
        .expect("cat.jpg has a focus entry");
    assert!((cat.0 - 0.5).abs() < 1e-6 && (cat.1 - 0.3).abs() < 1e-6);

    // The bbox entry resolves to the box centre (0.30+0.40/2, 0.28+0.40/2).
    let flower = db
        .focus_for(Path::new("flower.jpg"))
        .expect("flower.jpg has a focus entry");
    assert!((flower.0 - 0.5).abs() < 1e-6 && (flower.1 - 0.48).abs() < 1e-6);
}
