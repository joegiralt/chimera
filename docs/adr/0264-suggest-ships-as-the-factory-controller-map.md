# 0264. Ship SUGGEST, a controller map generated from the param tables

- **Status:** Superseded by 0323
- **Deciders:** owner (2026-11-11: "can we gen a 'suggest map' for all engines"), firmware

## Context
ADR 0226 made controller maps card files and shipped none. A tester with a generic box of knobs had to learn every engine's cells by hand. ADR 0263 gives GM's CCs (1, 7, 10, 11, 71–74, 91, 93, 94) fixed meanings on every Part.

## Decision
- **One map, SUGGEST, in the factory pack** (`factory-v1.zip`, `CHIMERA/MAPS/SUGG05F6.A`). Not device-specific.
- **Generated, never hand-edited.** `chimera-utils suggest-map` (`just suggest-map`) builds it from the param constants (`AlgoParams::MORPH`…) and writes it through the firmware's own `ctl::save`. `just release` builds it fresh; `tools/factory_zip.py` refuses a pack without a map. A test loads the file through `ctl::load` and checks each engine's layer: 16 rows, none lost, each a live mappable cell, absolute `Jump`, on a CC MIDI leaves undefined and outside ADR 0263's. The guide's tables are generated too and checked against `docs/guide/MIDI.md`.
- **Layout.** 6 shared rows (CC 14, 15, 85–88: LFO 1 RATE and DEPTH, ENV 2 LEVEL, glide, the FX slot's AMOUNT and MIX) and 10 an engine (CC 20–29), so every engine's layer is the full 16. The same CC plays the same role on each engine: 20 the main timbre (MORPH, STRUCT, WAVE1, POSITION), 21 the second, 29 the character (FDBK, HALO, STAB, AGE).
- **Absolute `Jump`**, learn's default: a generic controller has no display, and PICKUP is one **E** turn away on MIDI MAP.

## Alternatives considered
- **Pickup by default.** Safer against jumps, but a knob that does nothing until it crosses the value reads as broken on a box nobody configured for it.
- **Per-controller maps (FADERFOX, LCXL3…).** Need each device's factory layout and a unit to check them on; SUGGEST works with any box set to its CCs.
- **A committed binary.** It drifts from the param tables; generating at release can't.
- **Fewer shared rows, more engine rows.** ALGO and MODAL already fill ten with their most played cells; LFO and FX rows are what every engine lacks under GM's CCs.

## Consequences
- The CC 20–29 / 14, 15, 85–88 layout is now user-facing: renumbering it breaks testers' controller setups.
- A param renamed or removed fails the generator's build or its load test, not a tester's card.
- Bank 1, RAM, stack: none (host tool and card data).

## Sources
- ADR 0226, 0263; MIDI 1.0 control change table (undefined CCs 3, 9, 14–15, 20–31, 85–90, 102–119).
