---
name: downstream-resync
description: Use when upgrading a repo that adopted this kit to a newer kit version — walk ADOPTING.md section 6 to diff from the recorded kit commit, overwrite the kit-owned files, preserve your own, and re-stamp.
stacks: [python, powershell, go, rust, any]
domains: [any]
phases: [dev, release]
tags: [adopting, re-sync, kit-version, upgrade, migration]
scope: kit
---

# Downstream re-sync (upgrade an adopted repo to kit HEAD)

Pull newer kit updates into a repo that adopted this kit earlier. This is **not**
a fresh bootstrap — you merge kit changes into files you've filled in. The
authority is `docs/process.md`'s companion **`ADOPTING.md` §6**; this is the
checklist.

## 1. Make it a diff, not a guess

- Read `docs/kit-version` — the kit short-SHA + date this repo was last
  scaffolded/re-synced from.
- Choose the target kit commit. **Sync only from a committed kit state**, never a
  dirty kit tree (bootstrap stamps `<sha>-dirty` and warns — that stamp is
  unreproducible).
- Diff the recorded SHA against the target to see exactly which templates/scripts
  changed before touching anything.

## 2. Overwrite vs. preserve

- **Overwrite freely (kit-owned):** `scripts/trace.py`, `check_docs.py`,
  `check_flows.py`, `check_perf.py`, `gen_arch_map.py`, `gen_*`, the pre-commit
  hook, `docs/process.md` + `docs/process-options.md`, `pytest.ini` markers. For
  `check.py`, take the new version and re-apply only your **"EDIT FOR YOUR STACK"**
  block (`SRC`/`TESTS`, product-step commands).
- **Preserve always (yours):** every registry CSV, `stakeholder-needs.md`,
  `docs/status.md`, `docs/architecture.md` hand-written overview (regenerate only
  the marker blocks), `AGENTS.md` project content, `docs/gate`,
  `.gitignore`/`.gitattributes` (merge new kit lines by hand). `bootstrap.py`
  **skips existing files**, so a plain re-run won't clobber these — never run
  `--force` against a live repo without a diff pass.

## 3. Apply the migration recipes

Read the **"Migration recipes"** list in ADOPTING.md §6 for the specific changes
in your diff range (e.g. `process.md` split into `process.md` +
`process-options.md`; a legacy `UN-` → `SN-` rename keeping id numbers; a TC CSV
gaining a required `Tier` column; a `gen_release_checklist.py` function rename).
Apply only the ones your diff actually contains.

## 4. Re-stamp and verify

- Re-run bootstrap to refresh generated pieces, then **re-stamp `docs/kit-version`**
  and commit it as the **last** step, so the record reflects the state you landed
  on.
- End green: run the harness (`scripts/check.py` / `check.{sh,ps1}`) and
  `scripts/check_docs.py`; a re-sync that leaves the harness red isn't done.
