---
name: registry-hygiene
description: Use when the traceability registries (SN/SR/LLR/TC and the off-spine PB/PART/ASSET/MOD) need checking or fixing — run trace.py/check.py with the right flags for the current gate, read the orphan/integrity/schema findings, and correct them.
stacks: [python, powershell, go, rust, any]
domains: [any]
phases: [dev, gate]
tags: [traceability, trace, registries, orphans, schema]
scope: kit
---

# Registry hygiene (any repo built from this kit)

Keep the `SN → SR → LLR → TC` spine (and the off-spine `PB`/`PART`/`ASSET`/`MOD`
registries) clean: zero orphans, no duplicate/malformed ids, valid schema. Run
the right check for where you are, then read and fix the findings.

## Which command, when

```
# Always-valid floor (pre-commit, any stage): duplicate/malformed ids only.
python scripts/trace.py --strict-integrity

# Full orphan check (aim for orphans=0 before any gate).
python scripts/trace.py --strict

# G2+: also reject leftover template example rows (ids ending -000).
python scripts/trace.py --strict --no-placeholders

# G3: required fields non-empty + closed Verification/Tier vocabularies + Verified.
python scripts/trace.py --strict --no-placeholders --strict-schema --require-verified
```

`scripts/check.py` (or `scripts/check.{sh,ps1}`) runs the gate-appropriate set
automatically from the active gate in `docs/gate` — prefer it for a full gate run;
call `trace.py` directly to iterate on a specific finding.

## Reading findings

- **orphan** — a row not joined end-to-end. Common causes: an SR with no LLR/TC
  yet (fine early; a real orphan at G2+), a `Refs`/`SN-Refs`/`SR-Refs` id that
  points at nothing, a TC that verifies no real id. Fix the back-link, don't
  delete the evidence.
- **integrity** (duplicate/malformed id) — two rows share an id, or an id doesn't
  match the `PREFIX-###` shape. Renumber the duplicate; fix the malformed id.
- **schema** (`--strict-schema`) — a required field is empty, or a
  `Verification`/`Tier` value is outside the closed vocabulary
  (`Test|Demonstration|Inspection|Analysis|Manual|Attest`; `Smoke|Full|Release`).
  Fill the field or correct the value.
- **placeholder** (`--no-placeholders`) — a template `-000` example row survived;
  delete it once real rows exist.

## Verify the fix

Re-run the same command; it must print `orphans=0` (and exit 0). `trace.py`
regenerates `docs/test/report.md` — the readable matrix — so review that too.
Off-spine registries (PB/PART/ASSET/MOD) are **integrity-checked only**; a valid
inert `-000` placeholder there never blocks a gate.
