# Planning, design & historical docs

This folder holds the **planning, design, and performance-analysis documents** that
guided the project — moved out of the repository root to keep it tidy. They are kept for
history and context; for what the tool does **today**, start at the
[project README](../README.md) and the [Quick Reference](../docs/quick-reference.md)
(the single source of truth for commands, config, and recommended settings). The live
engineering registries (requirements/tests/traceability) live under [../docs/](../docs/README.md).

> Many command/config snippets in these files are **design sketches or historical
> snapshots** and may differ from the shipped code; `src/config/mod.rs` is the config
> schema of record.

## Current-era plans (Rust engine)

| Doc | Contents |
|---|---|
| [RUST_PERFORMANCE_PLAN.md](RUST_PERFORMANCE_PLAN.md) | **PROPOSED (2026-07):** phased speed-up plan for the shipped Rust engine — measurement, hardware encode, incremental segment cache, pipeline overlap, wgpu/Vulkan GPU rendering. |

## Rust rewrite (design / planning)

| Doc | Contents |
|---|---|
| [RUST_PROJECT_SUMMARY.md](RUST_PROJECT_SUMMARY.md) | High-level summary of the Rust rewrite effort (historical snapshot). |
| [RUST_IMPLEMENTATION_SPEC.md](RUST_IMPLEMENTATION_SPEC.md) | Intended architecture & data model. |
| [RUST_IMPLEMENTATION_ROADMAP.md](RUST_IMPLEMENTATION_ROADMAP.md) | Phase-by-phase implementation plan + status. |
| [RUST_NEXT_STEPS.md](RUST_NEXT_STEPS.md) | Early quick-start / next-steps guide (historical). |
| [OPTIMIZATION_PATHS.md](OPTIMIZATION_PATHS.md) | Analysis of the improvement options (why Rust was chosen). |

## Performance optimization (PowerShell pipeline)

| Doc | Contents |
|---|---|
| [README_PERFORMANCE.md](README_PERFORMANCE.md) | Index/overview of the performance docs below. |
| [PERFORMANCE_REVIEW.md](PERFORMANCE_REVIEW.md) | Deep technical bottleneck analysis. |
| [PHASE1_OPTIMIZATIONS.md](PHASE1_OPTIMIZATIONS.md) | Phase 1 "quick wins" implementation guide. |
| [PHASE2_IMPLEMENTATION.md](PHASE2_IMPLEMENTATION.md) | Phase 2 C# wrapper implementation guide. |
| [REFACTORING_SUMMARY.md](REFACTORING_SUMMARY.md) | Decision matrix & roadmap across the phases. |

## Workflow

| Doc | Contents |
|---|---|
| [RESUME.md](RESUME.md) | "Where were we" pointer into the multi-agent grind ([../docs/status.md](../docs/status.md)). |
