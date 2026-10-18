# 0148. The mixer hints MIX + B1-6 as the jump to another Part (amends 0086)

- **Status:** Proposed
- **Deciders:** firmware (issue #564, tester feedback 2026-10-16)

## Context
In the mixer, MIX + B*n* keeps the page and opens Part *n*'s. ADR 0044
designed it: "MIX+B*n* inside the mixer keeps the page, PART included".
A tester found the Part change unexpected. ADR 0086 left the ladder's
MIX + B*n* without a hint, as it belongs to no cell. In the mixer it
belongs to every cell.

## Decision
- The gesture stays. It is consistent with MIX + B*n* = Part *n*'s mixer.
- In the mixer (`Head::Mix`), a cell with no hint of its own shows
  `MIX + B1-6 PART`; a cell with only the generic learn shows it with the
  learn's short form after it, `MIX + B1-6 PART  MIX + SEQ LEARN` (214 of
  216 px). Both gestures stay hinted (`view::mixer_hint`). A cell's own
  words (REVERB ▸, CHORUS ▸, EDIT ▾ SYM) and PUSH ENCODERS' hint stay.
- The line runs under the focus glyph's box. A glyph's own redraw clears
  that box, which takes the hint's top row, so it redraws the hint after
  (`Renderer::redraw_gauge`).
- The landing is named already: the header reads `PART n · MIX` and the
  PART page's strip n is accent with a bold number.

## Alternatives considered
- Rule the gesture out: MIX + B*n* from the mixer would reopen SENDS and
  lose the page. It breaks the page-keeping row of ADR 0044.
- `MIX + B1-6 OTHER PART` with the full `LEARNS A CC`: 300 px, past the 216
  a hint may take. Short wording fits, so no alternating hints.

## Consequences
The learn hint reads `LEARN` in the mixer, `LEARNS A CC` elsewhere.
Amends 0086's "left without a hint" for MIX + B*n* in the mixer.

## Sources
ADR 0044, 0086; issue #564.
