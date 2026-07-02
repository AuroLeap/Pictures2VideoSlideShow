# Stakeholder Needs (SN-###)

Owned by the **Stakeholder** hat — whoever the system serves: an end user, an
operator, or **another system** (represented by its owner/integrator).
Plain-language needs + edge-case expectations. Engineering translations live in
`system-requirements.csv` (referenced by `SN-Refs`); do not restate them here.
Priority: **M**=Must · **S**=Should · **C**=Could.

> **Cover the whole lifecycle, not just steady state.** For each need, ask *when
> in the running product's life must this hold?* — **Provision** (before it runs:
> install, dependencies), **Startup** (once per launch: config, migrations), or
> **Runtime** (steady-state serving). Most authors write only Runtime needs and
> discover the install/first-run ones late; tag the non-runtime ones with an
> optional `Lifecycle` value (process.md §4 "Lifecycle phase").
>
> **Consider the cost, not just the behavior.** Where the scope warrants it, also
> capture **non-functional** needs — performance, memory/size, **cost** (unit/BOM,
> licensing, cloud spend), reliability, security, observability — and route each
> to its home (process.md §9). It's a prompt, not a mandate: skip the categories
> the scope doesn't need.

## Core needs

| SN-ID | Need (plain language) | Why it matters | Priority | Acceptance intent (how we'd know it's met) |
|---|---|---|---|---|
| SN-000 | _EXAMPLE — replace this row; number real needs sequentially (the `-000` id is a placeholder the tooling ignores)._ | | | |

## Edge-case expectations

How the system should behave when things go wrong (the highest-value part — be
specific; the System Engineer turns each into measurable SRs). Most of these rows
are **Provision** or **Startup** lifecycle concerns (first-run, missing
dependency, unwritable output) — exactly the phases that get neglected, so they
earn first-class SRs.

| SN-ID | Scenario | Expected behavior |
|---|---|---|
| SN-0xx | Interruption / power loss / killed mid-operation | |
| SN-0xx | Invalid / corrupt / unsupported input | |
| SN-0xx | Resource exhaustion (disk / memory full) | |
| SN-0xx | Missing dependency / wrong version | |
| SN-0xx | Output target removed / locked / unwritable | |
| SN-0xx | Unattended/automated run (must never block; clear failure) | |
| SN-0xx | First-run setup & discoverable docs / quick-reference | |
