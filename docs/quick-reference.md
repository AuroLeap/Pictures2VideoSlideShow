# Quick Reference — Rust Slideshow Engine

The one-page cheat-sheet for the Rust engine (`rust-rewrite` branch): the commands, the recommended frame settings, every config field, and what is **not implemented yet**. This is the **single source of truth** for these facts — other docs link here instead of restating them (per [anti-duplication](process.md#4-traceability--anti-duplication-read-this-carefully)).

Back to the [project README](../README.md) · docs index: [docs/README.md](README.md). Engineering acceptance for these facts: SR-019/SR-020/SR-021 in [system-requirements.csv](requirements/system-requirements.csv).

---

## 1. Setup in three steps (non-programmer path)

No source-code edits required (UN-001).

1. **Install prerequisites** — [Rust](https://rustup.rs/) (gives you `cargo`) and [FFmpeg](https://www.ffmpeg.org/download.html) on your `PATH`. Verify: `cargo --version` and `ffmpeg -version` both print a version.
2. **Edit one config file** — copy `test_config.toml`, point `media_root` at your photos and `base_dir` at where the MP4s should go. Leave everything else at the recommended defaults (§3). Every field is documented in §4.
3. **Run one command** — `cargo run --release -- --config your_config.toml build`.

Always run `validate` before a long `build` (UN-002) — it checks FFmpeg, config, and paths before committing to a multi-hour run.

---

## 2. Commands

Run from the repo root. `--release` is strongly recommended for `build`/`bench` (10-15x faster than debug).

```bash
cargo build --release                                            # compile once

cargo run --release -- --config your_config.toml validate        # check config + media root + prerequisites
cargo run --release -- --config your_config.toml stats           # list media found + output definitions
cargo run --release -- --config your_config.toml bench -i 2000   # frame-generation throughput (i = image count to project)
cargo run --release -- --config your_config.toml build           # produce the slideshow MP4(s)
```

Useful flags: `--input <dir>` and `--output <dir>` override the config paths; `--dry-run` plans without encoding; `-v`/`--verbose` (or `RUST_LOG=debug`) increases logging. `build` accepts `--output <name>` to build only one output definition.

---

## 3. Recommended starting point for a typical frame

> Panel-native resolution (or 1080p), **H.264 / MP4 / yuv420p / +faststart**, **30 fps**, **CRF 28**, even width & height, keep each file **≤ ~20 min and ≤ 3.5 GB**. Format the card as **exFAT** if your frame supports it (no 4 GB limit); otherwise FAT32 and keep files small (the Rust engine does not split yet — see §5).

This is the only copy of the recommended values; the README and constraints sections link here. The reasoning behind each value (the 4 GB FAT32 wall, codec/fps/CRF compatibility) lives in the README's [Output Constraints](../README.md#output-constraints--limitations-digital-picture-frames) — read that to understand *why*; use this block for *what to set*.

---

## 4. Config fields (TOML) — unit + default

Every user-facing field, its unit, and its default. The schema is defined in `src/config/mod.rs`; `test_config.toml` is a ready-to-edit example.

### `[input]`
| Field | Unit / type | Default | Meaning |
|---|---|---|---|
| `media_root` | path | *(required)* | Folder scanned for photos/videos (recursed). |
| `ignore_patterns` | list of strings | `[]` | Case-insensitive filename/path substrings to skip (e.g. `["DNP"]`). |
| `exception_pattern` | string | *(none)* | Optional pattern that re-includes otherwise-ignored items. |
| `exception_threshold` | integer | *(none)* | Optional numeric threshold paired with `exception_pattern`. |

### `[output]`
| Field | Unit / type | Default | Meaning |
|---|---|---|---|
| `base_dir` | path | *(required)* | Folder where one `<name>.mp4` is written per output definition. |

### `[processing]`
| Field | Unit / type | Default | Meaning |
|---|---|---|---|
| `temp_dir` | path | *(none)* | Optional scratch dir (e.g. a RAM disk `R:\`); empty/omitted = system temp. |
| `max_workers` | integer (threads) | *(all cores)* | Optional cap on parallel workers; omit to use every CPU core. |
| `use_parallelism` | bool | `true` | Use the rayon thread pool for frame generation. |
| `dry_run` | bool | `false` | Plan without encoding. |
| `verbose` | bool | `false` | Verbose logging. |

### `[[outputs]]` (one block per frame/target; repeat for multiple frames — UN-007)
| Field | Unit / type | Default | Meaning |
|---|---|---|---|
| `name` | string | *(required)* | Output base name → `<name>.mp4`. |
| `width` | pixels | *(required)* | Output width. **Must be even** (yuv420p H.264). |
| `height` | pixels | *(required)* | Output height. **Must be even**. |
| `fps` | frames/second | *(required)* | Frame rate; recommended **24-30** (see §3). |
| `pic_display_time_secs` | seconds | *(required)* | Seconds each image is shown (includes its share of the dissolve). |
| `fade_time_secs` | seconds | *(required)* | Cross-fade (dissolve) length; also the per-boundary overlap. |
| `max_rotation_degrees` | degrees | *(required)* | Subtle Ken Burns tilt; `0` disables rotation. |
| `bulk_video_time_min` | minutes | *(required)* | Target length per file. **Splitting not implemented in Rust** (see §5) — currently one continuous MP4. |
| `quality_crf` | CRF 0-51 (lower = better/larger) | *(required; recommend 28)* | H.264 quality. Validated to 0-51. |
| `enable_audio` | bool | `false` | **Audio not implemented in Rust** (see §5) — leave `false`. |
| `zoom_amount` | fraction | `0.12` | Ken Burns zoom (0.12 = up to 12% over the clip). Direction randomized per image. |
| `ken_burns` | bool | `true` | `false` = static cover-fit (fade only, no pan/zoom). |

> Note: fields marked *(required)* must be present in the TOML even though the recommended values are given in §3. `zoom_amount`/`ken_burns` have serde defaults and may be omitted.

---

## 5. Rust engine — what is NOT implemented yet (and the workaround)

The Rust engine is fast but feature-incomplete. These gaps are stated **here once** (authoritative per SR-019); other docs link here.

| Gap | Impact | What to do today |
|---|---|---|
| **Audio** | Output is silent (`enable_audio` is accepted but ignored). | Leave `enable_audio = false`. If you need audio, use the **PowerShell pipeline** on `main` (partial audio support). |
| **`bulk_video_time_min` splitting** | Rust writes **one continuous MP4 per output**, no matter how long. A long album can cross the **4 GB FAT32 wall**. | Keep the album small, raise `quality_crf` (smaller file), use an **exFAT/NTFS** card, or use the **PowerShell pipeline** (which splits into ~20-min parts). |
| Shared video-decode across multiple outputs | Each output re-decodes videos (slower with many outputs). | Cosmetic/perf only; no action needed. |
| GPU acceleration | CPU-only encode. | None; CPU path is already 10-15x faster than PowerShell. |

When the Rust engine falls short, the **PowerShell pipeline** (feature-complete reference on `main`) is the documented fallback — see the README [Project Status](../README.md#project-status) for which implementation to choose.
