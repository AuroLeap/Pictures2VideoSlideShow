# CLAUDE.md — Agent & Contributor Guide

**What this file does:** the standing brief for any agent or human working in
this repo. It encodes *how we build here* so quality doesn't depend on who (or
which model) shows up. Project facts live in `docs/` and the README; this file
points at them rather than restating them.

---

## Project

- **What this is:** converts a photo/video collection into slideshow MP4s for
  digital picture frames (Ken Burns motion, cross-dissolves, audio passthrough).
- **Primary user:** a non-developer end user running one binary on Windows.
- **Stack & layout:** two implementations — the **Rust engine**
  (`src/`, binary `make_video_slideshow`, branch `rust-rewrite`; tests in
  `src/**/tests` + `tests/`) and the **original PowerShell pipeline**
  (`*.psm1` + `BuildAlbum.ps1`, branches `main`/`dev`, kept as the reference for
  multi-part splitting).
- **How to run:** see [docs/quick-reference.md](docs/quick-reference.md) — the
  single source of truth for commands, config fields (units + defaults),
  recommended frame settings, and current gaps. Don't restate those facts here
  or anywhere else; link them.
- **Toolchain gotcha:** `cargo` may not be on the session PATH; the harness
  scripts prepend `%USERPROFILE%\.cargo\bin` — do the same in ad-hoc shells.
- **Non-goals:** `bulk_video_time_min` multi-part splitting in the Rust engine
  (use the PowerShell pipeline); editing/curation UI.

---

## How we work here (the process)

This repo follows a **gated, requirement-traced process**. Read
[docs/process.md](docs/process.md) once; it is the source of truth for roles,
gates, and the ID scheme. The short version an agent needs every session:

- **Everything traces:** `SN → SR → LLR → TC`
  ([docs/requirements/](docs/requirements/), [docs/test/test-cases.csv](docs/test/test-cases.csv)).
  Intent lives once, as an id, and children link to it. The matrix is generated
  (`Scripts/trace.ps1`) and must report **0 orphans**; `-Strict` fails on any.
  (Historical status.md/audit quotes say "UN-###" — that is the 2026-07-01
  rename of the top tier from User Need to Stakeholder Need; numbers unchanged.)
- **The check harness is the bar:** `Scripts/run-tests.ps1` runs
  `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --all`, coverage
  (`cargo llvm-cov`, ≥ 80% line), and the traceability report — the same steps
  CI runs ([.github/workflows/ci.yml](.github/workflows/ci.yml)). Never report
  a result you didn't run — paste the real output.
- **Gates pause for human approval.** The project passed FINAL acceptance
  (2026-06-04); new work is **maintenance**: add/extend SR/LLR/TC rows for any
  behavior change *in the same commit*, keep the harness green, and append a
  dated entry to [docs/status.md](docs/status.md).
- **The code map and dependency diagram are generated**
  (`Scripts/trace.ps1`): per-module summary (from `//!` docs), internal
  `use crate::` dependencies, public symbols with `Implements:` back-links, and
  a Mermaid dependency graph — all spliced between `GENERATED` markers in
  [docs/architecture.md](docs/architecture.md). **Read it to find where a
  capability lives before searching the tree**; never hand-edit between markers.
- **Diagrams are Mermaid fenced blocks in the docs** — rendered by GitHub and
  the VS Code preview, no toolchain. Don't commit exported diagram images.
- **Start each session** by reading the *Current state* header of
  [docs/status.md](docs/status.md); end by updating it (what changed, evidence,
  next action).

## Code we want (readability for humans *and* agents)

- **One responsibility per module/function; small functions.** If a function
  needs an "and" to describe it, split it.
- **Separate the pure, testable core from the I/O shell.** Decision logic goes
  in pure functions (exhaustively unit-tested — e.g. `transform::ClipPlan`,
  `ffmpeg::resolve::pick`, `audio::delay_ms`); processes/GUI/network live in
  thin shells (integration/Demonstration-tested).
- **Entry points orchestrate, they don't compute.** `main`/pipeline routines
  read as a short ordered list of well-named step calls.
- **One fact, one home — in code too.** Shared behavior lives in exactly one
  place and is imported.
- **Back-link to requirements.** Annotate implementing symbols
  `Implements: SR-007, LLR-014` (doc comment or a comment just above) and name
  tests so the verified id is visible (e.g. `build_passes_through_video_audio_sr032`).
  `Scripts/trace.ps1` harvests these into the architecture map.
- **Comment for humans — and the map.** Every module gets a `//!` header
  stating its single responsibility (it becomes the module's summary in the
  map); every public item gets a `///` doc comment with purpose, parameter
  meaning/units, and failure modes. Explain *why* at every non-obvious point;
  update comments in the same edit as the code.
- **Interface contracts live at the code, referenced — not restated.** A
  constraint that already lives in an SR's AcceptanceCriteria/Permutations is
  cited by id, not copied.
- **Fail loudly, never silently;** map errors to plain-language messages
  (`SlideshowError`), non-zero exit on failure.
- **Automation-safe by default.** Anything interactive needs a non-interactive
  path that never blocks (`--non-interactive`, `should_prompt`); no destructive
  default; final outputs appear atomically (`.part` → promote).

## Communication style

- Direct and concrete; explain the *why* behind a recommendation, then the *how*.
- Surface trade-offs and uncertainty honestly; ask before assuming on anything
  irreversible or ambiguous.
- Prefer the simplest thing that satisfies the requirement; flag when a request
  looks over-engineered for its need.

---

> This guide follows the [project-trajectory kit](templates/project-trajectory/README.md)
> (upstream: `ai-template` repo, kit commit stamped in `docs/kit-version`).
> `AGENTS.md` exists as the cross-tool standard pointer; it defers to this file
> for all project-specific content. Add a rule here only after you've had to
> repeat it; delete rules you don't enforce.
