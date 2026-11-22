# 0311. A ringing DRUM hit bills the filter it runs and the follow it does

- **Status:** Accepted (2026-11-22)
- **Deciders:** firmware (bug batch E: #962, #977, #836)

## Context
The DRUM bill (ADR 0269, 0274, 0287) is worked out from the kit as last published. A ringing hit does not play the kit as it is now: it follows its pad's continuous values (ADR 0269), but it keeps the choices it was struck on: filter KIND, MODE and VOICE, and each route's SOURCE and DEST (`PadSound::live_target`). Two gaps followed from that.

- **#977: a hit billed for a filter it no longer has.** Strike a LADDER pad, then publish the kit with that pad on CLEAN. The hit still runs LADDER, but recost billed it at the kit's CLEAN: 965 → 789 a sample for the rest of its ring. With STACK, eight such tails are about 1,400 a sample short.
- **#962: the follow itself was not billed.** A pad edit on a ringing hit costs three things, none of them booked:
  - the publish's block decodes the pad (`KitView::sound_if_newer`);
  - the glide then steps the filter's coefficients (`FilterKind::glide_cost`);
  - each gliding block eases 20 places and rebuilds both envelopes' coefficients (`AmpEnv::coefs`).
- **#962, an over-bill.** `drum_glide_extra` folded the Sound's own filter glide into the DRUM bill, although the pads mask it once a kit is read. `drum_cost` already took the pads alone.

## Decision
- **The kit's bill takes in each ringing hit's struck sound.** `KitBill::with_sound` adds a sound's filter shape, its move, its mods and its gate. `kit_bill` folds every pad through it. `Instrument::recost` then folds each sounding hit's struck sound into its Part's bill before the Part's bills are worked out. A Part's rest therefore covers every filter its hits still run. Nothing is taken off when the kit moves on.
- **A hit's follow is billed as a knob's glide** (ADR 0117): admission counts it and shedding does not. This is `DrumEngine::follow_bill`, through `Voice::drum_follow_extra`, added to the voice's glide in `recost`:
  - **a publish the hit hasn't seen** bills `FOLLOW_DECODE_COST` plus the glide (it starts this block);
  - **a glide under way** bills `FOLLOW_GLIDE_COST` plus the hit's filter's `glide_cost`;
  - **otherwise** it bills 0.
- **The constants are static upper bounds.** Counted on the `filter-probes` ELF at origin/0.4 eb8ae2a9 with `tools/loop_count.py` and a call-tree count, each instruction counted once (no branch skipped), at `ceil5(instr × 1.606 + 12 × divides)` (ADR 0056), / 64:
  - `FOLLOW_GLIDE_COST` 55: `follow`'s body (683), the ease over 20 places (~200), `set_places` (3 × 72) and both `AmpEnv::coefs` (2 × (46 + `ACoefs::new` 341)), about 1,923 instructions and 18 divides, 51.6 a sample;
  - `FOLLOW_DECODE_COST` 80: `sound_if_newer`'s 70-byte read (~470), `PadSound::from_bytes` with its loops (~1,125), the default pad (113), the copies (~330) and both `PadSound::places` (2 × 338), about 2,716 instructions and 48 divides, 77.2 a sample.
  - **Unit read owed** (#1026): a ringing pad edited from the KIT page, read with `stats` for the publish's block and the glide's blocks. Lower these to the reading only if it reads under them, and never below what the count can prove.
- **A DRUM glide bills its pads' filters only** (`drum_glide_extra`): `.max().unwrap_or(own)`, as `drum_cost` does. The Sound's own kind counts only before the first publish.
- **The admission tables in `cost_test` take their room from admission's own constants** (#836). Before, the room was a literal, 2,064 (1,204 bus + 24 + an 836 burst), that no longer matched the code.
  - The room is now `FxBus::COST_NO_TAPE` + `CONTROL_RESERVE` + `Instrument::BURST`, 2,073. With the master tape's bus it is 2,377.
  - The ORBIT column adds `OrbitRunner::COST` on top. That is exactly what admission books while ORBIT plays (`Instrument::reserved`). The old literal put it 9 a sample over, so the ORBIT column claimed a voice that admission refuses: VA PHASE 7 → 6.
  - The columns without ORBIT keep one burst's margin on purpose. Admission books no burst there, and a DIN chord arrives a note a millisecond, but the tables are a planning room, not a ceiling.
  - The rows that move: VA PHASE beside ORBIT 7 → 6. With the master tape, VA LOOPFM 8 → 7, TAPE LOOP and TRON at TAPE/AGE ½ 8 → 7, and STRETCH GRAIN 5 → 4.

## Alternatives considered
- **Recost a hit from its struck shape alone** (a per-hit rest beside the Part's). This is exact for that hit, but it needs a second filter bill per DRUM voice per block in bank 1. The union over-bills a Part's other voices only while a hit with an older shape rings.
- **Treat a DRUM Part as gliding for 300 ms after its view moves** (the issue's cheapest idea). It bills every voice of the Part, ringing or not, and a fixed time is not when the glide ends. `follow_bill` reads the hit's own glide.
- **Bill the follow in the voice's shed cost.** A pad edit could then fade out ringing hits on a full pool. A knob's glide never sheds (ADR 0117), and a pad edit is the same kind of UI move.
- **Wait for the unit before billing.** The rule for this batch is that a bill is set from the best static count, with margin, when only a unit read can settle it. The counts here are upper bounds.

## Consequences
- **Sound:** none changes. Bills move admission only, and every golden is bit-identical.
- **Voices:** a pad edit under ringing hits books up to 140 a sample a hit in the publish's block, and 60 while the glide runs (about 300 ms). A full pool refuses new hits for that window rather than overrunning. A kit republished on a cheaper filter keeps its ringing hits' bills until they end.
- **Bank 1:** `KitBill::with_sound`, the recost fold and `follow_bill` (measured in the batch's budget lines).

## Sources
- private issue #962, private issue #977, private issue #836, private issue #1026.
- ADR 0056 (static counts), 0117 (glide billing), 0269 (live follow), 0274, 0287 (DRUM bills).
- `chimera-engine/src/dsp/drum/engine.rs` (`follow_bill`, `FOLLOW_*_COST`), `sound.rs` (`KitBill::with_sound`), `instrument.rs` (`recost`), `voice.rs` (`drum_glide_extra`, `drum_follow_extra`); `chimera-core/tests/drum_bill_test.rs`, `cost_test.rs`.
