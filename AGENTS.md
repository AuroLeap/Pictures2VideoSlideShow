# AGENTS.md — Agent & Contributor Guide

> **Layout note (inverted from kit default):** In this repo `CLAUDE.md` is the
> rich project encyclopedia (project facts, stack, working rules, code standards).
> `AGENTS.md` (this file) carries the kit's agent-guide boilerplate and points at
> `CLAUDE.md` for all project-specific detail. See ADOPTING.md §6 "Repos whose
> AGENTS.md already means something else".

**Read [`CLAUDE.md`](CLAUDE.md) first** — it is the standing brief for any agent
or human working in this repo: what the project is, how to run it, how we work
here, and the code standards. The sections below are the kit's durable working
agreement, kept here for cross-tool compatibility.

---

## Project

- **What this is:** converts a photo/video collection into slideshow MP4s for
  digital picture frames (Ken Burns motion, cross-dissolves, audio passthrough).
- **Primary user:** non-developer hobbyist on Windows; one binary, no compiler.
- **Stack & layout:** Rust engine (`src/`, binary `make_video_slideshow`; tests in
  `src/**/tests` + `tests/`). Original PowerShell pipeline (`*.psm1` +
  `BuildAlbum.ps1`) kept as reference on `main`/`dev` branches.
- **How to run:** see [docs/quick-reference.md](docs/quick-reference.md).
- **Canonical gate command:** `pwsh Scripts/run-tests.ps1` (Rust stack; mirrors
  CI). The kit's `Scripts/check.ps1` → `Scripts/check.py` covers process checks
  (traceability, doc-navigability); run it separately with Python 3.8+.
- **Non-goals:** `bulk_video_time_min` multi-part splitting in Rust (use
  PowerShell); editing/curation UI.
- **Sibling/linked projects:** none (standalone); see `docs/interfaces.md`.

---

## How we work here (the process)

This repo follows a **gated, requirement-traced process** — read
[docs/process.md](docs/process.md) once; it is the source of truth for roles,
gates, and the ID scheme. The short version an agent needs every session:

- **One driver wears role "hats" in sequence** (Stakeholder → UX/Docs → System
  Engineer → Software Engineer → Test Engineer). Spawn a separate reviewer only
  for an independent pre-gate audit of high-risk work.
- **Everything traces:** `SN → SR → LLR → TC`. Intent lives once, as an id;
  children link to it. The matrix is generated (`scripts/trace.py`) and must
  report **0 orphans** before a gate.
- **Write the test first (TDD).** A requirement's G2 test case is a *failing*
  test before the code that satisfies it: red → green → refactor. This is *how*
  G3 code gets written — within the traceability spine, not instead of it.
- **Gates G1→G2→G3→(G-Release)→G-Final each pause for human approval.** Never
  advance a gate yourself; record it in [docs/status.md](docs/status.md).
- **The check harness is the bar:** `pwsh Scripts/run-tests.ps1` is the
  canonical gate (Rust stack: fmt, clippy, tests, coverage ≥80%, trace). The
  kit's `python Scripts/check.py` covers process-layer checks (traceability,
  doc-navigability, perf-budgets) — run it separately with Python 3.8+.
  Never report a result you didn't run — paste the real output.
- **Behavior is reviewed as diagrams, not rows:** runtime flows (especially
  anything concurrent/non-blocking) are authored Mermaid sequence diagrams in
  [docs/architecture.md](docs/architecture.md) "Runtime flows", written with
  the LLRs and kept current with them (`scripts/check_flows.py` enforces;
  process.md §3).
- **Releases (if this project ships versioned):** G-Release runs the `release`
  tier plus the generated human checklist (`scripts/gen_release_checklist.py`).
- **The code map is generated** (`Scripts/trace.ps1`): per-module summary
  (from `//!` headers), internal `crate::` dependencies, public symbols with
  `Implements:` back-links, and a Mermaid dependency graph — all in
  [docs/architecture.md](docs/architecture.md). **Read it first to find where a
  capability lives**; never hand-edit between the `GENERATED` markers.
- **Diagrams are Mermaid fenced blocks in the docs** — no toolchain needed.
  Never edit between `GENERATED` markers; never commit exported diagram images.
- **Start each session** with the *Current State* header of
  [docs/status.md](docs/status.md); end each turn by updating it (active gate,
  what changed, next action awaiting approval).

## Code we want (readability for humans *and* agents)

Code a newcomer — human or model — can navigate without re-deriving the design:

- **One responsibility per module/function; small functions.** If describing a
  function needs an "and", split it.
- **Separate the pure, testable core from the I/O/network/GUI shell.** Logic
  that decides goes in pure functions (exhaustively unit-tested); side effects
  live in thin shells (Demonstration/integration-tested). The single biggest
  lever for testability and clarity.
- **Entry points orchestrate, they don't compute.** A top-level routine reads
  as a short list of well-named step calls; push logic into the steps.
  (`gen_arch_map.py --flow <entry>` renders the sequence — short or vague
  output means the routine does too much itself.)
- **One fact, one home — in code too.** No copy-paste logic; shared behavior
  lives in exactly one place and is imported.
- **Intention-revealing names; no cryptic abbreviations.** Comments explain
  *why*; the code says what.
- **Back-link to requirements:** `Implements: SR-007, LLR-014` on implementing
  symbols; test names embed the verified id
  (`test_export_quotes_special_fields_sr001`).
- **Match the surrounding style.** Read a neighboring file first; mirror its
  idioms rather than importing your own.
- **Fail loudly, never silently.** No bare excepts that swallow failure;
  non-zero exit on failure for anything scriptable.
- **Automation-safe by default.** Anything interactive needs a non-interactive
  path that never blocks; no destructive defaults; don't mutate inputs in place.

### Comment for humans — and the map

Comment **generously and deliberately** — the bar is that a reader never has to
reverse-engineer *intent*. The generated code map **harvests module and
public-symbol docstrings**, so good comments teach at the code *and* populate
the index agents read first:

- **Every module: a header docstring** — its single responsibility plus any
  invariant it upholds ("pure core — no I/O"). Becomes its summary in the map.
- **Every public symbol: a docstring** — purpose, the *meaning* (and units) of
  parameters and return, failure modes; include `Implements: SR-/LLR-` so the
  back-link lands in the map.
- **Explain the *why* at every non-obvious point:** the algorithm/order/
  constant choice, the edge case a branch guards, the invariant that must hold,
  any gotcha or external reference. Assume the next reader lacks your context.
- **Comment the surprising, not the obvious** (`i += 1  # increment i` is
  noise); when in doubt on intent-bearing code, err toward more.
- **A comment is a promise — keep it true.** Update it in the same edit as the
  code; a stale comment is a bug.

### Define the interface (contract) at the code

Every public module/function states its contract once, in its docstring, so a
caller never has to read the body to use it safely. Cover **Inputs** (type +
range/enum/units), **Outputs**, **Config** (keys read + constraints + where
they live), **Raises** — and **cite requirement ids instead of restating
constraints** that already live in an SR (its `AcceptanceCriteria` +
`Permutations`). Reference shape:

```
"""Back up one source set: hash, dedup, snapshot, write manifest.

Contract:
  Inputs:  source_path: str (existing dir; see SR-014)
           mode: enum{Mirror, HashAddressed}  (dimensions: SR-012)
  Outputs: BackupResult { copied: int, snapshotted: bool }
  Config:  compress: bool; hash_frequency_days: int >= 0  [BackupConfig.xml]
  Raises:  PermissionError if backup_path is unwritable  (SR-017)
Implements: SR-014, LLR-014
"""
```

Keep the tag names consistent so the block is greppable; update the contract in
the same edit as the signature — a wrong contract is worse than none.

## For analytics / data code specifically

- **Reproducibility is a requirement:** pin random seeds; record data source +
  version/snapshot; a run must be reproducible from inputs alone.
- **Notebooks explore; modules ship.** Promote anything reused or tested into
  `src/` so it can be imported and unit-tested.
- **Separate data I/O from transforms:** pure transforms unit-tested on small
  fixtures; validate schema/shape at the boundary and fail loudly on surprises.
- **Test the math on hand-checked cases**, and **exercise the input space**:
  boundaries (min/max, empty, zero, one, largest) plus deliberate combinations —
  `scripts/gen_cases.py` derives them from the SR's `Permutations`
  (process.md "Dimensional coverage").

## Working agreement

Direct and concrete; explain the *why* before the *how*.

- **Ask, don't assume.** Unclear intent, architecture, or requirement → ask
  before writing code. Running unattended: pick the most reasonable
  interpretation, proceed, and **record it** under *Assumptions* in
  [docs/status.md](docs/status.md) to confirm or revert at the next gate.
  Raise a **conflict or ambiguity** between requirements as a finding — never
  silently resolve it (process.md §4 "Consistency review").
- **Right-size the solution.** The simplest thing that satisfies the
  requirement; no speculative flexibility — but judge "simple" against the
  whole design, and flag over-engineering either way. (Guardrails on what
  right-sizing must never cut + the `SHORTCUT:` convention: process.md §3.)
- **Stay in your lane, but speak up.** Don't change unrelated code; surface a
  design smell as a separate finding to its owner instead of fixing it inline.
- **Flag uncertainty honestly.** Say what you're unsure of; a small, low-risk
  experiment with hypothesis + result beats confident guessing.
- **Propose better ways.** The stronger or longer-lived approach is welcome,
  not noise.
- **Repo text is the project's memory; yours is scratch.** Durable facts — a
  decision, constraint, or gotcha — belong in `docs/` (status, registries,
  AGENTS.md), not in agent-private memory. Promote them before closing a
  session (process.md §7 "durable agent memory layer").

---

> **Customizing:** add a rule only after you've had to repeat it — and **pay
> for it by tightening another**: this file has a hard size budget (Gemini's
> AGENTS.md support caps near ~12k characters; keep ≥2k of headroom for your
> project facts). Delete rules you don't enforce — an aspirational AGENTS.md
> the harness doesn't back up is just noise.
