# 0050. Gain staging: voice-sum trim and a final peak limiter

- **Status:** Proposed
- **Deciders:** owner; firmware (#190)

## Context
Nothing in the bus left headroom. A host probe that drives the real FX
types and the real `Instrument` (#190) measured:

- The voice sum had no trim. Eight saws at 0.8 peak summed to 4.93
  (+13.9 dBFS). Three voices, with the FX off, already clipped the DAC.
  With the default Sound, eight voices at velocity 127 and the FX off, 12 %
  of pair 1's samples hit the clamp.
- The sends are taken from that hot bus. The reverb's return is `WET_GAIN`
  4.54, about +11 dB over the send's peak at MIX 1 (+12 dB with TIME and
  SIZE at max). The i16 ring reaches its rail only at maximum settings
  (#142).
- The compressor's makeup (up to +24 dB) and the tape's 1.78 ceiling both
  fed the only safety, the hard `clamp(-1, 1)` in `to_dac`. At maximum
  settings 85 % of samples were clamped.
- The desktop applied `tanh(0.7·x)` before its speakers, so it hid all of
  this.

## Decision
- **A trim, around the FX bus.** `VOICE_SUM_TRIM` is 1/√8 (−9.03 dB), so
  eight voices summed at random phase land where one voice did.
  - The dry path takes it in `mix_parts`' hoisted gains (`level × trim ×
    pan`): no work per sample.
  - The sends stay untrimmed, and the FX bus's return is trimmed as it is
    added to pair 1. That multiply fuses into the existing add (VMLA), so
    it adds no instruction.
  - Why around the bus and not into it: to keep the reverb's character.
    The ring is fixed point, and GRIT's grid does not scale with its
    input. A send 9 dB lower made its quantisation relatively 9 dB louder
    (0.7 % of the return, −43 dB, in `reverb_send_on`). Now the ring hears
    exactly what it heard before the trim, and its output is the old
    output × 1/√8.
  - The same rule covers the whole bus. The chorus is linear float (its
    only integer is the LFO phase). The delay is float, but its SAT
    `tanh` depends on level, and REV SEND feeds the delay's return into
    the reverb. Keeping every send at its old level keeps both exact.
  - The dry/wet balance is unchanged.
  - The tape and the compressor, in the master section, do see the
    trimmed mix.
- **One limiter, last.** `Limiter` (`dsp/limiter.rs`) runs after the
  master section (`FxBus::limit`, called by `mix_parts` after
  `FxBus::master`). It is a peak limiter linked across all three pairs,
  both sides:
  - ceiling −1 dBFS (`CEILING` 0.891; each chunk that goes over is scaled
    to 2^-20 below it, room for the roundings after the detector);
  - lookahead of one block: 64 samples, 1.33 ms at 48 kHz;
  - the gain is found once per 4-sample chunk, the compressor's `STEP`,
    and ramped linearly between chunks with the compressor's
    `ramp_gains`;
  - the attack is instant. Each chunk end takes the least gain any of the
    next 16 chunks asks for, a van Herk minimum over two blocks. The gains
    are then averaged over 16 chunk ends, so the gain reaches its target
    within the lookahead and no sample passes the ceiling;
  - the release is a 50 ms one-pole. It snaps to its target within 1e-4:
    closer than that, an f32 one-pole's steps fall under half an ulp and
    it would stall short of unity;
  - the average is summed in 24-bit fixed point, so it cannot drift. At
    rest it is exactly 1.0: below the ceiling the output is the input one
    block late, bit for bit;
  - the detector compares magnitudes as integer bits, with no FPU flag
    round trip;
  - a non-finite sample reads as full scale. It has no level to measure,
    and the DAC plays it at full scale at most (±inf clamps, NaN plays
    silence), so it asks for the ceiling's 1 dB and no more. A fault
    upstream cannot mute the music around it, and a finite over in the
    same chunk is still measured exactly. The slower exact pass runs only
    for a chunk that holds a non-finite sample.
- **The clamp stays.** `to_dac` keeps its clamp as a safety that the
  limiter keeps from ever engaging.
- **One output stage on both targets.** Both get the limiter from
  `Instrument::render`, through `mix_parts`. The desktop plays each pair
  as its DAC does, `to_dac(x).level()`: the same clamp and 24-bit steps,
  not tanh. The hardware's three DACs go to three outputs; the desktop sums
  them for its speakers. It puts that sum through the same stage again, so
  the speakers never get more than full scale, and it counts any frame
  that stage clamps and reports it from the UI thread. Only several busy
  pairs together can do that.
- **The reverb is untouched.** Its tone, tail, internal gains and
  `WET_GAIN` stay as they are, and so do its input levels. Its level is a
  separate decision, made by ear (see Consequences).

## Alternatives considered
- **A soft clipper after each stage.** It spreads distortion across the
  bus, and each stage's curve colours the sound even below full scale.
  Superposition would no longer hold anywhere. The owner wants one
  non-linear stage, at the end.
- **tanh at the output, as the desktop had.** It is not transparent: the
  desktop's `tanh(0.7·x)` scaled everything by 0.7 and was already 1 dB
  into its curve at 0.87. It hides overs instead of preventing them, and
  the hardware had no such stage.
- **Recalibrating the reverb (`WET_GAIN`).** The reverb's hot return is
  part of the sound the owner likes. With the trim and the limiter in
  place it cannot clip the DACs. Whether it should be quieter is a
  question for the ear, kept for later (#190).
- **Trimming the sends too (this ADR's first draft).** It changed the
  reverb's character, as above.
- **A trim of 1/8 (the worst coherent case).** That takes 18 dB from every
  note, when eight coherent peaks almost never line up. The limiter covers
  the rare case.
- **A lookahead of exactly 48 samples (1 ms).** That needs a ring buffer
  and a window that is not a power of two. One block delays by a single
  swap per chunk, and its reciprocal is exact.
- **Muting on NaN (gain 0), the first draft.** One bad sample would
  silence up to 50 ms around it.

## Consequences
- A single note is about 9 dB quieter than before, at every output. The
  FX returns are too, by the same amount, so the balance holds.
- Measured with the probe after the change:
  - the default Sound, eight voices: 0 % clamped (was 12 %);
  - the synthetic eight-saw chord with the FX off peaks at 0.99 before the
    limiter (was 2.79) and 0.89 after it;
  - at maximum settings, with +24 dB makeup: 6.0 before the limiter, 0.89
    after it, 0 % clamped (was 85 %).
- The reverb's ring sees what it saw before this change.
  - With typical FX (sends 0.3) its peak is 17,700 of 32,767 in the probe,
    and 6,852 to 13,646 with real voices, with no word on the rail.
  - At send 1 with the default Sound, or at maximum settings, it reaches
    the rail as before: 6.3 % of the words at maximum (#142, separate).
- **The limiter will pump at high reverb sends.** At MIX 1 the reverb's
  return peaks 11–12 dB over its send, and more with the long settings. At
  send 1 the limiter takes 16–18 dB of gain reduction. Whether the
  reverb should be quieter is left to the owner, by ear.
- Output latency grows by one block, 1.33 ms.
- **Cost: estimated, unmeasured.** The limiter is about 88 instructions
  per sample on the M7 (static count of the release build), all three
  pairs every block, so an estimated 50–65 cycles per sample, pending a
  bench reading.
  - The bench's BUS row runs `mix_parts`, so it times the limiter.
  - `FxBus::COST` (1,360) is the reading from before the limiter and stays
    until a bench run replaces it.
  - If the limiter costs more than 28 cycles, the costliest patch (889)
    drops from five voices to four on rev Y. Rev V keeps six up to 306
    cycles.
  - The FX bus is already over its target (#141).
  - The trim itself costs nothing: its multiplies fold into gains or fuse
    into existing adds.
- Memory: `Limiter` is 1,688 bytes inside `FxBus`, in AXI SRAM: one block
  of all three pairs (1,536 B) and the gain state. No heap. Flash grows by
  about 2.7 KB.
- The five instrument goldens were re-recorded. Each is its old render ×
  1/√8, one block late, within 5e-8 of full scale; the reverb case
  included. The per-voice and FX goldens are upstream of the mix and do
  not change.

## Sources
Issue #190 and its probe (`gainprobe`); #141 (FX bus cost); #142 (ring
rail); ADR 0030 (master section); ADR 0011 (goldens); M. van Herk, "A fast
algorithm for local minimum and maximum filters on rectangular and
octagonal kernels", Pattern Recognition Letters 13 (1992);
`gain_staging_test.rs`.
