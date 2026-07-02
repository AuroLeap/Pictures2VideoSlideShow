# Process Options — the opt-in layers

Companion to [`process.md`](process.md), which carries the load-bearing **core**
every project reads. This file expands the **opt-in layers** that doc summarizes,
each with an **applies-when** so a small project can tell at a glance whether it
needs the layer at all. Nothing here is required for the minimum profile (a
standalone one-module project — see the core doc's header); skip any section whose
applies-when doesn't match your scope.

Section headings mirror the core-doc sections that point here.

---

## Proportionality doctrine

*Referenced from PROCESS.md header ("Proportionality") and §3 "Right-sizing".*
**Applies always** — this is the philosophy that frames how hard every other
layer is applied; it is opt-in only in the sense that it tells you when to *not*
reach for machinery.

The core is the process's own guardrail against turning a sustainability tool
into a straitjacket. Four points, one voice:

- **(a) The tracked-artifact ideal, not an entry gate.** The whole method is
  built to perform change management and transparency **where possible**: a
  text-representable, line-diffable, mechanically-checkable artifact is the
  **ideal** it reaches for. But some work genuinely can't produce one, and that
  is not a disqualification. When the artifact itself can't be diffed, **track
  *about* it in text** — provenance, license, version, a content hash (§8
  "Binary assets") — so the *record* is change-tracked even when the *asset*
  isn't. The ideal is a direction, not an admission ticket.
- **(b) Attestation is the honest floor — and honestly trust-based.** Where
  verification cannot be mechanized, the floor is a **recorded human
  attestation**: a named person's recorded judgment that the acceptance
  criterion is met (a playtest, a creative review, a physical action). Be honest
  about what this is: **the box can be checked without the work having
  happened.** Attestation is trust; a mechanized check is proof. The process does
  **not** pretend they are equivalent — its job is to make the attestation
  **explicit** (a real verification kind, not a silent "Verified"), **named** (who
  attested), and **auditable** (when, against which criterion), so a reader can
  always see how much of the project rests on trust. That is the `Attest`
  verification kind (§4) and the "attested vs mechanized" split in the trace
  report.
- **(c) Over-aggressive traceability is a failure mode.** Traceability founds
  sustainability — *and* pushed past what a scope earns, it becomes an overly
  complex, overly constrained process that bogs development down. The balance is
  the whole game. **Right-sizing the traceability is the process working, not a
  compromise of it.** A gate that demands fine-grained decomposition of work no
  script can verify isn't more rigorous; it is theater that trades real velocity
  for the *appearance* of control. Reach for the lightest structure that keeps
  key items from being missed or silently broken.
- **(d) For creative/subjective domains, fly high.** Story, music, artwork,
  voice acting, level design — mostly binary, mostly subjectively verified. Here
  the `SN→SR→LLR→TC` spine's value is at **high altitude**: use `SN→SR` to
  ensure nothing key is **missed or silently broken** as development moves
  forward (the through-line of a story, the mood targets of a soundtrack, the
  cast a script needs). **Descend to LLR/TC granularity only where a mechanized
  check earns its keep** — a save-file schema, an audio-loudness bound, a
  build-size budget — and stop there. Decomposing a subjective judgment ("is this
  scene moving?") into finer rows a script still can't check adds process weight
  with no verification return; mark it `Attest` and move on.
- **(e) Decision-surfacing rate is a setup dial, not a constant.** How often the
  driver pauses for the human to **ratify a decision** is project-specific:
  calibrate it **at project setup** on the same risk axis as review-depth triage
  (PROCESS.md §6) and record the setting in `AGENTS.md` (Project section). In
  specialized or high-consequence domains — where safety is a risk even an
  *ancillary* one, money, privacy, anything irreversible — surface decisions
  **often**: bring even medium calls to the human to ratify. In low-risk domains
  (creative content is the archetype), where a wrong call is cheap to revert and
  carries little tech debt, a **confident** agent may decide **autonomously** —
  and the non-negotiable price of that autonomy is that every autonomous
  decision is **recorded** (a *Decisions log* / *Assumptions* entry in
  `status.md`: the call, the alternatives passed over, why) so it stays visible,
  auditable, and cheaply revertible. The dial moves *how often you ask*, never
  the fixed points: gates still pause for human approval (§4), and a requirement
  **contradiction** still routes as a finding to its owner — an unrecorded
  autonomous decision is a *silent* one, which no dial setting permits.

## Phased delivery

*Referenced from PROCESS.md §4.* **Applies when** a roadmap ships v1 before
v2/v3; a single-shot deliverable skips it.

A roadmap that ships v1 before v2/v3 needs gates that close *per phase* without
dishonesty. SRs may carry an optional **`Phase`** tag (e.g. `v1`, `v2`; blank =
in scope for every phase). Semantics:

- **Traceability is phase-blind.** Every SR keeps its LLR + TC rows from G2 on,
  whatever its phase — decomposition is cheap and pins the design.
- **The G3 Verified criterion is phase-scoped.** `check.py --gate G3 --phase v1`
  (cumulative for later closures: `--phase v1,v2`) requires Verified only for
  in-scope SRs; out-of-scope SRs are listed in the trace report as
  **phase-deferred** — an explicit, recorded exemption, never a silent skip.
- **G-Release is phase-scoped the same way:** `gen_release_checklist.py --phase v1`
  includes only in-scope human items and the release-tier/manual TCs verifying
  them.
- Later phases re-enter at G1/G2 as requirement increments and close their own
  G3/G-Release with the grown phase list.

## Lifecycle phase

*Referenced from PROCESS.md §4.* **Applies when** install/startup/steady-state
requirements are easy to miss — i.e. most non-trivial products; a pure library
with no runtime lifecycle can leave the tag blank.

Distinct from the delivery `Phase` (which is *when we ship it* — v1/v2), a
requirement also has a **lifecycle phase**: *at what point in the running
product's lifetime must this hold, and how often?* Naming it stops the perennial
miss of writing only steady-state requirements and discovering the install/setup
ones late. Capture it as an **optional `Lifecycle` tag** on an SN/SR (a column or
inline tag, mirroring `Area`; blank = unspecified, treat as **Runtime**) — use the
distinct name `Lifecycle`, never overload the delivery `Phase` column. The default
vocabulary is an **open, project-named set** (extend it per scope like `Area`; it
is **not** a fixed enum):

- **Provision** (ready) — must hold *before the process can run at all*: install,
  dependencies/runtime present, infra provisioned.
- **Startup** (set) — established *once per launch, before it serves*: load +
  validate config, run migrations, open the initial pool, allocate fixed
  resources, readiness probe.
- **Runtime** (go) — steady-state serving, *including recurring acquisition*:
  handle requests, reconnect on drop, per-request alloc, dynamic config reload.

Optional **Shutdown**/**Teardown**, **Upgrade**/**Rollback**, **Recovery** extend
the set when the scope needs them.

- **Discriminate by *when / how often*, not by the word "setup"** — almost
  everything readies *something*. Opening the connection pool *at boot* is Startup;
  reconnecting *mid-operation* is Runtime; a fixed buffer at launch is Startup,
  per-request alloc is Runtime. **One capability legitimately spans phases** — that
  is the payoff: a DB feature yields *provision the DB* (Provision) → *open the
  pool + migrate at boot* (Startup) → *reconnect on drop* (Runtime), and people
  usually write only the Runtime one.
- **Configuration straddles Provision↔Startup, app-dependently.** Config is
  **Provision** when it *must pre-exist* and the app has no way to obtain it at
  launch; it is **Startup** when the app *can* obtain/validate it at launch (a
  first-run wizard, a clear error, or a default fallback). Capture both the
  *definition* (where the config lives) and the *launch behavior when it is
  missing*.
- **Keep one axis.** Dependencies and config are *subjects*, not phases — a
  dependency is required at Provision but used at Runtime; config must exist at
  Provision, is loaded at Startup, may reload at Runtime. The `Lifecycle` tag on
  the concrete requirement already places it; don't add a second "kind" axis.

## §7 boundary notes

*Referenced from PROCESS.md §7.* These three notes draw lines around what the kit
is and isn't; a small project can read the one-line summaries in §7 and come here
only if a boundary is contested. **Applies when** onboarding contributors, wiring
a developer workstation, or deciding whether to add an external measurement or
agent-runtime tool.

**A third toolchain layer — the developer workstation.** The two check layers (§7)
cover what the *project* needs to pass its own gates. A third, often-conflated
concern is what a **human** needs to view, render, edit, and run any of it at all:
a language/runtime, `git`, an **offline** Markdown+Mermaid renderer (e.g. VS
Code's preview, or `@mermaid-js/mermaid-cli`), and optionally an IDE or a
domain-specific viewer (CAD/image/publication tooling). "No required tools" was
always a claim about the **process** layer (stdlib only); it never meant a human
needs nothing. Naming this third layer resolves the conflation between
"procurement for the product" and "procurement for developing the product."

**The onboarding ladder — Provision-for-development, applied to the act of
developing itself.** A fresh contributor's path to a running checkout mirrors the
§4 lifecycle phases, one level up:

```
Stage 0           →  dev-setup       →  setup          →  check
get git + repo        workstation        product deps      run gates
(pre-clone)           (post-clone)       (venv/tools)       (exists)
```

`Stage 0` and `dev-setup` provision the **developer workstation** above (rare,
once per contributor); `setup` provisions the **product toolchain** (recurs per
clone/CI run); `check` is the **process** floor that already exists. Each rung is
an optional, readable, **consent-first** helper — never a silent or compiled
installer — so a contributor (including a non-code one, whose deliverable is still
a reviewable git change) can go from a bare machine to an editable, testable
checkout without needing prior git literacy.

**The evaluator's rungs — README + run launchers.** The ladder above serves the
*contributor*; a project also has *evaluators* — the stakeholder, a tester, the
future you — whose path is shorter: understand it, then run it. Two artifacts
serve that path, both scaffolded by bootstrap:

- **`README.md` is the human front door and exists from day one.** Bootstrap
  lays down a skeleton (project name filled from the folder; everything else a
  marked fill-in) and the kickoff agent **builds it out from the project brief**
  — purpose, how to run it, how to get started. An adopted repo keeps its own
  README (bootstrap never overwrites); retrofit the run/getting-started pointers
  into it instead (ADOPTING.md §1).
- **Root `run.{cmd,sh,command}` launchers — one double-clickable start per
  platform the project supports** (the PROJECT BRIEF's "Supported platforms"
  line). Ease of access is a requirement of its own: the launch command may be
  obvious, and it may be documented in the README, but *recall is still the
  enemy* — a launcher turns "remember the incantation" into "open the folder and
  click". Each is a short, readable script with one `RUN_CMD` slot (filled twice:
  `run.cmd` for Windows, `run.sh` for POSIX; `run.command` delegates to `run.sh`
  so macOS costs no third copy). They ship **inert** — an unfilled `RUN_CMD`
  prints guidance and exits nonzero, the same always-scaffolded-inert stance as
  the optional registries — and a pure library deletes them and describes usage
  in the README instead.

**Offline-render principle.** Legibility artifacts (the Mermaid diagrams, the
trace HTML map, the code map) must render with **local, offline** tooling — never a
cloud rendering service — the same reason the kit chose Mermaid-in-Markdown (§3) in
the first place. Point contributors at a local renderer; reach for a Kroki/PlantUML
*container* only if a project genuinely outgrows Mermaid.

**The kit generates legibility; it does not score it.** The harness *builds* the
traced spine, the committed code map, and the gates, so a repo scaffolded from this
kit should score well **by construction**. *Measuring* that legibility over time
(AI-readiness, complexity/churn dashboards, doc-navigability scores) is a separate,
deliberately **external** concern — run an **external readiness assessor** (e.g. a
deterministic codebase-scoring tool) as **optional downstream tooling**, never a
kit dependency. This is the same stance the kit takes on `ruff`/`pytest`: it names
the gate; the project picks the tool. Generate here; measure there.

**The kit is a spec; a turnkey agent-runtime harness is a different layer.** This
kit is a stack-agnostic, stdlib, agent-neutral process **spec** you copy into a
repo. A **turnkey agent-runtime harness** — e.g. an `npx`/Node-installed engine
shipping skills/agents/hooks/MCP for one tool, with deterministic verification
gates, model-tiered subagents, and a project-context layer — is a different,
installed **product** a downstream shop may run *in addition*. They **compose** (a
repo scaffolded from this kit can be driven by such a harness) but neither depends
on the other: a runtime harness is optional, tool-specific, downstream tooling,
never a kit dependency. Its "back every verdict with a deterministic gate" stance
is the same one §6 already takes — the philosophical fit is real, the dependency
isn't.

**Repo text is the durable agent memory layer.** An agent session starts cold;
**re-reading `AGENTS.md` + `docs/status.md` + the code map is the context reload**,
not a custom memory tool. The kit's committed artifacts already form the
agent-neutral, reviewable memory layer: `status.md` *Current State* (cheap
context reload, §6), `AGENTS.md` (guide re-read every session), the generated
code map (layout without re-deriving it), the registries (requirement + interface
truth), `docs/gate` (current bar). **Agent-native memory tools** — e.g. auto-memory
dirs, MCP memory servers, `.planning/`-style context layers — are a legitimate and
optional *scratch* space for a session's working notes; they are **not** the home
for any load-bearing fact. Why: agent memory is per-session, per-host, and often
per-tool; it is invisible to other agents and humans, unreviewable, and silently
erodes the single-source-of-truth discipline the kit is built on.

**The promote rule.** When a working note ripens into something durable — a
decision, a constraint, a gotcha, an assumption confirmed — **promote it into the
repo**: record a decision in `status.md` *Open items* or *Decisions log*, add a
constraint to `status.md`'s constraints block, update `AGENTS.md` if it changes
how contributors should behave, or amend the relevant registry row. This is the
flip side of the *Assumptions* log (§4, Thread 3): an unattended assumption is
logged to `status.md` so a human can confirm or revert it; a confirmed finding is
committed into the appropriate artifact and drops out of the assumptions list.

**No agent-memory tooling is installed or required.** Dev-setup provisions the
*workstation* (§7 "Onboarding ladder"), not the agent runtime; the kit does not
install, scaffold, or depend on any memory tool. A larger repo makes the committed
layer matter *more* (keep `status.md` *Current State* tight so re-reads stay
cheap), and a query-time semantic index (§7 map-vs-index note) can help chase
references across a large tree — but both are optional, downstream, and orthogonal
to the promote rule.

## Skills layer

*Referenced from PROCESS.md §7 "boundary notes".* **Applies when** a repo will be
worked by an AI agent (Claude Code, Gemini CLI, …) and you want that agent to load
this repo's repeatable procedures as first-class, on-demand **skills**. Skip it for
a repo with no agent — nothing here is required, and the gates never read a skill.

A **skill** is a small, focused capability — a procedure grounded in this repo's
actual commands and files — that an agent loads on demand to work faster and more
correctly. Skills are **opt-in accelerators, not process gates** (the
Proportionality doctrine applied to tooling): the gates, the traceability spine,
and the git/CI floor are the bar; a skill only helps an agent clear it. The full
contract lives in the kit's `skills/README.md`; the shape:

- **Neutral source → per-agent materialization.** The kit ships skills as
  agent-neutral `skills/<name>/SKILL.md` files. `bootstrap.py --agents
  claude|gemini|both|none` materializes the selected agent's skills into its native
  location (Claude Code `.claude/skills/<name>/SKILL.md`; Gemini CLI
  `.gemini/skills/<name>/SKILL.md`) — both read the same Agent-Skills `SKILL.md`
  frontmatter, so materialization is a straight copy. `none` (the non-interactive
  default) materializes nothing, preserving the agent-neutral scaffold; run
  interactively and bootstrap **asks**. `AGENTS.md` stays the canonical guide
  whichever agent is chosen.
- **The optional hook config is copied inert.** The chosen agent's
  `agent-hooks/*.settings.json` is copied as `settings.json.example`, **never** a
  live `settings.json` — the scaffold must not silently install a `Stop` hook that
  runs commands. Enforcement stays in git + CI (`agent-hooks/README.md`); activating
  the example is the user's explicit choice.
- **Applicability schema + generated index.** Each `SKILL.md` frontmatter carries
  `stacks`/`domains`/`phases`/`tags` (+ a `scope` of `kit` or `this-repo`) so a
  skill's fit is machine-readable. `scripts/gen_skills_index.py` regenerates
  `skills/INDEX.csv` (one row per skill) as the cheap scan surface, with `--check`
  as the freshness gate — the same "generated, don't hand-maintain" stance as the
  code map. At setup bootstrap asks up to three scope questions (stack? domain?
  binary/hardware?) and selects the `kit`-scope skills whose tags **intersect** the
  answers — a trivial set-intersection, no engine. The **metadata convention is the
  deliverable**, so a later tool can match/fetch smarter without redesign.
- **Future external sources plug in here.** `skills/README.md` documents the
  contract (naming, the frontmatter shape, the neutral-source landing zone,
  trust/review) for how a later tool would fetch remote/community skills — they land
  in the same `skills/` source layout and materialize via the same path, never
  written straight into an agent dir bypassing the index.

## §8 purchased parts

*Referenced from PROCESS.md §8.* **Applies when** the product incorporates
**purchased/external parts** it buys rather than builds (motors, arms, cameras,
compute boards) and wants their status and source tracked in-repo.

**One row per bought part, owned by an interface row.** A purchased part that *no
repo builds* still has a contract of record — its datasheet, vendor, pinned
version — and §8's rule already places that: a **coordinator/repo-held `IF-###`
row is the owner-of-record** for such a part (MULTI_REPO.md §3.3). The
`procurement.csv` registry (`PART-###`) sits **alongside** that, adding only the
**acquisition** facts the interface row doesn't carry: `PART-ID, Name, IF-Ref,
Vendor, Cost, Status, Quantity, Notes`, where `IF-Ref` back-links the owning
`IF-###` and `Status ∈ {needed, ordered, on-hand, backordered, obsolete}`. Off
the `SN→SR→LLR→TC` spine and optional like `interfaces.csv`/`PB-###`: a project
that buys nothing ignores the file; a leftover `PART-000` never blocks a gate.

- **What `trace.py` checks (integrity only).** It flags a malformed/duplicate
  `PART-` id, the always-on floor. It does **not** resolve `IF-Ref` against
  `interfaces.csv`, because trace.py never reads the `IF-###` tier (it is off the
  joined spine, §8); keeping PART integrity-only holds the "no more than PB"
  minimal line and avoids teaching trace.py the interface registry. Cross-checking
  `IF-Ref` against a real interface row is a natural first extension if it earns
  its keep.
- **Deliberately minimal — deferred extensions.** This is a flat parts list, not a
  bill of materials. **Full BOM tracking** — alternates/second-sources,
  per-module allocation and quantity roll-ups, assembly trees, lead-time/reorder
  logic — is **explicitly deferred**; add it only when a project demonstrably
  needs it, extending this registry rather than replacing it.

## Binary assets

*Referenced from PROCESS.md §8 "Binary assets".* **Applies when** a project ships
unavoidably-binary deliverables — game art, music, voice acting, video, rendered
CAD, publication artwork — the kind of asset that can't be line-diffed or
mechanically verified.

This is the Proportionality doctrine's *"track about the asset in text"* stance
(this file, "Proportionality doctrine" (a)) made operational. The asset itself is
binary; the **record of it** is text, tracked, and reviewable.

- **Manage the binary as a pointer + manifest, not as a blob in the tree.** Store
  the asset in **git-LFS** or an **out-of-repo store** (an object store, an asset
  server) and keep, in the repo, a **manifest row** that points at it and pins its
  identity: the optional `requirements/assets.csv` registry (`ASSET-###`). This
  keeps the git history diffable and the checkout small while the manifest stays
  the change-tracked source of truth *about* every asset.
- **Columns (what to track *about* an un-diffable asset).** `ASSET-ID, Name,
  Refs, Kind, Provenance, License, Attribution, ContractRef, Location, Hash,
  Version, Notes`. The load-bearing ones:
  - **`Provenance`** = `human-made | ai-generated | mixed`. Real-world driver:
    distribution platforms (e.g. **Steam**) require **AI-content disclosure**, so
    the provenance of every shipped asset must be recordable and auditable, not
    guessed at release time.
  - **`License`** (SPDX id or `proprietary`) and **`Attribution`** (any required
    credit line) — so a licence obligation can't be lost between acquisition and
    ship.
  - **`ContractRef`** links the **voice-actor release** or **commissioned-work
    agreement** that grants the right to ship the asset — the paperwork a purely
    binary asset would otherwise carry no trace of.
  - **`Location`** is the **pointer** (git-LFS path or store URL); **`Hash`**
    (e.g. `sha256:…`) + **`Version`** make that pointer **verifiable** — you can
    confirm the bytes on the store match the row even though you can't diff them.
  - **`Refs`** back-link the SR/LLR the asset realizes, keeping it on the spine's
    high-altitude thread (usually an `Attest` SR — this file, "Proportionality
    doctrine" (d)); `trace.py` integrity-checks the `ASSET-` id only, off-spine
    like `PART-###`.
- **Registry choice — a sibling registry, not a widened `procurement.csv`.**
  Procurement (`PART-###`) tracks parts the project **buys** (owner-of-record is
  an `IF-###` interface row; columns are vendor/cost/status/quantity). A created
  or commissioned **digital asset** is a different concern — license, provenance,
  release paperwork — so it gets its own minimal registry rather than overloading
  procurement's columns with fields that don't apply to a motor, or forcing an
  asset row to fake a vendor/cost. Same off-spine, integrity-only, optional
  pattern; different subject.
- **Deferred product-layer idea — the "asset manifest freshness check."** A
  natural next step is a tool that verifies each `ASSET-###` row against its store
  — the pointer resolves, the `Hash` still matches, no manifest row is orphaned
  from its file and no shipped file is missing a row. This is a **product-layer,
  project-owned** check (it must reach a git-LFS or object store — outside the
  kit's stdlib, offline, no-network line), named here and **deliberately
  deferred**, in the **same family as the Thread-16 CAD/non-code-artifact
  verification stub** (render-on-change, visual diff, design-rule checks): the kit
  **names and routes** these, the project **wires** them, the gate **records** the
  verification (the meters-vs-comparator split, PROCESS.md §9). Until then the
  manifest is the honest, text-tracked record — an ideal reached for, not a check
  faked.

## §9 NFR checklist

*Referenced from PROCESS.md §9.* **Applies when** deciding which non-functional
concerns a project must consider at G1.

**Consideration checklist (a prompt, not a mandate — don't wear a hat the scope
doesn't need).** At G1, consider which categories apply and route each to a home
(anchor: the **ISO/IEC 25010** product-quality model):

- performance efficiency (time, throughput) and resource use (RAM/VRAM, disk);
- reliability / availability / recoverability;
- **security** (authn/authz, data protection, secrets, audit, dependency / supply-chain);
- **observability / operability** (logging, metrics, tracing, health — also the
  prerequisite for *measuring* any of the perf budgets);
- scalability / capacity; compatibility / interoperability;
- portability / installability (incl. artifact size); compliance / legal / licensing;
- safety (cyber-physical); data integrity / durability;
- **cost / economics** (unit/BOM cost, licensing fees, cloud spend; for hardware
  scopes also procurement / supply-chain). Note 25010 is a *software-quality* model
  and omits cost entirely — these systems-engineering categories sit **alongside**
  it, and a quantitative cost budget is just a `PB-###` row (metric-agnostic:
  `Metric=Unit BOM cost, Unit=USD, Direction=lower-better`), compared by
  `check_perf.py` like any RAM budget. No new mechanism.

The kit already covers some — **don't double-prompt**: maintainability (= the core
discipline), usability (= the end-user lens), basic fault tolerance (= the
edge-case table and the SN edge cases), cross-project contracts (= `IF-###`, §8).

## §9 perf comparator

*Referenced from PROCESS.md §9.* **Applies when** a project has captured `PB-###`
budgets it wants tracked over time.

A captured budget is inert until something compares the *measured* number against
it. That comparison answers two distinct questions per metric: **absolute** —
"worse than the budget?" (measured vs `Budget`, per `Direction`) — and
**regression** — "suddenly much worse?" (measured vs a committed baseline, outside
the `Tolerance` band). The work splits along the §7 **process/product** line:
*measuring* a metric is **product** work the project wires (`/usr/bin/time`,
`tracemalloc`, `nvidia-smi`, a size command, `pytest-benchmark`/`hyperfine`),
emitting a `docs/test/perf-metrics.json` map of `PB-ID → number`; *comparing* is
**process** work the kit owns — `check_perf.py`, stdlib-only and metric-agnostic
(arithmetic over JSON). The kit owns the comparator; the project owns the meters.

- **Three artifacts, three reviewability classes (§3):** `performance-budgets.csv`
  is the tracked source of truth; `perf-baseline.json` is a **committed golden**
  updated *deliberately*; `perf-report.md` is a **gitignored composite** (current
  vs baseline vs budget + deltas), regenerated each run and published by CI.
- **Baseline-as-golden protocol.** Accepting a regression = committing a new
  `perf-baseline.json` **in the same PR**, so the number move is explicit and
  reviewed — never silent (the same discipline as the coverage threshold and
  phase-deferred SRs). `check_perf.py --update-baseline` rewrites it from the
  current metrics for exactly that purpose.
- **Warn-first; start with the deterministic metrics (honest-gate rule, §4).** The
  per-row `Gate` decides fail-vs-warn and `Tier` decides *when* a row is in scope:
  gate the **low-noise, deterministic** metrics (artifact/binary size, dependency
  count) at `full`; default **noisy runtime** metrics (latency, peak RAM, VRAM,
  throughput) to `Gate=warn` at `release`, with tolerance bands and same-runner /
  best-of-N measurement. A number that can't be a reliable `Test` gate is
  warn-tracked or `Demonstration`, never faked into a binary gate. A budget with no
  measurement this run is skipped, like a missing tool — absent metrics never fail.

## §10 several modules, one repo

*Referenced from PROCESS.md §10.* **Applies when** a repo grows distinct
sub-systems that still build and release as one (rung 2 of the scale ladder).

**No new machinery, just partition the spine.** A multi-module repo is the *same*
spine, grouped by columns that already exist: the LLR **`Module`** column and the
optional **`Area`** tag on SR/TC (§1 "Domain hats"). Each module is a sub-tree of
`SN→SR→LLR→TC`; where a module needs its own discipline it gets its own **domain
hat** owning that slice (§1 already allows this). The repo still builds, gates, and
releases as a whole.

- **Module-scoped review is a convention over the existing columns, not a new
  flag.** A module owner reviews their slice by filtering the registries on
  `Area`/`Module` (a grep or spreadsheet filter); the **repo-level gate stays the
  source of truth** — `trace.py --strict` still requires **0 orphans across the
  whole repo, seams included**. The kit deliberately ships **no**
  `--module`/`--area` filter on `trace.py`/`check.py`: a per-module gate would
  either hide the cross-module seams (a false "green" masking exactly the
  integration gaps this method wants first-class) or need real machinery to tell a
  legitimate seam from an orphan. The whole-repo gate already spans every module;
  per-module *ownership* is a reading convention, not a gate of its own.
- **Integration TCs for the seams.** A module boundary is where two parts must
  agree, so it gets its **own** TCs — not merely each module's internal unit tests.
  These are integration/system-level, usually `Tier=Full` or `Release` (§4 "Test
  tiers"), so the seam is a tested contract rather than an untested gap between two
  individually-green modules.
- **`IF-###` applies *within* a repo, too.** The interface registry (§8) is not
  only for separate repos: two modules in one repo that share a contract record it
  as an `IF-###`, with the counterpart naming the **other module** instead of
  another repo and both rows living in the one `interfaces.csv`. Same
  direction/owner/version/stability discipline, same "one contract, one home,
  backed by a test" rule — applied to the internal seam, with no cross-repo build
  machinery.
