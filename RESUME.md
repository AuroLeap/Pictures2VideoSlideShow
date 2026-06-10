# Session Resume / Handoff

_Last updated: 2026-06-09. Single source of truth for live status is
[`docs/status.md`](docs/status.md); this file is the quick "where were we" pointer._

> **Maintenance since acceptance:** smooth sub-pixel Ken Burns + fixed-focus/ROI
> database (SR-031), source-video audio passthrough (SR-032), the deferred doc
> inspections closed (SR-003/019/020/021/025), a root [CLAUDE.md](CLAUDE.md)
> agent guide, and the enriched generated architecture map + Mermaid dependency
> diagram (template-kit sync from `ai-template`). Registries now SR=32 / LLR=42 /
> TC=59, orphans=0. Details in [docs/status.md](docs/status.md) "MAINTENANCE"
> entries; the snapshot below is the original 2026-06-04 handoff.

## TL;DR

The **Rust rewrite** (`rust-rewrite` branch) is **functionally complete and
accepted**. A multi-agent SDLC "grind" drove it through requirements → design →
implementation → end-user acceptance. All gates are signed, including the
**FINAL human acceptance** (the built exe was run end to end: first-run GUI
wizard → config → slideshow produced on real media).

- **Branch:** `rust-rewrite` · **HEAD at handoff:** `5c5f484` ("FINAL acceptance APPROVED — project complete")
- **Binary:** `make_video_slideshow` (renamed from `slideshow`)
- **Tests/quality:** `cargo test` green (lib 34 + bin 34 + integration ~18, one `#[ignore]`d network test); `clippy -D warnings` clean; `fmt` clean; **line coverage ~81%**; traceability **0 orphans** (SR=30, LLR=35, TC=51).

## What works (Rust engine)

Ken Burns pan/zoom + rotation (no black corners), cross-fade (dissolve)
transitions, inline video passthrough, H.264/MP4/yuv420p/+faststart encode.
Robustness: `validate` prereq gating, atomic `.part`→final output, oversize
(3.5 GB FAT32) warnings, disk-full/path-named errors, configurable FFmpeg
inactivity timeout. Distribution/setup: `--config` optional (defaults beside
exe / `%APPDATA%`), first-run **PowerShell/WinForms GUI wizard**, startup
dependency self-check, **integrity-verified FFmpeg auto-fetch** (pinned
GyanD/codexffmpeg **7.1**, SHA-256 `fa7d4d7e…24fc`) with offline / use-existing
fallback, `--non-interactive` never-block guardrail, release CI workflow.

## How to build / run (Windows)

`cargo` is NOT on the session PATH — prefix it: `$env:Path="$env:USERPROFILE\.cargo\bin;$env:Path"`.

```powershell
cargo build --release                                   # -> target/release/make_video_slideshow.exe
.\Scripts\run-tests.ps1                                  # fmt + clippy + tests + coverage + trace
.\Scripts\trace.ps1 -Strict                              # traceability/orphan report + arch map
.\target\release\make_video_slideshow.exe --config test_config.toml validate
.\target\release\make_video_slideshow.exe --config test_config.toml build
cargo test --test ffmpeg_fetch -- --ignored             # demo the live FFmpeg auto-fetch (network, ~90 MB)
```
Test media lives in `TestInput/`; the test config is `test_config.toml` → output `TestOut/RustOut/`.

## The multi-agent "grind" framework (reusable)

- Personas: [`.claude/agents/`](.claude/agents/) (end-user, ux-designer, system-engineer, software-engineer, test-engineer)
- Orchestrator command: [`.claude/commands/grind.md`](.claude/commands/grind.md) — `/grind [1|2|3|final|auto]`
- Process (roles, gates, ID scheme, anti-duplication, verdict protocol): [`docs/process.md`](docs/process.md)
- Live blackboard / gate sign-offs / full audit log: [`docs/status.md`](docs/status.md)
- Registries: [`docs/requirements/`](docs/requirements/) (user-needs.md, system-requirements.csv, low-level-requirements.csv), [`docs/test/test-cases.csv`](docs/test/test-cases.csv)
- One-page cheat sheet: [`docs/quick-reference.md`](docs/quick-reference.md)

## Optional follow-ups (nothing required)

1. **Publish a release** — push a `v*` tag so `.github/workflows/release.yml`
   uploads `make_video_slideshow.exe` to GitHub Releases (SR-023, the last
   Demonstration item). Not yet done.
2. **Bump pinned FFmpeg** when 7.1 ages — two-line change in
   `src/setup/ffmpeg_fetch.rs` (`PINNED_URL` + recomputed `PINNED_SHA256`).
3. **Merge `rust-rewrite`** down to `dev`/`main` if/when ready.
4. Remaining Demonstration/Manual items a human may want to exercise: real
   ENOSPC disk-full (SR-015), end-to-end FFmpeg inactivity timeout (SR-013).

## Environment gotchas (so a new session doesn't relearn them)

- Prefix `cargo` with the `.cargo\bin` PATH (above).
- Don't open the `docs/**.csv` files in **Excel** while editing — it takes an exclusive lock and blocks writes (open in VS Code instead).
- Bash and PowerShell tools share one working directory; a Bash `cd` changes what PowerShell sees next — use absolute paths.
- FFmpeg/ffprobe 7.1 are on PATH; outbound network works here.
