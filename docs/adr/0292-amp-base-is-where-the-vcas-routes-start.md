# 0292. AMP's BASE is where the VCA's routes start

- **Status:** Proposed
- **Deciders:** owner (2026-11-19, tester request chimera-releases#10, the design's recommendation B), firmware

## Context
A tester routed PRESS alone into VCA and found the note near-silent until the pad was pressed; ENV 1 +50 with PRESS −50 brought the attack back and let pressure duck it, but a positive PRESS on top of an ENV 1 that already reaches full did nothing. They asked whether a VCA route replaces the amp contour or adds to it, for a hint on the first VCA route, and for a way to keep the normal attack and swell or duck with pressure (design: (private notes), #10).

What the code did (origin/0.4 b136ef37):
- `Voice::render_mono`: the VCA's gain buffer exists only with a route into VCA, starts at 0.0, and each route adds `amount/127 × source`; the sum is clamped to 0..1 and multiplies the voice. With no route the engine decides (ALGO's carrier EGs, MODAL's decay, DRUM's kernels; TAPE and VA by the key).
- OUT `VCA` (OUT param 2, NRPN 18:2, ident `VCA`) was a stored, modulatable parameter whose value nothing read: "its value is the sum of its routes". No Sound stores anything but its default 0 (no factory recipe writes `OUT.VCA`; every INIT stores 0; RANDOMIZE never touches OUT).
- The Cascadia's VCA A has an initial LEVEL beside its CV input (ADR 0036 already follows the Cascadia's modulators).

The owner chose, on 2026-11-19, to expose the stored value as a cell, bit-identical for saved Sounds, and a band hint in the matrix that says routes into VCA replace the contour.

## Decision
1. **The stored VCA value is BASE: the sum of the routes into VCA starts there.** With a route into VCA, the gain buffer is filled with BASE, then each route adds as before, and the clamp to 0..1 is unchanged: `gain = clamp(BASE + Σ amountᵢ/127 × sourceᵢ, 0, 1)`. BASE 127 + PRESS −64 plays the engine's contour and ducks it by half under full pressure; BASE 64 + PRESS +63 swells it; BASE 0 is today's pressure-only VCA.
2. **0 is today's VCA, bit for bit.** At BASE 0, last block and this, the buffer stays the zeroed one, untouched (`vca_start` isn't called); a steady BASE steps by +0. Every saved Sound stores 0, so every saved Sound plays as before (proof below).
3. **With no route BASE does nothing**, and its cell is dimmed, as VEL's is (`view::dimmed`), the band saying `NEEDS A ROUTE INTO VCA`. My call: the alternative, BASE above 0 engaging the VCA on its own, would jump from the engine's own contour at 0 to a flat −40 dB gain at 1, a discontinuity on the first tick; and BASE alone is OUT LEVEL.
4. **BASE glides as a knob** (`Smoothing::KNOB`, 30 ms, one of the Part's `KNOBS`) and is ramped across each block from the value the voice played last block (`played.out.vca`, read before the block overwrites it: no state of its own), so a turn never steps the gain. A fresh note's first block ramps from the last note's BASE, under the attack.
5. **BASE above 0 holds the voice as the engine would with no route** (`Voice::base_holds`, inside `vca_holds`, so the gate's `holds` takes it too): ALGO, MODAL and DRUM until their own tails end (the carriers' release rings out); TAPE and VA, which never end, while the key is down. A pressure-only VCA (BASE 0) still ends at key up through the fade, as before. The match over `EngineSlot` has no wildcard.
6. **The name is BASE**, on the AMP cell only (`ParamSlot::with_label`). My call, the design's: LEVEL collides with OUT LEVEL, OFFSET reads as a route. The spec's label, ident and NRPN stay `VCA`: the matrix column, its readout (`OUTPUT VCA`), the editor's address (`OUT.VCA`) and the NRPN chart name the destination, and the cell names where its sum starts.
7. **The VCA's law stays linear.** My call, the design's default: a squared law would make pressure feel less "dead until I lean in", but it changes every Sound with a VCA route. BASE answers the "keep the attack, swell or duck" ask without it.
8. **AMP is VEL · LPG · DECAY · STRIKE · BASE · F.** F is ADR 0294's.
9. **The matrix says what VCA routes do** (`ui::notes::matrix`): on the VCA column, on a cell with no route, the hint is `VCA: ROUTES REPLACE CONTOUR` while no ENV is routed into VCA, and `VCA: SUM STARTS AT AMP BASE` once one is. A route's own cell keeps `MIX + MINUS DELETES THE ROUTE`; an X cell keeps its tap words. Each fits the 216 px a hint may take.
10. **Bank 2 holds the words.** `ui::notes` (the matrix hint and BASE's note, its words one named static) is an S4 group (`*12chimera_core2ui5notes*`, pinned there by `check-door-placed`). The matrix hint is reached through a new `UiDoor` entry, `notes` (an `Ask` enum, as `feel`'s, for the 0.4 tester work's later asks); a cell's note through `walk::band_note_in`, bank 2's already, after TAPE's. Bank 1 keeps one stub, `view::matrix_hint`. A closed door shows no note and the grid's own hint. `mod_grid::inert_dests` never marks the VCA column inert: BASE's dimming is only its wait for a route.

## Alternatives considered
- **Words only (the design's A):** a FAQ entry and the matrix hint. Kept as part of this (the hint and the FAQ text below), but alone it leaves no way to keep the engine's contour under a pressure route on ALGO and MODAL, where ENV 1 is not the carriers' shape.
- **Auto-baseline (C):** MIX + PLUS into an empty VCA column adds ENV 1 +127. Wrong on ALGO (ENV 1 is not the carrier EGs' shape, so the attack changes); VA and TAPE already route ENV 1 at INIT.
- **BASE on its own engages the VCA:** see decision 3.
- **A squared VCA law:** see decision 7.
- **The cell's spec label BASE:** the matrix readout and the editor would then name the destination BASE.

## Consequences
- RAM: no voice state. `Knobs` and the knob publisher grow one knob a Part: 96 B of D2 on bench-algo's link, which had none to spare, so its bench text gives 128 B (`BENCH_TEXT_LEN` 5,632 → 5,504, as ADR 0217 and 0252 did).
- CPU: none at BASE 0 (no call). Above 0, `vca_start` writes the 64 samples over the zeroed buffer once a block per routed voice, about 130 cycles (0.02 % a voice); the bill (`ModRouting::cost`, the VCA's `CLAMP` term) is not raised for it. Host, release, best of five runs of 20,000 blocks: an ALGO voice under a PRESS route renders a block in 3,263 ns at BASE 0 and 3,344 ns with BASE moving every block (+2.5 %, the worst case: a still BASE above 0 costs the fill alone). Not read on the unit.
- Flash: below.
- The matrix hint on the VCA column replaces `MIX + PLUS ON A PARAM PRIMES IT` there; the fixed columns are permanent, so priming never applied to them.
- Docs: `docs/chimera-sitemap.md`'s AMP row describes BASE and the matrix's words. The AMP page and the matrix's VCA column need new annotated screenshots for the tester guide.

## FAQ entry (chimera-releases `FAQ.md`, Modulation; text only, not published here)
> **What does a route into VCA do to the engine's own envelope?**
> Routes into VCA are the amp. With no route, each engine shapes its own level: ALGO's carrier envelopes, MODAL's decay, VA's and TAPE's key. The first route into VCA replaces that: the note's level is AMP's BASE plus every route into VCA, stopping at full. BASE is 0 to begin with, so PRESS alone is silent until you press.
> To keep the normal attack and duck with pressure, set BASE (AMP, cell E) to 127 and route PRESS at −64. To swell with pressure, set BASE to 64 and route PRESS at +63. On VA and TAPE, ENV 1 is already routed into VCA at +127, so it reaches full on its own: a positive PRESS on top can't go higher. Lower ENV 1's amount to leave room, or use BASE instead.
> Pressure is linear: 10 % pressure is 10 % of the route's amount.

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, origin/0.4 b136ef37 against this change, each set built in both trees. Each budget line is raised by exactly its set's measured change, never the slack:

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---|---|
| orbit | +200 | +320 |
| default | +288 | +320 |
| bank2-gate | +240 | +320 |
| orbit,usb-config-writer | +216 |  |
| fault-inject | +272 |  |
| bench-algo | +312 |  |
| bench-algo,master-tape | +264 |  |
| bench-routing | +360 |  |
| bench-memory | +336 |  |
| bench-stress | +264 |  |
| bench-orbit | +248 |  |
| bench-tape | +248 |  |
| filter-probes | +264 |  |

Bank 1's growth is `vca_start` (60 B, out of line and sized), `render_knobs`' call to it and the knob's spec walk, `KNOBS`' new row (20 B), the renderer's `matrix_hint` stub and `inert_dests`' VCA test. The bench sets, which link no UI door, keep `ui::notes` dead and lose more inlining. ORBIT bank 1 has 18,256 B free after it (917,504 less 899,248).

Null: `golden_test` (every factory Sound and case, voice and instrument paths) and `vca_lpg_test::the_gate_at_zero_is_todays_vca_bit_for_bit` (every engine's INIT through no route, ENV 1, ENV 1 + LFO 1, five filters, two VELs, every VA MODE, uneased and glided) hash bit for bit as before. `vca_base_test` holds the rest.

## Sources
- Design: (private notes) § #10.
- Tester request: chimera-releases#10.
- Code: `chimera-engine/src/dsp/voice.rs` (`vca_start`, `Voice::base`, `base_holds`), `params.rs` (OUT VCA), `chimera-core/src/ui/notes.rs`, `block_registry::AMP`.
- Intellijel Cascadia manual, VCA A's LEVEL.
