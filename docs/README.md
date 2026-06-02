# Documentation Index

Engineering documentation for Pictures2VideoSlideShow, produced and maintained by the [multi-agent grind](process.md). Start at the [project README](../README.md) for overview, setup, and the Rust engine; this folder holds the requirements, traceability, architecture, and test artifacts.

## Map (single source of truth per topic)

| Doc | Owner | Contains |
|---|---|---|
| [quick-reference.md](quick-reference.md) | UX Designer | Rust engine cheat-sheet: commands, recommended frame settings, config fields (unit+default), current gaps — the single source for these facts |
| [process.md](process.md) | shared | Roles, objectives/gates, ID scheme, traceability & anti-duplication rules, verdict protocol |
| [status.md](status.md) | orchestrator | Current objective/round, gate sign-offs, review log |
| [requirements/user-needs.md](requirements/user-needs.md) | End User | User Needs (UN-###) + edge-case expectations |
| [requirements/system-requirements.csv](requirements/system-requirements.csv) | System Engineer | System Requirements (SR-###) + acceptance criteria |
| [requirements/low-level-requirements.csv](requirements/low-level-requirements.csv) | Software Engineer | Low-Level Requirements (LLR-###) ↔ code |
| [test/test-cases.csv](test/test-cases.csv) | Test Engineer | Test Cases (TC-###) ↔ requirements |
| [test/report.md](test/report.md) | generated | Coverage + traceability (orphans) — produced by `scripts/trace.ps1` |
| [architecture.md](architecture.md) | Software Engineer | One-page flow + generated module/function map |
| [ux/notes.md](ux/notes.md) | UX Designer | Usability findings, doc map, (UI assets if any) |

Anything in these docs is referenced **by ID**; no topic is duplicated across files (see [process.md §4](process.md#4-traceability--anti-duplication-read-this-carefully)).
