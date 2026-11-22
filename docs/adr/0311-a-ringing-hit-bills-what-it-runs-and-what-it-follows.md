# 0311. A ringing DRUM hit bills the filter it runs and the follow it does; a low SYMP note's clear is paced by the block's room

- **Status:** Accepted (2026-11-22)
- **Deciders:** firmware (bug batch E: #962, #977, #836); owner (#306, #704: pace the clear, 2026-11-22)

## Context
The DRUM bill (ADR 0269, 0274, 0287) is worked out from the kit as last published. A ringing hit does not play the kit as it is now: it follows its pad's continuous values (ADR 0269), but it keeps the choices it was struck on: filter KIND, MODE and VOICE, and each route's SOURCE and DEST (`PadSound::live_target`). Two gaps followed from that.

- **#977: a hit billed for a filter it no longer has.** Strike a LADDER pad, then publish the kit with that pad on CLEAN. The hit still runs LADDER, but recost billed it at the kit's CLEAN: 965 → 789 a sample for the rest of its ring. With STACK, eight such tails are about 1,400 a sample short.
- **#962: the follow itself was not billed.** A pad edit on a ringing hit costs three things, none of them booked:
  - the publish's block decodes the pad (`KitView::sound_if_newer`);
  - the glide then steps the filter's coefficients (`FilterKind::glide_cost`);
  - each gliding block eases 20 places and rebuilds both envelopes' coefficients (`AmpEnv::coefs`).
- **#962, an over-bill.** `drum_glide_extra` folded the Sound's own filter glide into the DRUM bill, although the pads mask it once a kit is read. `drum_cost` already took the pads alone.

- **#306, #704: a SYMP note-on could overrun a full block.** SYMP clears up to 31,392 B of string lines at its note-on, one worst note-on a block (`SYM_CLEAR_BUDGET`, spec § 4.8). The unit read that clear at 225,533 cycles (SYM NOTE-ON LOW, DWT, rev V, 2026-10-02), 35 % of the 640,000 deadline. On a full pool (480,000) with ADR 0262's unbilled overhead, that block passes the deadline. The load net (ADR 0273) acts only after the block. #704 reproduced it from plain MIDI.

## Decision
- **The kit's bill takes in what each ringing hit runs, from the hit's engine state** (`DrumEngine::runs`, a `Runs`), not from its followed `PadSound`:
  - the filter shape it was struck on (KIND and VOICE are latched);
  - **moving** while a filter route moves at the amounts the engine runs (`moves_filter`, on `PadMods::amounts`), or at the amounts its glide heads to (the follow's target). The followed sound's route amounts never change mid-glide, so they cannot say this;
  - **mods** from what the engine struck: its mod envelope and routes (`Routed`), else its AMP envelope (`Amp`), else none. Both run for the whole ring whatever the pad's amounts or AMP become;
  - its gate where it is, or where it glides to.
  - `KitBill::with(Runs)` folds one in. `kit_bill` folds each pad (`Runs::of`). `Instrument::recost` (`ringing_kits`) folds each sounding hit into its Part's bill before the Part's bills are worked out. Nothing is taken off when the kit moves on.
- **A hit's follow is billed as a knob's glide** (ADR 0117): admission counts it and shedding does not. `DrumEngine::follow_state(KitSeq)` is pure. It is given the kit's publish count (`KitView::seq`, a `KitSeq`) of the Part the voice *sounds* for (`Instrument::sounding`), never the allocator's: a voice stolen across Parts still follows its old Part's kit until its fade ends. It returns a `FollowState`:
  - `Decoding(kind)`, a publish the hit hasn't seen: `FOLLOW_DECODE_COST` + `FOLLOW_GLIDE_COST` + the kind's `glide_cost` (the glide starts this block);
  - `Gliding(kind)`: `FOLLOW_GLIDE_COST` + the kind's `glide_cost`;
  - `Still`: 0. Once a glide lands, its hit books nothing more.
- **`follow` eases its glide in place.** It used to copy the `(Places, Places)` pair out and back each block. It now keeps only the three lanes the rebuilds compare, sound-identically. `follow` shrank by 112 B.
- **The constants are static upper bounds, counted honestly.** They are counted on the `filter-probes` ELF of this branch with `tools/loop_count.py`, the disassembly and a call-tree count. Each instruction is counted once with no branch skipped, loops at their trip counts, and copies at `__aeabi_memcpy4`'s 13 instructions a 16 B pass plus 10 a call (40 for the byte-wise `__aeabi_memcpy`), at `ceil5((instr × 1.606 + 12 × divides) / 64)` (ADR 0056):
  - `FOLLOW_GLIDE_COST` **75**: `follow`'s whole body with the decode's branch (651) and its outlined pieces (90); the ease loop, 20 × 27 less the 19 in the body (521); both `AmpEnv::coefs` with `ACoefs::new` (2 × 387); `denormalize` 4 × 72; three lane compares (3 × 27); the copies, 80, 64 and 2 × 36 B (305). That is 2,710 instructions and 21 divides, 71.9 a sample;
  - `FOLLOW_DECODE_COST` **115**: `sound_if_newer` (168) with its 70-byte read at all three tries (3 × 5 × 76) and its clear (79); the default pad (113) and its copy (92); `PadSound::from_bytes` (316) with its loops over 57 bytes (861) and three `Reader::env` (192); the decode branch's copies, 64 B, 3 × 80 B and 160 B (457); both `PadSound::places` (2 × 338, 24 divides each). That is 4,094 instructions and 48 divides, 111.7 a sample.
  - The first count (55 and 80) took each copy as one call, counted three `denormalize`s where four run, and counted the ease's loop once. It was not a bound.
  - **Unit read owed** (#1026): a ringing pad edited from the KIT page, read with `stats` for the publish's block and for the glide's blocks. Lower these to a reading only if it reads under them.
- **A DRUM glide bills its pads' filters only** (`drum_glide_extra`): `.max().unwrap_or(own)`, as `drum_cost` does. The Sound's own kind counts only before the first publish.
- **The admission tables in `cost_test` take their room from admission's own constants** (#836). Before, the room was a literal, 2,064 (1,204 bus + 24 + an 836 burst), that no longer matched the code.
  - The room is now `FxBus::COST_NO_TAPE` + `CONTROL_RESERVE` + `Instrument::BURST`, 2,073. With the master tape's bus it is 2,377.
  - The ORBIT column adds `OrbitRunner::COST` on top. That is exactly what admission books while ORBIT plays (`Instrument::reserved`). The old literal put it 9 a sample over, so the ORBIT column claimed a voice that admission refuses: VA PHASE 7 → 6.
  - The columns without ORBIT keep one burst's margin on purpose. Admission books no burst there, and a DIN chord arrives a note a millisecond, but the tables are a planning room, not a ceiling.
  - The rows that move: VA PHASE beside ORBIT 7 → 6. With the master tape, VA LOOPFM 8 → 7, TAPE LOOP and TRON at TAPE/AGE ½ 8 → 7, and STRETCH GRAIN 5 → 4.

- **A low SYMP note's clear is paced by the block's room** (owner, 2026-11-22, #306, #704). A low SYMP note may start up to about 5 ms late at full load.
  - **The room.** `sym_clear_room` gives the bytes the next block may clear. It takes the deadline (the audio share over `AUDIO_BUDGET_PERCENT`) less the bookings, the reserve and ADR 0262's overhead (`BLOCK_OVERHEAD` 160, `PART_OVERHEAD` 230 for each Part with a voice booked). It divides the rest by `SYM_CLEAR_CYCLES_PER_BYTE`, 8: the unit's 7.2, rounded up. It is capped at one worst note-on (`SYM_CLEAR_BUDGET`). `Instrument::clear_room` sets `clear_left` from it at each block's end, in place of the fixed budget. A pool with no ceiling (the bench, the tests' `u32::MAX`) keeps the old budget exactly.
  - **The pacing.** A SYMP note whose clear doesn't fit waits, as it did. Now the block's room clears its lines ahead first (`Voice::sym_pre_clear`, `SymPool::pre_clear`, `KsString::clear_some`), from the top of each line's `dirty`. Each pass leaves the line at rest the way `clear` does (the shortest ring from the start), so it is silent above `dirty` whatever plays it next. The note strikes once what is left fits.
  - **What gets cleared.** The main string only while the voice is idle. The set it will ring at any time, its own or the one promised to it: no voice renders a promised set. A stolen voice's lines wait for its fade; its promised set clears meanwhile.
  - **Progress.** Admission keeps the bookings under the share, so the room never falls below the share's headroom less the overhead. On rev V with six Parts that is about 8 KB a block, so a worst note-on takes four blocks (5.3 ms). A typical note-on (short lines) still fits one block.
  - **Sound.** Zeros are zeros: the note sounds bit for bit as alone, from its first sample (`a_paced_note_sounds_as_alone`, `a_note_retriggered_mid_clear_sounds_once_as_alone`). Only its start moves, and only under load.
  - **Real-time.** No allocation: a slice fill and a few compares a block.
  - **Unit reads owed** (#1026): SYM NOTE-ON LOW on today's build, which sets `SYM_CLEAR_CYCLES_PER_BYTE`, and #704's two repros reading `overruns` and `load_sheds`.

## Alternatives considered
- **Recost a hit from its struck shape alone** (a per-hit rest beside the Part's). This is exact for that hit, but it needs a second filter bill per DRUM voice per block in bank 1. The union over-bills a Part's other voices only while a hit with an older shape rings.
- **Treat a DRUM Part as gliding for 300 ms after its view moves** (the issue's cheapest idea). It bills every voice of the Part, ringing or not, and a fixed time is not when the glide ends. `follow_state` reads the hit's own glide.
- **Bill the follow in the voice's shed cost.** A pad edit could then fade out ringing hits on a full pool. A knob's glide never sheds (ADR 0117), and a pad edit is the same kind of UI move.
- **Wait for the unit before billing.** The rule for this batch is that a bill is set from the best static count, with margin, when only a unit read can settle it. The counts here are upper bounds.
- **Read the struck `PadSound` for the fold** (the first cut). A followed sound keeps its struck route amounts mid-glide and lands on the pad's, so it said "not moving" while the engine's amounts moved, and "no mods" once a routed pad was edited to 0, while the engine still ran them (review of #1080).
- **For #306, bill the clear** (225,533 / 64 = 3,524 a sample a SYMP Part): no voice would be left. **Make the clear cheap in hardware** (D2 write-back no-write-allocate, or MDMA zeroing): this changes every D2 resident and needs the unit. Both were offered; the owner chose the pacing.

## Consequences
- **Sound:** none changes. Bills move admission only, and every golden is bit-identical.
- **Voices:** a pad edit under ringing hits books up to 195 a sample a hit in the publish's block (CLEAN), and 80 while the glide runs. A full pool refuses new hits for that window rather than overrunning. A kit republished on a cheaper filter keeps its ringing hits' bills until they end.
- **Flash:** the DRUM fold (`ringing_kits`, `DrumEngine::runs`, `follow_state`) and SYMP's pacing (`SymPool::pre_clear`, `KsString::clear_some`, `Instrument::clear_room`) sit in bank 1, out of line and built for size. `follow` shrank 112 B. Measured by `just check`'s builds on a clean tree after merging origin/0.4 e963edbc: bank 1 is +912 to +1,032 B over origin/0.4's lines in every set (ORBIT 908,528 → 909,440; default 894,416 → 895,352). Bank 2 is unchanged (ORBIT 634,016). Each bank-1 line is set to its measure.
- **Latency:** at full load a worst SYMP note-on starts up to four blocks late (5.3 ms on rev V with six Parts). A light pool, or a typical note-on, starts at once, as before.

## Sources
- private issue #306, private issue #704, private issue #962, private issue #977, private issue #836, private issue #1026.
- ADR 0056 (static counts), 0117 (glide billing), 0269 (live follow), 0274, 0287 (DRUM bills).
- `chimera-engine/src/dsp/drum/engine.rs` (`follow_bill`, `FOLLOW_*_COST`), `sound.rs` (`KitBill::with_sound`), `instrument.rs` (`recost`), `voice.rs` (`drum_glide_extra`, `drum_follow_extra`); `chimera-core/tests/drum_bill_test.rs`, `cost_test.rs`.
