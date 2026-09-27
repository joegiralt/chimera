# FX Diet — Design

A small effects bus that sounds good, so the engine gets the CPU. One Alesis-style reverb replaces the three reverbs. The tape delay and Juno chorus stay, and each gets a hard budget. Tracks #16.

## Roadmap position

This comes first among the remaining algo-engine work, because it frees the CPU that the modulation modes need. The order is: FX diet, then algo sub-project 4 (pages and gang edit), 2 (modulation modes), 3 (full wave set), 5 (TX character).

## Intent

- The whole FX bus runs in **≤ 1,000 cycles per sample** on rev V at 480 MHz. Today it takes 3,310.
- Every factory Sound gets 6 voices beside the bus. The costliest possible patch (842 per voice) fits 6 × 842 + 1,000 = 6,052 ≤ 7,000, leaving about 190 cycles per voice for the modulation modes.
- The reverb should call to mind the early Warp records: grainy, dark, wide and metallic in a musical way. Its reference is the Alesis Quadraverb.

**Done when:**
- the chip bench measures the bus at ≤ 1,000;
- `FxBus::COST` is committed from that reading;
- a six-voice test passes on the costliest patch;
- the user has heard the loopback takes.

## What changes

| | Before | After |
|---|---|---|
| Reverb | Plate, FDN and MidiVerb, chosen by TYPE | One Alesis-style ring (§ Reverb) |
| Delay | Tape delay, mono | Same controls, mono, centred; budget 250 |
| Chorus | Juno I, II and I+II, mono return | Same modes, **stereo** return; budget 150 |
| Return | Mono, on both sides of DAC pair 1 | Reverb and chorus return stereo, delay centred |
| Bus distortion | None | None: each voice's Drive covers it |

**Budget split:**

| Part of the bus | Cycles per sample |
|---|---|
| Reverb | 450 |
| Delay | 250 |
| Chorus | 150 |
| Bus and mixing | 150 |

**Unchanged:**
- each Part has three sends (chorus, delay, reverb);
- the FX settings belong to the Performance;
- each effect returns wet only, and its MIX is the return level (ADR 0015);
- the dry signal reaches the DACs through the Parts.

## Reverb

### Rate
- The reverb runs at 24 kHz.
- The input is decimated by 2 through a short half-band FIR. The output is interpolated back to 48 kHz with the same half-band.
- The cost halves, and the band limit at 12 kHz supplies the dark top.

### Topology
The Quadraverb is described by Sean Costello, Valhalla DSP, KVR thread 349039: "4 parallel loops (2 x AP + 1 delay), outputs from delay taps", with the end of each loop fed into the start of the next.

- Four stages, S1 to S4, arranged in a ring. Stage *k* runs: allpass → allpass → delay → one-pole low-pass (DAMP) → gain *g* (TIME).
- The ring runs S1 → S2 → S3 → S4 → S1. The input is added at S1.
- **Left output:** the sum of two taps inside S1's delay and one inside S3's.
- **Right output:** the sum of two taps inside S2's delay and one inside S4's, at different fractions of their lengths.
- The allpass coefficients are fixed at 0.6 or 0.5, alternating by stage.
- All 12 lengths (8 allpasses and 4 delays) are distinct and mutually prime at SIZE 1. They are chosen so the longest round trip of the ring is about 1 s at 24 kHz. The lengths are set once in the plan, then fixed.

### Storage
- Every allpass and delay line holds i16 samples. The whole ring, at 24 kHz with SIZE at its maximum, is at most 48 KB.
- Writing through i16 caps precision at 16 bits, like the hardware, even with GRIT at 0.

### Controls
The page is EFX. TYPE's slot becomes GRIT; the other slots keep their order.

| Slot | Param | Range | Maps to |
|---|---|---|---|
| a | GRIT | 0..1 | Loop-state quantisation, from none (16-bit storage only) to 10 bits |
| b | TIME | 0..1 | Target RT60, exponential from 0.3 s to 12 s. Each stage's gain is *g_k* = 10^(−3·L_k / (RT60·24000)), where *L_k* is the stage's current length, capped at 0.97 |
| c | DAMP | 0..1 | In-loop low-pass cutoff, 11 kHz down to 1.5 kHz |
| d | SIZE | 0..1 | Scales every delay and allpass length, 0.35 to 1.0 |
| e | MIX | 0..1 | Return level |

`ReverbParams { grit, time, damping, size, mix }` replaces `reverb_type`. Nothing is persisted yet, so nothing migrates.

### Guarantees
- **Stable:** the ring's loop gain is below 1 at every setting. *g* ≤ 0.97, the allpasses are unity-gain, and the low-pass gain is ≤ 1.
- **No zipper noise:**
  - TIME and DAMP are smoothed per sample.
  - SIZE moves the read taps with linear interpolation, slewed over about 50 ms.
  - GRIT is smoothed.
- **Allocation-free:** the ring lives in the FX bus in AXI (ADR 0014), and the existing size assertion still holds.

## Delay

- The controls are unchanged: TIME 10–500 ms, FEEDBACK, WOW, SATURATION, TONE and MIX.
- The return is mono and centred.
- **If it goes over 250 on the bench:** its saturation becomes a cubic soft clip, `x − x³/3` clamped at ±1 after scaling, matched to the old curve within 0.5 dB over the used range.

## Chorus

- Modes I, II and I+II are unchanged.
- The wet signal returns in stereo: left +wet, right −wet (the Juno trick).
- **If it goes over 150 on the bench:** I+II derives both lines from one shared triangle LFO, with the second phase-offset.

## Bus

- Each Part's sends are summed into three mono buses, one per effect.
- The chorus and reverb returns are stereo. The delay return is added to both sides at equal level.
- The sum lands on DAC pair 1, as today.
- Work is done per block. Nothing allocates, and nothing blocks.

## UI

- On the EFX page, the TYPE slot becomes GRIT. The page's viz, EffectsFlow, is unchanged.
- The DELAY and CHORUS pages are unchanged.

## Testing

All tests run on the host.

- **Stability:** at every combination of TIME, SIZE and DAMP from 0 to 1 in 5 steps, and GRIT at 0 and 1:
  - an impulse decays;
  - the output stays finite and bounded for 60 s of rendering;
- **RT60:** it rises monotonically with TIME, is within ±25 % of the target at SIZE 0.35, 0.7 and 1.0, and stays within the range in § Controls.
- **Stereo:** the correlation of the left and right impulse responses is below 0.5, and neither side is silent.
- **GRIT:**
  - at 0, the loop state takes only i16 values;
  - at 1, it takes only values on the 10-bit grid;
  - the change between the two is monotonic.
- **Band limit:** with white noise in, the return's energy above 12 kHz sits below the half-band's stopband (−40 dB relative to the passband).
- **Clicks:** moving TIME, SIZE, DAMP or GRIT mid-tail makes no sample step beyond `click_free_test`'s bound.
- **Chorus stereo:** left is +wet and right is −wet, exactly.
- **Delay:** if the soft clip replaces the old curve, it is within 0.5 dB of it across the input range.
- **Budget:**
  - the per-effect `COST` constants add up to `FxBus::COST`;
  - a six-voice test on the costliest patch passes beside `FxBus::COST`.
- **Goldens:**
  - the `reverb_send_on` / `reverb_send_off` instrument goldens and any FX goldens are re-recorded after the sanity gate (ADR 0011);
  - every other golden is unchanged.

## Bench and hardware

- **Bench:** rows REVERB, DELAY, CHORUS and BUS, each a per-sample cost, plus the TOTAL. The user runs the bench, and `FxBus::COST` is committed from the reading, rounded up to the next 10.
- **Loopback:** MORPH PAD through the reverb at three settings (default, long and dark, full GRIT), published as a page for the user to judge.

## ADRs

- **New: one Alesis-style reverb.**
  - The Quadraverb topology, citing Costello.
  - The half rate, the i16 storage and GRIT.
  - It retires Plate, FDN and MidiVerb.
- **New, if the bench confirms it:** every patch gets 6 voices, which supersedes the voice-count consequence of ADR 0026.

## Out of scope

- Tempo-synced delay, which comes with the internal clock later.
- A ping-pong delay, which needs a second line, and there is no RAM for it.
- Bus distortion.
- Per-Part effect buses.

## Risks

- **The reverb misses 450:** first move the allpasses to a cheaper form, then drop to three taps per side. If it still misses, bring the numbers to the user.
- **RT60 accuracy:** each stage's gain is derived from the target RT60 and its current length, so SIZE doesn't change the decay time. The 0.97 cap only binds for very short stages with long targets. The tests check the measured RT60 against the target within ±25 %.
- **The i16 grain is too strong at GRIT 0:** store the ring as i32 instead. That doubles the memory to 96 KB, which still fits AXI once the old reverbs are gone.
