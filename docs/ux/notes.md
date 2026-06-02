# UX Designer — Notes

Usability findings, the documentation map (who owns what), and recommendations for Pictures2VideoSlideShow. Owned by the [UX Designer persona](../../.claude/agents/ux-designer.md). Reviews recorded in [status.md](../status.md) per [process.md §5](../process.md#5-verdict--status-protocol).

Back to [docs index](../README.md) · [project README](../../README.md).

---

## Doc map (single source of truth per topic)

| Topic | Owner doc | Notes |
|---|---|---|
| Rust commands, recommended frame settings, config fields (unit+default), current gaps | **[quick-reference.md](../quick-reference.md)** | Canonical. README + all Rust planning docs link here; values appear nowhere else. |
| Why each frame constraint exists (4 GB FAT32, codec/fps/CRF, audio) | [README → Output Constraints](../../README.md#output-constraints--limitations-digital-picture-frames) | Explains *why*; defers the *what to set* values to the Quick Reference. |
| Project status / which engine to use | [README → Project Status](../../README.md#project-status) | PowerShell (feature-complete, `main`) vs Rust (fast, `rust-rewrite`). |
| PowerShell pipeline config + optimization | [README → Configuration Guide / Performance Optimization](../../README.md#configuration-guide) | PowerShell-specific; CRF table now labeled general guidance, defers default to Quick Reference. |
| Rust design/architecture (historical) | RUST_IMPLEMENTATION_SPEC/ROADMAP/NEXT_STEPS/PROJECT_SUMMARY.md | Banner-flagged as historical/design; command/config samples superseded by Quick Reference. |
| Requirements / SRs | [user-needs.md](../requirements/user-needs.md), [system-requirements.csv](../requirements/system-requirements.csv) | UN/SR registries. |

---

## Findings (Round 1)

### Done / satisfied
- **UN-010 / SR-020 — quick-reference:** Created [docs/quick-reference.md](../quick-reference.md) as the single canonical copy of the Rust commands and recommended frame settings. Linked from the README (callout under the TOC + Documentation Index) and the docs index. Duplicate command/config/recommended-settings blocks elsewhere were replaced with links.
- **UN-009 / SR-019 — honest gap reporting:** The Rust gaps (no audio, no `bulk_video_time_min` splitting, no decode-sharing, no GPU) are stated **once** in [Quick Reference §5](../quick-reference.md#5-rust-engine--what-is-not-implemented-yet-and-the-workaround), each with the PowerShell fallback/workaround. The README Rust section now links there instead of restating.
- **UN-001 / SR-001 — setup path:** [Quick Reference §1](../quick-reference.md#1-setup-in-three-steps-non-programmer-path) documents the non-programmer path as exactly *install prereqs → edit one config → one command*, with `validate` before `build`. No source edits required.
- **UN-003 / SR-003 — config fields:** [Quick Reference §4](../quick-reference.md#4-config-fields-toml--unit--default) documents every field from `src/config/mod.rs` with unit + default.

### Conflicts found and how resolved (UN-011 / SR-021)
1. **H.265 advice contradiction (MAJOR):** README Performance Optimization §4 *recommends* switching to H.265, while Output Constraints §4 says *avoid* H.265 for frames. **Resolved:** added a note in the optimization section that for picture frames the default is H.264, only switch if the frame is verified to decode H.265, with a cross-link to the constraints section. (Both are now consistent: H.264 is the recommended default; H.265 is an explicit, caveated dev option.)
2. **CRF "recommended" default drift (MAJOR):** README Configuration Guide labeled CRF "22-26 ... (recommended)"; the PowerShell example used `Quality = 30`; Rust/constraints/SR-008 say default ~28 (26-30). **Resolved:** relabeled the README CRF table as *general guidance* and pointed the frame-friendly default (CRF 28) at the Quick Reference, which is now the single source. SR-008 (26-30) and Quick Reference (28) are consistent.
3. **`bulk_video_time_min` example drift (MINOR):** RUST_IMPLEMENTATION_SPEC shows a second config example with `bulk_video_time_min = 30, quality_crf = 26`; RUST_NEXT_STEPS shows `max_workers = 8, verbose = true`. These restated the canonical config with different values. **Resolved:** banner-flagged all four Rust planning docs as historical/design with samples superseded by the Quick Reference + `src/config/mod.rs`; the authoritative example is `test_config.toml`.
4. **Stale "stub / ready for Phase 1" status (MINOR):** RUST_NEXT_STEPS and RUST_PROJECT_SUMMARY described the engine as stubbed/not-yet-implemented, conflicting with the README's "working end-to-end". **Resolved:** banners point to the README Project Status as the live status owner.

### Doc gaps handed to System / Software Engineers
- **[MINOR → @software-engineer]** Config fields `exception_pattern`, `exception_threshold` (`[input]`) and `max_workers` (`[processing]`) exist in `src/config/mod.rs` but were **undocumented** in every prior doc and absent from `test_config.toml`. I documented them with best-effort units/defaults in [Quick Reference §4](../quick-reference.md#4-config-fields-toml--unit--default), but their exact semantics (how `exception_pattern`/`exception_threshold` interact, the `max_workers` default) should be confirmed against the implementation and reflected in `test_config.toml` as commented examples. This relates to SR-003 (every field documented with default+unit).
- **[MINOR → @system-engineer]** SR-003's `[input]`/`[processing]` field enumeration in its Permutations does not list `exception_pattern`/`exception_threshold`/`max_workers`; consider including them so the "every field documented" acceptance is complete.

---

## Recommendations (not blocking)
- Consider trimming/merging the four overlapping Rust planning docs (SPEC/ROADMAP/NEXT_STEPS/PROJECT_SUMMARY) into one historical "design notes" doc post-OBJ1; they overlap heavily and are now superseded by the README + Quick Reference. Banners are a stopgap.
- Add `exception_pattern`, `exception_threshold`, `max_workers` as commented lines to `test_config.toml` once their semantics are confirmed, so the example shows every field.
