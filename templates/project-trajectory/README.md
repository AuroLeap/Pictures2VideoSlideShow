# Project Trajectory Template

A portable, **stack-agnostic** kit for taking a project from brief (or draft)
to an accepted, maintainable deliverable via a **gated, requirement-traced**
process — with modular/deduplicated code, testable chunks, and explicit
attention to end-user usability and corner cases.

It encodes a key lesson: the value is in the **artifacts, gates, and role
discipline** — not in spawning many agents. One driver wears the role "hats" in
continuous context; a separate reviewer is summoned only for high-risk pre-gate
audits.

## Contents

| File | Use |
|---|---|
| `KICKOFF_PROMPT.md` | **Paste this into an agent to start.** Fill the PROJECT BRIEF at the bottom first. |
| `CLAUDE.template.md` | Agent/contributor guide → copy to the new repo's `CLAUDE.md`. Encodes the readability conventions + points at the process. |
| `PROCESS.md` | The canonical method → copy to `docs/process.md`. Roles, gates, ID scheme, anti-duplication, verdict protocol, review triage, harness contract. |
| `STATUS.template.md` | The live blackboard → copy to `docs/status.md`. |
| `ARCHITECTURE.template.md` | One-page overview + generated map → copy to `docs/architecture.md`. |
| `INTERFACES.template.md` | Cross-project contracts (IF-###) → copy to `docs/interfaces.md`. Use only for interlinked projects. |
| `registries/user-needs.template.md` | UN-### (user needs + edge cases). |
| `registries/system-requirements.template.csv` | SR-### with measurable acceptance criteria. |
| `registries/low-level-requirements.template.csv` | LLR-### ↔ code. |
| `registries/test-cases.template.csv` | TC-### ↔ requirements. |
| `registries/interfaces.template.csv` | IF-### ↔ cross-project contracts (paired with `INTERFACES.template.md`). |
| `scripts/bootstrap.py` | **One command to scaffold a new repo** from this kit (copies templates → `docs/`/`scripts/`/CI, renames, won't clobber). |
| `scripts/check.py` | **The harness.** Runs format · lint · tests · coverage · traceability · arch-map freshness; gate-scoped; nonzero on failure. Python-first reference — wire to your stack. |
| `scripts/trace.py` | **Ready-to-use** traceability checker (Python 3, stdlib only): joins the registries, writes `test/report.md`, exits nonzero on orphans with `--strict`. Called by `check.py`. |
| `scripts/gen_arch_map.py` | Generates the **code map** from the source AST — per-module summary, internal dependencies, and public symbols with `Implements:` back-links — plus a Mermaid **dependency diagram** (rendered natively by GitHub/VS Code; no toolchain). Routes into `architecture.md` and/or `AGENTS.md`/`CLAUDE.md` (repeatable `--doc`). `--flow <entry>` also renders an orchestrator's ordered call sequence (the high-level flow); `--check` fails if stale. |
| `scripts/gen_release_checklist.py` | Generates the human **release checklist** for G-Release from the registries (every Demonstration/Manual/Inspection SR, Release-tier/manual TC, UN acceptance intent, provided interface) as back-linked tick-boxes. |
| `scripts/gen_cases.py` | Expands an SR's input **dimensions** (`Permutations`) into boundary-aware test combinations — full / **pairwise (all-pairs)** / boundary-corners — so tests exercise the input space without the full Cartesian blow-up. |
| `scripts/setup.{sh,ps1}` · `scripts/check.{sh,ps1}` | Cross-platform launchers: one-command venv + dependency setup, and a thin wrapper over `check.py`, for Linux/macOS and Windows. |
| `pytest.ini` | Registers the `smoke`/`full`/`release` test-tier markers the harness selects with `--tier` (unmarked tests run in `full`+`release`). |
| `gitignore.template` | Minimal `.gitignore` for the new repo (venv, tool caches, the regenerated trace report). |
| `ci/check.yml` | Reference GitHub Actions workflow → copy to `.github/workflows/check.yml`. Runs the same `check.py`. |
| `EXAMPLE.md` | A fully worked UN→SR→LLR→TC chain to copy the pattern from. |

## How to use

1. **Scaffold:** from this kit, run
   `python scripts/bootstrap.py --dest /path/to/new/repo` (add `--dry-run` to
   preview). This copies the templates into `docs/`, `scripts/`, `CLAUDE.md`, and
   CI, renaming `*.template.*` to working names.
   *(Manual alternative: copy this folder in and rename by hand.)*
2. **Brief:** fill the **PROJECT BRIEF** in the new repo's `CLAUDE.md` and
   `docs/status.md`. To drive it conversationally instead, paste
   `KICKOFF_PROMPT.md` (brief filled) into your agent.
3. **Wire the harness to your stack:** edit `scripts/check.py`'s `STEPS` for your
   toolchain (the reference uses `ruff`/`pytest`); `trace.py` and
   `gen_arch_map.py` are stdlib-only.
4. The agent runs the gates **G1 → G2 → G3 → G-Release → G-Final** (G-Release
   only for versioned releases), pausing for your approval at each, with
   `python scripts/check.py` as the bar.

## The core ideas (why it produces sustainable code)

- **Traceability:** `UN → SR → LLR → TC`, joined by a generated matrix that must
  report **zero orphans**. Every line of intent is traceable to a need and a test.
- **Single source of truth + decomposition (not paraphrase):** facts live once
  and are referenced by ID; children add detail. This is what keeps docs and code
  from rotting into contradiction.
- **Modularity & dedup:** shared logic in one place; pure testable cores split
  from I/O/GUI shells; one-page architecture, generated so it can't drift.
- **Testability:** measurable acceptance criteria; tests cite requirement IDs;
  coverage threshold; a harness that runs locally and in CI.
- **Usability & corner cases:** a standing End-User lens for setup/first-run,
  failure modes, safety, automation/never-block, and honest docs.
- **Honest gates:** machine-checkable criteria where possible; everything else is
  explicitly classified Demonstration / Manual / Inspection — nothing hand-waved.

## Tuning knobs

- `COVERAGE_THRESHOLD` and `MAX_ROUNDS` in `PROCESS.md`.
- Drop a hat/gate for tiny projects (e.g. skip UX for a library); keep the
  UN→SR→LLR→TC spine.
- Scale review depth to risk — don't gate a rename like you'd gate a crypto path.
