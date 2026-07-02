# Development Process

Canonical method for a gated, requirement-traced project. Stack-agnostic core;
see §7 for this repo's stack-specific wiring. Other docs reference this file by
section rather than restating it.

**This repo's harness commands (§7 wired to Rust):**
- Product gate: `pwsh Scripts/run-tests.ps1` (fmt, clippy, tests, coverage ≥80%, trace)
- Process checks: `python Scripts/check.py` (traceability, doc-links, arch-map; Python 3.8+)
- Traceability only: `pwsh Scripts/trace.ps1 [-Strict]`

Back to [README](../README.md) · docs index: [docs/README.md](README.md).

**Read this file top to bottom for the load-bearing core** (§1–§7): roles, ids,
the §3 discipline, the gates, the verdict protocol, the harness contract. The
opt-in layers — phased delivery, lifecycle tags, cross-project interfaces (§8),
NFR/perf budgets (§9), the multi-repo scale ladder (§10), and the §7 boundary
notes — are summarized here with an **applies-when** and expanded in
[`process-options.md`](process-options.md); skip any that your scope doesn't hit.

**Minimum profile — a standalone one-module project needs exactly:** the five
spine hats (§1), the id scheme (§2), the §3 traceability/anti-duplication
discipline, gates **G1→G2→G3** + G-Final (§4), the verdict protocol (§5),
review triage (§6), and the harness (§7). Everything else is opt-in: skip §8,
§9, §10, the `Phase`/`Lifecycle` tags, and every "optional" tripwire until the
scope forces it. That default is rung 1 of the §10 ladder.

---

## 1. Roles (hats), not necessarily separate agents

One driver wears these hats in sequence, keeping context. Spawn a *separate*
agent only for an independent pre-gate review (see §6).

| Hat | Owns (single source of truth) |
|---|---|
| Stakeholder | `requirements/stakeholder-needs.md` (SN-###) + edge-case expectations |
| UX / Docs | documentation quality, quick-reference, usability findings |
| System Engineer | `requirements/system-requirements.csv` (SR-###); **gatekeeper** |
| Software Engineer | `requirements/low-level-requirements.csv` (LLR-###) + code + `architecture.md` |
| Test Engineer | `test/test-cases.csv` (TC-###) + the check harness + coverage/trace reports |

A hat only edits artifacts it owns; to change another, file a finding addressed
to its owner (§5).

**Domain hats (scope-dependent).** The five above are the spine; add discipline
hats at setup to match the scope — e.g. **Network**, **Security**, **Data/ML**,
**Hardware/Mechanical**, **Mechatronics**, **DBA**, **SRE/Ops**, and an
**Integration/Coordination** hat that allocates cross-module budgets
(`performance-budgets.csv`, §9). A domain hat owns the `SR`/`LLR` rows in its
area (tag them, e.g. an `Area` column or `SR-NET-###` prefix) and brings its own
edge-case and release-checklist items. Record the **active hats** in
`status.md`; don't wear a hat the scope doesn't need. Like the others, it is
usually the same driver switching context — spawn a separate specialist agent
only for an independent high-risk review (§6).

## 2. Identifier scheme

| Prefix | Level | Parent link |
|---|---|---|
| `SN-###` | Stakeholder Need | — |
| `SR-###` | System Requirement | `SN-Refs` |
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

**Right-sizing has guardrails — and a name for the calibrated shortcut.**
"Simplest thing that works" (the agent guide's "Right-size the solution") is
calibrated, not flimsy: it never trims **validation at trust boundaries**, error
handling that would **lose or corrupt data**, **security**, **accessibility**, or
understanding the problem (root cause, not symptom) before fixing it. Where a
deliberate simplification is still right, mark it inline with a **`SHORTCUT:`**
comment naming the **ceiling** it accepts (a global lock, an O(n²) scan, a naive
heuristic) and the **upgrade path** past it — so it is greppable, reviewable, and
never mistaken for the final design. One tag, defined once; not a taxonomy.

**Reviewability — review the source, not the render.** The registries (the
`SN`/`SR`/`LLR`/`TC` CSVs) are the tracked, line-by-line-reviewable source of
truth; every other view is *generated* from them. Generated output splits by size:

- **Small, diff-meaningful blocks** live in tracked files behind `GENERATED`
  markers, kept honest by a freshness gate — the code map, dependency diagram,
  and program flow (`gen_arch_map.py --check` fails a commit that left them
  stale). These you *do* read in diffs.
- **Large composite artifacts** — the full trace report (`test/report.md`: counts,
  matrix, the `SN→SR→LLR→TC` text outline, and the Mermaid graph), the HTML map
  (`trace.py --html`), and the perf report (`test/perf-report.md`, §9) — are
  regenerated every run, **gitignored**, and published by CI as artifacts. Don't
  diff these; review the registry change that produced them.
- **Committed goldens** — a small generated file you *do* commit and review as the
  record of an accepted change: the perf baseline (`test/perf-baseline.json`,
  §9). Moving a number means committing the new golden in the same PR — explicit,
  never silent.

This is the "composite artifacts are ignored from change tracking" rule, named:
the cost of reviewing a big regenerated file is never paid, because the small
registry diff already carries the intent.

**The doc set must stay navigable (the doc map stays honest like the code map).**
The freshness gate above keeps *generated* blocks honest; the hand-written docs
get the same guarantee. `scripts/check_docs.py` (stdlib, a process check — §7)
parses the Markdown under `docs/` plus root `*.md`, builds the link graph, and
**fails on broken intra-repo links** (a missing target file or `#anchor`) — the
machine version of the "verify no broken intra-doc links" step the gates ask a
human to do. It also **warns on orphan docs** (no path from an entry root —
root `*.md`, an optional `docs/index.md` Map-of-Content, or a configured entry)
and, with `--stale` (git-gated), on a doc left frozen beside a non-doc file it
links that has changed. Broken links are a hard finding; orphans/staleness are
warnings (a young project legitimately has standalone docs). Run by `check.py`
from G1 on.

**Interface contracts live at the code, referenced — not restated.** Every public
module/function documents its contract once, where it is implemented, as a
structured block a reader can grep inline: *Inputs* (each parameter's type and,
where it matters, its **range/enum/units**), *Outputs* (return type/shape),
*Config* (keys it reads + constraints + where they live), *Raises/Errors*
(failure modes and what they signal). Keep it **non-duplicative by referencing
IDs**: a constraint already captured as a requirement (an input range, an
accepted set) lives once in the SR (its `AcceptanceCriteria` and `Permutations`)
and the block cites the id (`SR-012`) instead of restating it. The block carries
`Implements: SR/LLR`, so intent stays in the registry, the implemented signature
stays in code, and the link is explicit; the code map harvests the summary and
back-links so a reader finds the contract in one hop. (Exact tag syntax is the
agent guide's job — `AGENTS.template.md` "Define the interface (contract) at the
code".)

**Generated code map — route the AST into the agent's working file.** An agent
edits faster and more safely with a *current* index of the code in the file it
already reads, instead of re-deriving the layout each session. So the harness
parses the source (AST) and generates, between marker comments, a per-module map:
each module's **one-line summary** (from its docstring/header); its **internal
dependencies** (which in-tree modules it imports) — making layering invariants
auditable (e.g. "Common must not import Engine") and showing a change's blast
radius; and each public symbol's **signature**, summary, and `Implements:
SR/LLR` back-links. Because it is harvested from docstrings and `Implements:`
comments, commenting for humans (agent guide's "Comment for humans — and the
map") directly improves the map. Reference generator: `scripts/gen_arch_map.py`
(Python AST, stdlib); each stack ships its own equivalent (a PowerShell or
ts-morph version) writing into the **same marker block** — that block is the only
contract.

**Routing (where the map lands).** `gen_arch_map.py --doc` is repeatable; put the
marker pair wherever agents read and the generator keeps it fresh — *full map in
`architecture.md`, the agent guide links to it* (one home, one hop; the default
for large codebases) or *embedded directly in `AGENTS.md`/`CLAUDE.md`* (zero
hops, but the guide's diff churns with the code; good when the map fits on a
screen). Either way `--check` fails the gate if stale, so it never rots. Don't
hand-maintain a code map.

**The committed map is a contract, not a search index.** `gen_arch_map.py`
produces a **committed, diff-reviewable, drift-gated** artifact — part of the
source of truth, read to learn the code's *intended* shape. Query-time
**semantic-retrieval tools** (LSP-backed code-graph servers, Serena-style MCP
indexes) are a *different* thing: not committed, language-server-dependent,
rebuilt on demand. They are a legitimate **optional downstream accelerator** for
chasing references across a large repo, but they **don't replace** the committed
map and the kit must **not** hard-wire one (it would break stdlib-only and add a
server/LSP dependency). Use one if it helps; keep it off the required path.

**Generated high-level flow.** `gen_arch_map.py --flow <entry>` emits an
entry/orchestrator function's ordered internal calls (each with the callee's
summary) into a `GENERATED FLOW` marker block — a drift-proof rendering of the
"Thin orchestrators" rule. Put the markers in `architecture.md` (and/or the agent
file) and add `--flow` to the harness's map step. It complements, not replaces,
the hand-written flow overview.

**Design-time runtime flows (authored at G2, checked).** Everything above is
harvested from code, so none of it exists at G2 — yet G2 is when a human reviews
the LLRs, and runtime *behavior* (ordering, concurrency, background work, what
blocks on what) is the thing most easily misread from CSV rows. So the Software
Engineer hat authors a **"Runtime flows"** section in `architecture.md` **with
the LLRs, before the G2 review**: one Mermaid `sequenceDiagram` per key
user-visible scenario, and always one for any concurrent / asynchronous /
non-blocking behavior. Participants are the planned modules (the LLR `Module`
column); each diagram cites the SR/LLR ids it renders. `scripts/check_flows.py`
(G2/G3) fails when the section is missing, has no diagrams, a diagram cites no
SR/LLR id, or a cited id doesn't exist. The human's G2 review starts from these
diagrams — verify the flow there, then spot-check the rows. Update a flow in the
same change that alters its LLRs; from G3 on the generated map/flow corroborates
these authored diagrams rather than replacing them.

**Diagrams are text (Mermaid); the dependency graph is generated.** Diagrams live
as ```` ```mermaid ```` fenced blocks inside the Markdown — rendered natively by
GitHub/GitLab/Gitea and the VS Code preview (offline-capable), so no diagram
toolchain is required and the source diffs like prose. Hand-written diagrams (the
one-page flow, sequence diagrams) follow the same anti-duplication rule: reference
IDs, don't restate requirements. The module **dependency diagram is generated** —
`gen_arch_map.py` splices a Mermaid graph of the internal imports into the
`GENERATED DEPENDENCY DIAGRAM` markers wherever a routed doc carries them
(`architecture.md` ships the pair), covered by the same `--check`, so the layering
picture can't drift. Don't commit exported diagram images; the text block is the
source. A project that genuinely outgrows Mermaid (PlantUML/C4/BPMN, AsciiDoc
sources) wires a Kroki/PlantUML toolchain as *project* tooling — deliberately
outside the kit's required path.

## 4. Objectives, gates, and exit criteria

Advance only when criteria pass; **pause for human approval at each gate**.
The active gate is recorded machine-readably in the one-line `docs/gate` file;
closing a gate bumps it in a reviewed commit (§7 "The active gate").
Define machine-checkable criteria wherever possible; classify the rest honestly.

- **G1 — Requirements, UX & constraints.** SN complete (priority + measurable
  acceptance intent + edge cases); every SR links ≥1 SN with measurable
  acceptance criteria; usability/doc needs + constraints + non-goals captured.
  Sign-offs: Stakeholder, UX, System Engineer.
- **G2 — Decomposition & test coverage.** Every SR → ≥1 LLR (or
  Analysis/Inspection); every SR and LLR → ≥1 TC; traceability **0 orphans** and
  ids unique/well-formed; **no `-000` placeholder rows or flow citations remain**
  (`trace.py`/`check_flows.py --no-placeholders`); **every SR with variable
  inputs has its dimensions enumerated (`Permutations`) and a stated combination
  strategy, with boundary values covered** (see "Dimensional coverage" below);
  **key runtime flows are diagrammed and pass `check_flows.py`** (see §3
  "Design-time runtime flows"); harness runs locally + CI. Sign-offs: System
  Engineer, Test Engineer.
- **G3 — Implementation (test-first).** Code is written **test-first**: each G2
  TC becomes a *failing* test before the code that satisfies it, then the minimal
  code to pass, then refactor (red → green → refactor). TDD is *how* G3 code gets
  written; the SN→SR→LLR→TC spine is *what* it must satisfy — it operates within
  the traceability discipline, not instead of it. The exit criteria below
  (coverage, every in-scope SR Verified) are what that loop drives toward.
  Format/lint clean; every source module parses
  (`gen_arch_map.py --strict-parse`); the **full** test tier passes; coverage ≥
  `COVERAGE_THRESHOLD`; registry **schema** holds (required fields non-empty,
  `Verification`/`Tier` in vocabulary — `trace.py --strict-schema`); every
  **in-scope** test-verifiable SR **Verified** (phase-scoped — see "Phased
  delivery" below); every other SR explicitly **Demonstration / Manual /
  Inspection**; each in-scope SR's implementing symbol is **substantive, not a
  stub** (Inspection — see "No-stub / substance review" below). Sign-offs: System
  Engineer, Test Engineer.
- **G-Release — Release readiness** *(per release; skip for a one-off
  deliverable)*. The **release** test tier passes (incl. slow/hardware tests);
  the generated **release checklist** (`scripts/gen_release_checklist.py`) is
  completed and signed; version bumped; changed `Stable` interface versions
  communicated to counterparts; docs/changelog updated. Sign-offs: Test Engineer,
  any active domain hats, Human.
- **G-Final — Acceptance.** Human/stakeholder exercises the real product (incl.
  Demonstration/Manual items) and approves. For shipped software this is the
  human half of G-Release; for a bespoke deliverable it stands alone.

**Consistency review (G1; re-checked at G2).** Separate from the *structural*
checks `trace.py` runs — orphans, duplicate ids, schema — the **System Engineer**
hat reads the needs and requirements **against each other** for the conflicts a
script can't see: contradictory acceptance criteria or limits, mutually exclusive
behaviors, duplicate or overlapping requirements, ambiguous / underspecified
needs, and overlapping `Area`/hat ownership. This is the **consistency**
complement to G1's *completeness* criteria, not a restatement of them, and it is
**human/LLM judgment, not a machine check** — classify it as a Manual/Analysis
activity and never imply `trace.py` performs it. (An independent LLM reviewer
(§6) is well-suited to a first-pass contradiction sweep, but the **human makes the
call**.) Route each contradiction or ambiguity through the §5 findings protocol to
its owner; where it needs a human decision, **pause and ask — don't guess**. This
is the reachable-human flip side of *Assumptions* logging: record an assumption
only when **unattended**; when a human is available, **solicit clarification**.
Track unresolved ambiguities in `status.md` *Open items*, and re-run the review at
G2 when SRs decompose into LLRs.

**No-stub / substance review (G3).** Traceability, coverage, and a green suite
confirm an implementation *exists* and *passes*; none confirms it has
**substance**. A body that is `pass` / `...` / `raise NotImplementedError` / a
bare `return None` / a placeholder return satisfies its trace links and can even
hold a coverage line, yet does nothing. So the G3 criterion adds: **every in-scope
SR's implementing symbol does real work, not a stub.** TDD mitigates this (a
red-first test should fail against a stub), but coverage can be met by exercising
a stub's trivial path, and Demonstration/Manual/Analysis SRs have **no** automated
test to fail — so name it. It is **Inspection** (human/LLM judgment, **never a
machine verdict**): fold the prompt into §6's independent-reviewer checklist — a
fresh-context reviewer reads the §3 code map (which harvests each symbol's summary
and `Implements:` back-links) and confirms the body matches the requirement. The
kit ships an **optional, Python-reference tripwire**, `scripts/check_stubs.py`
(§7), listing trivial-bodied public symbols; like the perf *meters* it is
**product-layer and warn-first** (a stub's shape is language-specific; a tiny
pure function is not an unfinished one), so it informs the Inspection, not
replaces it. Same stance as `ruff`/`pytest`: name the criterion; the project wires
the tool.

**Phased delivery (version subsets) — opt-in.** *Applies when* a roadmap ships
v1 before v2/v3. SRs carry an optional **`Phase`** tag; traceability stays
phase-blind while the G3 Verified criterion and G-Release scope by phase
(`check.py --gate G3 --phase v1`), reporting out-of-phase SRs as explicitly
**phase-deferred**. Full semantics in
[`process-options.md`](process-options.md#phased-delivery); standalone single-shot
deliverables skip it.

**Lifecycle phase (when in the product's life a requirement holds) — opt-in.**
*Applies when* install/startup requirements are easy to miss (most non-trivial
products). Distinct from the delivery `Phase` above, an optional **`Lifecycle`**
tag (mirroring `Area`; blank = **Runtime**) records *at what point in the running
product's lifetime must this hold, and how often?* — default vocabulary
**Provision** (ready) · **Startup** (set) · **Runtime** (go), an open,
project-named set. Naming it stops the perennial miss of writing only
steady-state requirements. Full vocabulary, the "discriminate by when/how-often"
rule, and the config-straddles-Provision↔Startup guidance are in
[`process-options.md`](process-options.md#lifecycle-phase).

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
each variable input as a **dimension** and test deliberately: defects cluster at
the **boundaries** of each dimension and in the **interactions** between them.

1. **Per dimension — pick the values that matter.**
   - *Boundary-value analysis:* for any range, test **min and max** and the
     **degenerate** boundaries — empty, zero, one, single-element, largest allowed
     (catches off-by-one, overflow, empty-input bugs). For validated inputs, also
     test **just outside** each bound (the first invalid value) as its own,
     usually error-path, case — these assert *rejection*, not the SR's acceptance
     criteria, so hand-design them as their own TCs; `gen_cases.py` combines over
     the valid space only.
   - *Equivalence partitioning:* for discrete modes/types, test **one
     representative per class the code treats differently**, not every literal.
2. **Across dimensions — choose a combination strategy by risk and cost.** The
   full Cartesian product grows as `k**d` and becomes untenable; don't default to
   it. Per requirement:
   - **Full product** — combination count small (≤ ~12) **or** the interaction is
     high-risk (data loss, corruption, security, money) *and* each case is cheap.
   - **Pairwise (all-pairs)** — the default for ≥3 dimensions: cover every value
     pair across every dimension pair at least once. Catches the large majority of
     interaction defects for a fraction of the cases.
   - **Boundary-corners** — when even pairwise is too costly or each run is
     expensive (hardware / integration): all-low, all-high, and each dimension
     flipped to its other extreme (single-factor sweeps that localize the failure).
3. **Balance via the tiers.** Cheap pure-core combinations afford full/pairwise in
   `Smoke`/`Full`; expensive integration/hardware combinations use
   boundary-corners in `Release`. Don't run a 4-mode × N-size sweep on every push.

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

**Voice policy — warmth has a layer boundary.** Personality is a human value, not
a machine one: **human-facing** output (CLI narration, a kickoff greeting, a
release-checklist intro) may carry warmth and **dry wit at most**;
**machine/agent-facing** output — this protocol's findings and verdicts,
subagent prompts, registry cells, commit messages — stays **literal, terse,
structured: no whimsy**. Levity there costs tokens, reads ambiguously to the
next agent (irony/understatement is exactly what a parser misreads), and
erodes the honesty/severity signal this protocol depends on. Default voice is
**restrained**
("direct and concrete; dry wit at most; never at the expense of clarity or
honesty"); a project may expose an optional, named **tone knob** to dial levity
up or down — never a baked-in persona, since no single tone fits both a
medical-device repo and a game studio.

## 6. Review-depth triage (efficiency)

- **High-risk** (security, data loss, crash-safety, money, irreversible, gate
  closure): spawn an **independent** reviewer with a fresh-context, defect-
  hunting prompt. Verify its file edits; never trust an unverified "green."
- **Medium**: self-review against the gate checklist + run the harness.
- **Low/mechanical** (rename, doc tweak, config): just run the harness.

Keep the status file's *Current State / Open Items* header short so a reviewer
can orient cheaply; the full log lives below and need not be re-read each pass.

**Model/agent tiering — recommend + record, not enforce.** The risk triage above
is also a **tiering** axis: planning, decomposition, decisions, and high-risk
review need a **strong model**; mechanical execution, well-specced builds, and
low-risk/prose work tolerate a **cheaper tier**. Tiering down is **safe
specifically because of the gates** above — the harness + tests mean a cheaper
executor can't silently drift past a check, a guarantee an ungated workflow can't
make. The kit **cannot force** a model choice (a fast-moving, host-specific
concern); it offers a **recorded-tier-hint** convention instead: any planned
unit of work (a thread, a phase, a `status.md` task) may carry a **model-tier
hint** — metadata an agent reads and may act on, guidance like any other
`AGENTS.md` directive, not a guarantee. Host-specific levers (e.g. a
strong-model-plans/cheaper-model-executes mode, per-subagent model overrides,
a model-selection command) are optional, documented per-host examples — name
the pattern, never a vendor-specific model-selection engine.

## 7. Harness contract (wire to your stack)

`scripts/check` (and the CI workflow) must run, and fail nonzero on any failure:
format check · linter (warnings as errors) · unit + integration tests · coverage
(≥ threshold) · the traceability check (0 orphans for the active gate). Emit the
coverage + traceability reports as artifacts. Prefer a generated architecture
map step so `architecture.md` stays current.

**The active gate is recorded, and CI reads it.** The current gate lives in the
one-line `docs/gate` file (bootstrap starts it at `G1`). `check.py` defaults
`--gate` to it and the reference CI passes no explicit gate, so **CI enforces the
bar the project is actually at** — a fresh G1 scaffold is green, and the bar rises
when the human closes a gate by bumping `docs/gate` in a reviewed commit (the same
explicit-diff discipline as the perf baseline). A release tag runs the full bar
regardless. Without this, CI would apply the end-state G3 bar from day one and
stay red for months — training everyone to ignore it.

**Two check layers — process vs. product.** The harness runs two kinds of check,
and naming the split is what keeps the kit portable across stacks:

- **Process checks are kit-owned and stdlib-only** (`requires=()` in `check.py`):
  traceability (`trace.py`), design-flow validation (`check_flows.py`),
  doc navigability (`check_docs.py`), perf-budget comparison (`check_perf.py`), and
  architecture-map freshness (`gen_arch_map.py`). They are identical in every
  project and every language — **don't rewrite them.** (The perf *comparator* is
  process; the *measurement* that feeds it is product — see §9.) The
  agent-neutral `pre-commit` hook (`.githooks/pre-commit`, enabled by
  `scripts/setup.{sh,ps1}`) enforces their **always-valid subset** on every
  commit: map freshness, id integrity (`trace.py --strict-integrity`), and
  format. Orphan strictness stays gate-scoped in `check.py` — a mid-G1 registry
  legitimately has SRs not yet decomposed, and the floor must never block a
  legitimate early-stage commit.
- **Product checks are project-owned and language-specific** (`requires` names a
  tool — `ruff`/`pytest` in the Python reference): format, lint, and
  tests+coverage. **You wire these to your stack** in `check.py`'s "EDIT FOR YOUR
  STACK" block; a non-Python project swaps the commands or drops a step it lacks.

The empty-vs-named `requires` tuple already implies which layer a step is in;
`check.py --list` makes it explicit, tagging each step `[process]`/`[product]` so
a newcomer sees at a glance which steps are fixed and which they must localize.

**A third toolchain layer — the developer workstation.** The two layers above
cover what the *project* needs to pass its gates; a third, often-conflated
concern is what a **human** needs to view, render, edit, and run any of it: a
language/runtime, `git`, an **offline** Markdown+Mermaid renderer (VS Code's
preview or `@mermaid-js/mermaid-cli`), optionally an IDE or a domain viewer
(CAD/image/publication). "No required tools" was always a claim about the
**process** layer (stdlib only); it never meant a human needs nothing.

**The onboarding ladder — Provision-for-development.** A fresh contributor's path
to a running checkout mirrors the §4 lifecycle phases one level up: `Stage 0`
(get git + repo, pre-clone) → `dev-setup` (workstation, post-clone) → `setup`
(product deps, per clone/CI) → `check` (run gates). `Stage 0`/`dev-setup`
provision the developer workstation (rare, per contributor); `setup` provisions
the product toolchain; `check` is the process floor. Each rung is an optional,
readable, **consent-first** helper — never a silent or compiled installer — so
even a non-code contributor can reach an editable checkout without prior git
literacy. Details and the full rationale for these three §7 boundary notes
(developer-workstation · onboarding ladder · offline-render) are in
[`process-options.md`](process-options.md#7-boundary-notes).

**Offline-render principle.** Legibility artifacts (Mermaid diagrams, the trace
HTML map, the code map) must render with **local, offline** tooling — never a
cloud service (the reason the kit chose Mermaid-in-Markdown, §3). Reach for a
Kroki/PlantUML *container* only if a project outgrows Mermaid.

**Three more boundary notes (opt-in reading — [`process-options.md`](process-options.md#7-boundary-notes)):**
**the kit generates legibility, it does not score it** (measuring AI-readiness
over time is an *external readiness assessor*, optional downstream tooling — the
`ruff`/`pytest` stance: name the gate, the project picks the tool); **the kit
is a spec, not a turnkey agent-runtime harness** (an `npx`-installed engine
shipping skills/agents/hooks/MCP for one tool is a different, optional product
that *composes* with a scaffolded repo but neither depends on the other); and
**repo text is the durable agent memory layer** — the committed artifacts
(`status.md`, registries, `AGENTS.md`, the code map) are the agent-neutral,
reviewable memory; agent-native memory tools (auto-memory, MCP memory servers)
are *scratch*, never the home for load-bearing facts. Durable facts — a
decision, constraint, or gotcha — are **promoted** into `status.md` or the
registries. The kit does not install or require any agent-memory tooling; a
larger repo simply makes the committed layer matter more.

Ready reference scripts ship with this template (Python 3.8+, stdlib only — no
pip needed to run them):

- `scripts/check.py` — the harness itself. Gate-scoped (`--gate G1|G2|G3|all`,
  defaulting to the `docs/gate` active gate), runs
  format · lint · tests · coverage · traceability · arch-map freshness, and exits
  nonzero on any failure. Wire it to your stack by editing the step list its
  `steps()` function returns (and the `SRC`/`TESTS`/tool names in the "EDIT FOR
  YOUR STACK" block at the top); the contract is the gates + exit code, not the
  specific tools. CI runs the same command (`ci/check.yml`).
- `scripts/trace.py` — joins the registries, writes `docs/test/report.md` (counts,
  the SR→LLR→TC matrix, a line-reviewable `SN→SR→LLR→TC` **text outline**, and a
  small **Mermaid `graph LR`** colored by orphan/draft state), and exits nonzero
  on orphans with `--strict`. `--html` also writes a dependency-free collapsible
  `docs/test/report.html` map that scales to any size (a gitignored composite —
  §3). It always checks **integrity** (duplicate/malformed ids); `--strict-integrity`
  fails on *only* that class (the always-valid pre-commit floor).
  `--require-verified` adds the G3 status criterion (every `Verification=Test` SR
  must be `Verified`); `--phase v1` scopes it for phased delivery (§4).
  `--no-placeholders` rejects leftover `-000` rows; `--strict-schema` requires the
  non-empty fields and the two closed vocabularies (`Verification`, `Tier`) —
  `Priority`/`Status` stay open. Called by `check.py` at G2/G3 (G2+ adds
  `--no-placeholders`; G3 adds `--require-verified` and `--strict-schema`, plus
  `--phase` when given).
- `scripts/check_flows.py` — verifies the authored **"Runtime flows"** section
  (§3 "Design-time runtime flows"): present, ≥1 Mermaid diagram, every cited
  SR/LLR id real. Run by `check.py` at G2/G3.
- `scripts/check_docs.py` — **doc navigability** (§3 "The doc set must stay
  navigable"): parses the docs' link graph and fails on broken intra-repo links
  (missing file or `#anchor`), warns on orphan docs (and, with `--stale`,
  git-gated freshness). Stdlib-only; run by `check.py` from G1 on.
- `scripts/check_perf.py` — the **perf-budget comparator** (§9): compares the
  product-emitted `perf-metrics.json` against `performance-budgets.csv` and the
  committed `perf-baseline.json` — absolute breach (vs `Budget`) and regression
  (vs baseline ± `Tolerance`), warn-vs-fail per the row's `Gate`, tier-scoped —
  and writes the gitignored `perf-report.md`. `--update-baseline` accepts a move.
  Stdlib-only, metric-agnostic; run by `check.py` at G3 (absent metrics skip).
- `scripts/check_stubs.py` — the **no-stub / substance** tripwire (§4 G3): lists
  public symbols whose body is a stub (`pass` / `...` / `raise NotImplementedError`
  / bare `return None` / docstring-only), writing the gitignored `stub-report.md`.
  Stdlib, but **product-layer, not process** — a stub's shape is language-specific,
  so it ships like the perf *meters*: **opt-in and warn-first** (exit 0 unless
  `--strict`), **not** wired into `check.py`'s required floor. A Python project runs
  it to inform the G3 Inspection; a non-Python stack swaps or drops it.
- `scripts/gen_arch_map.py` — regenerates the module/function map in
  `architecture.md` from the source tree (and surfaces `Implements:` back-links),
  plus the Mermaid **dependency diagram** between its markers; `--check` fails
  when the doc is stale, so neither can drift. `--strict-parse` additionally
  fails on any module that won't parse (the G3 run passes it).
- `scripts/gen_release_checklist.py` — generates the human **release checklist**
  for `G-Release` from the registries: every Demonstration/Manual/Inspection SR,
  every Release-tier/manual TC, the SN acceptance intents, and provided
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
repo in one command. See `EXAMPLE.md` for a complete worked SN→SR→LLR→TC chain.

## 8. Cross-project interfaces (only when projects interlink)

When this project provides or consumes a contract shared with another repo,
record each shared surface once in `requirements/interfaces.csv` as an `IF-###`
(see `INTERFACES.template.md`): direction, counterpart, contract, the `SR-Refs`
that realize/rely on it, version, and stability. The owning (`Provides`) side
holds the authoritative spec; the consuming side links the same `IF-###` and
pins the version. Every interface is backed by an SR and a contract/fixture test.
This keeps interlinked projects from silently drifting apart without imposing a
multi-repo build system. Standalone projects skip this section.

A **purchased/external part** the product buys rather than builds (a motor, board,
camera) is owned the same way — a repo/coordinator-held `IF-###` is its
owner-of-record (MULTI_REPO.md §3.3) — with acquisition facts (vendor, cost,
status, quantity) in the optional `requirements/procurement.csv` (`PART-###`).
Minimal by design; full BOM tracking is deferred. See
[`process-options.md`](process-options.md#8-purchased-parts).

## 9. Non-functional requirements & performance budgets *(opt-in)*

*Applies when* the product has resource, performance, or other quality costs
worth pinning (RAM/VRAM, latency, artifact size, security, reliability, …).
Standalone projects with no such concerns skip this section, exactly like §8.

The `SN→SR→LLR→TC` spine verifies **behavior**, never on its own the **cost** of
that behavior. NFRs are expressible as ordinary SRs, but nothing makes you
*consider* them, and quantitative budgets often aren't the author's to invent (a
module is *handed* a slice of a system-level budget by an integrator; most metrics
should be **minimized within reason**, not guessed at). At G1, run the
**consideration checklist** — a prompt, not a mandate, anchored on **ISO/IEC
25010** plus cost/economics — and **route each NFR to one of three homes:**

1. *Allocation / coordination* NFRs (perf budgets, capacity, availability) → the
   **`performance-budgets.csv`** registry below.
2. *Behavioral* NFRs (security, observability, safety, data integrity) → ordinary
   **SRs** with measurable `AcceptanceCriteria` + honest `Verification`, owned by
   a domain hat.
3. *Hard external limits* (compliance, supported platforms) → `status.md`
   constraints.

The full 25010-anchored checklist and the "don't double-prompt what the kit
already covers" list are in
[`process-options.md`](process-options.md#9-nfr-checklist).

**The performance-budgets registry (`requirements/performance-budgets.csv`,
`PB-###`).** Quantitative budgets live **separate from the spine** (like `IF-###`,
§8) so `SN→SR→LLR` stays functional-focused and an **Integration/Coordination**
hat (§1) can (re)allocate them without churning the breakdown. **Separation is not
disconnection:** every row **back-links** the SR/LLR/Module it bounds (its
`Refs`), and `trace.py` flags a row whose `Refs` name an unknown id or whose `PB-`
id is malformed. Columns: `PB-ID, Metric, Refs, Budget, Unit, Tolerance,
Direction (lower-better | higher-better), Tier, Gate (fail | warn), Owner, Notes`.

**The comparator (`scripts/check_perf.py`).** A budget is inert until something
compares the *measured* number against it — **absolute** (measured vs `Budget`,
per `Direction`) and **regression** (measured vs a committed baseline outside the
`Tolerance` band). Split along the §7 process/product line: *measuring* is
**product** work the project wires (`/usr/bin/time`, `tracemalloc`, `nvidia-smi`,
a size command), emitting `docs/test/perf-metrics.json` (`PB-ID → number`);
*comparing* is **process** work the kit owns (`check_perf.py`, stdlib, metric-
agnostic). Three artifacts map to the §3 reviewability classes:
`performance-budgets.csv` (tracked truth), `perf-baseline.json` (committed golden
— accepting a regression = committing a new baseline in the same PR,
`--update-baseline`), `perf-report.md` (gitignored composite). Warn-first: gate
low-noise deterministic metrics (size, dep count) at `full`; default noisy runtime
metrics (latency, RAM, VRAM) to `Gate=warn` at `release`; absent metrics never
fail. Full guidance in
[`process-options.md`](process-options.md#9-perf-comparator).

## 10. Project scale — one module, several modules, several repos *(opt-in past rung 1)*

*Applies when* the scope outgrows one module. Everything above (§1–§9) assumes the
common case, **one module in one repo** — the default and rung 1. Scale is an
**escalation ladder**: climb a rung only when the scope forces it, decide the rung
**at project creation**, and bias to the lowest, because each higher rung buys
coordination cost a single module never pays.

1. **One module, one repo** — the default; the whole `SN→SR→LLR→TC` spine, one
   gate run, one release.
2. **Several modules, one repo** — distinct sub-systems that still **build and
   release as one**. No new machinery: partition the same spine by the columns that
   already exist (the LLR **`Module`** column and the optional **`Area`** tag,
   §1), give each module its own **domain hat**, add **integration TCs** for the
   seams, and record shared internal contracts as `IF-###` (§8). The **repo-level
   gate stays the source of truth** — `trace.py --strict` requires 0 orphans
   across the whole repo, seams included; the kit ships **no** `--module`/`--area`
   filter (per-module ownership is a reading convention, not a gate).
3. **Several repos + a coordinator** — only when modules genuinely need
   *independent* versioning, ownership, access, or release cadence at a scale one
   repo can't sustain. A heavier, deliberately **rare** step with its own
   coordinator role, documented separately in `MULTI_REPO.md` (a *design*, heavy
   cross-repo tooling deferred); a reviewer should push back on a premature jump.
   **Revisitable** — promote a module to its own repo *later*, once it proves it
   needs the independence, which is far cheaper than a speculative split.

Rung 2 details (module-scoped review, seam TCs, in-repo `IF-###`) are expanded in
[`process-options.md`](process-options.md#10-several-modules-one-repo).
