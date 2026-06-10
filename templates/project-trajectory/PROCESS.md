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

**Domain hats (scope-dependent).** The five above are the spine; choose
additional discipline hats at project setup to match the scope — e.g. **Network
Engineer**, **Security Engineer**, **Data/ML Engineer**, **Hardware/Mechanical
Engineer**, **Mechatronics Engineer**, **DBA**, **SRE/Ops**. A domain hat owns
the slice of `SR-###`/`LLR-###` rows in its area (tag them, e.g. an `Area`
column or an `SR-NET-###`-style prefix) and brings its own edge-case and
release-checklist items. Record the **active hats** for this project in
`status.md`; don't wear a hat the scope doesn't need. Like the others, a domain
hat is usually the same driver switching context — spawn a separate specialist
agent only for an independent high-risk review (§6).

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
- **Thin orchestrators**: an entry point / top-level routine should *compose, not
  compute* — a short, ordered sequence of well-named calls so that reading it is
  the high-level flow. Push logic down into the named steps. The flow is
  generated from the orchestrator (`gen_arch_map.py --flow`, see below), so a
  routine that inlines logic instead of delegating shows up as a short,
  uninformative flow — a built-in tripwire.

**Interface contracts live at the code, referenced — not restated.** Every public
module/function documents its contract once, where it is implemented, as a
structured block an agent (or human) can read inline and grep:

- *Inputs* — each parameter's type and, where it matters, its **range/enum/units**;
- *Outputs* — return type/shape;
- *Config* — config keys it reads and their constraints (+ where they live);
- *Raises/Errors* — failure modes and what they signal.

Keep it **non-duplicative by referencing IDs**: a constraint that is already a
requirement (an input range, an accepted set) lives once in the SR — its
`AcceptanceCriteria` and the `Permutations` dimensions — and the block cites the
id (`SR-012`) instead of restating the range. The block carries `Implements:
SR/LLR`, so the *intent* stays in the registry, the *implemented signature* stays
in the code, and the link is explicit. The code map harvests the symbol's summary
and back-links, so a reader can find the contract from the map in one hop. (The
exact tag syntax is the agent guide's job — see `CLAUDE.template.md` "Define the
interface (contract) at the code".)

**Generated code map — route the AST into the agent's working file.** An agent
edits faster and more safely when a *current* index of the code is in the file it
already reads, instead of re-deriving the layout each session. So the harness
generates, by parsing the source (AST), a per-module map between marker comments:

- each module's **one-line summary** (from its module docstring/header),
- its **internal dependencies** (which in-tree modules it imports) — this makes
  layering invariants auditable (e.g. "Common must not import Engine") and shows
  the blast radius of a change,
- each public symbol's **signature**, summary, and `Implements: SR/LLR` back-links.

Because the map is *harvested from docstrings and `Implements:` comments*,
commenting for humans (see the agent guide's "Comment for humans — and the map")
directly improves the map. The reference generator is `scripts/gen_arch_map.py`
(Python AST, stdlib); each stack ships its own equivalent (e.g. a PowerShell or
ts-morph version) writing into the **same marker block** — that block is the only
contract.

**Routing (where the map lands).** `gen_arch_map.py --doc` is repeatable. Put the
marker pair wherever agents read and the generator keeps it fresh:
- *Full map in `architecture.md`, the agent guide links to it* — cleanest;
  one home; the agent takes one hop. Good default for large codebases.
- *Map embedded directly in `AGENTS.md` / `CLAUDE.md`* — the agent sees it inline
  with zero hops; cost is that the guide's diff churns whenever the code changes.
  Good for small/medium codebases where the map fits on a screen.
Either way the harness regenerates it (`--check` fails the gate if stale), so it
never rots. Don't hand-maintain a code map.

**Generated high-level flow.** `gen_arch_map.py --flow <entry>` emits the ordered
internal calls of an entry/orchestrator function (each with the callee's summary)
into a `GENERATED FLOW` marker block — a generated, drift-proof rendering of the
"Thin orchestrators" rule above. Put the markers in `architecture.md` (and/or the
agent file) and add `--flow` to the harness's map step. It complements, and does
not replace, the hand-written flow overview that shows control flow.

**Diagrams are text (Mermaid); the dependency graph is generated.** Diagrams
live as ```` ```mermaid ```` fenced blocks inside the Markdown docs — rendered
natively by GitHub/GitLab/Gitea and the VS Code Markdown preview (offline-
capable), so no diagram toolchain is required and the diagram source diffs like
prose. Hand-written diagrams (the one-page flow, sequence diagrams for key
interactions) follow the same anti-duplication rule as prose: reference IDs,
don't restate requirements. The module **dependency diagram is generated**:
`gen_arch_map.py` splices a Mermaid graph of the internal imports into the
`GENERATED DEPENDENCY DIAGRAM` markers wherever a routed doc carries them
(`architecture.md` ships with the pair), covered by the same `--check` — so the
picture of the layering can't drift any more than the map can. Don't commit
exported diagram images; the text block is the source. If a project genuinely
needs diagram types beyond Mermaid (PlantUML/C4/BPMN) or has AsciiDoc sources,
wire a Kroki/PlantUML toolchain as *project* tooling — it is deliberately
outside the kit's required path.

## 4. Objectives, gates, and exit criteria

Advance only when criteria pass; **pause for human approval at each gate**.
Define machine-checkable criteria wherever possible; classify the rest honestly.

- **G1 — Requirements, UX & constraints.** UN complete (priority + measurable
  acceptance intent + edge cases); every SR links ≥1 UN with measurable
  acceptance criteria; usability/doc needs + constraints + non-goals captured.
  Sign-offs: End User, UX, System Engineer.
- **G2 — Decomposition & test coverage.** Every SR → ≥1 LLR (or
  Analysis/Inspection); every SR and LLR → ≥1 TC; traceability **0 orphans**;
  **every SR with variable inputs has its dimensions enumerated (`Permutations`)
  and a stated combination strategy, with boundary values covered** (see
  "Dimensional coverage" below); harness runs locally + CI. Sign-offs: System
  Engineer, Test Engineer.
- **G3 — Implementation.** Format/lint clean; the **full** test tier passes;
  coverage ≥ `COVERAGE_THRESHOLD`; every test-verifiable SR **Verified**; every
  other SR explicitly **Demonstration / Manual / Inspection**. Sign-offs: System
  Engineer, Test Engineer.
- **G-Release — Release readiness** *(per release; skip for a one-off
  deliverable)*. The **release** test tier passes (incl. slow/hardware tests);
  the generated **release checklist** (`scripts/gen_release_checklist.py`) is
  completed and signed; version bumped; changed `Stable` interface versions
  communicated to counterparts; docs/changelog updated. Sign-offs: Test Engineer,
  any active domain hats, Human.
- **G-Final — Acceptance.** Human/end-user exercises the real product (incl.
  Demonstration/Manual items) and approves. For shipped software this is the
  human half of G-Release; for a bespoke deliverable it stands alone.

**Constants:** `MAX_ROUNDS = 4` per gate (then escalate to the human);
`COVERAGE_THRESHOLD = 80%` line coverage (adjust by agreement; record here).

**Verification methods:** `Test` (automated) · `Demonstration` (run + observe,
e.g. a GUI or a real device) · `Manual` (human procedure) · `Analysis` ·
`Inspection`. Pick the cheapest method that actually establishes the criterion;
don't claim `Test` for something only a human can confirm. The method drives
what `trace.py` requires: only `Analysis`/`Inspection` SRs are exempt from the
LLR requirement (they have no code to decompose; `Demonstration`/`Manual` SRs
still describe implemented behavior, so they keep it), and **every SR needs ≥1
TC row regardless of method** — for human methods the TC records the procedure
(`Automated=No`, usually `Tier=Release`), which is how the release checklist
finds it.

**Test tiers (run cost vs. confidence).** Running the whole suite every iteration
gets untenable as a project grows (and CI has time/quota limits), so each
`TC-###` carries a **`Tier`**: `Smoke` (fast, run every iteration / on every
push), `Full` (the pre-merge suite, run on PRs), `Release` (slow, hardware,
manual-adjacent, or long-running — run at `G-Release`). Tiers are cumulative:
`full` includes smoke, `release` includes both. The harness selects a tier
(`check.py --tier`) via pytest markers, with a safe default: an **unmarked test
runs in `full` and `release`**, so a forgotten marker can never silently drop a
test from the pre-merge suite — `smoke` is opt-in, and marking `release` opts a
test out of pre-merge. The `Tier` column is the source of truth. Keep at least
the critical paths in `Smoke` so the cheap gate still catches regressions; the
coverage threshold is enforced at `full`/`release` only (the smoke subset alone
isn't expected to meet it).

**Dimensional coverage (exercise the input space, not just the happy path).** A
requirement with variable inputs is rarely satisfied by one example test. Treat
each variable input as a **dimension** and test deliberately, because defects
cluster in two places: at the **boundaries** of each dimension and in the
**interactions** between dimensions.

1. **Per dimension — pick the values that matter, not arbitrary ones.**
   - *Boundary-value analysis (BVA):* for any range, test the **min and the
     max**, and the **degenerate** boundaries — empty, zero, one, single-element,
     and the largest allowed. These catch off-by-one, overflow, and empty-input
     bugs. For inputs with validation, also test **just outside** each bound (the
     first invalid value) as its own — often error-path — case. These invalid
     cases assert *rejection*, not the SR's acceptance criteria, so design them
     by hand as their own TCs; `gen_cases.py` combines over the valid space only.
   - *Equivalence partitioning:* for a set of discrete modes/types, test **one
     representative per class** (classes that the code treats differently), not
     every literal value.
2. **Across dimensions — choose a combination strategy by risk and cost.** The
   full Cartesian product exercises every interaction but grows as `k**d` and
   becomes untenable; don't default to it. Decide per requirement:
   - **Full product** — when the combination count is small (rule of thumb ≤ ~12)
     **or** the interaction is high-risk (data loss, corruption, security, money)
     *and* each case is cheap.
   - **Pairwise (all-pairs)** — the default for ≥3 dimensions: cover every pair of
     values across every pair of dimensions at least once. Empirically catches the
     large majority of interaction defects for a small fraction of the cases.
   - **Boundary-corners** — when even pairwise is too costly or each run is
     expensive (hardware / integration): all-low, all-high, and each dimension
     flipped to its other extreme (single-factor sweeps that localize the failing
     dimension).
3. **Balance against time/complexity via the tiers.** Cheap pure-core
   combinations (unit level) can afford full/pairwise and live in `Smoke`/`Full`;
   expensive integration/hardware combinations use boundary-corners and live in
   `Release`. Don't run a 4-mode × N-size sweep on every push — push the heavy
   combinations to the release tier and keep a boundary slice in smoke.

Record each requirement's dimensions in the SR **`Permutations`** column using
this grammar, so one SR stands in for many near-duplicate rows and the intent is
machine-readable:

```
field=set{plain,comma,quote,newline}; size=range[0..2GiB]; enc=set{utf8,utf16}; @pairwise
```

`scripts/gen_cases.py` reads exactly that grammar and emits the derived value sets
and the chosen combinations (and shows the reduction vs. the full product) — copy
its output into `Parameters` cells / parametrized tests. The generated cases are
the source; do not hand-curate combinations the generator should produce.

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

Ready reference scripts ship with this template (Python 3.8+, stdlib only — no
pip needed to run them):

- `scripts/check.py` — the harness itself. Gate-scoped (`--gate G2|G3|all`), runs
  format · lint · tests · coverage · traceability · arch-map freshness, and exits
  nonzero on any failure. Wire it to your stack by editing its `STEPS` table; the
  contract is the gates + exit code, not the specific tools. CI runs the same
  command (`ci/check.yml`).
- `scripts/trace.py` — joins the registries, writes `docs/test/report.md`, exits
  nonzero on orphans with `--strict`; `--require-verified` adds the G3 status
  criterion (every `Verification=Test` SR must be `Verified`). Called by
  `check.py` at G2/G3 (the G3 run adds `--require-verified`).
- `scripts/gen_arch_map.py` — regenerates the module/function map in
  `architecture.md` from the source tree (and surfaces `Implements:` back-links),
  plus the Mermaid **dependency diagram** between its markers; `--check` fails
  when the doc is stale, so neither can drift.
- `scripts/gen_release_checklist.py` — generates the human **release checklist**
  for `G-Release` from the registries: every Demonstration/Manual/Inspection SR,
  every Release-tier/manual TC, the UN acceptance intents, and provided
  interfaces — each a tick-box back-linked to its id. Keep the completed copy as
  the sign-off record.
- `scripts/gen_cases.py` — expands an SR's `Permutations` (input dimensions) into
  boundary-aware test combinations by strategy (full / pairwise / boundaries),
  and reports the reduction vs. the full product (see "Dimensional coverage" in
  §4). Use it at G2 to design test cases that exercise the input space.

**Cross-platform launchers** (so a fresh clone is trivial to run on any OS):
`scripts/setup.{sh,ps1}` create a venv and install the toolchain;
`scripts/check.{sh,ps1}` are thin wrappers that forward to `check.py`. Provide
the pair for every platform the project supports.

`scripts/bootstrap.py` scaffolds all of the above (plus `docs/` and CI) into a new
repo in one command. See `EXAMPLE.md` for a complete worked UN→SR→LLR→TC chain.

## 8. Cross-project interfaces (only when projects interlink)

When this project provides or consumes a contract shared with another repo,
record each shared surface once in `requirements/interfaces.csv` as an `IF-###`
(see `INTERFACES.template.md`): direction, counterpart, contract, the `SR-Refs`
that realize/rely on it, version, and stability. The owning (`Provides`) side
holds the authoritative spec; the consuming side links the same `IF-###` and
pins the version. Every interface is backed by an SR and a contract/fixture test.
This keeps interlinked projects from silently drifting apart without imposing a
multi-repo build system. Standalone projects skip this section.
