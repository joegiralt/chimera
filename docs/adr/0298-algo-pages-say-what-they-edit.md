# 0298. ALGO's pages say what they edit: stage names, RATE SCALE's words, the ratio, feedback loops

- **Status:** Accepted (2026-11-20)
- **Deciders:** owner (the tickets, on 0.4.0 since 2026-11-15), firmware

## Context
Five tester tickets on ALGO's pages, done as one batch (the owner's rule: related tickets share a branch): #956 (AR reads as an attack time), #987 (are D1R and D1L swapped; the labels are cryptic), #988 (RATE SCALE gives no clue), #986 (FINE and DETUN move, nothing shows where the operator ended up), #989 (nothing shows which operators feed back). From rephazed, Elektronauts PreenFM3 thread post 251, and a 0.3.0 tester.

What the code had (origin/0.4 a1dd3a9c):
- The envelope's group pages were named `Env AR` … `Env RR`, their row words ATTACK · DECAY 1 · SUSTAIN · DECAY 2 · RELEASE, their cells OP1–OP6, so the focus band read `OP1` over a number.
- The order and wiring are right against the TX81Z (finding below).
- `UiDoor::notes` (ADR 0292) already carries bank 2's words to the band through one `Ask`.

## The #987 finding: nothing is swapped
Checked against the engine and the NRPN chart. The pages run AR, D1R, D1L, D2R, RR, the TX81Z's own order; each page's cells write the param its tab names (`ALGO_D1R` → `AlgoOpParams::D1R`, id 6, NRPN 3–8:6; `ALGO_D1L` → `D1L`, id 7, NRPN 3–8:7); `AlgoOpParams::rates` hands each to the `EnvRates` field of the same name; and `EnvCoefs` plays them as the TX81Z does (attack to full, D1R falls to D1L's level, D2R falls on while the key is held, RR on release). `each_envelope_cell_edits_the_parameter_its_label_names` pins all of it, per page and per operator.

What the tester most likely met: INIT has D1R 0, and at D1R 0 the envelope holds at full (as the TX81Z's does), so turning D1L does nothing, while the row called that page SUSTAIN. That reads as "these two are the wrong way round". No behaviour changes; the page now says so (decision 3).

## Decision
1. **The stages are spelled out** (#956, #987). The five pages are named ATTACK RATE, DECAY 1 RATE, DECAY 1 LEVEL, DECAY 2 RATE and RELEASE RATE: in the header (`BlockDef::name`; DECAY 1 LVL there, 95 px where 90 fit beside `PART 6 · SOUND`), on the map's row (`ROW_WORDS`), and in the focus band, which reads `ATTACK RATE` over the value where it read `OP1` (the cells stay OP1–OP6 and the lit one names the operator; the tabs keep the TX81Z's AR D1R D1L D2R RR, as the tickets ask). `OP1 ATTACK RATE` does not fit: the shortest is 148 px and the label has 139 px before the gauge (a test holds both). SUSTAIN is gone from the row: D1L is where decay 1 ends, and it is a sustain only while D2R is 0. The operator's own page spells FDBK out as FEEDBACK in the band (its cell is 65 px in a 62 px cell, ADR 0130's Ruling 12).
2. **RATE SCALE says what it does** (#988): `HIGH NOTES, FASTER ENVELOPES` (the ticket's `HIGHER NOTES, …` is 5 px over the line at the house tracking), in the hint's place while one of its cells has the focus. The label question: its cells read OP1–OP6, the header RATE SCALE, the tab RS; no cell repeats the tab, so nothing is dropped.
3. **DECAY 1 LEVEL says when it is never reached** (my call, from the finding): with that operator's D1R at 0 and its D1L under 15, `DECAY 1 RATE 0: NEVER REACHED`. Not at D1L 15, where D2 runs at once (`OpEnv`).
4. **CRSE, FINE and DETUN show the ratio they come to** (#986): `RATIO 1.41`, and with DETUN off 0 `RATIO 1.41, DETUNE +4.5 CENTS`, in the hint's place, on the group pages and the operator's own. The ratio is `tx::ratio(coarse, fine)` and the cents `detune × tx::DETUNE_CENTS`, the functions `AlgoEngine` multiplies the note's frequency by: one source of truth. Two decimals, as COARSE's own names and the TX81Z's display. A noise band has no pitch and shows none. Finer FINE and DETUN steps were asked for too: they change existing Sounds, so they stay a separate decision for the owner (the ticket's ruling).
5. **Each operator that feeds back wears a loop on the chart** (#989): an ear 3 px round on the node's right (its left where the right would leave the band), drawn under the nodes. MID while FDBK is under the TX81Z's step 4, ACCENT from there, where a sine starts to break toward a saw (`notes::Loop`). A noise band ignores FDBK and wears none.
6. **FBK's page shows the chart** (my call, for the ticket's "live: it appears as FDBK is turned"): `ALGO_FEEDBACK`'s band is `VizType::AlgoDiagram` at the stored MORPH, so a turn of any operator's FDBK draws or brightens its loop under the hand. ALG's page draws the same loops. FBK's page loses the live scope it had; the operator's own page keeps its envelope and wave.
7. **Bank 2.** Every word, the label, the ratio and the loops are `ui::notes`' (S4, `bank2.x`'s notes group), through `UiDoor::notes`' one entry: `Ask::OpLabel`, `Ask::Ratio`, `Ask::Loops`; RATE SCALE's and D1L's words ride `notes::cell` (`band_note`). Closed, the band reads as before (`OP1`, the plain hint) and the chart has no loops. Bank 1 gains the three door calls, the page names and row words, and the chart's key (measured below).

## Alternatives considered
- **`HIGHER IS FASTER` on the four rate cells** (#956): the ticket asks for the name, and a note there would hide MIX + SEQ's learn hint on five pages.
- **Full words in the cells**: the group pages' cells are the operators; the op page has no envelope cells.
- **The loop's size following the amount**: a larger ear crowds the compact layouts (11 px rows); two colours read at a glance.
- **A loop over the node's top**, as the DX7's panel draws it: it lands in the node above on a stack.
- **The loops drawn in bank 1** (`draw::ring`): about the door call's size, but new feature code in frozen bank 1.
- **The ratio in three decimals**: finer than COARSE's own names, and wider than the line.

## Consequences
- Sounds unchanged: UI only (the audio goldens stand).
- Screens that change on purpose: the five envelope pages (header, row, band), RATE SCALE's band, FBK's band (the chart), CRSE, FINE and DETUN's bands and the operator pages' on those cells. Goldens re-recorded; the list is in the PR.
- The browser editor's chart is the unit's renderer (WASM), so it draws the loops too.
- Guide: the pages above need new annotated screenshots.

## Measurements
See ADR 0300's table: the batch was measured once, as one branch.

## Sources
- #956, #986, #987, #988, #989 (https://github.com/joegiralt/chimera/issues).
- Elektronauts PreenFM3 thread, post 251 (rephazed).
- Yamaha TX81Z owner's manual (EG parameters and their order; frequency ratio display), ADR 0023.
- ADR 0086 (hints), 0130 (operator pages), 0153 (the chart), 0292 (`ui::notes`).
