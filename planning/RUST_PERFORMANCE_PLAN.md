# Rust Engine Performance Plan (2026-07)

**Status: PROPOSED — current-era plan for the shipped Rust engine.** Unlike the
other documents in this folder (which analyze the retired PowerShell pipeline),
this plan targets the code that ships today: `make_video_slideshow` streaming
raw `rgb24` frames into FFmpeg. Config/schema of record: `src/config/mod.rs`.

**Goal:** make builds over a large and *growing* photo library as fast as
feasible — including GPU acceleration (hardware encode first, then optionally
GPU rendering via wgpu/Vulkan) — without breaking the frame-compatibility
profile (SR-005), atomic outputs (SR-011), or determinism (SR-022).

---

## 1. Where the time actually goes today

The engine's hot path (per output definition, `src/pipeline/mod.rs`):

```
for each media item (serial):
  image:  decode JPEG → prescale (Triangle)          ← serial, on the clip boundary
          per frame: bilinear warp_into (rayon batch) ← CPU resample, full frame
  video:  ffmpeg decode → rgb24 pipe
  crossfade: scalar f32 blend per byte                ← single-threaded
  write rgb24 frame → ffmpeg stdin
ffmpeg child: libx264 -preset medium -crf N           ← software encode, hardcoded
```

Known structural costs (from code inspection — **all must be confirmed by
Phase 0 measurement before acting**):

| # | Cost | Where | Why it hurts at scale |
|---|---|---|---|
| C1 | Software H.264 encode, `-preset medium` hardcoded | `src/ffmpeg/mod.rs` (~line 119) | libx264 medium at 1080p+ is often the pipeline ceiling; competes for the same CPU as the renderer |
| C2 | Clip decode+prescale is serial on the boundary | `FrameRenderer::load_with_focus` (`src/image/mod.rs:35`), called from the clip loop (`src/pipeline/mod.rs:238`) | A 24 MP JPEG decode + Triangle prescale is ~100–400 ms of dead time per photo, per output; ×5,000 photos = ~10–30 min of pure stall |
| C3 | Every output re-decodes every image | same | 2 outputs ⇒ 2× decode cost |
| C4 | Generic bilinear `warp_into` even when rotation is off | `src/image/mod.rs:83` | With `max_rotation_degrees = 0` the projection is an axis-aligned scale+crop — a SIMD resize would do the same work several times faster |
| C5 | Crossfade `blend`/`scale` are scalar per-byte f32 | `src/pipeline/mod.rs:462-477` | Single-threaded float math over full frames during every transition and fade |
| C6 | Render batch and pipe writes alternate | `ImageFrameSource::next_frame` (`src/pipeline/source.rs:39`) | While a rayon batch renders, ffmpeg's stdin starves; while frames are written, rayon idles |
| C7 | Whole album re-encoded on every run | `FrameGenerationPipeline::execute` | A growing library pays full cost every build; yesterday's 4,900 photos are re-rendered to add today's 100 |
| C8 | Scan re-probes everything each run | `src/media/mod.rs` (`ffprobe` per video, decode header per file) | Minutes on a NAS/large tree; grows with the library |

Two distinct user-visible problems fall out of this:

- **Cold build too slow** → phases 1–4 (throughput).
- **Rebuild after adding photos costs the same as a cold build** → phase 2
  (incremental cache) — for a "library only growing" workflow this is the
  single biggest win, bigger than any raw-speed work.

---

## 2. Phase 0 — Measure before touching anything (½–1 day)

You cannot rank C1–C8 without numbers, and the kit already has the machinery
for exactly this (`docs/requirements/performance-budgets.csv`, `Scripts/check_perf.py`).

1. **Stage timers.** Add lightweight instrumentation (behind `--verbose` or a
   `log::debug!` target): per-clip decode ms, prescale ms, render ms/frame,
   blend ms, stdin-write stall ms, and ffmpeg wall time. The pipeline already
   logs end-of-output `frames/s` (`src/pipeline/mod.rs:334`) — extend, don't duplicate.
2. **Benchmark corpus.** A fixed, checked-in-by-reference album (e.g. 200
   representative photos + 3 videos) and a `Scripts/bench.ps1` that runs a
   build twice (warm cache) and records frames/s per stage. Not CI-gated at
   first — Tier=Release, like the other hardware-bound TCs.
3. **Budgets.** Replace `PB-000` with real rows, e.g.
   `PB-001 end-to-end frames/s @1080p` (higher-better, warn),
   `PB-002 scan seconds per 1k files`, `PB-003 clip-boundary stall ms`.
   Each later phase must move a PB row, or it gets reverted.
4. **Record the baseline** in `docs/status.md` with the exact hardware
   (CPU, GPU, source-disk type) — every "×" claim below is relative to this.

**Exit criterion:** a table showing % of wall time per stage for (a) 1080p and
(b) 4K output on the real library disk. That table decides the order of
phases 1 vs 3 vs 4.

---

## 3. Phase 1 — CPU/encoder quick wins (days each, low risk, independent)

Ordered by expected value; each is an independent maintenance change (SR/LLR/TC
in the same commit, harness green).

### 1a. Hardware video encode (NVENC / QSV / AMF) — the cheap "GPU" win
- New config field on `OutputDef` (or `ProcessingConfig`):
  `encoder = "auto" | "software" | "h264_nvenc" | "h264_qsv" | "h264_amf"`.
  Default **`software`** so SR-005's verified profile is untouched; `auto`
  probes `ffmpeg -encoders` + a 1-frame trial encode at preflight and falls
  back to libx264 with a logged reason (never fails the build for a missing GPU).
- Changes are confined to the arg list in `FfmpegEncoder::start` + a small
  pure `encoder_args(choice) -> Vec<String>` function (unit-testable like
  `ffmpeg::resolve::pick`). Map `quality_crf` → `-cq`/`-global_quality`
  per encoder; keep `yuv420p`, `+faststart`, mp4 — the `encode_profile`
  integration test must pass identically for every encoder choice.
- **Why first:** if Phase 0 shows encode ≥ 40% of wall time, this alone is
  ~1.5–3× end-to-end, frees the whole CPU for rendering, and is ~a day of work.
  On typical GPUs NVENC does 1080p H.264 at several hundred fps.

### 1b. Prefetch the next clip (kill the boundary stall — C2)
- While clip *N* streams, decode+prescale clip *N+1* on a background thread
  (lookahead 1–2, bounded channel; `std::thread` + `sync_channel` is enough —
  no new architecture). The clip loop then receives ready `FrameRenderer`s.
- Skip/error handling stays identical: a failed prefetch delivers the error to
  the loop where `record_skip` already handles it (SR-014 unchanged).

### 1c. Faster decode + SIMD resample (C4, and half of C2)
- Upgrade `image` 0.24 → 0.25+ (zune-jpeg decoder, ~2–3× JPEG decode) and
  `imageproc` to match. Mechanical but touches lockstep APIs — one commit, full
  harness.
- Adopt `fast_image_resize` (SIMD SSE4/AVX2) for the prescale, and for the
  per-frame path **when rotation is off**: the projection is then pure
  scale+translate, so render a frame as "SIMD-resize the float crop window"
  instead of generic `warp_into`. Keep `warp_into` as the rotation path.
  Guard with the existing determinism tests (SR-022): same seed ⇒ same output
  *within the new implementation* (bit-exactness vs the old resampler is not
  required by the SR, but confirm visually once).

### 1d. Integer/SIMD crossfade (C5) + drop the stray clone
- Rewrite `blend`/`scale` in fixed-point u16 (`(a*(256-w) + b*w + 128) >> 8`)
  over chunked slices; rayon-chunk if still visible in profiles. Remove the
  `f.clone()` for surplus head frames (`src/pipeline/mod.rs:410`).
- Small win (transitions only) but trivial and permanent.

### 1e. Share decodes across outputs (C3)
- When >1 output is configured, decode each source once and prescale per
  output from the shared decoded image (prescale sizes differ; the decode
  doesn't). Fits naturally into the 1b prefetcher.

### 1f. Expose the software preset
- `x264_preset = "medium"` (default unchanged) in config for users who accept
  `faster`/`veryfast` on bulk builds. Document the quality trade in
  quick-reference. Zero risk, occasionally 1.5–2× when stuck on software encode.

**Expected after Phase 1 (hardware-dependent):** ~2–4× cold-build, boundary
stalls ≈ 0, CPU freed for rendering wherever a hardware encoder exists.

---

## 4. Phase 2 — Incremental builds: per-clip segment cache (the growing-library win — C7)

**Motivation:** the library only grows. Today +100 photos ⇒ re-render 5,000.
The PowerShell pipeline's part-concat design is precedent that concat works
for this product.

**Design sketch:**
- Render/encode each clip (or small group of clips) to an intermediate
  segment (`.ts` or fragmented `.mp4`) in a cache directory, keyed by
  `hash(source path+size+mtime, OutputDef fields, ClipPlan seed, engine version)`.
- A build then: computes the segment list → renders only cache misses →
  `ffmpeg -f concat` (stream-copy, no re-encode) → audio mux → atomic promote.
  Final `.part → .mp4` promotion and SR-011 semantics unchanged.
- **Crossfade boundaries:** transitions span two clips, so a segment =
  "second half of transition in + body + first half of transition out",
  keyed additionally by the *neighbor* identity on each side. Only segments
  whose neighbor changed are re-rendered — inserting photos in sorted order
  invalidates O(inserted + 2 neighbors), not O(album).
- Cache management: size-capped LRU, `--no-cache` and `--clear-cache` flags,
  cache misses are the normal path (a cold cache is just today's behavior).
- **Risks to burn down first:** concat of independently-encoded x264/NVENC
  segments must be seam-free on the target frames (validate on real frame
  hardware early — a spike TC before committing to the design); timestamps
  across segment joins; audio-delay bookkeeping (`audio::delay_ms`) now maps
  through the concat timeline.

**Expected:** rebuild time ≈ O(new photos), i.e. **10–100× on the routine
"I added photos, rebuild" workflow.** This changes the user's experienced
speed more than any throughput work; recommend scheduling it immediately
after Phase 1a/1b even though it's the most design-heavy phase.

Also in this phase (same theme, small): **scan cache (C8)** — persist probe
results keyed by path+size+mtime (JSON beside the ROI db) so rescans of an
unchanged library are near-instant; `ffprobe` only runs for new/changed videos.

---

## 5. Phase 3 — Overlap the pipeline (C6) (≈1 week, medium risk)

Restructure the per-output flow into bounded producer/consumer stages:

```
[prefetch decode+prescale] → [render pool (rayon)] → [mixer/blend] → [encoder-writer thread]
        lookahead 2                ordered frames        bounded channel (~2×fade)
```

- The encoder-writer owns ffmpeg stdin; the mixer no longer blocks rendering
  while a frame is being piped. Memory stays bounded by channel capacities
  (replaces `RANGE_MEMORY_BUDGET` batching with the same cap expressed as a
  channel depth).
- Keep `CrossfadeMixer`'s logic (head/tail ring) intact — it is well-tested;
  only its *inputs* become async. Watchdog (SR-013) and disk-full mapping
  (SR-015) move with the writer thread.
- Do this **after** Phase 1, because 1a/1b may already get utilization near
  the ceiling; Phase 0 timers tell you whether the remaining gap justifies it.

**Expected:** 1.3–2× on top of Phase 1 when render and encode are comparable
in cost; ~0 when one side dominates (which is why it's measured, not assumed).

---

## 6. Phase 4 — GPU rendering via wgpu (Vulkan/DX12) (2–4 weeks, highest effort)

The "run on Vulkan" option, done the maintainable way: **`wgpu`**, not raw
Vulkan. wgpu is safe Rust over Vulkan/DX12/Metal, needs no SDK install for
users, and picks the best backend per machine (DX12/Vulkan on Windows).

**Design:**
- New `render_backend = "auto" | "cpu" | "gpu"` in `ProcessingConfig`;
  `auto` = GPU when an adapter exists, silent fallback to the CPU path
  otherwise. The CPU renderer is **kept forever** — it is the correctness
  reference and the no-GPU fallback (same shape as the encoder fallback in 1a).
- Introduce a small `trait ClipRenderer` (the current `FrameRenderer` becomes
  the CPU impl) so `ImageFrameSource` doesn't care which backend renders.
- GPU impl: upload the prescaled image once per clip as a texture; per frame,
  draw one textured quad through the `ClipPlan` projection (hardware bilinear
  sampling makes the warp essentially free — a 1080p quad is ~microseconds);
  do crossfade blends and black-fades in the same shader (second texture +
  mix weight — this subsumes C5 on GPU); readback via ring of staging buffers
  (3-deep, async map) → rgb24 → existing ffmpeg stdin path.
  Readback bandwidth is a non-issue: 1080p30 rgb24 ≈ 190 MB/s; 4K30 ≈ 750 MB/s
  vs ~16 GB/s PCIe 4.0 ×8.
- **`ClipPlan` stays the single source of truth**: the CPU computes the
  per-frame projection matrix and uploads it as a uniform — the shader gets no
  independent math to drift. Determinism (SR-022) is redefined per-backend:
  same seed ⇒ same plan (bit-exact, existing tests) ⇒ same GPU output on the
  same machine; CPU-vs-GPU pixel identity is *not* claimed (document in SR).
  GPU-path pixel checks become tolerance-based (mean |Δ| per channel < ε) and
  Tier=Release/Demonstration, like the other hardware-bound TCs.
- Crates: `wgpu` + WGSL shader (checked in, ~100 lines) + `bytemuck`. No
  shaderc, no Vulkan SDK, no build-time toolchain change.

**When it pays:** only if Phase 0/1 numbers show CPU rendering is still the
bottleneck *after* hardware encode offload (e.g. 4K output, high fps, weak
CPU + decent GPU). On a strong CPU at 1080p, Phases 1–3 may already hit the
encoder/disk ceiling and Phase 4 buys little — decide on the measured PB rows,
not on enthusiasm.

**Expected when render-bound:** render cost → near zero; end-to-end limited by
JPEG decode + NVENC, plausibly another 2–4×. Combined stack (1+3+4) on a
GPU-equipped machine: order of 10× cold-build vs today's baseline, plus
Phase 2's ~O(new photos) rebuilds.

---

## 7. Explicitly rejected / deferred

- **Raw Vulkan / CUDA / OpenCL kernels** — wgpu delivers the same GPU sampling
  hardware with a fraction of the code and no vendor lock (see the historical
  [OPTIMIZATION_PATHS.md](OPTIMIZATION_PATHS.md) §1 for the old analysis).
- **GPU JPEG decode (nvJPEG)** — prefetched parallel CPU decode (1b/1c) is
  simpler and stops being the bottleneck once overlapped.
- **Distributed/multi-machine** — operational complexity far beyond the
  one-user, one-binary product goal (CLAUDE.md non-goals spirit).
- **Parallel encoding of multiple `OutputDef`s** — deferred; it multiplies
  memory and fights 1a's GPU encoder sessions (consumer NVENC caps concurrent
  sessions). Revisit only if multi-output builds are common.

---

## 8. Process integration (how this lands in this repo)

Every phase is maintenance work under the standing rules:

- **Requirements:** new SRs for encoder selection + fallback, incremental
  cache semantics, render-backend selection + fallback; LLRs per module;
  TCs in `docs/test/test-cases.csv` (unit TCs for the pure decision functions
  — `encoder_args`, cache-key derivation, backend pick — mirror the
  `ffmpeg::resolve::pick` pattern; hardware paths are Tier=Release
  Demonstrations, same as SR-013/024/027 today).
- **Budgets:** PB rows from Phase 0 are the acceptance criteria for every
  subsequent phase; `Scripts/bench.ps1` output pasted into `docs/status.md`
  per phase (never report an unmeasured speedup).
- **Safety invariants that must not regress:** SR-005 default profile
  (software encode stays default; hardware is opt-in/`auto` with trial-encode
  verification), SR-011 atomic promote (unchanged in all phases), SR-013
  watchdog, SR-014 skip-and-continue (prefetch errors route to `record_skip`),
  SR-022 determinism (per-backend, as redefined in Phase 4).
- **One branch per phase**, harness green, dated `status.md` entry with the
  before/after PB numbers.

## 9. Recommended order & decision points

| Order | Phase | Effort | Expected gain | Gate to proceed |
|---|---|---|---|---|
| 1 | 0 — Measure | ½–1 day | the roadmap itself | baseline table exists |
| 2 | 1a Hardware encode | ~1–2 days | 1.5–3× (if encode-bound) | PB shows encode share |
| 3 | 1b–1f CPU wins | ~1 week total | → ~2–4× cumulative | each moves a PB row |
| 4 | 2 — Incremental cache (+scan cache) | 1–2 weeks | 10–100× on rebuilds | concat-seam spike passes on real frame |
| 5 | 3 — Pipeline overlap | ~1 week | 1.3–2× | PB shows idle gap between render & encode |
| 6 | 4 — wgpu GPU render | 2–4 weeks | 2–4× when render-bound | PB shows render still dominant after 1–3 |

The first three rows are low-risk and high-certainty; do them regardless.
Rows 4–6 each have an explicit measurement gate so effort is never spent on a
stage that is no longer the bottleneck.
