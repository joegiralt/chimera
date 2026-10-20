# 0178. MODEL lists ONE, LOOP and PING beside the other machines, and DECK holds the glyph's knobs

- **Status:** Proposed
- **Deciders:** owner (UAT, 2026-10-19, #651), firmware
- **Supersedes in part:** [0170](0170-tape-machines-are-play-kit-tron-reel-scrub-stretch-and-wave.md) § PLAY (ONESHOT, LOOP and PING-PONG folded in as MODE)
- **Amends:** [0173](0173-tape-machine-glyphs-are-knob-driven-still-and-live.md) (which cells carry the glyph, the still cell, the dimmed rule's reach)

## Context
ADR 0170 folded ONESHOT, LOOP and PING-PONG into one PLAY machine with a MODE cell on its model page. At UAT the owner found the two levels confusing: "you're in ONE and you think… I thought I was in a different mode". MODEL said PLAY while the machine was really ONE, and the glyph (0173) already drew ONE, LOOP and PING as three machines.

The pages were laid out by param, not by what the machine shows. DECK held POSITION, MOTION, TAPE and AGE on every machine. ONE ignored POSITION and MOTION there (dimmed cells, 0173's dimmed glyph), and its START and END, the knobs that burn its match, were on SRC only. TAPE and AGE sat on DECK while WOW sat at the end of each model page.

## Decision
- **MODEL lists seven machines:** ONE, LOOP, PING, TRON, SCRUB, STRETCH (STRCH), WAVE, in that order (`TapeMachine`, `MODEL_NAMES`). MODEL's value is that index. The neutral is LOOP (1).
- **The block keeps PLAY and its MODE.** `TapeEngineParams` still holds `model: TapeModel` and `mode: PlayMode`. MODEL's `get` is `TapeMachine::of(model, mode)`. Its `write` sets the model and, on ONE, LOOP or PING, the MODE. Off PLAY the MODE is left as it was, and no machine reads it. MODEL's disk code is the model's (PLAY 0, TRON 6, SCRUB 4, STRETCH 5, WAVE 3) and MODE keeps id 15 and its codes. Files and the sparse codec are unchanged, so old saves load as they did. MODE has no cell, and `reads` counts it with MODEL.
- **Each machine is a row of `machine::MACHINES`** (seven, by `TapeMachine::index`): `deck`, DECK's cells after MODEL, and `cells`, the model page's cells after TAPE, AGE and WOW. `reads(machine, id)` is that table plus MODEL, MODE, START, END, TAPE, AGE, WOW, ROOT and PERIOD. `reads_in(model, mode, id)` is `reads` of their machine.
- **DECK (page 1) is MODEL, then every knob that moves the machine's glyph:**

  | Machine | DECK after MODEL | Model page after TAPE · AGE · WOW |
  |---|---|---|
  | ONE | START, END | FADE (ends in HOLD) |
  | LOOP | POSITION, WALK, LENGTH, XFADE | FADE |
  | PING | POSITION, WALK, LENGTH | FADE |
  | TRON | LENGTH, FADE | — |
  | SCRUB | FINGER, SPEED, LENGTH, XFADE, DRAG | — |
  | STRETCH | SPEED, D-TIME | POSITION, TYPE |
  | WAVE | FRAME, SCAN, RANGE | — |

  ONE's START and END show on DECK as well as SRC, as one param in two places. `deck_is_what_moves_the_glyph` pins the rule: a glyph input moves the key on a machine exactly when its cell is on DECK.
- **The model page (page 2) opens on TAPE, AGE and WOW in cells A, B and C on every machine.** They still wear the glyph there (TAPE's bend, AGE's grain and dropouts). The page opens focused on a live cell, never an empty one, on TRON, SCRUB and WAVE too. A control that doesn't move the glyph goes after them, in D to F: PLAY's FADE (a release), STRETCH's POSITION (the bellows draw neither it nor TYPE) and TYPE. The page is named by its machine (ONE, LOOP, PING, TRON, SCRUB, STRCH, WAVE).
- **A MODEL change writes the new machine's defaults**, as before, and ONE, LOOP and PING are machines now. So LOOP → PING resets POSITION, WALK and LENGTH as LOOP → TRON does, where a MODE edit wrote only FADE. ONE's FADE default is HOLD, the others' 32 ms. The window (START, END) and the wear (TAPE, AGE, WOW) are never written.
- **The glyph reads MODEL alone** (`TAPE_GLYPH_PARAMS`: 10 inputs, MODE gone; the door's `inputs` takes `[f32; 10]`). `tape_glyph::Kind` is `TapeMachine`. MODEL's cell shows the still pose. The other machine cells, now DECK's and the model page's, show the live glyph.
- **No TAPE page cell is dimmed.** A machine lists only what it reads, so 0173's dimmed-glyph rule has no TAPE cell left to apply to, apart from STRETCH's TYPE over an unpitched source. That cell reads POLY (UNPITCHED): a fixed text, not a param the machine ignores. So, like every dimmed cell but those, it shows no gauge, and its long word has the band. What a machine ignores is still dimmed where it shows elsewhere (the matrix's TAPE columns).

## Alternatives considered
- **Keep MODE, rename PLAY to the MODE's word on MODEL.** One turn would still not reach ONE from TRON, and the page would still carry a cell that changes the machine.
- **Seven disk codes (ONE and PING their own MODEL codes again).** It would re-key every saved PLAY Sound and revive 0170's retired codes 2 and 7 with a new meaning. The flat list is a view over the same two fields.
- **Fill DECK's spare cells with non-glyph controls** (LOOP's FADE in F, STRETCH's TYPE beside SPEED). It would make DECK mean "whatever fits". The owner's rule is that DECK is what the picture shows, and FADE was the example of a control that moves to page 2.
- **TAPE and AGE on DECK as before.** They wear every glyph but are no machine's own shape. On page 2, in the same cells on every machine beside WOW, they read as the one tape-wear group.

## Consequences
- **Flash:** ORBIT's bank 1 gains 172 B (34,024 → 34,196 B free on `tape-streaming`); the per-mode special cases in `reads_in` and the MODE edit are gone.
- **Goldens:** every TAPE DECK and model page is re-recorded on purpose (`tape_deck_*`, `tape_page_*`, `tape_strip_*`). The PNGs for the owner are in (private notes), outside the repo.
- **CC learn, the matrix and the header** name the seven machines. A CC on MODEL counts the new machine's default writes against the Part's knob budget, as before.
- **A MODEL turn from LOOP to PING now resets LOOP's cells** to PING's defaults, which a MODE turn did not do.
- **KIT and REEL** (0170) join the list as machines of their own, with DECK cells for whatever moves their glyphs.

## Sources
- #651 (the owner's ask), #645 and #648 (the dimmed-glyph UAT), #641 (the glyphs).
- ADR 0170 (the lineup), 0173 (the glyphs), 0093 (the machine table).
- `chimera-engine/src/dsp/tape_engine/{params,machine}.rs`, `chimera-core/tests/tape_pages_test.rs`, `chimera-core/tests/tape_glyph_test.rs`.
