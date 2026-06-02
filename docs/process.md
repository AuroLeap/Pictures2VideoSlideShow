# Development Process — Multi-Agent Grind

Canonical definition of the roles, workflow, gates, identifier scheme, traceability, and anti-duplication rules used by the [persona agents](../.claude/agents/) and the [`/grind` orchestrator](../.claude/commands/grind.md). Every agent reads this file first. Other docs **reference** this one rather than restating it.

Back to the [project README](../README.md) · docs index: [docs/README.md](README.md).

---

## 1. Roles (who owns what)

| Persona | Owns (source of truth) | Agent file |
|---|---|---|
| End User | `requirements/user-needs.md` (UN-###) | [end-user](../.claude/agents/end-user.md) |
| UX Designer | doc quality + `ux/notes.md` (+ `ux/` assets) | [ux-designer](../.claude/agents/ux-designer.md) |
| System Engineer | `requirements/system-requirements.csv` (SR-###); **gatekeeper** | [system-engineer](../.claude/agents/system-engineer.md) |
| Software Engineer | `requirements/low-level-requirements.csv` (LLR-###) + `src/` + `architecture.md` | [software-engineer](../.claude/agents/software-engineer.md) |
| Test Engineer | `test/test-cases.csv` (TC-###) + harness + `test/report.md` | [test-engineer](../.claude/agents/test-engineer.md) |

A persona only edits artifacts it owns. To change another artifact, file a finding (see §5) addressed to its owner.

---

## 2. Objectives, participants, and gates

The orchestrator runs one **objective** at a time, in bounded **rounds**, and **pauses for human approval at each gate**.

### Objective 1 — Requirements, UX & constraints consensus
Participants: End User, UX Designer, System Engineer.
**Gate criteria:**
- `user-needs.md` exists; every UN has priority + acceptance intent; edge-case expectations captured (power loss, app crash, corrupt/unsupported media, full disk, removed media).
- `system-requirements.csv`: every SR links ≥1 UN and has measurable AcceptanceCriteria; digital-picture-frame constraints captured as SRs.
- No open `CHANGES-REQUESTED` for OBJ1; sign-offs: **End User, UX Designer, System Engineer = SIGNED**.

### Objective 2 — Low-level requirements & test coverage
Participants: Software Engineer, Test Engineer, System Engineer.
**Gate criteria:**
- `low-level-requirements.csv`: every LLR links ≥1 SR; every SR has ≥1 LLR or `Verification=Analysis/Inspection` justification.
- `test-cases.csv`: every SR and LLR has ≥1 TC. `scripts/trace.ps1` reports **0 orphans**.
- Harness runs locally and in CI (skeleton green). Sign-offs: **System Engineer, Test Engineer = SIGNED**.

### Objective 3 — Implementation meets requirements
Participants: Software Engineer, Test Engineer, System Engineer.
**Gate criteria:**
- `scripts/run-tests.ps1`: `cargo fmt --check` clean, `cargo clippy` clean, all tests pass, coverage ≥ **COVERAGE_THRESHOLD**.
- Every TC `Automated=Yes` or `Manual` with justification; trace shows all SR/LLR `Status=Verified` by passing tests.
- Sign-offs: **System Engineer, Test Engineer = SIGNED**.

### Final — End-user acceptance
Participant: End User (with Test Engineer evidence).
**Gate criteria:** End User runs the app on sample media, judges demonstrated results + docs against the UN list, and records **APPROVE**. Then the human gives final sign-off.

**Constants:** `MAX_ROUNDS = 4` per objective (then escalate to human). `COVERAGE_THRESHOLD = 80%` line coverage (adjust by agreement, record here).

---

## 3. Identifier scheme

| Prefix | Level | Owner | Parent link |
|---|---|---|---|
| `UN-###` | User Need | End User | — |
| `SR-###` | System Requirement | System Engineer | `UN-Refs` |
| `LLR-###` | Low-Level Requirement | Software Engineer | `SR-Refs` |
| `TC-###` | Test Case | Test Engineer | `Verifies` (SR/LLR) |

IDs are stable and never reused. Numbering is zero-padded, allocated in order (UN-001, UN-002, …).

---

## 4. Traceability & anti-duplication (read this carefully)

**Single source of truth.** Each fact lives in exactly one place, owned by one persona. Everything else **references it by ID** and links to it. If two documents would say the same thing, keep it in the owner's doc and link.

**Decompose, don't paraphrase.** A child item (SR under UN, LLR under SR, TC under SR/LLR) adds *new detail or decomposition*. If a child would merely restate its parent, it should not exist — link to the parent instead.

**The registries are the machine source of truth; prose is thin.** The `.csv` files carry IDs, links, acceptance criteria, and status. The `.md` docs give human context and *link by ID*; they do not copy rows.

**The traceability matrix is a generated view, not a copy.** `scripts/trace.ps1` joins the CSVs on their ID/parent columns to produce `test/report.md` and to report **orphans**: requirements with no child/test, and tests/LLRs with no parent. Never hand-maintain the matrix.

**Code carries back-links.** The implementing item is annotated `// Implements: SR-007, LLR-014`; tests embed the verified ID in their name or a `// Verifies: SR-007` comment. The CSV `CodeSymbol`/`TestRefs`/`Verifies` columns are authoritative; code annotations are the reverse index that keeps code and docs honest.

**Architecture is generated.** Keep a hand-written one-page overview in `architecture.md`; generate the module/function detail (see §6) so it cannot drift.

**Where to put a fact:** same document only when the data is tightly coupled and co-owned (e.g., a requirement and its acceptance criteria — they live together in the SR row). Different documents when ownership or lifecycle differs (user needs vs. test cases). Prefer fewer, well-structured documents over many fragments.

---

## 5. Verdict & status protocol

All review output goes into [status.md](status.md). A persona review appends:

```
### <ROLE> — OBJ<n> — Round <r> — <YYYY-MM-DD>
Verdict: APPROVE | CHANGES-REQUESTED
Findings:
- [BLOCKER|MAJOR|MINOR] <ID or area> → <issue> → <suggested change> → @<owner-role>
```

Sign-offs live in the **Gate Sign-offs** table in status.md (`PENDING` | `SIGNED(date)`). The orchestrator records the round log and the gate decision there.

---

## 6. Commands the gates rely on

```bash
# Build / unit + integration tests / lint / format
scripts/run-tests.ps1            # local (Windows); mirrors CI
scripts/trace.ps1                # traceability join + orphan/coverage report -> docs/test/report.md

# Regenerate the architecture module/function map (one page):
cargo doc --no-deps              # API reference
# module + public signature map appended to docs/architecture.md by scripts/trace.ps1 (Architecture section)
```

CI (`.github/workflows/ci.yml`) runs the same checks on push/PR and uploads the coverage + traceability reports as artifacts.
