# Adversarial Review Checkpoint — 2026-08-12

Status: **PAUSED at the owner's request before implementation.**

This document preserves where the repository-and-assets adversarial review landed
so a later session can resume without repeating the discovery work. No production
code, tests, requirements, CI configuration, or assets were changed during the
review. Findings below are evidence-backed, but their final disposition still
belongs to the relevant project owner/gate.

## Review scope and evidence

The review covered the Rust engine, legacy PowerShell pipeline, configuration and
filesystem boundaries, FFmpeg process handling, caches, tests, requirements and
traceability, CI/release workflows, documentation, current binary assets, and
reachable Git history. Three independent reviewers separately examined security,
reliability/correctness, and assets/process concerns.

Validation completed before the pause:

- The worktree was clean at review start.
- `cargo test --test probe_cache save_is_atomic_tmp_rename_sr038 -- --nocapture`
  passed on this Windows host. This disproved the initial suspicion that
  `std::fs::rename` could not replace an existing destination here; that broad
  finding was retracted.
- Independent process checks found 31 SNs, 40 SRs, 82 LLRs, 118 TCs, and zero
  traceability orphans/schema findings.
- The documentation-link check found no broken internal links.
- The flow checker found five diagrams and no unknown requirement citations.
- No common secret/private-key patterns were found in the currently tracked tree.
- The canonical `pwsh Scripts/run-tests.ps1` baseline was started, but its
  coverage build monopolized the shared Windows shell for several minutes without
  output and prevented all reviewers from collecting evidence. It was terminated
  to restore the shell. **Do not treat the canonical harness as run or passed for
  this review.**

## High-confidence issues suitable for direct repair

These issues have clear evidence and a low-ambiguity defect mechanism. Before
implementation, follow the repository's maintenance traceability/TDD rules and
decide whether existing requirements/TCs should be extended or new rows added.

### 1. Output names can escape `output.base_dir` (MAJOR)

`Config::validate` checks duplicate output names but does not require a name to be
one safe filename component (`src/config/mod.rs:281-335`). Both build paths form
the target with `output_dir.join(format!("{}.mp4", output_def.name))`
(`src/pipeline/mod.rs:301` and `:721`), and segmented staging names also embed the
name (`src/pipeline/segment.rs:70-71`). A name containing `..`, separators, or an
absolute/rooted Windows path can write outside the configured output directory.

Candidate fix: reject empty/rooted/multi-component names, separators, `.`/`..`,
Windows reserved device names, and trailing dots/spaces. Add table-driven config
tests for accepted and rejected stems.

### 2. The documented legacy installer executes unverified downloads as Administrator (BLOCKER)

`README.md:109-114` recommends `InstallDependencies.ps1`. The script requires
administrative privileges (`InstallDependencies.ps1:1-5`), downloads an unpinned
mutable FFmpeg archive directly into `C:\` (`:11-18`), and downloads then executes
JPEGR and ImageMagick installers without checksum or signature verification
(`:57-63`, `:76-82`). Compromise of a host, mutable artifact, or vendor account
would become elevated code execution.

Candidate immediate fix: remove/deprecate the automated recommendation and make
the script refuse to run until all artifacts are immutable-version pinned and
verified before extraction/execution. Prefer the Rust engine's per-user,
checksum-gated FFmpeg acquisition for current users.

### 3. A corrupt segment-cache index can address files outside the cache (MAJOR)

`SegmentStore` deserializes arbitrary JSON map keys and maps them to
`root.join(format!("{key}.ts"))` during reconciliation, lookup, prune, and clear
(`src/cache/store.rs:38-42`, `:78-93`, `:120-123`, `:150-160`, `:225-246`). A
poisoned key containing traversal or a rooted path can cause metadata access or
deletion outside `segment-cache`.

Candidate fix: introduce a validated cache-key type and accept only the production
SHA-256 form (`^[0-9a-f]{64}$`). Drop invalid index rows without deriving or
touching a path from them.

### 4. The writability probe can truncate/delete a real file (MAJOR)

`ensure_writable_dir` always uses `.slideshow_write_test`, opens it with
`File::create` (which truncates an existing file), then removes it
(`src/util/file_utils.rs:66-74`). Concurrent runs collide; a link/reparse-point
collision increases the damage surface.

Candidate fix: create a unique unpredictable probe with
`OpenOptions::create_new(true)` and delete only the exact file successfully
created by this invocation. Add a regression test proving a pre-existing
collision remains byte-identical.

### 5. Numeric config fields permit overflow, OOM, and unbounded work (MAJOR)

Validation rejects only zero dimensions/fps and CRF above 51
(`src/config/mod.rs:294-314`). It does not bound or require finite/nonnegative
display/fade time, rotation, zoom, focus values, audio settings, dimensions, or
fps. Frame counts cast floats to integers (`src/config/mod.rs:213-219`), frame
sizes use unchecked multiplication (`src/transport.rs:44-53`), and downstream
code allocates based on these values.

Candidate fix: define documented supported bounds, reject NaN/infinity/negative
values, validate pixel and frame-count budgets, and use checked arithmetic before
allocation or subprocess startup. Cover every boundary and just-outside value.

### 6. `dry_run` is inert and still performs a real build (MAJOR)

The CLI/config flag only sets `processing.dry_run` (`src/main.rs:143-145`). No
production path reads it afterward; `run_build` still scans, executes the pipeline,
writes MP4s, and can mutate/clear caches (`src/main.rs:310-334`). The quick
reference promises “Plan without encoding” (`docs/quick-reference.md:121`).

Candidate fix: implement an explicit side-effect-free planning path or, if the
feature is not intended, remove the field/flag and correct the documented
contract. A test should assert no MP4, temp, or cache mutation.

### 7. Output/temp paths may be placed inside `media_root` (MAJOR)

`Config::validate` never compares `media_root` with `output.base_dir` or
`processing.temp_dir`. Preflight may create/probe the output directory and the
build/cache then write beneath it. This directly violates SR-010's promise that
nothing under `media_root` is created or modified and can feed a prior MP4 back
into the next scan.

Candidate fix: reject equal or descendant output/temp paths after robust absolute,
case-aware Windows normalization and junction/symlink consideration. Validation
must occur before any directory or probe file is created.

### 8. Scan/decode failures do not consistently satisfy SR-014 (MAJOR)

Walk/probe failures are logged or silently dropped during scanning and never
reach `BuildSummary.skipped` (`src/media/mod.rs:108-131`). A failed video probe is
converted to dimensions `(0,0)` (`:306-333`); later decoder read/wait errors
propagate from `mixer.add_clip(...)?` (`src/pipeline/mod.rs:590`) and can abort the
whole output rather than skip the bad video. Existing skip tests cover truncated
images, not these paths.

Candidate fix: carry scan failures into the album/build summary and make per-video
decode failure transactional at clip scope. Add valid+bad-video and corrupt-header
cases that prove a named skip, correct count, continued output, and exit semantics.

### 9. The canonical and release gates can be falsely green or bypassed (HIGH)

`Scripts/run-tests.ps1:35-44` silently skips coverage when `cargo-llvm-cov` is
missing, and its trace step omits `-Strict` (`:48`). It also does not run the
process-layer checks. `ci.yml` repeats non-strict trace generation and excludes
the active `Optomizations` branch. `release.yml` builds and publishes without a
dependent test/process/release-checklist job. The separate `check.yml` is an
unadapted Linux/Python template using lowercase `scripts/` paths, while this repo
uses `Scripts/`.

Candidate fix: make missing coverage tooling fatal in the canonical gate, run
strict trace/process checks, adapt or remove the template workflow, and make
release publication depend on a single successful release-tier validation job.

### 10. Authored architecture and TC status contradict current implementation (MAJOR)

`docs/architecture.md:166-180` still calls SR-040/Phase 4a.5 Draft and says rgb24
is shipped; lines 39, 193, 202, 220, and the module text also retain stale rgb24
wording. Current status and registries say GPU yuv420p transport is implemented.
Separately, TC-117 is marked Verified while `docs/status.md` says its device-lost
yuv extension remains demo-pending.

Candidate fix: update the authored flows/overview and either mark TC-117 pending
or split its witnessed chroma leg from the unperformed device-loss extension.

### 11. FFmpeg acquisition embeds unescaped paths in PowerShell source (MINOR)

`src/setup/ffmpeg_fetch.rs:114-121` and `:152-159` interpolate paths inside a
single-quoted `-Command` string. Apostrophes break legitimate paths and crafted
path/environment values can change PowerShell syntax.

Candidate fix: use a Rust HTTP/ZIP implementation or a fixed PowerShell script
whose paths are passed as separately bound arguments, never source text.

## Issues needing owner/design decisions before changes

These are credible concerns, but their remediation changes policy, architecture,
or externally coordinated state. Do not silently choose a solution.

### 1. Privacy-sensitive and license-restricted files remain in Git history (CRITICAL)

Commit `af66bd6` remains an ancestor of the active/local and origin maintenance
branches. `Scripts/ContReportPath` at that commit is 3,621,467 bytes and contains
family-media paths. `Scripts/autocolor`, `autogamma`, and `autotone` state that
they are free only for non-commercial use and require permission to redistribute.
Deleting them from the current tree did not remove them from cloneable history.

Decision required: authorize and coordinate a `git filter-repo` rewrite,
force-push, collaborator re-clone, release/cache cleanup, and fork exposure
assessment. This cannot be repaired safely as an ordinary local commit.

### 2. Tracked test media lacks provenance/license records (MAJOR)

`TestInput/` contains roughly 55 MB of tracked images/videos. The asset registry
still contains only its example row, and files such as `Music1.mp4` and
`Music2.mp4` have no source/license record. Decide whether to replace them with
generated or pinned-download fixtures, move them to LFS/release storage, or add
complete provenance/license/attribution/hash records. A current-tree move alone
would not reduce or sanitize prior Git history.

### 3. `ffmpeg_timeout_secs` covers only the output encoder (MAJOR)

Video decode reads/waits (`src/video/mod.rs:89-117`), scan probes
(`src/media/mod.rs:285-320`), and audio mux (`src/ffmpeg/audio.rs:126`) block
without the configured deadline. A stalled decoder can block the mixer while the
encoder watchdog watches only its own child. Decide whether SR-013 is intended to
bound every FFmpeg/FFprobe phase; if yes, centralize child supervision and add
phase-specific timeout tests.

### 4. Cache corruption/integrity and concurrency policy (MAJOR)

Segment validity is nonzero-size equality only; an equal-length bit flip remains
a hit despite SR-037's “corrupt entry = miss” wording. Segment and probe caches
also use shared fixed index/temp paths without locking, so concurrent processes
can lose entries or delete one another's planned segments. Decide whether to add
content digests and exclusive locking/merge semantics, or explicitly state that
concurrent cache use and hostile/shared cache roots are unsupported.

### 5. Supply-chain hardening for CI/releases (MAJOR)

Actions use mutable major tags, Rust uses the mutable `stable` channel, tools and
Chocolatey FFmpeg are not version pinned, and the release action has contents
write authority. No present compromise was found, but decide the project's pin
and automated-update policy before changing all workflow dependencies.

### 6. Inert resource controls (MINOR/MAJOR)

`max_workers` and `use_parallelism` are documented but have no production reads;
Rayon remains active regardless. Decide whether these are supported controls to
implement or stale fields to remove.

### 7. Other lower-priority design concerns

- Native image/FFmpeg parsers run with the user's normal privileges and no
  resource sandbox; decide whether arbitrary untrusted downloads are in scope.
- `promote` deletes a completed `.part` on any rename failure; a transient AV,
  removable-drive, or open-handle failure loses expensive completed work.
- Concurrent/rerun semantics for preserving an older valid final MP4 after a new
  failed run are not explicitly pinned.
- A configured FFmpeg executable with a nonstandard filename can resolve, yet
  later process calls still spawn literal `ffmpeg`.
- The first-run wizard writes a predictable executable script under `%TEMP%` and
  launches it with ExecutionPolicy Bypass; a private unique temp location would
  reduce races.
- Scaffold residue remains (`Scripts/onboard.*` uses `OWNER/REPO`; setup scripts
  are Python-template oriented; `README.adoc` duplicates older PowerShell docs).
- Trace/report generation has dual PowerShell/Python owners, nondeterministic
  generated timestamps, a 12-public-symbol map cap, and no effective freshness
  gate. This already causes report-only commits and should be rationalized.

## Suggested resume order

1. Ratify the history/privacy response first because delay can broaden exposure.
2. Disable the elevated unverified installer recommendation.
3. Repair path trust boundaries: output name, cache key, source/output/temp overlap,
   and write probe.
4. Define numeric limits and add checked arithmetic.
5. Restore honest gates before relying on any subsequent green result.
6. Fix SR-014 scan/video behavior and implement or remove `dry_run`.
7. Resolve timeout/cache/concurrency policies, then address lower-priority
   hardening and documentation drift.
8. Run focused tests, `pwsh Scripts/run-tests.ps1`, and the process-layer checks;
   record only observed output.

## Resume state

At the pause point, no finding had been patched and no requirement/test rows had
been added. The working tree was clean before this checkpoint document and status
entry were created. Resume from this file rather than the older partial audit in
`docs/review-2026-07-17.md`.
