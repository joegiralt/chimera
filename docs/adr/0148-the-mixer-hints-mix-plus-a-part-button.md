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
- In the mixer (`Head::Mix`), a cell with no hint of its own, or only the
  generic MIX + SEQ learn, shows `MIX + B1-6 PART` in the focus band
  (`view::mixer_hint`). The two do not fit one line (300 px of 216), so the
  jump takes the learn's place. A cell's own words (REVERB ▸, CHORUS ▸,
  EDIT ▾ SYM) and PUSH ENCODERS' hint stay.
- It is kept short so it ends left of the focus arc gauge (the dirty
  redraw of the gauge clips a longer line).
- The hint is short so it ends left of the focus arc gauge; a longer line
  is clipped by the gauge's dirty redraw.
- The landing is named already: the header reads `PART n · MIX` and the
  PART page's strip n is accent with a bold number.

## Alternatives considered
- Rule the gesture out: MIX + B*n* from the mixer would reopen SENDS and
  lose the page. It breaks the page-keeping row of ADR 0044.
- Learn and jump together: 300 px, past the 216 a hint may take.

## Consequences
MIX + SEQ's learn is no longer hinted in the mixer; ADR 0139 documents it.
Amends 0086's "left without a hint" for MIX + B*n* in the mixer.

## Sources
ADR 0044, 0086; issue #564.
