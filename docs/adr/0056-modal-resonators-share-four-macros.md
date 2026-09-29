# 0056. Modal's resonators share four modulatable macros; loops are stable by construction

- **Status:** Proposed
- **Deciders:** project owner
- **Supersedes in part:** [0010](0010-modulation-targets-are-honest.md),
  [0040](0040-eight-voices-modal-strings-to-g1.md),
  [0054](0054-sympathetic-strings-from-a-shared-pool.md) (each as Modal 2
  step A's later tasks record here)

## Context
The owner's bench report (#191) and the survey behind the Modal 2 step A
spec found the string loops unsafe. FDBK added `filtered · fdbk · 0.3`
inside the loop, so any FDBK above about 0.012 made the loop gain exceed 1
(the default, 0.2, did). The string then grew until a ±1.5 clamp, which
can latch DC. No string loop had a DC blocker, so Bowed's stick-slip and
any asymmetric excitation could drift off zero.

## Decision
So far (step A, task 1):
- `modal::loop_parts::LoopGain` is a string loop's gain per pass. Its
  constructors clamp to `[0, 0.9995]`, and NaN gives 0. Every string loop
  multiplies by one: STRING, the SYMP main string, each halo string and
  BOWED. On STRING and the SYMP main string it multiplies the loop's
  sample after the BODY and STIFF taps, so no tap bypasses it.
- `modal::loop_parts::DcBlocker` is a one-pole high-pass at `DC_HZ = 10`,
  `y = g·(x − x1) + r·y1`, `r = e^(−2π·10/fs)`, `g = (1 + r)/2`. Its gain
  is 1 at Nyquist and below 1 elsewhere. Every string loop runs through
  one before the sample is written back. It is reset when the line is
  cleared.
- FDBK and its ±1.5 clamp are deleted from the DSP. `ModalParams::ks_feedback`
  stays, unread, until the parameters are reworked.

Task 2 adds fractional tuning (#163):
- A string's loop is a ring read `delay` samples behind the write, plus
  `loop_parts::Allpass1`, a first-order allpass `(η + z⁻¹)/(1 + η z⁻¹)`
  that carries the fraction of a sample.
- Every in-loop phase delay is compensated exactly at f0.
  - `split(period, other, w)` takes `other` off the period, which today is
    the DC blocker's advance, `dc_phase_delay(r, w)`.
  - It puts the whole part on the line (`floor(d − 0.5)`, at least
    `MIN_LINE = 2`) and the rest, in `[0.5, 1.5)`, on the allpass.
  - `eta_for` inverts the allpass's phase delay exactly:
    `θ = ω(1 − frac)/2`, `η = sin θ / sin(ω − θ)`.
- The loop low-pass is the linear-phase three-tap
  `c/2·(x[d−1] + x[d+1]) + (1 − c)·x[d]`, centred on the line, so it adds
  no delay.
- BOWED's ring runs through the same allpass.
- The fundamental of STRING and the SYMP main string lands within
  0.05 cents from G1 to C7. Whole-sample tuning was up to 54 cents off
  at G1 and 84 cents at C7.
- The line grows from ADR 0040's 984 samples to 1,016, which supersedes
  that in part. At G1 the blocker is a 31.4-sample phase advance, so
  979.6 samples of period need a 1,011-sample line, plus the low-pass's two
  taps. The spec's corner stays at 10 Hz, and the line grows instead.
  - It costs 5,472 B of D2: `Instrument` grows from 161,496 B to
    166,968 B, which leaves 119,752 B.
  - F♯1 and lower clamp to the longest line.

## Alternatives considered
- Keep FDBK and clamp its range below the unity point: its useful range
  would be 0–0.012, and the knob would still be one bad mapping from a
  runaway.
- A DC blocker at the output only: DC would still build up inside the loop
  and eat headroom.

## Consequences
- No undriven string sustains forever: DECAY's longest is today's 0.999 per pass.
- The DC blocker adds a phase delay that sharpens low strings slightly; the
  fractional tuning (step A, task 2) corrects it through `DcBlocker::r`.
- That correction holds at f0 only. The blocker's advance falls with
  frequency, so the upper partials of low strings sit flat of harmonic:
  - about 50 cents at G1, 20 at C3, 10 at C4 and 5 at C5;
  - at a 1 Hz corner, about 5 cents at G1.
- Modal's goldens and the INIT Modal fixtures move, and are re-recorded
  once, at the end of step A.

## Sources
- docs/superpowers/specs/2026-09-29-modal-2-resonators-design.md § 2
- docs/superpowers/plans/2026-09-29-modal-2-resonators.md, Tasks 1 and 2
- ADR 0040 (the 984-sample line), ADR 0054 (the dirty extent)
