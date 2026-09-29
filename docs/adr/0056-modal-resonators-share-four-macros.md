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
- Modal's goldens and the INIT Modal fixtures move, and are re-recorded
  once, at the end of step A.

## Sources
- docs/superpowers/specs/2026-09-29-modal-2-resonators-design.md § 2
- docs/superpowers/plans/2026-09-29-modal-2-resonators.md, Task 1
