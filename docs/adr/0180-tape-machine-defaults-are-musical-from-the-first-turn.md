# 0180. TAPE's machines are musical from the first turn: ONE's hit, LOOP's LENGTH, TRON's wear and strip, ONE's START and RETRIG

- **Status:** Proposed
- **Deciders:** owner (#698), firmware
- **Amends:** [0178](0178-tape-model-lists-one-loop-and-ping-and-deck-holds-the-glyphs-knobs.md) (a MODEL change writes the wear on TRON; ONE's model page)

## Context
Musical findings 4, 7, 11 and 12. ONE's hits went through ENV 1's MED attack (10 ms as an RC, −15 dB at 1 ms) and a 0.7 sustain: the beater click and the snare's crack were gone. LOOP opened with LENGTH 1, so POSITION and WALK, half its DECK, moved nothing (ADR 0173 said so). TRON had no wow at INIT, and on a RAM source shorter than 8 s the top of LENGTH ran past END. ONE had no modulatable control of where it plays (the MPC and Digitakt hat-variation move, VEL → START), and no way to choke one hit with the next.

## Decision
- **ONE's ENV 1:** a MODEL change to ONE (and every factory ONE recipe) writes ENV 1 SPEED FAST, ATTACK 0 (0.2 ms; the 1 ms note-on ramp keeps it click-free) and SUSTAIN 1 (`machine::env_defaults`): the sample is its own envelope, as an MPC pad's. The CC budget counts these writes (`cc::model_writes`).
- **LOOP's and PING's LENGTH default 0.8** (`LOOP_LENGTH_DEFAULT`), and INIT's TAPE Sound takes it (`Sound::plain_init`), so POSITION scans from the first turn. The neutral (the codec's base) keeps SIZE 1.
- **TRON's wear:** a MODEL change to TRON writes TAPE 0.3 and WOW WARBLE (`machine::TRON_WEAR`); every other machine still keeps the wear the player set.
- **TRON's LENGTH runs over the strip:** `0.25 · (4·T)^LENGTH` seconds for a strip of T seconds from START to END at ROOT (0.25 to 8 s), so LENGTH 1 is always the whole strip and its top is never dead (`tron::tron_len_s_in`). TRON streams (ADR 0183), so 8 s strips at 24 kHz fit.
- **TAPE's block is a matrix destination at last** (`BlockRef::voice_reads`): its five macros (POSITION, MOTION, TAPE, AGE, SIZE), marked modulatable since ADR 0093, had no route that played (a stale "not yet"); every one is now heard through a route (`modulatable_test`), and TAPE's and AGE's bills follow their routes (`tape_runs`, `age_runs`).
- **ONE's START is a matrix destination**, latched by the note's first block: VEL, NOTE or an LFO into START places the head before a sample plays (`TapeDeck::moved_start`); streamed, the cursor starts again there, warm if START's head holds it.
- **ONE's RETRIG (param 16, `Retrig`): STACK (default) or CUT.** At CUT a note on the Part fades out the Part's other notes as a shed does (`Instrument::cut_part`): an open hat choked by a closed one, the MPC's mute group within a Part, until KIT's choke groups land. A key struck again always takes its own voice.

## Alternatives considered
- **ONE at HOLD with no VCA route at all.** It would make ONE the one machine whose AMP page does nothing.
- **LOOP's neutral SIZE 0.8.** It would re-key every sparse saved TAPE Sound, SCRUB's and WAVE's too.
- **A SLICE NOTE cell back on ONE.** Left for KIT (slice 4); RETRIG CUT covers the choke now.

## Consequences
- New ONE, LOOP, PING and TRON Sounds start from these; saved Sounds are unchanged (defaults are written only on a MODEL change). The factory TAPE library's ONE Sounds rebuild with ENV 1 FAST/0/1 (`factory_tapes::tape_sound`).
- RETRIG is a new param id (16) with its own codes; old files load it at STACK.
- Goldens of ONE's model page and TRON's pages are re-recorded on purpose.

## Sources
- Musical findings 4, 7(a), 11, 12; #698. MPC 2000XL manual (Mute group, Velocity → Start); Digitakt II manual § Sample (STRT under modulation).
- `chimera-engine/src/dsp/tape_engine/{machine,params,tron,mod}.rs`, `chimera-core/src/ui/part_page.rs`, `chimera-engine/src/instrument.rs`.
