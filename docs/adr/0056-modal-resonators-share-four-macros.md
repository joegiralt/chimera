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
  sample after every in-loop stage, so none bypasses it.
- `modal::loop_parts::DcBlocker` is a one-pole high-pass at `DC_HZ = 10`,
  `y = g·(x − x1) + r·y1`, `r = e^(−2π·10/fs)`, `g = (1 + r)/2`. Its gain
  is 1 at Nyquist and below 1 elsewhere.
  - There is one per voice, on each string model's output, outside the loop:
    STRING's string, SYMP's main-and-halo mix and BOWED's ring. It is reset
    at note-on.
  - It is not in the loop because there its phase delay, which falls with
    frequency, detunes the upper partials. That is about 50 cents flat of
    harmonic at G1 at 10 Hz, and no single compensation fixes every
    partial.
  - The loop needs no blocker: with `LoopGain` below 1, DC can't grow or
    latch, and decays with the ring.
- FDBK and its ±1.5 clamp are deleted from the DSP. `ModalParams::ks_feedback`
  stays, unread, until the parameters are reworked.

Task 2 adds fractional tuning (#163):
- A string's loop is a ring read `delay` samples behind the write, plus
  `loop_parts::Allpass1`, a first-order allpass `(η + z⁻¹)/(1 + η z⁻¹)`
  that carries the fraction of a sample.
- Every in-loop phase delay is compensated exactly at f0.
  - `split(period, other, w)` takes `other` off the period. Today `other`
    is 0: the low-pass adds no delay, and the blocker is outside the loop.
    Step A's dispersion (task 8) adds its own.
  - It puts the whole part on the line (`floor(d − 0.5)`, at least
    `MIN_LINE = 2`) and the rest, in `[0.5, 1.5)`, on the allpass.
  - `eta_for` inverts the allpass's phase delay exactly:
    `θ = ω(1 − frac)/2`, `η = sin θ / sin(ω − θ)`.
- The loop low-pass is the linear-phase three-tap
  `c/2·(x[d−1] + x[d+1]) + (1 − c)·x[d]`, centred on the line, so it adds
  no delay.
- BOWED's ring runs through the same allpass.
- The fundamental of STRING and the SYMP main string lands within
  0.05 cents from G1 to C7, and partials 2 to 4 are harmonic to it at G1
  and C3. Whole-sample tuning was up to 54 cents off at G1 and 84 cents at
  C7.
- The line is 981 samples. That supersedes ADR 0040's 984 in part.
  - G1's 979.6-sample period takes a 979-sample line, 0.59 on the
    allpass, plus the low-pass's two taps.
  - F♯1 and lower clamp to the longest line.
  - `Instrument` is 161,208 B, which leaves 125,512 B of D2 (task 1: 161,496 B).

Task 5 makes the four macros modulatable:
- STRUCTURE, BRIGHT, DAMP and POS are read every block from the voice's
  modulated params. The loops read `modal::Macros`, never the params.
  Each block eases them `EASE` = 0.3 of the way to the block's values. A
  note's first block snaps them, since nothing sounds yet.
- This supersedes in part ADR 0010's "Modal settings are read only at
  note-on", for these four. The model page (EXCITE, BODY, ENS, COUPLE,
  HALO, MODES) stays note-on and unmodulatable.
- POS on STRING and SYMP shapes the pluck from the first block's
  modulated value: `KsString::excite` fills the noise at note-on, and
  `shape` combs and smooths it before the first tick. So VEL and NOTE
  routes reach it. With no route, this is bit-identical to shaping at
  note-on. BANK reads POS live.
- Until the chord table (task 10), SYMP's halo retunes to the eased
  STRUCTURE whenever it moves.
- MODES latches at note-on. A sounding bank note keeps its modes, and the
  voice is billed for them (`ModalEngine::playing_cost`) until it ends.

Task 8 adds STRUCTURE's dispersion on STRING and moves BODY out of the
loop (#10):
- `modal::dispersion::Dispersion` is a chain of four first-order allpasses
  in the loop, after the low-pass and before the tuning allpass; the gain
  follows them all. There is no new buffer.
  - Its law follows Rings' `ap_gain` curve, `s/(0.15 + s)` (MIT, ADR 0032),
    scaled by the period, as Rings scales its allpass line: each stage's
    DC delay is `1 + 1.15·s/(0.15 + s)·0.1·period/4` samples. It is 1 at
    STRUCTURE 0, a plain delay, and the chain's DC delay is capped at half
    the period.
  - Rings' fixed coefficient, −0.618·s/(0.15 + s), moved the 8th partial
    of C3 by 0.95 cents. The scaled law moves it 24 cents at STRUCTURE 1,
    and by the model about 21 at G1, 30 at C4 and 75 at C6: sharper up the
    keyboard, as on piano wire.
  - The chain's phase delay at f0 comes off the line (`split`'s `other`),
    so the fundamental holds within 0.05 cents from STRUCTURE 0 to 1. At G1
    and STRUCTURE 1 the line is about 880 samples, within the 981.
  - A moved STRUCTURE re-splits the loop once a block. SYMP's main string
    has none: its STRUCTURE tunes the halo.
  - STIFF's two-sample mix is gone.
- `modal::body::Body` is BODY: three fixed resonances on the output,
  outside the loop, (102 Hz, Q 3, 1), (236 Hz, Q 4, 0.7) and (517 Hz, Q 3,
  0.5). Each is a peak-normalized band-pass, and the output is
  `(x + b·Σ gᵢ·bpᵢ(x)) / (1 + b/2)`, which peaks at about 1.4. It colours
  without transposing: the old half-delay comb in the loop sounded low
  notes an octave up. On SYMP it colours the main-and-halo mix. BODY is
  latched at note-on, as a model-page setting.
- A note-off's `Release` moves from each line to the voice: STRING and
  SYMP's main string (`string::StringVoice`) and BOWED. Halo lines never
  release.
- Sympathetic is String's voice plus a lease, and is sized within one
  align of it. That supersedes in part ADR 0054's const assert that Bowed
  or String sizes the voice. `Instrument` is 162,232 B, which leaves
  124,488 B of D2.

## Alternatives considered
- Keep FDBK and clamp its range below the unity point: its useful range
  would be 0–0.012, and the knob would still be one bad mapping from a
  runaway.
- A DC blocker in every loop (task 1's first placement): its phase delay
  falls with frequency, so compensating it at f0 left the upper partials of
  low strings flat. They were about 50 cents off at G1, 20 at C3 and 10 at C4.
  Keeping G1 in tune also grew the line to 1,016 samples.

## Consequences
- No undriven string sustains forever: DECAY's longest is today's 0.999 per pass.
- DC inside a loop decays with the ring, not at 10 Hz. BOWED's stick-slip
  can hold a small offset in its ring, which the output blocker removes. Its
  output DC over 10 s is about 2e-5.
- Measured over one second, a high-passed output's mean is set by the
  window's edge samples, up to about 1e-2. The stability test measures DC
  over 10 s.
- A macro route costs nothing extra on BANK, which already recomputes its
  filters every block. On SYMP, a moving STRUCTURE retunes the seven halo
  strings each block until task 10.
- Modal's goldens and the INIT Modal fixtures move, and are re-recorded
  once, at the end of step A.

## Sources
- docs/superpowers/specs/2026-09-29-modal-2-resonators-design.md § 2
- docs/superpowers/plans/2026-09-29-modal-2-resonators.md, Tasks 1, 2 and 8
- Mutable Instruments Rings, `dsp/string.cc` (`ap_gain`)
- ADR 0040 (the 984-sample line), ADR 0054 (the dirty extent)
