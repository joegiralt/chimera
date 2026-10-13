# 0135. FREE LFOs run on a Part clock

- **Status:** Proposed
- **Deciders:** owner (#489, 2026-10-13), firmware

## Context
Each voice owns its three LFOs. A CLASSIC LFO at SYNC FREE never reset its phase on a note-on, an idle voice stopped advancing it, and notes are allocated round-robin, so after normal playing every voice's slow LFO sat at an unrelated phase. A slow LFO on the cutoff across a chord smeared instead of moving together (#489): the planned Prophet-style swell, TAPE DRIFT (0.3 Hz on the cutoff) and FADED HYMN (0.17 Hz) wobbled voice against voice.

## Decision
- **Each Part keeps a phase clock a slot** (`lfo::LfoClock`, three phases, in the `Instrument`): every block, for every Part, playing or not, each phase advances by its LFO's RATE, `phase += RATE × 64 / 48 000`, wrapped. It is an accumulator, not `frac(t × RATE)`: a RATE change moves the speed, never the phase.
- **A CLASSIC FREE LFO plays its Part's clock.** Each block a voice's FREE LFO takes the clock's phase for its slot, then runs as before (shape, depth, S&H draw at the wrap). Every voice of the Part, whenever it started, sits at the same phase, as the Prophet-5's one LFO does for every voice.
- **RETRIG is unchanged:** a SYNC 1 LFO restarts at its PHASE on each note-on and runs its own phase. FUNC LFOs are unchanged.
- **The clock reads the Part's stored LFO settings** (the LFOs are not modulation targets). A voice fading out on the Sound it last played still reads its Part's clock.
- **Billed** as a reserve beside the bus, like the knobs' (ADR 0116): `LfoClock::RESERVE`, **10 cycles a sample** by ADR 0056's count on the release ELF: every Part's step (`Instrument::step_lfo_clocks`, 173 instructions with its call and one `vdiv`, 289.8 a block) and eight voices' three LFOs taking their phase (26 instructions each, 41.8 a block), 624.1 a block, 9.75 a sample. It is added wherever the reserve is (`Instrument::reserved`, the shed, the bench's bill).

## Alternatives considered
- **`frac(part_time × RATE)` at each note-on** (the issue's sketch): a RATE change would jump every new note's phase against the held ones, and an f32 time loses precision within hours.
- **Copying the clock only at the note-on**, the voice running its own phase after: a note started after a fade's end, or after the voices render, read the clock a block apart; reading it each block is exact and costs one store.
- **A per-LFO choice of FREE (Part clock), RETRIG or per-voice DRIFT:** a later option if deliberate spread is wanted; not needed for alpha.

## Consequences
- **Sounds change:** every Sound with a CLASSIC FREE LFO now moves its voices together. TAPE DRIFT and FADED HYMN's chords sweep as one instead of wobbling apart; S&H FREE LFOs still draw a value a voice.
- A FREE LFO's phase no longer depends on which voice a note lands on, so a repeated chord sounds the same each time.
- Cost: the reserve takes 10 cycles a sample from the voices' share in every room. `cost_test`'s four VA and TAPE rooms (rev V, rev Y, rev V with ORBIT, rev V with the master tape) keep their voices.

## Sources
- #489; Sequential Prophet-5 Rev 4 user manual (one LFO for all voices).
