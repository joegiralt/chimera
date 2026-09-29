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
  4.54, about +11 dB over the send's peak at MIX 1 (+14 dB with the long
  settings). The i16 ring reaches its rail only at maximum settings (#142).
- The compressor's makeup (up to +24 dB) and the tape's 1.78 ceiling both
  fed the only safety, the hard `clamp(-1, 1)` in `to_dac`. At maximum
  settings 85 % of samples were clamped.
- The desktop applied `tanh(0.7·x)` before its speakers, so it hid all of
  this.

## Decision
- **A trim.** Every Part's bus is scaled by `VOICE_SUM_TRIM`, 1/√8
  (−9.03 dB), so eight voices summed at random phase land where one voice
  did. The trim is folded into `mix_parts`' hoisted gains, the dry
  `level × pan` and each send, so it adds no work per sample. The sends
  follow the trim: the reverb's input drops by the same 9 dB as the dry
  signal, and the dry/wet balance is unchanged.
- **One limiter, last.** `Limiter` (`dsp/limiter.rs`) runs after the
  master section (`FxBus::limit`, called by `mix_parts` after
  `FxBus::master`). It is a peak limiter linked across all three pairs,
  both sides:
  - ceiling −1 dBFS (`CEILING` 0.891; each chunk that goes over is scaled
    to 2^-20 below it, room for the roundings after the detector);
  - lookahead of one block, 64 samples (1.33 ms);
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
    round trip. NaN reads as infinity, so the gain goes to 0 and then
    releases.
- **The clamp stays.** `to_dac` keeps its clamp as a safety that the
  limiter keeps from ever engaging.
- **One output stage on both targets.** The desktop plays
  `to_dac(x).level()` for each pair: the DAC's clamp and 24-bit steps, not
  tanh. Both targets get the limiter from `Instrument::render`, through
  `mix_parts`.
- **The reverb is untouched.** Its tone, tail, internal gains and
  `WET_GAIN` stay as they are. Its level is a separate decision, made by
  ear (see Consequences).

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
- **A trim of 1/8 (the worst coherent case).** That takes 18 dB from every
  note, when eight coherent peaks almost never line up. The limiter covers
  the rare case.
- **A lookahead of exactly 48 samples (1 ms).** That needs a ring buffer
  and a window that is not a power of two. One block delays by a single
  swap per chunk, and its reciprocal is exact.

## Consequences
- A single note is about 9 dB quieter than before, at every output.
- Measured with the probe after the change: the default Sound, eight
  voices, 0 % clamped (was 12 %). The synthetic eight-saw chord with the
  FX off peaks at 0.99 before the limiter (was 2.79) and 0.89 after it. At
  maximum settings, with +24 dB makeup, 6.0 before the limiter, 0.89 after
  it, 0 % clamped (was 85 %).
- With typical FX the ring's peak is 6,247 of 32,767 (was 17,700), with
  no word on the rail. At maximum settings 1.8 % of the words still sit on
  the rail (was 6.3 %; #142). At MIX 1 the reverb's return still peaks
  11 dB over its send (13 dB with the longest settings). At send 1 the
  limiter then takes up to 17 dB of gain reduction, so heavy reverb
  settings pump. That is the data for the separate reverb-level decision.
- The ring is fixed point: GRIT's grid does not scale with its input. A
  send 9 dB lower gives the same tail 9 dB lower, with the ring's
  quantisation relatively 9 dB louder: 0.7 % of the return in the
  `reverb_send_on` golden, −43 dB.
- Output latency grows by one block, 1.33 ms.
- Cost: the limiter is about 88 instructions per sample on the M7 (static
  count of the release build), all three pairs every block. Expect about
  50–65 cycles per sample. The bench's BUS row runs `mix_parts`, so it
  times the limiter. `FxBus::COST` (1,360) is the reading from before the
  limiter; the next bench run replaces it. If the limiter costs more than
  28 cycles, the costliest patch (889) drops from five voices to four on
  rev Y. Rev V keeps six up to 306 cycles. The FX bus is already over its
  target (#141).
- Memory: `Limiter` is 1,688 bytes inside `FxBus`, in AXI SRAM: one block
  of all three pairs (1,536 B) and the gain state. No heap. Flash grows by
  2 KB.
- Five instrument goldens were re-recorded. Each is its old render × 1/√8,
  within 1.6e-7, one block late; `reverb_send_on` also moves by the ring's
  grid, as above. The per-voice and FX goldens are upstream of the mix and
  do not change.

## Sources
Issue #190 and its probe (`gainprobe`); #141 (FX bus cost); #142 (ring
rail); ADR 0030 (master section); ADR 0011 (goldens); M. van Herk, "A fast
algorithm for local minimum and maximum filters on rectangular and
octagonal kernels", Pattern Recognition Letters 13 (1992);
`gain_staging_test.rs`.
