# Design note: better multiple-frame support in the setup GUI

_Status: exploration / options only — no GUI code is being changed in this pass._
_Relates to UN-007 / SR-017 (multiple output definitions) and SR-026 (first-run wizard)._

## The problem

The config already supports **multiple `[[outputs]]`** — one MP4 per block, each with
its own resolution, fps, quality, audio, etc. ([SR-017], implemented). The slideshow
engine produces them all in one `build`.

The **first-run GUI wizard** ([`src/setup/wizard.ps1`](../../src/setup/wizard.ps1)),
however, only asks for a single **"Number of frames"** integer. That value maps, via
[`build_config`](../../src/setup/config_builder.rs), to N **identical** outputs named
`frame-1..frame-N` — same width/height/fps/quality. Producing several byte-for-byte
equivalent videos is rarely what anyone wants; the *point* of multiple frames is
**different sizes** (e.g. a 1920×1080 living-room frame and a 1440×900 bedroom frame).

So today, a non-programmer who wants two different frame sizes has no GUI path — they
must hand-edit the TOML. This note lays out the options for closing that gap.

## Options

### A. Editable frame table (WinForms `DataGridView`) — most capable

Replace the single "Number of frames" box with a grid: **one row per frame**, with
editable columns `name`, `width`, `height`, `fps`, `quality_crf`, `pic_display_time_secs`,
`enable_audio` (and room for more). Buttons: **Add**, **Duplicate selected**, **Delete
selected**. On OK the wizard emits one record per row.

- **Pros:** directly matches the mental model ("a list of frames I'm targeting");
  duplicate-then-tweak is exactly the common workflow; fully covers heterogeneous sizes.
- **Cons / work:**
  - `DataGridView` setup + per-cell validation (even dims, CRF 0–51, numeric fps) in PowerShell/WinForms — moderate effort.
  - The wizard's stdout contract is currently flat `key=value` lines parsed by
    [`parse_wizard_output`](../../src/setup/config_builder.rs). A table needs a
    **repeating/0-indexed encoding**, e.g. `frame.0.width=1920`, `frame.0.height=1080`,
    `frame.1.width=1440`, … and `build_config` must accept N heterogeneous rows instead
    of one template × N. New unit tests for the multi-row mapping.
  - Slightly busier first-run UI (mitigate: pre-seed one sensible default row).

### B. Presets + "add another size" — middle ground

Keep the current single primary frame, plus a small repeater: a **preset dropdown**
(1920×1080, 1440×900, 1280×800, 1024×768, custom) and an **"Add this size"** button
that appends a secondary output cloning the primary's fps/quality at the chosen size.

- **Pros:** much less UI than a full grid; covers the most common case (same look, a few
  sizes); smaller change to the stdout contract (a list of extra `WxH` sizes).
- **Cons:** can't independently vary fps/quality/audio per frame without falling back to
  the table or TOML; presets drift from reality as panels change.

### C. TOML-only (today) — zero GUI change

Document that multiple distinct frames are configured by adding `[[outputs]]` blocks to
the config, and keep the wizard single-frame. The quick-reference already links here and
to the field table.

- **Pros:** no code; the capability fully exists; power users are comfortable here.
- **Cons:** the GUI's "Number of frames" remains misleading (identical outputs); a
  non-programmer still can't get two sizes from the GUI alone.

## Recommendation

If/when this is prioritized, **Option A (frame table)** is the right end state — it is the
only one that matches the feature (independent per-frame settings) and makes
duplicate/delete first-class, at the cost of a `DataGridView` + a repeating wizard
encoding. **Option B** is a reasonable interim if the goal is just "same look, a few
sizes." Until either is built, **Option C** stands: the config supports it and the docs
point there.

Independently of which GUI path is chosen, the current wizard's **"Number of frames"
should be revisited** — emitting N identical outputs is a footgun; at minimum the label
should say the frames are identical, or it should be replaced by Option A/B.

[SR-017]: ../requirements/system-requirements.csv
