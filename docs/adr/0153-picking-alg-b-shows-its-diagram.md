# 0153. Picking ALG B shows its diagram

- **Status:** Proposed
- **Deciders:** owner (0.2.0 epic #615, #565)

## Context
The ALGO page draws ALG A's diagram morphing into ALG B's with MORPH. At
MORPH 0, the usual state, ALG B's diagram is never on screen, so a tester
choosing B has to morph over to see what each algorithm is (#565, #338).

## Decision
- While the focused cell is ALG B, the viz band draws ALG B's own diagram
  (`Renderer::draw_band_viz`), whatever MORPH holds.
- A refused algorithm held by the UI (ADR 0151) draws too, so a dimmed
  choice can still be looked at.
- On any other cell the band is as before: A blending into B.
- Nothing is written, and the Sound's morph is not touched.

## Alternatives considered
- A thumbnail grid of all 32 per pick: no room in the band, and a turn
  already steps one at a time.
- Show ALG A the same way: ALG A is what the band shows at rest; not asked.

## Consequences
Picking B previews it live; the focus moving off B returns the blend.
`alg_pick_test` locks it.

## Sources
private issue #565, ADR 0151.
