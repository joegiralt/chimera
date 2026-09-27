# FX Diet — Design

A small effects bus that sounds good, so the engine gets the CPU. One Alesis-style reverb replaces the three reverbs. The tape delay and Juno chorus stay, and each gets a hard budget. Tracks #16.

## Roadmap position

This comes first among the remaining algo-engine work, because it frees the CPU that the modulation modes need. The order is: FX diet, then algo sub-project 4 (pages and gang edit), 2 (modulation modes), 3 (full wave set), 5 (TX character).

## Intent

- The whole FX bus, including the Instrument's per-Part mixing, runs in **≤ 1,000 cycles per sample** on rev V at 480 MHz. Today it takes 3,310.
- On rev V every factory Sound gets 6 voices beside the bus. The voice budget is 7,000; minus the bus that leaves (7,000 − 1,000) / 6 = 1,000 per voice. The costliest patch (A16 ∪ A17) is 842 per voice in ADR 0026's model, so about 158 cycles per voice are left for the modulation modes. 842 is a model value, not a bench reading; the bench measures it in this work.
- On rev Y at 400 MHz the budget is 5,833, and (5,833 − 1,000) / 842 = 5.7, so the costliest patch gets **5 voices** there.
- The reverb should call to mind the early Warp records: grainy, dark, wide and metallic in a musical way. Its reference is the Alesis Quadraverb.

**Done when:**
- the chip bench measures BUS at ≤ 1,000;
- `FxBus::COST` is committed from that reading;
- the bench's A16 ∪ A17 row is ≤ 1,000 per voice, and the allocator grants that patch 6 voices on rev V;
- the user has heard the loopback takes.

## Order of work

1. **Bench each effect first**, on today's code (§ Bench). The three reverb totals sit within 234 cycles of each other, so most of today's 3,310 is likely the delay, chorus and mixing, not the reverb.
2. The delay's `libm::sinf` fix (§ Delay).
3. The chorus's stereo tap and loop hoists (§ Chorus).
4. The reverb.
5. The stereo bus, then re-bench and commit `FxBus::COST`.

## What changes

| | Before | After |
|---|---|---|
| Reverb | Plate, FDN and MidiVerb, chosen by TYPE | One Alesis-style ring (§ Reverb) |
| Delay | Tape delay, mono | Same controls, mono, centred |
| Chorus | Juno I, II and I+II, mono return | Same modes, **stereo** return |
| Return | Mono, on both sides of DAC pair 1 | Reverb and chorus return stereo, delay centred |
| Bus distortion | None | None: each voice's Drive covers it |

**Budget split (provisional):** these are targets until step 1's bench reads each effect. The plan re-derives them from that reading; only the 1,000 total is fixed.

| Part of the bus | Cycles per sample |
|---|---|
| Reverb | 450 |
| Delay | 250 |
| Chorus | 150 |
| Per-Part mixing and bus | 150 |

**Rule:** no f64-backed libm call (`sinf`, `cosf`, `powf`, `exp2f` …) runs per sample in any FX path. On `thumbv7em-none-eabihf` they go through soft-float f64. Per-block calls are allowed and are billed in the effect's row.

**Unchanged:**
- each Part has three sends (chorus, delay, reverb);
- the FX settings belong to the Performance;
- each effect returns wet only, and its MIX is the return level (ADR 0015);
- the dry signal reaches the DACs through the Parts.

## Reverb

Provenance: the topology follows Sean Costello's description of the Quadraverb (Valhalla DSP, KVR thread 349039): "4 parallel loops (2 x AP + 1 delay), outputs from delay taps", the end of each loop feeding the start of the next.

### Rate
- The ring runs at `fs_ring = sample_rate / 2` (24 kHz on the chip). Every formula below uses `fs_ring`, never a literal.
- The send is decimated by 2, and each output side is interpolated back, through the same 35-tap half-band FIR:
  - passband 0–10.5 kHz, ripple ±0.06 dB; stopband from 13.5 kHz, ≥ 43 dB down;
  - centre tap 0.5; odd taps h[±1], h[±3] … h[±17] = 0.316963, −0.102118, 0.057184, −0.036748, 0.024702, −0.016701, 0.011078, −0.007044, 0.006066; all even taps off centre are 0;
  - interpolation zero-stuffs and applies gain 2; there are two interpolators, L and R.
- The cost halves, and the band limit supplies the dark top.

### Topology
- Four stages, S1 to S4, in a ring S1 → S2 → S3 → S4 → S1. Stage *k* runs: allpass → allpass → delay → one-pole low-pass (DAMP) → gain *g_k* (TIME).
- Each allpass is a Schroeder allpass with coefficient *c*: `v = x + c·v[n−M]`, `y = v[n−M] − c·v`. The stored value is `v`.
- **Input:** the decimated send *u* is added as +0.5·*u* at S1's input and −0.5·*u* at S3's, so both halves of the ring start at once.
- **Lengths and coefficients at SIZE 1** (ring samples, all primes):

| Stage | Allpass 1 (M, c) | Allpass 2 (M, c) | Delay D | L_k = D + M1 + M2 |
|---|---|---|---|---|
| S1 | 151, +0.6 | 313, −0.6 | 4,903 | 5,367 |
| S2 | 181, +0.5 | 353, −0.5 | 5,147 | 5,681 |
| S3 | 211, +0.6 | 397, −0.6 | 5,393 | 6,001 |
| S4 | 241, +0.5 | 277, −0.5 | 5,641 | 6,159 |

  The round trip is ΣL = 23,208 samples, 0.97 s. The allpasses are 9 % of it. Within each stage the shorter allpass is +*c* and the longer −*c*, so their group-delay peaks don't all land at DC; the low band's decay runs about 4 % long from it.
- **Output taps** (a fraction of that stage's current delay length, rounded down; sign):
  - Left: S1 @ 0.07 (+), S2 @ 0.41 (−), S3 @ 0.73 (+).
  - Right: S3 @ 0.11 (+), S4 @ 0.47 (−), S1 @ 0.79 (+).
  - Each side is `WET_GAIN · Σ sign·tap`. Each side's first tap is in a stage fed directly (14 ms and 25 ms at SIZE 1).
  - `WET_GAIN` is a const, set so the return's RMS at default settings matches the old plate's at the same MIX within 1 dB. The plate's RMS is recorded as a number before the plate is deleted.

### SIZE
- SIZE is quantised to 32 steps: step *i* = round(SIZE·31), scale *s_i* = 0.35 + 0.65·*i*/31.
- Every length at step *i* is the nearest prime to its SIZE-1 length × *s_i* that is not already used in that step. A `const fn` builds the 32 × 12 table of `u16`. Lengths are integers, so reads need no interpolation.
- **A step change crossfades, it never slews.** For 720 ring samples (30 ms), every allpass read, delay read and output tap reads at both the old and the new length, blended linearly. Writes don't change. Nothing moves a read pointer, so there is no pitch shift.
- The blend is linear, not equal-power: each blended read's gain stays ≤ 1, so the ring stays stable mid-crossfade.
- A step that arrives during a crossfade waits for it to end; the latest target wins.
- The crossfade doubles 18 reads while it runs. The bench bills it (§ Bench).

### Storage
- Each line is sized to its SIZE-1 length + 1: 23,220 samples. It is held as i16 with full scale ±2.0 (1.0 is stored as 16,384), leaving 6 dB above a full send. Writes saturate (`as i16` saturates).
- The ring is 46,440 B, asserted in a `const`. After the diet the FxBus is about 160 KB of the 256 KB `FX_BUS_BUDGET`, down from 253,612 B.
- f32 storage (92,880 B) would also fit. i16 is chosen for the grain, and for a smaller D-cache footprint, not for memory.

### GRIT and quantisation
- GRIT sets a continuous bit depth *b* = 16 − 6·GRIT, from 16 bits down to 10. The step is Δ = 2^(6·GRIT) i16 LSBs.
- Every value written into a delay line, an allpass state or the DAMP state is quantised onto the grid, then onto the LSB, then stored. Nothing else is quantised.
  - The 8 allpass state writes (`v`) use magnitude truncation toward zero: `q_trunc(x) = trunc(x·16384 / Δ) · Δ`.
  - The delay writes and the DAMP state round to nearest: `q_round(x) = round(x·16384 / Δ) · Δ`.
  - Both saturate at the i16 range.
- **Why the mix, and no gate.** Rounding everywhere sustains a deadband tail of 12–32 Δ with no input, far above any gate that would not also cut a live tail. Truncation in the allpass recursions kills it: the ring reaches exact zero on its own, in about 10 s at GRIT 0 and 4 s at GRIT 1, after 1 s of noise at TIME 1. There is no silence gate. The earlier "truncation cut T30 by 79 %" came from an impulse, measured at the quantisation floor; with a noise burst, every quantiser holds GRIT 0's RT60 within 0.76–1.08 of the target.
- At GRIT 0, Δ = 1: the i16 storage only. At GRIT 1 the grid is 64 LSB, 1/256 of 1.0 (−48 dB).

### Controls
The page is EFX. TYPE's slot becomes GRIT; the other slots keep their order.

| Slot | Param | ParamId | Range | Default | Maps to |
|---|---|---|---|---|---|
| a | GRIT | 5 (new) | 0..1, step 1/128 | 0.3 | § GRIT |
| b | TIME | 1 | 0..1, step 1/128 | 0.5 | Target RT60 = max(0.3·40^TIME s, T_min(SIZE)) |
| c | DAMP | 2 | 0..1, step 1/128 | 0.3 | Cutoff *f_c* = 11,000·(1.5/11)^DAMP Hz |
| d | SIZE | 3 | 0..1, step 1/31 | 0.5 | § SIZE |
| e | MIX | 4 | 0..1, step 1/128 | 0.0 | Return level |

- **ParamIds:** GRIT takes the new `ParamId(5)`. TYPE's `ParamId(0)` is retired and never reused (ADR 0009).
- **TIME:** the target is frequency-averaged. The ring's round-trip gain Π*g_k* is spread over its mean delay ΣL_k. So *g_k* = 10^(−3·L_k / (RT60·fs_ring)), with L_k from the current SIZE step, capped at 0.97. The cap never binds in these ranges: the largest *g_k* is 0.956 (SIZE 0.35, 12 s).
- **TIME floor:** each allpass rings on its own for about 13.5·M / fs_ring. TIME can't go below T_min(SIZE) = max(0.3 s, 1.5 × 13.5 × M_max(SIZE) / fs_ring), where M_max is the longest allpass at that step. T_min is 0.3 s up to SIZE ≈ 0.9 and rises to 0.335 s at SIZE 1. At short TIME and large SIZE the tail is a few discrete passes rather than a smooth decay; that is the character.
- **GRIT and the tail:** at high GRIT the truncation shortens long tails. At GRIT 1 the tail reaches about 0.34–0.9 of the TIME target, shortest at long TIME and small SIZE. That is part of the grain's character.
- **DAMP:** the one-pole is `y += a·(x − y)`, with *a* = 1 − exp(−2π·*f_c* / fs_ring), in (0, 1], so its gain is ≤ 1. It acts once per pass, so a smaller SIZE (more passes per second) gives a darker tail. That coupling is intended.
- **Defaults:** `ReverbParams::default()` and `REVERB_SPECS` agree: GRIT 0.3, TIME 0.5, DAMP 0.3, SIZE 0.5, MIX 0.0. `FxParams::default()` uses `ReverbParams::default()` instead of spelling the fields out.
- `ReverbParams { grit, time, damping, size, mix }` replaces `reverb_type`. Nothing is persisted yet, so nothing migrates.

### Smoothing
- TIME, DAMP and GRIT are one-pole smoothed once per block (64 samples, 32 ring samples): TIME 50 ms, DAMP 20 ms, GRIT 20 ms.
- From the smoothed values, the four *g_k*, *a* and Δ are computed once per block: four `powf`s per block, not per sample. *g_k* and *a* ramp linearly across the block from the previous block's values. Δ holds for the block.

### Guarantees
- **Stable:** the loop gain is below 1 at every setting and mid-crossfade. *g_k* ≤ 0.97, the allpasses are unity-gain, the low-pass gain is ≤ 1, a linear crossfade's gain is ≤ 1, and quantisation adds at most ½ Δ + ½ LSB per rounded write. The truncated allpass writes only ever shrink a value's magnitude, so with no input the ring reaches exact zero on its own, with no limit cycle.
- **No zipper noise:** see § Smoothing and § SIZE.
- **Allocation-free:** the ring lives in the FX bus in AXI (ADR 0014), and the existing size assertion still holds.

## Delay

- The controls are unchanged: TIME 10–500 ms, FEEDBACK, WOW, SATURATION, TONE and MIX.
- The return is mono, added at unity to both sides.
- **First fix:** `TapeDelay::run` calls `libm::sinf` twice per sample for wow and flutter, and each goes through soft-float f64. Both are replaced with an f32 sine approximation (a parabola with one correction step on the folded phase, error < 0.1 %). The three `%` wraps become compare-and-wrap.
- **Only if it is still over its budget on the bench:** the saturation `tanhf(g·x)/g` becomes a Padé `p(g·x)/g`, where `p(x) = x(27 + x²)/(27 + 9x²)` for |x| ≤ 3 and ±1 beyond. It is within 0.25 dB of tanh over the whole input range.

## Chorus

- Modes I, II and I+II are unchanged.
- **Stereo:** each BBD line gets a second interpolated read tap, driven by the same triangle LFO inverted. Left is the normal tap and right the inverted one. In I+II each side averages its two lines' taps. That is 1 extra read per line (2 in I+II), and no memory.
- The mono sum doesn't cancel, and a panned Part's dry doesn't leave a lone −wet on the far side.
- The chorus has no f64 or libm use. The per-sample `lfo_rate / sample_rate` division and the ms-to-samples multiply move out of the loop, and the `%` wraps become compare-and-wrap.
- **Only if it is still over its budget on the bench:** I+II derives both lines from one shared triangle LFO, the second phase-offset.

## Bus

- Each Part's sends are summed into three mono buses, one per effect.
- The chorus and reverb return stereo. The delay return is added to both sides at unity.
- The sum lands on DAC pair 1, as today. `FxBus::process` writes an L and an R block.
- The Instrument's steps 2–4 (per-Part pan, level, sends and the bus) are split into one function, so the bench can time them without voices.
- Work is done per block. Nothing allocates, and nothing blocks.

## UI

- On the EFX page, the TYPE slot becomes GRIT. The page's viz, EffectsFlow, is unchanged.
- The DELAY and CHORUS pages are unchanged.

## Retired

- `chimera-core/src/dsp/midiverb.rs` and its `pub mod` in `dsp/mod.rs`.
- In `reverb.rs`: `PlateReverb`, `FdnReverb`, `MidiVerbReverb`, `ReverbType`, their lengths and the TYPE dispatch.
- In `reverb_test.rs`: every plate, FDN, MidiVerb and MidiVerbII test. The file is rewritten for the ring.
- In `click_free_test.rs`: the plate, FDN and MidiVerb cases, replaced by one ring case.
- In `fx_golden_test.rs`: `reverb_plate`, `reverb_fdn_max_size` and `reverb_midiverb`, replaced by ring cases that hash both sides.
- In `bench.rs`: `REVERB_TYPES`, its loop and the PLATE/FDN/MV labels.
- Every other use of `reverb_type` (`cost_test.rs`, `in_place_test.rs`, `mixer_page_test.rs`, `block_registry.rs`).

## Testing

All tests run on the host, at the test profile's default opt-level, so the renders are kept short. RT60 and stereo are measured at GRIT 0 only.

- **Stability:** TIME, SIZE and DAMP each at 0, 0.5 and 1, with GRIT at 0 and 1 (54 cases). Each renders 1 s of noise, then 3 s of silence:
  - the output is finite, and within ±12·`WET_GAIN` (three taps at full scale, with 2× for the interpolator's overshoot);
  - the energy of the last 0.5 s is below that of the first 0.5 s after the input stops, by at least half the drop the target RT60 predicts.
- **No limit cycles:** after 1 s of noise at TIME 1, SIZE 1, DAMP 0 and GRIT 1, the return is exactly 0.0 within 15 s of silence. At GRIT 0 the bound is 25 s.
- **RT60:** at TIME 0.25, 0.5, 0.75 and 1, SIZE steps 0, 15 and 31, DAMP 0, GRIT 0. The send is 0.5 s of −6 dBFS noise, then silence. Each side is band-passed to 500 Hz–4 kHz, and the Schroeder EDC of L² + R² from the end of the input gets a least-squares line from −5 to −35 dB; RT60 is 60 dB over its slope. It is within ±35 % of the target and rises monotonically with TIME. An impulse is not used: its diffuse tail starts near 100 LSB and measures the quantisation floor.
- **No early zero:** under a noise send every output block sounds, and after the send stops the tail sounds for at least half the target RT60 at TIME 0.5, at GRIT 0, 0.3 and 1.
- **Stereo:** for an impulse in, each side's 10 ms RMS reaches −20 dB of its own peak within 50 ms. The zero-lag correlation of L and R over a 100 ms window starting at 500 ms is below 0.5.
- **GRIT:**
  - at GRIT 1, every stored value is a multiple of 64; at GRIT 0, `q` is the identity on i16;
  - with a steady −12 dB 1 kHz sine in, the output's noise floor (energy outside 1 kHz ± 50 Hz, 1 s after 2 s of settling) is non-decreasing across GRIT 0, 0.1 … 1, within 0.5 dB, and is at least 24 dB higher at 1 than at 0.
- **Band limit:** with white noise in, at DAMP 0 and GRIT 0, the return's mean power density above 13.5 kHz is at least 40 dB below its mean in 0.5–8 kHz.
- **Clicks:** each of TIME, DAMP, GRIT and SIZE (one step and a full sweep) moves mid-tail. Over the 50 ms after the move, the largest second difference `|x[n] − 2x[n−1] + x[n−2]|` is at most 1.5× the larger of two held renders over the same window, one held at the old value and one at the new. `click_free_test`'s 0.15 absolute bound doesn't suit tails.
- **Chorus stereo:** with a sine in, the mono sum (L + R)/2 keeps at least −6 dB of L's RMS, and L ≠ R.
- **Delay:** the f32 sine is within 0.1 % of `sinf` over a cycle. If the Padé fallback lands, it is within 0.5 dB of `tanhf(g·x)/g` across the input range.
- **Goldens:**
  - `reverb_send_off` stays bit-identical. It is not re-recorded: its send is 0, so a nonzero return would be a regression.
  - Only goldens with reverb, chorus or delay active change: `reverb_send_on` and the FX goldens, re-recorded after the sanity gate (ADR 0011).

## Bench and hardware

- **Method:** every row runs the same fixed noise block through the bus function, with six Parts written and no voices. Rows:
  - MIX: per-Part mixing, every effect off;
  - CHORUS, DELAY and REVERB: that effect alone, at its worst settings, less MIX;
  - BUS: all three at their worst settings, plus mixing.
- **Worst settings:**
  - chorus I+II, RATE 1, DEPTH 1;
  - delay TIME 500 ms, WOW 1, SAT 1;
  - reverb TIME 1, SIZE 1, GRIT 1, DAMP 0.5, with a SIZE crossfade kept running (stepping between 30 and 31).
- `FxBus::COST` is the BUS reading rounded up to the next 10. There is no sum test: cache effects make BUS differ from the sum of its rows.
- The bench adds an A16 ∪ A17 voice row, so the 842 model value is measured.
- The user runs the bench.
- **Loopback:** MORPH PAD through the reverb at three settings (default, long and dark, full GRIT), published as a page for the user to judge.

## ADRs

Each new ADR follows the template, goes in `docs/adr/README.md`, and marks what it supersedes in that ADR's status line, as 0027 did.

- **New: one Alesis-style reverb.**
  - The Quadraverb topology, citing Costello; the half rate, i16 storage and GRIT.
  - It retires Plate, FDN and MidiVerb.
  - It supersedes ADR 0014's rejection of 16-bit delay lines, and records the new FxBus and AXI totals.
  - GRIT takes `ParamId(5)`, and TYPE's `ParamId(0)` is retired (ADR 0009).
- **New: stereo FX returns.** The reverb and chorus return stereo, and the delay mono at unity on both sides. It supersedes ADR 0015 decision 2's mono-return clause.
- **New, if the bench confirms it:** on rev V every patch gets 6 voices. This supersedes ADR 0026's voice-count consequence for rev V only. On rev Y the costliest patch gets 5.

## Out of scope

- Tempo-synced delay, which comes with the internal clock later.
- A ping-pong delay, which needs a second line, and there is no RAM for it.
- Bus distortion.
- Per-Part effect buses.

## Risks

- **The reverb misses its budget:** first move the allpasses to a cheaper form, then drop to two taps per side, keeping each side's early tap. If it still misses, bring the numbers to the user.
- **The delay misses after the sinf fix:** the Padé soft clip (§ Delay).
- **The A16 ∪ A17 row measures over 1,000 per voice:** the costliest patch gets 5 voices on rev V too; bring the numbers to the user before the six-voice ADR.
- **The i16 grain is too strong at GRIT 0:** store the ring as f32 (92,880 B, still inside `FX_BUS_BUDGET`). GRIT then quantises the f32 writes to the same grid.
