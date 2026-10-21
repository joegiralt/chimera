# 0167. FLT's front reads KIND · FX · Q · MODE · FX PLACE · FREQ (supersedes in part 0155)

- **Status:** Proposed
- **Deciders:** owner (2026-10-20)

## Context

ADR 0155's layout A put FLT's front in the order KIND · MODE (2040: VOICE) · FX · PLACE · FREQ (2040: CUTOFF) · Q (2040: RES). The owner wants cutoff and resonance swapped, FX on encoder B and the placement on encoder E, and the PLACE cell to say what it places: "it says just PLACE, it should say FX PLACE."

## Decision

- **The table.** Cells run top row A B C, bottom row D E F, encoders 1:1: A KIND · B FX · C Q (2040: RES) · D MODE (2040: VOICE) · E FX PLACE · F FREQ (2040: CUTOFF). The same for every kind.
- **The label.** The PLACE cell reads `FX PLACE` (the cell's label override; the param, its spec name and its matrix header stay `PLACE`). It fits a cell column at the label font.
- **Nothing else moves.** Same params, same matrix views, same `MiniGlyph::Place` in E, FLT › MOD and FLT › FX unchanged. The layout is data (`FILTER`'s slot list), so saved Sounds, routes and MIDI maps, which are addressed by param, are unaffected.
- **Supersedes** the layout-A table of ADR 0155 and nothing else in it.

## Alternatives considered

- Relabel only, keep the order: does not give the owner the encoder positions.
- Rename the `PLACE` param itself: would touch the matrix header and focus bands for no gain.

## Consequences

Goldens for every FLT front page (CLEAN, DIRTY, LADDER, 2040 and their variants), `row_flt` and the LARGE VA FLT page are re-recorded on purpose. Tests that pinned cell positions or drove FLT's encoders follow the new table. Hardware muscle memory for FREQ and Q swaps.

## Sources

ADR 0155; owner request 2026-10-20.
