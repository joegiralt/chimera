# UI Refresh Design (Direction A — refined Elektron)

**Date:** 2026-09-24
**Status:** Approved (design approved in conversation; user said "execute")
**Builds on:** `instrument-core` (PR #17). Branch `ui-refresh`.
**ADR:** [0016 Visual direction: refined Elektron](../../adr/0016-visual-direction-refined-elektron.md).
**Mockups (approved):** https://claude.ai/artifact/B1ydzBo2GpjTY1QjH5hdvS — "Direction A" and "The other page types".

## Intent

Replace the test-equipment look (one 6×10 mono font, 1-px outline boxes,
grid lines, flat hierarchy) with Direction A across every page, as a
renderer/theme change: the page anatomy, navigation and parameter bindings
stay as they are. First pass = the foundation applied to all pages; the
existing visualizations keep their shapes, restyled.

## Principles (from ADR 0016)

- Dark ground, warm-grey text, **one accent** (cyan `#7fd4c8`-ish in RGB565)
  used only for the active element: focus arc, active cell, map pill,
  selected row/route.
- **No outline boxes.** Separation by spacing. Values as thin bars / arcs.
- **Type hierarchy** with `u8g2-fonts`:
  - Numerals: **Logisoso** (large for the focus value; medium for readouts).
  - Labels/values: a small clean proportional face from the u8g2 set
    (chosen in the plan; uppercase labels with slight tracking).
  - No `FONT_6X10` left in page rendering (the tiny 4×6 may remain only if
    unavoidable; the plan lists any survivor).
- Values that are choices display as text (POLY, P1, CH 1, pan L/C/R, mode
  names); numbers as numbers — reuse `ValFmt`/`fmt.rs`.
- All animated values keep going through the existing lerp (`AnimatedValue`);
  nothing snaps (CLAUDE.md).

## Shared components

1. **Header** (y 0–28): small grey context label + bold name
   (e.g. `PART 1` `PIZZA`; `MIXER` `PART 1`; `LOAD SOUND` `→ PART 1`); an
   accent dot at the right when the part is sounding.
2. **Focus band** (engine CellGrid pages and Mixer PART, y 28–118): the
   **last-touched parameter** (default slot a) — label + large Logisoso value
   + arc gauge (bipolar arc for bipolar params). Updates on the first encoder
   tick of another slot; stays until another is touched. No timer.
3. **Viz band**: the page's visualization, soft accent fill under a 1.5-px
   accent line, no grid.
4. **Cells** (six slots in knob order a–f, 3×2): label (accent when active),
   value, thin 2-px bar (bipolar bars grow from centre). Empty slots show a
   dim "—".
5. **Map** (y 266–320): nodes on a thin line; current block = filled accent
   pill with dark label; others = small ring + grey label below; sub-page
   branches drawn as indented nodes under the pill (same data as today's
   dungeon map, restyled).

## Page types

| Page type | Layout |
|---|---|
| **Engine / CellGrid** (Pizza, Modal, Drive, Folder, …) | Header · Focus band · Viz band = **live output** from the scope buffer as a filled waveform (replaces the separate scope strip) · Cells · Map |
| **BigViz** (Filter, envelopes, FM algorithm/operator pages) | Header · large Viz (no focus band) with the **touched value riding on the viz** (filter: marker + readout at the cutoff point; envelope: edited segment lit; FM algorithm: edited operator lit) · Cells · Map |
| **Mixer PART** | Header · Focus band · Viz band = **six-part overview** (level bars + pan dots; selected part lit) · Cells (CH, MODE, OUT, LEVEL, PAN as text/values) · Map |
| **Mixer SENDS / FX pages** | as Engine/CellGrid, viz band shows the effect's existing visualization restyled |
| **Mod matrix** | Header · Focus band naming the selected route (`LFO → CUTOFF`, bipolar value) · **dot grid** (sources down, primed destinations across; filled = positive, ring = negative, size = |amount|, tiny dim dot = none; selected cell outlined in accent) · hint line · Map |
| **Sound browser** | Header (`LOAD SOUND` `→ PART n`) · list rows (slot / name / engine tag), selected row = accent pill, empty slots dimmed, thin scroll indicator · key hints (EDIT load · SEQ save · B cancel) |
| System / Demo pages | shared header/cells/map styling only; no new layouts |

Exact y-coordinates are set in `theme.rs` layout constants; the plan fixes
them from the mockups.

## Engineering

- **Theme tokens:** `theme.rs` becomes the single source of colours, font
  handles and layout constants for Direction A; no colour literals in the
  renderer.
- **Fonts:** add `u8g2-fonts` (MIT/Apache) to `chimera-core`. Only fonts
  used are linked. Record each font's licence in ADR 0016's Sources
  (edit is allowed only as an addendum note "Font licences"; decision text
  unchanged) or a new ADR if that is cleaner.
- **Flash budget (ADR 0013 parity):** firmware must still link; the plan
  measures the `.text/.rodata` delta and records it. Target: fonts ≤ 16 KB.
- **RAM:** no new large statics; AXI headroom (~13 KB) must not shrink by
  more than a few hundred bytes (renderer state only).
- **Redraw regions:** keep the dirty-region system; add a `Focus` region;
  the scope strip region goes away where the viz band shows live output.
  Changes only redraw their region (SPI bandwidth).
- **Focus tracking:** `UiState` tracks the last-touched slot per page key;
  default slot 0. The Mixer PART and matrix pages use the same mechanism.
- **Legacy removal:** delete outline-box drawing paths, the separate scope
  strip, and dead theme constants once unused.

## Testing

- **Screen goldens:** render each page type into an in-memory 240×320
  RGB565 framebuffer (core test target implementing `DrawTarget`) and lock a
  hash per page type: engine page (Pizza), BigViz (Filter, Env, FM Op),
  Mixer PART, SENDS, Mod matrix, Sound browser, System. Re-record only when
  a task changes that page on purpose (same discipline as audio goldens).
- **Behaviour tests:** focus band follows the last-touched slot and survives
  page switches per page key; text formats (POLY/P1/CH/L-C-R) render;
  no pixel outside 240×320 is written; selected row/route/cell uses the
  accent colour; nothing snaps (value animates toward target).
- **Builds:** `just check` (core + hal + desktop tests, firmware link).
- **Manual:** run the desktop simulator and compare against the mockups.

## Out of scope

- New visualizations (redrawn filter/env/FM art) — later pass.
- Direction B/C elements; per-encoder colours.
- Long-press to open the Sound browser (#-to-file separately if wanted).
- Animation beyond the existing lerp (page transitions etc.).
