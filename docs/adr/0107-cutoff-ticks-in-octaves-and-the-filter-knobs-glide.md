# 0107. CUTOFF ticks 1/24 octave; a Part's filter knobs glide 30 ms before the routes

- **Status:** Proposed
- **Deciders:** owner (UAT 2026-10-07: "the filter should be smooth and never ever steppy"), firmware

## Context
The root-cause report (private notes) found the stepping in the parameters, not in any TYPE's DSP. Every TYPE already ramps `g` and `k` across a block (#53). But:

- CUTOFF ticked **156 Hz linear**: 1,099 c a tick at 176 Hz, 17 c at 15.6 kHz. The panel's acceleration (1, 2, 4, 5, 10 ticks a detent) made a normal turn up to 10 ticks, **998 c in one block** (750 c/ms) on the real path.
- Nothing eased CUTOFF, RES or the FREQ/Q route amounts (ENV AMT, KEYBD, FM1–3, QM) between the encoder and the voice. Each detent reached the filter as a one-block jump, then held: a staircase, and on a resonant 2040 each stair pings.
- CLAUDE.md: "All parameter changes lerped in UI — never snap". The stage gains were eased (20 ms, `StageEase`), the filter knobs never were.

## Decision
1. **CUTOFF ticks 1/24 octave (50 c)**, a ratio, anywhere in 20 Hz–20 kHz. `ParamSpec` gains a byte-sized `scale`: `Scale::Linear` (every other param, as before) or `Scale::Octaves`, in which `step` is octaves a tick (a ratio). An `Octaves` param also displays, snaps and denormalizes linearly in octaves (`normalize`/`denormalize`), so the bar, the filter graph and the mod bars sit on a log axis. With the acceleration a slow detent is 50 c, a fast one 500 c, and a fast turn crosses the whole range in **27 detents** (was 13).
2. **CUTOFF reads in Hz**: `ValFmt::Law(Law::Cutoff)` over `CUTOFF_HZ` (20 Hz × 2^(9.966·p)): `440 Hz`, `2.65 kHz`, `20.0 kHz`. Its shift-snap points are the `Law` ones: 20 Hz, ~4.6 kHz, 20 kHz.
3. **A Part's filter knobs glide**, once a block, in `Instrument` (`Knobs`, one per Part): CUTOFF in octaves (its 0..1 place on `CUTOFF_HZ`), RES and the FREQ and Q columns' eight amounts each linearly, all a one-pole of **τ = 30 ms** (`KNOB_EASE_S`). The voices read the eased knobs in place of the stored ones (`Voice::render_knobs`), *then* add their routes' offsets (ENV, LFO, NOTE, taps), so an envelope or a tap keeps its snap, block for block, exactly as before.
4. **A silent Part and a new Sound land**: with no voice on a Part, its knobs aren't eased at all and the next note's first block lands on the stored values, so a turn made while nothing sounds is where the next note starts. A Sound loaded into a Part (`Part::loads`, bumped by each load, published in `PartAudio`) lands too, even under a voice still ringing: no sweep from the old Sound's knobs. A NaN target (a raw write) lands, so it never sticks. A settled glide hands the voice the stored CUTOFF itself, bit for bit.
5. The glide is before the filter, so **every TYPE** (CLEAN, 2040, DIRTY, and LADDER or any later one) gets it with no code of its own.
6. **Patches are untouched**: CUTOFF is still stored in Hz, so an old patch loads and sounds as it did. Nothing to translate. Only the turn (tick size) and the readout change.

τ = 30 ms: within the 20–40 ms the report measured. A lone 50 c detent spreads over ~90 ms (3τ), 2.2 c a block, under the ~5 c a pitch must move to be heard moving. The report's 10-tick turn peaks at 26 c/ms (35 c a block) against the hand's own 17 c/ms, where it was 998 c in one block. The knob trails the hand by 30 ms; 40 ms would shave the fast peak to ~24 c/ms for a laggier feel.

## Alternatives considered
- **Ease per voice** (two more `Ease`s in `StageEase`, +16 B a voice): costs ADR 0101's tight voice chain, and each voice would glide on its own from its own start. Per Part costs no voice bytes and every voice of the Part moves together.
- **Ease in the UI only:** the UI frame is ~33 ms, so a UI lerp makes a big step into frame-rate steps. The glide must be at block rate.
- **Ease the stored value inside each TYPE:** every TYPE (and LADDER) would need its own, and the envelope's offset would be eased with it, losing the pluck.
- **A second one-pole (critically damped):** smoother still (no slope kink per detent), but the report's 40 ms one-pole already measured 0 ms over 20 c/ms of centroid slew on every TYPE. Kept for later if UAT asks.
- **A finer RES or route-amount law:** not needed for smoothness once they glide; RES and the amounts keep their 128 steps.
- **Store CUTOFF as a 0..1 position:** changes patch meaning and the DSP's reads for nothing the tick law and the display don't already give.

## Consequences
- CPU (estimated, not benched on the unit): per Part with a voice, a block: one `log2`, one `exp2` while gliding, 16 amount divides and 18 one-pole steps, ~400–500 cycles: 6–8 cycles a sample per sounding Part, ≤ ~50 with all six sounding, ~0.5 % of the 10,000 a sample at 480 MHz. A silent Part costs a flag. The voices' FREQ/Q sums read eight `f32`s in place of eight `i8`s.
- RAM: `[(Knobs, Loads); 6]` and the coefficient in `Instrument`, ~0.5 KB of D2's 121 KB left. No voice-chain bytes.
- Flash: +1.8 KB on the default build (49.0 KB left with LADDER and EXP B). The bench's `bench-stress,orbit` set was 28 B from full on main and overflowed with this; the bench builds now leave out the console's `put` (`card-put`, ~7 KB, which no bench run needs), so every bench set keeps ≥ 5 KB (#303).
- A turn of CUTOFF, RES, ENV AMT, KEYBD or a jack amount now lands 30 ms late (63 % in 30 ms, 95 % in 90 ms).
- A p-lock or MIDI CC on CUTOFF (when wired) glides too; it should denormalize through `ParamSpec::denormalize`, so a 7-bit CC steps 1/12.7 octave, eased.
- Not eased: the taps' amounts (OP 5/6 → FREQ, Q) keep their one-block ramp; ENV AMT's own law stays 10/127 oct a unit (94 c), coarse but now glided.
- Regression: `chimera-core/tests/filter_smooth_test.rs` turns CUTOFF slow and fast, RES, ENV AMT and KEYBD through the panel's acceleration on every TYPE and bounds the cutoff's move per ms; ENV 1 → FREQ is pinned to its pre-easing trajectory.

## Sources
- (private notes) and its harness `steppy/steppy.rs.txt`.
- ADR 0096 (filter types), ADR 0101 (voice chain bytes), #53 (the `g` ramp), #391 (ENV → FREQ sub-block, separate).
- `chimera-core/src/dsp/filter/knobs.rs`, `chimera-core/src/block.rs` (`Scale`), `chimera_hal::encoder_accel`.
