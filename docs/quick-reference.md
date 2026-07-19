# Quick Reference — Rust Slideshow Engine

The one-page cheat-sheet for the slideshow engine: the end-user setup, the commands, the recommended frame settings, every config field, and what is **not implemented yet**. This is the **single source of truth** for these facts — other docs link here instead of restating them (per [anti-duplication](process.md#3-traceability--anti-duplication)).

The binary is **`make_video_slideshow.exe`**. The setup path is split by audience (UN-021 / SR-025): **§1 End user** (download and run the prebuilt exe — no Rust, no manual config editing) and **§1a Developer** (build from source — Rust). **§1b** documents the non-interactive/automation path. Rust appears only in §1a.

Back to the [project README](../README.md) · docs index: [docs/README.md](README.md). Engineering acceptance for these facts: SR-001/SR-002, SR-019/SR-020/SR-021, SR-023/SR-024/SR-025/SR-026/SR-027/SR-028/SR-029/SR-030 in [system-requirements.csv](requirements/system-requirements.csv).

---

## 1. End-user setup — download, run, done (no Rust, no manual config editing)

You do **not** install Rust, compile anything, or hand-edit a config file for a basic run. The product is a single prebuilt `make_video_slideshow.exe`; its only runtime dependency is FFmpeg, which the exe fetches for you on first run (UN-001 / SR-001; audience split UN-021 / SR-025).

1. **Download `make_video_slideshow.exe`** from the project's [GitHub Releases](../README.md#rust-engine-high-performance-rewrite) (UN-019 / SR-023). It is one self-contained file — no installer, no Rust, no source checkout. FFmpeg is **not** bundled inside it (see step 3).
2. **Run it.** On first run with **no config**, the exe opens a **GUI wizard** (PowerShell/WinForms) that asks you, in plain terms, for: your **source/media path** (photos & videos), an **intermediary/temp path** (scratch space), **frame defaults** (resolution / fps / quality), and whether you want **one or multiple frames**. When you finish, the wizard writes a valid config for you (UN-022 / SR-026) and the run can proceed — no manual TOML editing. The config is saved **beside the exe** if that folder is writable, otherwise in **`%APPDATA%`**; the chosen location is reported and re-used on later runs (UN-025 / SR-030).
3. **It gets FFmpeg and builds.** On every startup the exe runs a **dependency self-check** (UN-020 / SR-024). If FFmpeg is missing it **auto-fetches** it to a per-user folder (no administrator/elevation needed) and **integrity-checks** the download against a pinned checksum before ever running it (UN-026 / SR-029). You can also **work offline** or **point it at an FFmpeg you already have** instead of downloading (UN-023 / SR-027). It then produces your slideshow MP4(s).

Subsequent runs reuse the config the wizard wrote, so you can re-run with a single command: `make_video_slideshow.exe --config your_config.toml build` (the config path is the one reported in step 2). Advanced users can hand-edit that config — every field is documented in §4.

Always run `validate` before a long `build` (UN-002 / SR-002): `make_video_slideshow.exe --config your_config.toml validate`. It checks the **runtime** prerequisites only — FFmpeg locatable (on PATH, configured, or auto-fetched), config valid, media folder readable, output folder writable — before committing to a multi-hour run. It never checks for Rust; Rust is not a runtime prerequisite.

> ⚠️ **Availability today.** The first-run GUI wizard (SR-026), the startup dependency self-check, and the integrity-verified FFmpeg **auto-fetch are implemented** (SR-024/SR-027/SR-029): the auto-fetch pulls a pinned, checksummed FFmpeg 7.1 build into a per-user folder and has been demonstrated end-to-end. The one piece still pending is a **published** prebuilt `make_video_slideshow.exe` on GitHub Releases (SR-023): the release workflow exists but nothing is published until a maintainer pushes a `v*` tag. **Until that first release is tagged**, obtain the binary by following the **Developer (build from source)** path in §1a once (`cargo build --release`) — the resulting exe behaves exactly as described above (wizard, self-check, auto-fetch all work). The feature-complete **PowerShell pipeline** on `main` remains an alternative (see the README [Project Status](../README.md#project-status)).

---

## 1a. Developer setup — build from source (Rust)

This section is **for developers only** — the audience that builds the binary (UN-021 / SR-025). End users do **not** need any of this. Rust appears **only** here.

Prerequisites: [Rust](https://rustup.rs/) (gives you `cargo`) and [FFmpeg](https://www.ffmpeg.org/download.html) on your `PATH`. Verify: `cargo --version` and `ffmpeg -version` both print a version.

```bash
cargo build --release        # produces target/release/make_video_slideshow.exe
```

The resulting `target/release/make_video_slideshow.exe` is the same binary an end user would otherwise download from GitHub Releases (§1) — copy it somewhere on your `PATH` and the §2 commands work as written. Until the prebuilt release ships, this is also the interim way to obtain the binary.

---

## 1b. Automation / non-interactive use (never blocks)

The GUI wizard and the dependency-fetch prompts appear **only in interactive use** (UN-024 / SR-028). When you run the exe non-interactively — passing `--config` / `--non-interactive`, from a scheduled task, script, or CI — it **never shows a GUI and never blocks on input**: it either runs to completion or exits with a clear non-zero status. Supply a ready config (one the wizard wrote earlier, or a hand-edited copy of `test_config.toml` — §4) and a locatable FFmpeg (on PATH, configured in the config, or already auto-fetched), and the run is fully unattended.

---

## 1c. Try the bundled demo (no media of your own needed)

Want to see it work before pointing it at your own photos? The release `.zip` ships a
**`demo.bat`** (next to `make_video_slideshow.exe`) that downloads a little free-use
sample media and builds an example slideshow (UN-029 / SR-033).

1. **Extract the whole release `.zip`** so `demo.bat` sits beside
   `make_video_slideshow.exe` (it does in the zip; if you built from source, copy
   `make_video_slideshow.exe` next to `demo.bat`).
2. **Double-click `demo.bat`.** It downloads ~12 MB of public-domain / CC0 photos and
   short video clips (from Wikimedia Commons) into a `demo-media\` folder — verifying each
   against a pinned SHA-256 — then runs `validate` and `build` using the bundled
   `demo_config.toml` + `focus.json`.

The example slideshow lands in **`demo-output\demo-1280x800.mp4`**. The bundled
`demo_config.toml` is a complete, valid config you can copy for your own media, and
`focus.json` is a worked **Ken Burns focus database** (§6). Re-running is instant
(already-downloaded media is reused). Details + the media license manifest:
[demo/README.md](../demo/README.md).

---

## 2. Commands

These work for both audiences once you have `make_video_slideshow.exe` (end user: downloaded from GitHub Releases per §1; developer: from `cargo build --release` per §1a).

```bash
make_video_slideshow.exe --config your_config.toml validate        # check config + media root + runtime prerequisites
make_video_slideshow.exe --config your_config.toml stats           # list media found + output definitions
make_video_slideshow.exe --config your_config.toml bench -i 2000   # frame-generation throughput (i = image count to project)
make_video_slideshow.exe --config your_config.toml build           # produce the slideshow MP4(s)
```

Useful flags: `--input <dir>` and `--output <dir>` override the config paths; `--dry-run` plans without encoding; `--non-interactive` forces the never-block automation path (§1b); `-v`/`--verbose` (or `RUST_LOG=debug`) increases logging. `build` accepts `--output <name>` to build only one output definition.

Rebuilds are **incremental by default** (SR-037): each clip's encoded segment is cached (in a per-user folder, or under `temp_dir` — see §4), so re-building after adding photos re-encodes only the new clips and their transition neighbors — the build summary reports `segments: X reused, Y re-encoded`. `--no-cache` bypasses the cache entirely (neither reads nor writes it); `--clear-cache` empties it before building.

> **Developers running from a source checkout** can use `cargo run --release -- --config your_config.toml <command>` instead of invoking the built `make_video_slideshow.exe` directly — it is the same engine. `--release` is strongly recommended for `build`/`bench` (10-15x faster than debug).

---

## 3. Recommended starting point for a typical frame

> Panel-native resolution (or 1080p), **H.264 / MP4 / yuv420p / +faststart**, **30 fps**, **CRF 28**, even width & height, keep each file **≤ ~20 min and ≤ 3.5 GB**. Format the card as **exFAT** if your frame supports it (no 4 GB limit); otherwise FAT32 and keep files small (the Rust engine does not split yet — see §5).

This is the only copy of the recommended values; the README and constraints sections link here. The reasoning behind each value (the 4 GB FAT32 wall, codec/fps/CRF compatibility) lives in the README's [Output Constraints](../README.md#output-constraints--limitations-digital-picture-frames) — read that to understand *why*; use this block for *what to set*.

---

## 4. Config fields (TOML) — unit + default

For a basic end-user run you do **not** edit this file by hand — the first-run GUI wizard (§1) collects the basics and writes a valid config for you (UN-022 / SR-026). This table is for **advanced users and automation** who want to locate, inspect, back up, or hand-edit that config (it is written beside the exe or in `%APPDATA%` per §1 / SR-030), and for the non-interactive path (§1b). Every user-facing field, its unit, and its default is below. The schema is defined in `src/config/mod.rs`; `test_config.toml` is a ready-to-edit example.

### `[input]`
| Field | Unit / type | Default | Meaning |
|---|---|---|---|
| `media_root` | path | *(required)* | Folder scanned for photos/videos (recursed). |
| `ignore_patterns` | list of strings | `[]` | Case-insensitive substrings matched against each file's path **relative to `media_root`** — a pattern can name a file or a whole folder (e.g. `["DNP"]`). |
| `exception_pattern` | string | *(none)* | **Not implemented in the Rust engine** (accepted for config compatibility, no effect — see §5). |
| `exception_threshold` | integer | *(none)* | **Not implemented in the Rust engine** (accepted for config compatibility, no effect — see §5). |
| `roi_db` | path | *(none)* | Optional JSON region-of-interest database that pins the Ken Burns zoom per image (e.g. a face). Keyed by each image's path **relative to `media_root`**; value `{"x":..,"y":..}` or `{"bbox":[x,y,w,h]}` (normalized 0-1; bbox center used). A missing/invalid file fails the run. See §6. |

### `[output]`
| Field | Unit / type | Default | Meaning |
|---|---|---|---|
| `base_dir` | path | *(required)* | Folder where one `<name>.mp4` is written per output definition. |

### `[processing]`
| Field | Unit / type | Default | Meaning |
|---|---|---|---|
| `temp_dir` | path | *(none)* | Optional scratch dir (e.g. a RAM disk `R:\`). When set, the segment cache (SR-037) roots there (in a `segment-cache` subfolder); omitted = the per-user cache folder. |
| `segment_cache_gb` | GB | `20` | Size cap for the incremental segment cache (SR-037); least-recently-used segments are pruned past it after each build. Must be positive. |
| `max_workers` | integer (threads) | *(all cores)* | Optional cap on parallel workers; omit to use every CPU core. |
| `use_parallelism` | bool | `true` | Use the rayon thread pool for frame generation. |
| `dry_run` | bool | `false` | Plan without encoding. |
| `verbose` | bool | `false` | Verbose logging. |
| `ffmpeg_timeout_secs` | seconds | `120` | Abort a stalled FFmpeg after this many seconds with no encoder progress; `0` disables. |
| `ffmpeg_path` | path | *(none)* | Explicit FFmpeg executable; takes priority over PATH / the per-user cache (offline / use-existing fallback). |
| `default_focus` | `[x, y]` (normalized 0-1) | *(none)* | Default Ken Burns focus for images with **no** `roi_db` entry. Omit to keep the default two-point pan; ROI entries override it. See §6. |
| `render_backend` | `auto` \| `cpu` \| `gpu` | `auto` | Frame-**render** backend (SR-039; distinct from the per-output `encoder` field). `auto` uses the GPU (wgpu, DX12/Vulkan) when a usable adapter exists, else the CPU renderer with a logged reason. `cpu` is the long-verified reference renderer. An explicit `gpu` whose adapter probe fails **falls back to cpu with a logged warning — a missing GPU never fails a build**. Output is equivalent, not bit-identical, across backends (per-backend determinism, SR-022). |

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
| `enable_audio` | bool | `false` | Preserve **source-video** audio in this output (images are silent). `true` carries each video clip's audio into the slideshow, in sync (see §7). |
| `audio_bitrate_kbps` | kbps | `192` | AAC bitrate for the muxed audio track (used when `enable_audio = true`). |
| `audio_sample_rate` | Hz | `48000` | Audio sample rate for the muxed track (used when `enable_audio = true`). |
| `encoder` | `software` \| `auto` \| `h264_nvenc` \| `h264_qsv` \| `h264_amf` | `software` | H.264 video encoder (SR-034). `software` = libx264 (the verified default). `auto` probes for a working hardware (GPU) encoder at start and uses the first that passes, else software. An explicit hardware value is probe-checked too; if it can't work, the build **falls back to software with a logged warning — a missing GPU never fails a build**. Hardware encode is typically several times faster at the same visual quality class. |
| `x264_preset` | libx264 preset (`ultrafast`…`veryslow`) | `medium` | Software-encoder speed/quality trade (SR-035; ignored for hardware encoders). Faster presets (`veryfast`, `faster`) encode up to ~1.5-2x quicker at slightly larger file size / marginally lower quality per bit; slower presets do the reverse. Default `medium` keeps the long-verified behavior. |
| `zoom_amount` | fraction | `0.12` | Ken Burns zoom (0.12 = up to 12% over the clip). Direction randomized per image. |
| `ken_burns` | bool | `true` | `false` = static cover-fit (fade only, no pan/zoom). |

> Note: fields marked *(required)* must be present in the TOML even though the recommended values are given in §3. `zoom_amount`/`ken_burns`/`audio_bitrate_kbps`/`audio_sample_rate`/`encoder`/`x264_preset` have serde defaults and may be omitted.

---

## 5. Rust engine — what is NOT implemented yet (and the workaround)

The Rust engine is fast but feature-incomplete. These gaps are stated **here once** (authoritative per SR-019); other docs link here.

| Gap | Impact | What to do today |
|---|---|---|
| **`bulk_video_time_min` splitting** | Rust writes **one continuous MP4 per output**, no matter how long. A long album can cross the **4 GB FAT32 wall**. | Keep the album small, raise `quality_crf` (smaller file), use an **exFAT/NTFS** card, or use the **PowerShell pipeline** (which splits into ~20-min parts). |
| Shared video-decode across multiple outputs | Each output re-decodes videos (slower with many outputs). | Cosmetic/perf only; no action needed. |
| **`exception_pattern` / `exception_threshold`** | Parsed but **inert** in the Rust engine — setting them re-includes nothing. | Curate with `ignore_patterns` only, or use the PowerShell pipeline if you rely on the exception mechanism. |
| GPU **blending/piping** | The Ken Burns **render** runs on the GPU when available (`render_backend`, §4 — SR-039); cross-fade blending and the encoder pipe remain CPU-side. | Nothing needed — `render_backend = "auto"` (the default) uses the GPU when present; hardware **encoding** is separate via the `encoder` field (§4). |

When the Rust engine falls short, the **PowerShell pipeline** (feature-complete reference on `main`) is the documented fallback — see the README [Project Status](../README.md#project-status) for which implementation to choose.

## 6. Ken Burns focus — smooth motion + pinning the zoom (SR-031)

The pan/zoom is rendered with a single sub-pixel warp, so motion glides instead of
stepping. By default each image gets a varied (but smooth) pan. To **pin the zoom**
on a chosen point (so it stays on a subject instead of drifting):

- **Whole run / per output:** set `default_focus = [x, y]` under `[processing]`
  (normalized `0-1`, e.g. `[0.5, 0.4]` slightly above center).
- **Per image:** point `input.roi_db` at a JSON file mapping each image's path
  **relative to `media_root`** to a focus. A per-image entry overrides
  `default_focus`; images with neither keep the default pan.

```json
{
  "2015/Trip/beach.jpg": { "x": 0.42, "y": 0.55 },
  "2016/Party/cake.jpg": { "bbox": [0.30, 0.20, 0.25, 0.25] }
}
```

Use `{"x":..,"y":..}` for a point or `{"bbox":[x,y,w,h]}` for a box (its center is
used) — all values are fractions of image width/height, clamped to `0-1`. This is
the hook for feeding **face/feature-recognition** output: emit one entry per image
and the zoom holds that region. Paths match case-insensitively and accept either
`/` or `\` separators.

## 7. Audio — source-video passthrough (SR-032)

Photos are silent, but the audio of your **source video clips** can be carried into
the slideshow. Set `enable_audio = true` on an output:

```toml
[[outputs]]
# ...
enable_audio = true
audio_bitrate_kbps = 192   # AAC bitrate (default 192)
audio_sample_rate  = 48000 # Hz (default 48000)
```

Each video clip's audio is delayed to **its own position on the final
(cross-dissolved) timeline** — the engine uses the clip's output start frame, so the
sound stays in sync even though dissolves overlap clips (this is what the original
PowerShell tool got wrong). Image segments are silent; an album with no audio-bearing
videos still produces a valid silent output. The track is AAC in the MP4.

> Limitation (v1): during the brief (≤ `fade_time_secs`) dissolve where two video
> clips overlap, their audio currently overlaps rather than crossfading. A per-clip
> audio fade is a planned refinement.

## 8. Multiple frames / sizes

The config already supports **multiple `[[outputs]]`** (one MP4 per block, each with
its own resolution/fps/quality) — see [SR-017]. The first-run GUI wizard only
scaffolds identical frames, so for **different** sizes either edit the TOML directly
(add `[[outputs]]` blocks) or see the options write-up in
[multi-frame-gui-options.md](design/multi-frame-gui-options.md).
