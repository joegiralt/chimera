# 0152. Numbers show the set value; modulation is a ghost

- **Status:** Proposed
- **Deciders:** project owner

## Context
A tester saw the filter cutoff number and the response curve's corner move
on their own when FM1, FM2, ENV AMT or an LFO modulated it (#575). Numbers
and curves were drawn from the modulated animator. #618 had made readouts
follow the animator's target so a turn shows at once; that target included
modulation.

## Decision
- Every number (cell, focus band, viz readout) shows the set value, never
  the modulated one. They read `renderer.set[i].target()`: a turn shows at
  once (#618's aim stands), and modulation never moves a number. This
  replaces #618's "follows the animator target" for modulation only.
- FLT's response curve is drawn at the set cutoff and RES, solid.
- Modulation on FLT is a separate, secondary mark (`viz::ModMark`): a faint
  band on the frequency axis under the plot from the set corner to where
  modulation has it, with a grey tick there; RES the same in the right
  margin. Grey tokens (FAINT, MID), never the accent. It is absent when
  nothing modulates, and a moving mark redraws only its two boxes, when it
  moves a pixel. Only FLT has a ghost; other pages show modulation in their
  bars and mod ring.
- Cell bars stay live (modulated), as before. A bar may disagree with its
  number; that split is tracked in #627.

## Alternatives considered
- A ghost corner on the curve: overlaps the curve and readout, and a redraw
  would damage the curve.
- Set values for the bars too: the key and the draw must read the same
  array; deferred to #627.

## Consequences
A modulated cutoff reads steady; the ghost says how far it is pushed.
New viz ghosts on other pages follow the same rule.

## Sources
#575, #618, #626, #627; ADR 0143 (cell glide).
