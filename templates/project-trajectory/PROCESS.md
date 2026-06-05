# Development Process (template)

Canonical method for a gated, requirement-traced project. Copy this into a new
repo as `docs/process.md`. It is **stack-agnostic** — wire the harness commands
to your project's language/tooling. Other docs reference this file by section
rather than restating it.

---

## 1. Roles (hats), not necessarily separate agents

One driver wears these hats in sequence, keeping context. Spawn a *separate*
agent only for an independent pre-gate review (see §6).

| Hat | Owns (single source of truth) |
|---|---|
| End User | `requirements/user-needs.md` (UN-###) + edge-case expectations |
| UX / Docs | documentation quality, quick-reference, usability findings |
| System Engineer | `requirements/system-requirements.csv` (SR-###); **gatekeeper** |
| Software Engineer | `requirements/low-level-requirements.csv` (LLR-###) + code + `architecture.md` |
| Test Engineer | `test/test-cases.csv` (TC-###) + the check harness + coverage/trace reports |

A hat only edits artifacts it owns; to change another, file a finding addressed
to its owner (§5).

## 2. Identifier scheme

| Prefix | Level | Parent link |
|---|---|---|
| `UN-###` | User Need | — |
| `SR-###` | System Requirement | `UN-Refs` |
| `LLR-###` | Low-Level Requirement | `SR-Refs` (+ Module/CodeSymbol) |
| `TC-###` | Test Case | `Verifies` (SR/LLR) |

Stable, zero-padded, never reused.

## 3. Traceability & anti-duplication

- **One fact, one home.** Reference by ID and link; never restate.
- **Decompose, don't paraphrase.** A child adds detail; if it would merely
  repeat its parent, link instead.
- **Registries are the machine source of truth; prose is thin** and links by ID.
- **The traceability matrix is generated** by a small join over the registries'
  ID/parent columns; it reports **orphans** (req with no child/test; test/LLR
  with no parent). Hand-maintaining the matrix is forbidden.
- **Code carries back-links** (`Implements: SR-007, LLR-014`); test names embed
  the verified ID. CSV columns are authoritative.
- **Architecture is generated** (module/function map) so it cannot drift; keep a
  hand-written one-page overview above it.
- **Modularity/dedup**: shared logic in exactly one place; pure cores separated
  from I/O/GUI shells; small functions; one-page-readable architecture.

## 4. Objectives, gates, and exit criteria

Advance only when criteria pass; **pause for human approval at each gate**.
Define machine-checkable criteria wherever possible; classify the rest honestly.

- **G1 — Requirements, UX & constraints.** UN complete (priority + measurable
  acceptance intent + edge cases); every SR links ≥1 UN with measurable
  acceptance criteria; usability/doc needs + constraints + non-goals captured.
  Sign-offs: End User, UX, System Engineer.
- **G2 — Decomposition & test coverage.** Every SR → ≥1 LLR (or
  Analysis/Inspection); every SR and LLR → ≥1 TC; traceability **0 orphans**;
  harness runs locally + CI. Sign-offs: System Engineer, Test Engineer.
- **G3 — Implementation.** Format/lint clean; all tests pass; coverage ≥
  `COVERAGE_THRESHOLD`; every test-verifiable SR **Verified**; every other SR
  explicitly **Demonstration / Manual / Inspection**. Sign-offs: System
  Engineer, Test Engineer.
- **G-Final — Acceptance.** Human/end-user exercises the real product (incl.
  Demonstration/Manual items) and approves.

**Constants:** `MAX_ROUNDS = 4` per gate (then escalate to the human);
`COVERAGE_THRESHOLD = 80%` line coverage (adjust by agreement; record here).

**Verification methods:** `Test` (automated) · `Demonstration` (run + observe,
e.g. a GUI or a real device) · `Manual` (human procedure) · `Analysis` ·
`Inspection`. Pick the cheapest method that actually establishes the criterion;
don't claim `Test` for something only a human can confirm.

## 5. Verdict & status protocol

Reviews append to `status.md`:

```
### <HAT or REVIEWER> — <Gate> — Round <r> — <YYYY-MM-DD>
Verdict: APPROVE | CHANGES-REQUESTED
Findings:
- [BLOCKER|MAJOR|MINOR] <ID or area> → <issue> → <suggested change> → @<owner>
```

Gate sign-offs live in the **Gate Sign-offs** table; the driver records the gate
decision and pauses for the human.

## 6. Review-depth triage (efficiency)

- **High-risk** (security, data loss, crash-safety, money, irreversible, gate
  closure): spawn an **independent** reviewer with a fresh-context, defect-
  hunting prompt. Verify its file edits; never trust an unverified "green."
- **Medium**: self-review against the gate checklist + run the harness.
- **Low/mechanical** (rename, doc tweak, config): just run the harness.

Keep the status file's *Current State / Open Items* header short so a reviewer
can orient cheaply; the full log lives below and need not be re-read each pass.

## 7. Harness contract (wire to your stack)

`scripts/check` (and the CI workflow) must run, and fail nonzero on any failure:
format check · linter (warnings as errors) · unit + integration tests · coverage
(≥ threshold) · the traceability check (0 orphans for the active gate). Emit the
coverage + traceability reports as artifacts. Prefer a generated architecture
map step so `architecture.md` stays current.

A ready reference traceability checker ships with this template:
`scripts/trace.py` (Python 3, stdlib only) — joins the registries, writes
`docs/test/report.md`, and exits nonzero on orphans with `--strict`. Call it
from `scripts/check`. See `EXAMPLE.md` for a complete worked UN→SR→LLR→TC chain.
