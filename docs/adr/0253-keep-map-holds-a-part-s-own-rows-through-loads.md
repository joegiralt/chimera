# 0253. KEEP MAP holds a Part's own rows through Sound loads

- **Status:** Proposed
- **Deciders:** firmware (#517; the owner's "stop pausing, get all 0.3 tickets done")

## Context
A Sound carries its own MIDI rows (ADR 0139, 0217), so a Sound load brings the loaded Sound's rows. The controller layer (ADR 0226) already survives any load, as it belongs to the Part and the project. The Sound's own rows don't, so a layout learned on a Part is lost as soon as the Part browses Sounds. The MIDI map spec (§ 12) filed `KEEP MAP` on load on MIX+PLUS, and ADR 0226 left the Part's own rows to it.

Constraints:
- A load into an `Edited` Part asks SAVE PART FIRST (the replace guard). If kept rows counted as an edit, every load after the first would ask, and browsing would stop at a prompt every step.
- Bank 1 is frozen (ADR 0198): the load paths (browser, library, RELOAD, #891's program change) are bank 1's.
- AXI is at its margin; the MIDI UI slot is in D2 (ADR 0226).

## Decision
- **The gesture.** MIX + PLUS on MIDI MAP's row 0 (`CONTROLLER`) turns KEEP MAP on or off for the active Part. Row 0's focus band shows `MIX + PLUS KEEP MAP OFF` or `ON` (the label as `mod_grid` and `source_cell` write the chord) on the line where other rows show MIN, MAX and ECHO (ADR 0086). Only row 0 answers: it is the Part's row, and MIX+MINUS already deletes on the others.
- **What it keeps.** The Part's own rows (the Sound's map), as they are when it is turned on and as later learned or edited. Each Sound load (browser, library, RELOAD, INIT, a program change) is followed by the kept rows written over the loaded Sound's.
- **Pulled, not pushed.** The MIDI UI's pass already sees a load by the Part's `Loads` count (it rests the ramps). There, with KEEP MAP on, it writes the kept rows back; otherwise it copies the Part's rows as they are now. No load path changes, so every present and future load path composes, #891's included. The rows land one UI pass after the load: a CC in that pass plays the loaded Sound's rows.
- **No edit.** After writing kept rows over a load that landed `Clean`, the pass records the Part's `sound_crc` (`Part::map_kept`, `Project::set_map_kept`). `part_status` reads an `Edited` Part whose CRC is that one as `Clean`, or `Stale` if its slot moved on since the load: no `*`, no prompt, `differ` skips it, and only SAVE TO NEW SLOT is offered. A load that lands edited (#891's program change returning a recall copy, ADR 0224) is not marked: it keeps its `*` and its prompt, so its edits are never dropped unasked. Any other edit moves the CRC, so it shows `*` and asks as before. A row learned while kept is such an edit until the next load (which asks once, and keeps the row on REPLACE).
- **Off.** Turning it off drops the mark on the next pass, so a Part whose rows differ from its Sound's reads `Edited`: save to keep them in the Sound, or revert.
- **Not saved.** KEEP MAP lives in the MIDI UI's state (`PartMidi`, D2), which a project load or NEW rebuilds: every Part starts off. The mark is cleared on the first pass after.
- **Placement.** Bank 2: the toggle, the pass, the key decode (`map_in` takes MIX, MINUS and PLUS as bits and makes the chords), row 0's words, label included (`ui::midi`), and `Project::set_map_kept` (globbed).
- **Bank 1's exception (ADR 0198, item 1: stubs and unavoidable churn).** `map_input` passes PLUS's bit beside MINUS's; row 0's `values()` returns bank 2's words as its one gesture, and the focus band draws that line on every row; `part_status`'s check (the CRC compare and the Stale test); the `Part` field (8 B a Part), whose wider stride is churn in every reader of `perf.parts`. Measured per set in `tools/bank1-budget.txt` and the PR (#895).
- **Bench builds** (no `midi-map`, so no `controller-maps`): none of it; `Part` and `PartMidi` don't grow.

## Alternatives considered
- **Kept rows played in place of the Sound's, never written into it.** No `*` question at all, but every reader of the Sound's map (resolution, echo, learn, the page, DELETE, a controller-map save) would need to read the kept copy instead: about 20 sites, several in bank 1.
- **Kept rows written in, `*` shown.** Honest, but each later load asks SAVE PART FIRST, which defeats browsing.
- **Keeping inside `Project::load_part`.** One place for the keep, but it is bank 1, the flag would have to be the Project's, and #891's recall path doesn't run it.
- **Saving KEEP MAP in the project.** A record in the project codec, in bank 1. The controller layer is the saved, cross-project way to keep a layout (ADR 0226); KEEP MAP is for a session's browsing.
- **A KEEP MAP row on MIDI MAP.** More visible, but it moves every row index the page, DELETE and the tests use, and the spec names the gesture.

## Consequences
- **RAM:** `PartMidi` grows by a `MidiMap` and a flag a Part (in D2, with `midi-map`); `Part` by an `Option<u32>` (D2).
- **CPU:** one `MidiMap` copy a pass for each Part with KEEP MAP on; one `sound_crc` a load for such a Part, and one in `part_status` while an `Edited` Part is marked.
- Measured flash and RAM deltas are in the PR (#517).

## Sources
- #517; spec `docs/superpowers/specs/2026-10-13-midi-map-design.md` § 12.
- ADR 0086, 0139, 0198, 0217, 0224 (#891), 0226.
- Ableton Live's "Lock to Control Surface" and Push's device lock: a control layout that stays while presets change.
