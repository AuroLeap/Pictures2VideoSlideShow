//! Integration test for the source-media read-only guarantee (SR-010; LLR-014).
//!
//! Creates a temp media dir with a couple of generated PNGs, snapshots a
//! content fingerprint (len + mtime + FNV content hash) of every file, runs a
//! full `build` to a SEPARATE temp output dir, then asserts every input file is
//! byte-for-byte unchanged and that none were added or removed under
//! media_root.
//!
//! Verifies: SR-010, LLR-014 (TC-017, build/complete permutation). FFmpeg must
//! be on PATH (CI invariant).

mod common;

use common::{slideshow_bin, snapshot_tree, write_config, write_png, TempDir};

#[test]
fn build_never_modifies_source_media_sr010() {
    let tmp = TempDir::new("readonly");
    let media = tmp.join("media");
    let outdir = tmp.join("out");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&outdir).unwrap();

    // A small album, including a nested subdir, to exercise the recursive walk.
    write_png(&media.join("one.png"), 96, 72, [220, 30, 30]);
    write_png(&media.join("two.png"), 96, 72, [30, 220, 30]);
    std::fs::create_dir_all(media.join("sub")).unwrap();
    write_png(&media.join("sub").join("three.png"), 80, 64, [30, 30, 220]);

    // Snapshot BEFORE the run.
    let before = snapshot_tree(&media);
    assert_eq!(
        before.len(),
        3,
        "expected three source files before the run"
    );

    let cfg = tmp.join("config.toml");
    write_config(&cfg, &media, &outdir, "show", 64, 48);

    let status = std::process::Command::new(slideshow_bin())
        .arg("--config")
        .arg(&cfg)
        .arg("build")
        .status()
        .expect("run slideshow build");
    assert!(status.success(), "build should succeed over valid media");

    // The output landed in the SEPARATE outdir, not under media_root.
    assert!(outdir.join("show.mp4").exists(), "output mp4 should exist");

    // Snapshot AFTER the run and compare.
    let after = snapshot_tree(&media);

    // No files added or removed under media_root.
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "no source files may be added or removed under media_root"
    );
    // Every file byte-for-byte unchanged (len + mtime + content hash).
    for (rel, fp_before) in &before {
        let fp_after = after.get(rel).expect("file still present");
        assert_eq!(
            fp_before, fp_after,
            "source file {rel:?} must be byte-for-byte unchanged (len/mtime/hash)"
        );
    }
}
