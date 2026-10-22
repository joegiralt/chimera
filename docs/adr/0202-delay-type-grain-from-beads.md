# 0202. DELAY TYPE GRAIN from Beads

- **Status:** Proposed
- **Deciders:** owner (#612: a Beads-style grain delay beside today's TAPE delay, no new RAM, one instance on the bus; 2026-10-21: in now, bank 1 frozen), firmware (the port, the controls, the bill)

## Context
#612 asks for a second delay TYPE taken from Mutable Instruments Beads, living in the delay slot as the reverb's TYPEs do: TAPE, today's Space Echo-style delay, and GRAIN, whose repeats are grains read from the delay's line. TIME, FEEDBACK and MIX are shared, and GRAIN adds about four pots (SIZE, DENSITY, PITCH for shimmer, TEXTURE for spray).

Constraints:
- **No new RAM for audio.** GRAIN reads the delay's existing line: `MAX_DELAY_SAMPLES` 24,064 `i16` samples, 500 ms at 48 kHz, in AXI (ADR 0014, 0090). So GRAIN is a grain delay of stutters, pitch-shifted repeats and shimmer, not Beads' seconds-long time-stretch clouds (those need a longer buffer and a memory-budget call, #337).
- **One instance, on the send bus** (`FxBus`), never one per voice.
- **Bank 1 is frozen** (ADR 0198): GRAIN's DSP and per-block maths live in bank 2 behind a door; bank 1 gains only a door stub and the closed door's data.
- **Saved projects load as TAPE and sound bit for bit as before.**

Beads' STM32 code is MIT (Copyright 2017 Emilie Gillet, `pichenettes/eurorack`, `beads/dsp/`). No GPL code was read; the repo's AVR projects were not opened.

## Decision
### The port: one grain core, the delay's side apart
- **The core,** `chimera-engine/src/dsp/grain/core.rs` (MIT notice in the file): the clock, the bell, the stealing fades, the Hermite read and the scheduler (`Engine<G>`), generic over where the audio comes from. TAPE's STRETCH will run on the same core (one grain engine, not two). It reads through:

  ```rust
  pub trait GrainSource: Sized {
      /// Frames it holds; a head wraps at the end. At least 4 and more
      /// than `MAX_SPEED`: asserted at compile time where the core is built.
      const FRAMES: usize;
      /// Frame `i`: an `At<Self>`, below `FRAMES`, made by the core alone.
      fn frame(&self, i: At<Self>) -> f32;
      /// Frames `i − 1 ..= i + 2`: a `TapAt<Self>`, all inside, made by the core alone.
      fn taps(&self, i: TapAt<Self>) -> [f32; 4] { /* from `frame` */ }
  }
  ```

  Its units are types, so a wrong one doesn't compile and an out-of-range one can't be made: `Pos` (a head, Q32.32 frames: a head reaches 2³² frames and moves within 10⁻⁹, where an `f32` position would lose a sample's fraction past 2²⁴ frames), `Speed` (signed Q32.32 frames a sample, −`MAX_SPEED` to `MAX_SPEED` 16: a grain reads backwards too, wrapping either end), `Frames` (a distance in a source), `Samples` (output time), `GrainLen` (2 samples or more), `Gain` (finite, 0 or more), `Rate` (grains a sample, 0 to ¼), `Unit` (TEXTURE, a trigger's fraction, a draw), `Rng` (a seed never 0), `Trigger`/`Triggers` (a sample in the block, at most `MAX_STARTS`). The index tokens replace the bounds checks: the delay's line reads both unchecked on their invariant. The Hermite is the core's, so a source only hands out frames. `Engine::set_speed` moves every grain's speed at once, for a user whose speed moves under its grains (STRETCH's flutter); GRAIN never calls it. At each trigger `Engine::block` asks its user for a `Start` (position, speed, length, gain) or none: `None` takes no slot and fades nothing. It returns the block's starts, each trigger with the `Slot<G>` it took. `Sched::steal` is an `Option<Steal>`: Beads' fades (GRAIN's), or `None`, every grain playing out.
- **The delay's side,** `chimera-engine/src/dsp/delay/grain.rs`: the line as a `GrainSource` (`Line`), the knobs' laws, where a grain starts behind the write head, its gain, the feedback path and the TYPE crossfade.

Both link into bank 2 (`*3dsp5grain*`, `*5delay5grain*`).

### What was taken from Beads
Taken from Beads:
- **The trigger:** `TriggerGenerator::RenderPulseTrain`. A phase accumulator at DENSITY's rate; each crossing starts a grain with the sub-sample fraction past it. Its random mode multiplies each interval's rate by `−1/ln(1 − x)`, `x` uniform in [0.1, 1), by `LogOneMinusX`'s four-term series. TEXTURE blends the ratio linearly from 1 (Beads' regular train) to that draw (its random train). Beads switches between the two on the DENSITY knob's halves; here DENSITY is a plain rate and TEXTURE the randomness.
- **A grain's start:** `DelayGrain::Start`. The delay is corrected by the sub-sample fraction times the consumption speed (`speed − 1`), the duration is trimmed so the line holds it (`MAX / |speed − 1|`), and the delay is held so the head never crosses the write head during the grain (Beads' trim, which makes its STOP policy's crossing check unneeded). The envelope starts the same fraction in.
- **The envelope:** `GrainEnvelope` at SHAPE ⅔ (attack ½, release ½, smoothness 1), which is smoothstep up the first half and down the second. Beads holds the release to at least 10 ms and the attack to 0.5 ms; here each half is half of SIZE, 5 ms at its floor.
- **The scheduler:** `GranularPlayer::SortGrainList`, sorted branch. A start takes the grain with the fewest samples left; then the grain now nearest its end fades out within the steal crossfade (`kInstantCrossfadeTime`, 5 ms) and the next within the interval just gone, bounded by the pre-emptive fade (`kPreemptiveCrossfadeTime`, 100 ms), through `VoiceStealingEnvelope`'s linear ramp. Beads fades four of 32; six grains fade two.
- **The read:** `DelayLine::Read<INTERPOLATION_HERMITE>`, Beads' own four-tap form, on the `i16` line.
- **The feedback limit:** `stmlib::SoftClip`, already in the engine as `fast_tanh` (ADR 0097), after a 20 Hz DC blocker (Beads: an SVF high-pass at 40 Hz). TONE's one-pole, TAPE's own law, sits before it, so TONE is shared.

Ours, and why:
- **Six grains** (Beads: 32 in its delay player). Each grain costs 74 cycles a sample; six keep an ALGO INIT's voices on rev Y but one (below). A start always finds a grain: the scheduler steals the nearest the end.
- **Positions in Q32.32** (Beads: 22.10 behind the head).
- **The gain.** Beads divides each grain by `√(grains playing + 1)`, which assumes grains add in power. Read from one place, at PITCH 0, they add in phase, so a cloud at one TIME swelled with its depth. Each grain is divided by its expected overlap `depth = rate · size / 2` (a bell's mean is ½, held between 1 and half the grains) to the power `1 − TEXTURE/2`: in phase at TEXTURE 0, in power at 1; a semitone of PITCH blends to power too. TEXTURE's random intervals start grains faster than DENSITY's rate, `1 + t(0.74 − 0.61t)` of it on average (Beads' draw blended by TEXTURE `t`, within 3 % above), and the depth counts them.
- **The feedback counts the grains in phase.** The return keeps the law above, for its loudness; the feedback takes `depth^(law − 1)` of it more, so each grain enters the loop at `1/depth`. Grains assumed to add in power do add in phase for a low tone under a small spray, and for a shifted regular train's own lines: with the return's law in the loop, FDBK 0.9 grew 11× over 6 s (PITCH +12, DENSITY 1, SIZE 0.4) and FDBK 0.98 8× (TEXTURE 0.25, TIME 10 ms). With the trim, FDBK under 1 always dies away (`feedback_under_one_always_dies_away`; swept over SIZE, DENSITY, TEXTURE, PITCH, TIME and a low and a mid tone at FDBK 0.9 and 0.98).
- **SIZE:** 10 ms · 50^SIZE, 10 to 500 ms (Beads: 8 ms to 4 s over its buffer). **DENSITY:** 2^(7·DENSITY) grains a second, 1 to 128 Hz (Beads' pulse train tops out at 130.81 Hz). **PITCH:** −24 to +24 semitones, a speed of 2^(PITCH/12). **TEXTURE** also sprays each start ±50 % of TIME (Beads' TIME randomness). The start position is TIME behind the head, as Beads' TIME knob sets it.
- **The block.** The grains are rendered for the whole block before the line is written, so a grain never reads closer than 68 samples to the write head (a block, the Hermite's newest tap, and two to spare). Each grain renders in runs whose taps stay inside the ring: no wrap test a sample.

Left out: the seconds-long buffer and its time-stretch clouds (no RAM); stereo pan and spread (the delay is mono, ADR 0014); SHAPE (one bell); reverse grains below a third of SIZE; the freeze and the SEED button; the delay mode's multitap (`DelayController`, `MultitapDelay`); the media models and their sample-rate converters; the input and output limiters (the bus's limiter and the soft-clipped write bound GRAIN); and Beads' reverb (the bus has one).

### TYPE, the controls and the pages
- **DELAY's cells:** TYPE · TIME · FDBK · TONE · REV · MIX. TYPE is a new delay param 7 (ident `TYPE`, TAPE 0, GRAIN 1, stored as its index, a real, as THEME's HINTS is: no enum codes). TIME, FDBK, TONE, REV and MIX are both TYPEs'.
- **DLY › CHAR** is TAPE's (MECHANICS, SAT), unchanged. **DLY › GRAIN** (new, page 111) holds GRAIN's four: SIZE (8), DENSITY (9), PITCH (10, `Signed(24)`, stepped semitones), TEXTURE (11). Both pages are always there and each pot keeps its value under the other TYPE, as a reverb TYPE's knobs do. Showing one page by TYPE (a panel binding) was built and measured: it is UI code in bank 1, which 0198 refuses, so the pages are two plain tables. Each page's cells dim under the other TYPE, the band saying TAPE TYPE ONLY or GRAIN TYPE ONLY, through the UI door (`walk::delay_note_in`, as STRETCH's TYPE dims; closed, nothing dims), the TYPE read into `SlotCtx` from the project's delay (review of #761); dimmed, a cell ignores its encoder, as every dimmed cell does, and keeps its value for its TYPE.
- **Smoothing:** each GRAIN pot is `Smoothing::Own`. A grain reads SIZE, DENSITY, PITCH and TEXTURE when it starts, under its own bell, so a turn is heard grain by grain with nothing to zipper. TIME under GRAIN moves the next grain's start; the grains playing finish where they were, so a TIME jump never clicks (`time_changes_never_click_under_grain`).
- **Modulation and MIDI:** as the delay's other params, none. The FX bus's params are a project's, not a Sound's, and nothing on the bus is modulatable or in a Part's MIDI map today (`BlockRef::in_sound`). GRAIN follows the delay.
- **TYPE switching:** a TYPE change crossfades GRAIN's share of the return and of the feedback over 20 ms (`TIME_FADE`, linear across each block), both heads running, so nothing steps (`type_switches_never_click`). From silence the grains start fresh. TAPE's TIME head waits at TIME while GRAIN plays.
- **Labels:** full words (DENSITY, TEXTURE), with DENS and TEXT as the matrix's short forms. TYPE takes the word's band (NONE), GRAIN's pots the arc; the rings stay TAPE's. No hidden gesture is added, so no new hint.

### Bank 2
- `delay::grain`, text and rodata, links into `.bank2` (`bank2.x`, `*5delay5grain*`), checked by `check-door-placed` (`dsp::delay::grain::`). `GrainDoor { run }` is its entry.
- **Bank 1's side** is one stub in `TapeDelay::run`: while TYPE is GRAIN, or its share is still fading, the block goes through the door; if the door is closed, `run` falls through to TAPE's own loop.
- **Closed, GRAIN plays as TAPE**, and TYPE reads NEEDS UPDATE at GRAIN (`bank2::closed_text`). TAPE rather than dry: TAPE's loop is already bank 1's (no new code), the line and every ease keep running so the door opening or closing never clicks, and a send keeps its echo; the label says why it isn't grains. The GRAIN page still edits its pots, saved as usual.
- `grain_proxy_test` covers the door open (GRAIN heard) and closed (GRAIN's render bit for bit TAPE's).

### Compatibility
- Saved projects carry no ids 7–11, so they load TAPE with GRAIN's defaults (`project_fixture_loads`). The disk codes fixture gains five `B 21` lines, appended.
- **TAPE is bit for bit:** `delay_tape_hash_test` hashes the delay's send and insert returns over 1,600 blocks of plucks and noise, with TIME, FDBK, MECHANICS, SAT, TONE and MIX all moving (TIME twice, its crossfade). The hashes were taken on origin/main `f745c0a1` before TYPE existed and match unchanged.

### Memory
- No new line. `TapeDelay` grows 272 B (`grain::State`: six grains of 40 B, the clock, the TYPE share, the feedback's filter states), in the FX bus's AXI budget. `DelayParams` grows 20 B (`grain: [f32; 5]`, ids 7–11 in one array so `get` and `write` reach them in one arm each). TYPE stays an `f32` index in that array, read typed through `kind` (a `DelayType`): an accepted exception to the type rule (review of #761), as a `DelayType` field of its own measured +56 B in bank 1 (`mix_parts` and the block's `get`/`write`).

### Cost
`tools/loop_count.py` on the ORBIT ELF (static counts, ADR 0056's 1.606 cycles an instruction and 12 a divide):

| Part | Instructions a sample | Cycles |
|---|---|---|
| The sample loop (the trim, TONE, DC, soft clip, write, return) | 62 + 1 `vdiv` (the trim's multiply estimated) | 112 |
| The block (clock, knobs' laws) | 10.1 + 0.23 `vdiv` | 20 |
| Two starts' gains and the feedback's trim (a `powf` each), a block | ~4.5 | 7 |
| A grain (Hermite, bell, accumulate; the loop runs two samples a round) | 46 | 74 |
| Six grains, and their runs' setup | 276 + 0.6 | 445 |

`GRAIN_COST` is 590 cycles a sample (580 counted, 584 with the feedback's trim estimated, rounded up): 7.9 % of rev V's 7,500-cycle audio budget (9.4 % of rev Y's 6,250). It is billed beside the bus while TYPE is GRAIN (`delay::extra_cost`, beside the reverb TYPE's extra in `Instrument::render_with`), and TAPE bills nothing new. TAPE's idle head isn't taken off. A TYPE crossfade runs both for 20 ms, in the headroom (ADR 0061). Each engine's INIT voices (`grain_bills_its_cost_and_keeps_the_voices`):

| | rev V TAPE → GRAIN | rev Y TAPE → GRAIN |
|---|---|---|
| ALGO INIT | 8 → 7 | 7 → 6 |
| MODAL INIT | 8 → 8 | 8 → 8 |
| VA INIT | 8 → 8 | 7 → 6 |

Eight grains (730) cost rev Y's ALGO INIT two voices; linear interpolation instead of Beads' Hermite would have kept rev V's eight, at the shimmer's top end. The unit's bench reading is pending (private issue #746).

### Bank 1 and bank 2
Each set's plain release build (`just check`'s), before (origin/main `5a2e50e7`, whose bank-1 bytes equal 0198's lines) and after, `rust-objcopy -R .bank2` (bank 1) and `-j .bank2` (bank 2). Rodata is the `r` symbols' bytes in bank 1, text the `t` symbols'; the rest is literal pools and alignment.

| Set | Bank 1 before → after (B) | Δ bank 1 | of it rodata | text | rest | Δ bank 2 |
|---|---|---|---|---|---|---|
| `orbit` | 886,120 → 887,620 | +1,500 | +888 | +480 | +132 | +6,816 |
| `default` | 865,716 → 866,712 | +996 | +888 | -4 | +112 | +6,816 |
| `bank2-gate` | 867,600 → 868,580 | +980 | +888 | -12 | +104 | +6,816 |
| `fault-inject` | 838,924 → 840,624 | +1,700 | +888 | +710 | +102 | +6,816 |
| `bench-algo,bank2` | 877,564 → 879,272 | +1,708 | +908 | +672 | +128 | +6,816 |
| `bench-algo,master-tape,bank2` | 873,528 → 874,772 | +1,244 | +908 | +234 | +102 | +6,816 |
| `bench-routing,bank2` | 874,816 → 876,476 | +1,660 | +888 | +650 | +122 | +6,816 |
| `bench-memory,bank2` | 863,652 → 865,336 | +1,684 | +888 | +660 | +136 | +6,816 |
| `bench-stress,bank2` | 877,288 → 878,900 | +1,612 | +908 | +608 | +96 | +6,816 |
| `bench-orbit,bank2` | 868,828 → 870,536 | +1,708 | +888 | +712 | +108 | +6,816 |
| `bench-tape,bank2` | 848,168 → 849,748 | +1,580 | +888 | +582 | +110 | +6,816 |
| `filter-probes` | 892,932 → 894,656 | +1,724 | +888 | +722 | +114 | +6,816 |

What the bank-1 bytes are, all of it the closed door's (0198 point 1):
- **Specs:** `DELAY_SPECS` grows by five `ParamSpec`s, +360 B, and their labels, idents and TYPE's two names.
- **The page table:** `DELAY_GRAIN`, +120 B; its map word, +12 B; the delay's sub-page list, +4 B.
- **The stored params:** `DelayParams` grows 20 B (`grain: [f32; 5]`), so `FxParams` and every Performance holding it grow. That alone, measured by giving origin/main's `DelayParams` 20 B of padding and nothing else, moves the ORBIT build's bank 1 by +232 B, all of it layout: field offsets and immediates in the UI's and the board's code (`UiState::input` +284, `chimera_board::synth` +124). Across the sets the text column swings with the layout for this reason, with no new function in it.
- **The stubs:** the door call in `TapeDelay::run` (+16 B in `render_half`), `bank2::closed_text`'s GRAIN arm (+36), `get` and `write` reaching `grain` (+36), and `extra_cost`'s bill.

**An owner-accepted exception to ADR 0198** (owner, 2026-10-21: "yes, accept the bank 1 growth for grain"). 0198 lets bank 1 grow by a closed door's data and its stubs. GRAIN's growth is more than that:
- about 900 B of closed-door data: the rodata column (+888 to +908 B), plus the literal pools and alignment in the rest column;
- about 600 B of layout churn from `DelayParams` growing 20 B: the text column, up to +722 B on the bench sets and −12 to +480 B on the shipping ones, apart from the ~90 B of stubs.

The churn is not new code, but it exceeds the guard's 64 B slack, so the owner accepted it by name. Each set's line rises by its whole measured Δ bank 1 in the table above, each naming this ADR. Bank 2 grows +6,816 B on every set: `delay::grain::run` (the TYPE crossfade's loop and the steady loop, the clock and the knobs' laws, the starts) and the core's `Engine::render` on the line.

## Fidelity of the port
- `the_engine_matches_its_f64_reference` runs the engine and an f64 reference of the port (its trims, gains, envelope, stealing and Hermite; the reference shares the engine's clock, whose starts have their own test) over six settings across SIZE, DENSITY, PITCH ±24 and TEXTURE: within 5·10⁻⁴ of signals near 0.1 RMS. What remains is f32's 2⁻⁹-sample grip on a start 24,000 samples back.
- `the_ported_pieces_match_beads`: the bell is smoothstep up and down, the soft clip and `LogOneMinusX` are Beads' formulas.
- `grains_start_at_density_and_regularly`: at TEXTURE 0, 1, 11.3 and 128 grains a second at DENSITY 0, ½ and 1, each interval within a sample. `texture_makes_the_intervals_irregular`: at TEXTURE 1 the intervals spread, the rate stays within 3× and a block never holds more than two starts.
- `pitch_shifts_each_grain_within_a_cent`: alone in a grain, a 440 Hz sine reads 440 · 2^(PITCH/12) within a cent at 0, +7, ±12 and ±24. Grains started on a regular train add up to lines a grain rate apart, as any granular shifter's do (Beads' too): TEXTURE spreads them.
- `full_feedback_with_pitch_up_never_runs_away`: FDBK 1 with PITCH +7, +12 and +24 at TEXTURE 0 to 1, after a noise burst, stays finite, under 4 and no louder after 12 s than after the burst; the soft-clipped write bounds the line. `shimmer_decays_below_full_feedback`: at FDBK 0.8 and PITCH +12 the tail is 20 dB down within 5 s.
- `nan_never_reaches_the_return`: NaN knobs read as their floors and NaN in the send writes silence into the line.
- `grain_render_is_pinned` pins one GRAIN render's bits.

## Alternatives considered
- **A grain line of its own** (Beads' seconds): needs RAM the FX bus doesn't have (ADR 0014, #337).
- **One CHAR page following TYPE** (a panel binding): built, and the UI code it adds is bank 1's (0198).
- **TYPE as an enum with disk codes:** three `Block` methods of codec code in bank 1. A real index needs none, as HINTS.
- **Closed GRAIN as dry:** a send would lose its echo, and the line and eases would have to restart on opening.
- **Beads' per-grain `1/√(n + 1)`:** a coherent cloud swells with its depth and pushes the feedback loop past unity.
- **Eight grains or Beads' 32:** 730 cycles cost rev Y's ALGO INIT two voices; 32 would cost the instrument.
- **Linear interpolation (TAPE's):** 12 instructions a grain-sample cheaper, at the cost of the pitch-shifted repeats' top end; Beads' Hermite was kept.
- **Bill GRAIN in `FxBus::COST`:** every Part would lose a voice whether or not GRAIN is on.

## Consequences
- The delay is two instruments in one slot; a send's echo keeps its level and TIME across TYPEs.
- GRAIN costs a voice of the heaviest INITs while it plays.
- A longer grain buffer, reverse grains, SHAPE and stereo spread are follow-ups, each needing RAM or a pan the bus doesn't have.
- The FX bus's params are not in a Part's MIDI map or the matrix; GRAIN inherits that.

## Sources
- Beads, `pichenettes/eurorack` `beads/dsp/` (MIT, Copyright 2017 Emilie Gillet): `granular_player.h`, `delay_granular_player.h`, `delay_grain.h`, `grain.h`, `grain_envelope.h`, `voice_stealing_envelope.h`, `randomized_parameter.h`, `trigger_generator.cc`, `granular_processor.h`, `processor.cc`, `delay_line.h`, `limiter.h`. THIRD_PARTY.md.
- #612; ADR 0014, 0053, 0061, 0090, 0097, 0137, 0141, 0198.
- The listening page: (private notes) (TAPE against GRAIN through the full engine, raw levels).
