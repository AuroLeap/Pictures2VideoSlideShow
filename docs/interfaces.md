# Cross-Project Interfaces (IF-###)

Shared contracts with sibling projects. Each interface keeps one stable id
used identically in both repos; the **providing** side owns the authoritative
spec and this page only links it (never restates it), per the trajectory-kit
process (§8 cross-project interfaces).

---

## IF-001 — Photo ROI sidecar ("focus database") — **Consumes**

| | |
|---|---|
| Direction | **Consumes** (PictureSorter provides) |
| Counterpart / spec owner | [`PictureSorter`](../../PictureSorter/) — authoritative spec: [`PictureSorter/docs/interfaces.md#IF-001`](../../PictureSorter/docs/interfaces.md) |
| Implemented today | **v1** — the single-file `roi_db` JSON map (relative path → normalized point or bbox), loaded by `src/roi/mod.rs` (`RoiDb`). Local spine: SR-031 / LLR-038 / TC coverage in `tests/roi_focus_build.rs` (Verified). |
| Target | **v2 (Draft)** — back-compatible superset written by PictureSorter as per-directory `roi.json` sidecars. |

### What v2 adds (deltas to adopt here, when scheduled)

Per the owning spec (do not restate details — read it there):

1. **Schema detection:** top-level `"schema": "picturesorter.roi/2"` selects
   v2 parsing; a flat map without that key stays v1. Unknown fields must be
   ignored (forward compatibility).
2. **Multiple ROIs per image** (`entries.<file>.rois[]`, each bbox + `kind` +
   `weight` + `margin`). Minimum conforming behavior maps cleanly onto today's
   `RoiDb`: primary focus = explicit `focus` or the highest-weight ROI's bbox
   center — i.e. v1 semantics fed from richer data.
3. **`margin` as the framing/zoom bound:** when focusing an ROI, the bbox plus
   its margin should stay inside the frame (bounds the Ken Burns max zoom).
   Richer multi-ROI pan paths (visiting ROIs by weight) are optional, later.
4. **Per-directory discovery:** in addition to the configured `roi_db` path,
   discover `roi.json` files per directory under `media_root`, keys relative
   to each file's directory; per-directory entries win on duplicates.

Invalid-file behavior is unchanged from v1: a syntactically bad ROI file fails
the run loudly (SR-031 acceptance).

### Adoption note (maintenance backlog)

Adopting v2 is a normal maintenance objective for this repo: add SR/LLR/TC
rows for the deltas above (extending SR-031's family), keep `RoiDb`'s v1 tests
green, and pin a v2 fixture from the PictureSorter contract test (its TC-042)
so both repos test the same bytes. Until then, nothing changes for current
users: PictureSorter's v2 sidecars are simply not pointed at by `roi_db`.
