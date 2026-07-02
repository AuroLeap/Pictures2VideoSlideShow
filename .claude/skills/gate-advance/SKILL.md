---
name: gate-advance
description: Use when moving a project honestly from one gate to the next (G1 to G2 to G3) — what each gate requires, how to record human attestation with Attest, and how the attested-vs-mechanized verification split is reported.
stacks: [python, powershell, go, rust, any]
domains: [any]
phases: [gate, release]
tags: [gates, g1, g2, g3, attest, verification, sign-off]
scope: kit
---

# Gate advance (move G1 → G2 → G3 honestly)

Advancing a gate is a **human decision recorded in a reviewed commit**, backed by
a green harness at that gate's bar. Never self-advance; never report a green you
didn't run. Authority: `docs/process.md` §4 (gates), §5 (verdict protocol), §7
(the active gate).

## The active-gate mechanism

The current gate is the one line in **`docs/gate`** (`G1|G2|G3|all`).
`scripts/check.py` defaults to that gate and runs only its checks, so CI enforces
the bar the project is *actually* at. Closing a gate = a human **bumps `docs/gate`
in a reviewed commit** after the sign-offs in `docs/status.md`.

## What each gate takes

- **G1 → G2 (Requirements/UX/Constraints done).** Real SN/SR rows (no `-000`
  placeholders where committed), acceptance criteria measurable, the UX/edge-case
  lens applied. Sign-offs recorded in `docs/status.md`.
- **G2 → G3 (Decomposition & Test Coverage).** Every SR decomposes to LLR/TC;
  each requirement's TC exists as a **failing test first** (red → green). The
  harness at G2 runs traceability + `--no-placeholders` + runnable tests + flow
  checks. `orphans=0`.
- **G3 (Implementation).** Code implements the LLRs with `Implements:` back-links;
  the full harness passes: `trace.py --strict --no-placeholders --strict-schema
  --require-verified`, coverage ≥ threshold, arch-map fresh, flows current. Every
  test-verifiable SR is `Verified`.

Run the gate bar with `scripts/check.py` (it selects the gate's checks from
`docs/gate`); paste the real output into the status audit log.

## Honest verification where machines can't (`Attest`)

Some SRs can't be mechanically tested (subjective quality, a binary asset, a
witnessed behavior). Use the **`Attest`** verification method: a **named human's
recorded judgment**, captured in the TC's `Parameters`/`Expected` cell (who + when
+ what was judged) — a trust-based floor, not a runnable check. `Attest` (like
`Analysis`/`Inspection`) is **LLR-exempt** but still needs ≥1 TC. Don't fake
subjective work into a `Test`; give it an honest `Attest` home.

## Report attested vs. mechanized

`trace.py` always emits a **"Verification basis (attested vs mechanized)"** count.
When reporting a gate's readiness, state that split honestly — how many SRs are
machine-verified vs. resting on a human attestation — so a reviewer sees the trust
footprint rather than a single "all green". An attested gate can be legitimate;
hiding that it's attested is not.
