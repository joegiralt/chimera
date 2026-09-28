# 0041. The mod matrix is an amount grid of outlined cells (supersedes in part 0016)

- **Status:** Accepted (2026-09-28)
- **Deciders:** owner (mockup review, 2026-09-28), firmware (#161)

## Context
ADR 0016 rules out outline boxes. The MTX page drew a dot per route under a
large focus band, showing three sources at a time; the owner's approved
mockup (env-ui, screen e6) instead shows every source at once as a grid of
outlined cells that print their amounts.

## Decision
MOD › MTX is a grid: all eight sources as rows (no vertical scroll, so
encoder C does nothing there), grouped as the mockup groups them: ENV1-3,
LFO1-3, VELO, NOTE, mapped to `ModSource` indices by one table
(`ROW_ORDER`) so saved routes and the audio matrix are unchanged; primed destinations as 36 px columns, five
visible between 12 px margins, scrolling sideways once the cursor passes the
last one; scrolling sideways (encoder D) drags the cursor along so it stays
on screen. Every cell is a FAINT outline; a route fills it with ACCENT_SOFT
and prints its amount in ACCENT (`0` unfilled, in the rest grey); the
cursor's outline is ACCENT. Column headers use a spec's `short` label where
its `label` is wider than 33 px (`CUT`, `MRPH`, `SHAP`). The big readout and
arc go. Under the grid, two lines: the route by full names (`ENV 3 → FILTER
CUTOFF`, the block tag only if the line would pass 216 px, the param name
never cut), then the amount and its effect in the destination's terms,
from the spec's offset law (`+42 = +3.3 oct` for CUTOFF, `+64 = +50%` of
the span for linear params); `--` with no route. The
exception to 0016's "no outline boxes" is this grid only.

## Alternatives considered
- Keep the dot grid: three sources on screen and amounts only as dot size,
  so reading a patch meant scrolling and guessing.
- The mockup's 42 px columns: only four destinations visible instead of
  five, for no gain in legibility.
- Clip headers to the column in pixels: arbitrary truncations (`CUTO`) that
  depend on the font, not on a name anyone chose.

## Consequences
Every route and its exact amount read at a glance, and the cursor's in
musical terms. Two readout lines always, so the band never reflows as the
cursor moves; the hint and route count move down to fit. Each new modulatable
param whose label is wider than 33 px needs a `short`; a test enforces it.
The readout carries the full name.

## Sources
Issue #161; `chimera-core/src/ui/mod_grid.rs`; env-ui mockup screen e6.
