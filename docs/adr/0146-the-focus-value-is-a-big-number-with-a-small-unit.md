# 0146. Set the focus value as a big number with a small unit

- **Status:** Proposed
- **Deciders:** owner, firmware

## Context
The focus band sets a value in the 42 px face beside a gauge. `2.14 kHz` in that face runs 130 px and the arc began at 157, so long values drew under it (#590). Hiding the arc on those values would have taken it from CUTOFF 77 % of the time, GLIDE 91 % and a formant's BW 74 %.

## Decision
- `components::split_unit` splits a value into a number and a trailing unit (`Hz`, `kHz`, `ms`, `s`, `%`, `BT`). The number is set in the focus face, the unit in the value face (`FONT_VALUE`) on the number's baseline, `UNIT_GAP` = 4 px after it. A word (`SINE`, `PLAY`) has no unit and stays whole in the focus face.
- The value, so set, must clear the gauge: `FOCUS_VALUE_X` + width + `GAUGE_GAP` (4 px) at most `gauge_left`, each gauge's leftmost drawn column.
- The fallback, in order (`components::layout`): number and unit with the gauge; past that, the value alone in the focus face and no gauge; past the row (216 px), the whole text in the value face, as for `NOT A TRANSWAVE`.
- Nothing overlaps, at any value, on any gauge. `Renderer::gauge` applies the same rule, so an animated gauge is not redrawn over a hidden value.
- Every Hz, kHz, ms or s value of an arc param keeps its arc (`focus_overlap_test`).

## Alternatives considered
- Hide the gauge where the whole value does not fit: it lost the arc on most of the params it matters on.
- Shrink the whole value to the value face: the number stops reading at a glance.
- Stack the unit below the number: the band's label and hint rows leave no height for it.

## Consequences
- The focus band's units read small; the goldens of every screen whose focus value carries a unit changed once.
- A new param is covered by the sweep: its widest value, its gauge and its layout changes are checked without a new case.
- A unit that makes a number wider than the room left of its gauge costs that param its gauge, and the test names it.

## Sources
- Issue #590.
- Elektron and Cascadia-style readouts: a large number, a small unit.
