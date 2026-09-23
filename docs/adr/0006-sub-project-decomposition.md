# 0006. Decompose the pivot into five sub-projects

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
The pivot grew from "new engines" to include a patch builder on the device
and on the web. Adding an engine touched ~16 places, chains were fixed per
chain type, and patches had no persistent format.

## Decision
Five sub-projects, each with its own spec → plan → build cycle, in order:
1. Engine refactor (per-block params, generic modulation, slot-bound UI)
2. Composable chain model + versioned binary patch format
3. On-device chain editor (MIX+MENU)
4. New engines (SWAVE, GND, wavetable, SID-style, VO; Modal upgrades)
5. Web patch builder over USB MIDI SysEx (Web MIDI), sharing `chimera-core`
   compiled to WebAssembly so definitions can't drift

4 depends only on 1; 3 and 5 depend on 2.

## Alternatives considered
- **Engines first** — every engine would pay the ~16-site cost, then be
  refactored anyway.

## Consequences
Visible progress on sound is delayed until sub-project 1 lands.

## Sources
`docs/superpowers/specs/2026-09-23-engine-refactor-design.md`
