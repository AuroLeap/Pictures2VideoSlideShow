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
| `PROCESS.md` | The canonical method → copy to `docs/process.md`. Roles, gates, ID scheme, anti-duplication, verdict protocol, review triage, harness contract. |
| `STATUS.template.md` | The live blackboard → copy to `docs/status.md`. |
| `ARCHITECTURE.template.md` | One-page overview + generated map → copy to `docs/architecture.md`. |
| `registries/user-needs.template.md` | UN-### (user needs + edge cases). |
| `registries/system-requirements.template.csv` | SR-### with measurable acceptance criteria. |
| `registries/low-level-requirements.template.csv` | LLR-### ↔ code. |
| `registries/test-cases.template.csv` | TC-### ↔ requirements. |
| `scripts/trace.py` | **Ready-to-use** traceability checker (Python 3, stdlib only): joins the registries, writes `test/report.md`, exits nonzero on orphans with `--strict`. Wire into the harness/CI. |
| `EXAMPLE.md` | A fully worked UN→SR→LLR→TC chain to copy the pattern from. |

## How to use

1. Copy this folder into the target repo (or just keep it handy and paste the
   prompt).
2. Open `KICKOFF_PROMPT.md`, fill the **PROJECT BRIEF**, and give it to the agent.
3. The agent scaffolds `docs/` from these templates, wires a `scripts/check`
   harness + CI to the repo's stack, and runs the gates **G1 → G2 → G3 →
   G-Final**, pausing for your approval at each.

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
