# 0096. Three filter TYPEs: the SVF as a Cascadia-style home filter, PROPHET (SSM2040) and DIRTY (Chamberlin) (amends 0037)

- **Status:** Proposed
- **Deciders:** owner (direction, 2026-10-05), firmware

## Context
The owner wants the SVF to be the home filter, with the Intellijel Cascadia VCF's UX and today's ZDF/TPT core ("reminds me of a 101"). Two more filters are wanted, as machines that nudge: the Prophet-5 Rev 1/2's SSM2040, and a cheap, edgy Chamberlin SVF.

Three defects block the Cascadia match today:
- PHASER has no dry path, so it has no notches.
- NOTCH sits at FREQ, not an octave above.
- LP6 ignores resonance.

The SVF also divides twice a sample, does not ramp `k`, and reads its top octave low (#57). Cost and memory are tight.

## Decision
- **Types, as machines.** KIND shows on screen as TYPE. The built types are SVF, PROPHET (code 4) and DIRTY (code 6). Each has its own modes, its own mode labels (`mode_label`) and its own `default_mode()`. That amends 0037's "`modes()[0]` is the default".
  - SVF: LP1 LP2 **LP4** BP2 BP4 HP4 NT2 PHZR, in the Cascadia's order and words.
  - PROPHET: LP4 only.
  - DIRTY: **LP2** BP2 HP2 NOTCH (HP2 is `FilterMode::Hp12`, code 9).
  - A TYPE or MODE change crossfades over one block, from separate state, and that block is not billed.
- **The SVF's panel is the Cascadia's,** amending 0037's SVF row:
  - FLT: TYPE · FREQ · Q · MODE · FM1 · FM2. FM1 is ENV 1 → CUTOFF and FM2 is NOTE → CUTOFF.
  - FLT › MOD: LEVEL · FM3 · FM3 IN · QM · QM IN. FM3 is *FM3 IN* → CUTOFF and QM is *QM IN* → RES. LEVEL is DRIVE.
  - FM3 IN and QM IN are new Filter params 8 and 11 (`ModSource`). Changing one moves its route's amount and presence to the new source's cell, never into an occupied cell.
  - MIX + MINUS on a route knob deletes the route, with ADR 0086's hint.
- **LEVEL** soft-clips the filter input while it runs (the DRIVE stage's gate). A per-Part `meter::FILTER_CLIP` drives a red ring LED, whose fill is a radius, beside LEVEL and in the FLT viz.
- **The SVF's DSP:**
  - `tan` is stmlib's ACCURATE polynomial, once a block.
  - `g`, `k`, `d = 1/(1 + kg + g²)`, `G₁` and the gain are computed per block and ramped per sample, with no per-sample divide.
  - NT2 is `lp + ¼·hp`, a notch at 2 × FREQ with Q's peak at FREQ.
  - LP1 is `lp + 2·bp`, which resonates.
  - PHZR is the SSM2040 data sheet's phase shifter: dry plus four TPT first-order all-passes, notches at 0.414 and 2.414 × FREQ, Q as one-sample-delayed feedback ≤ 0.7, and the dry signal taken after the feedback node so the notches stay exact.
- **PROPHET:**
  - four TPT one-poles, each `y = s + G·sat(x − s)` (the OTA nonlinearity on the stage's error);
  - `sat` is a divide-free cubic, flat at ±1 from ±1.5;
  - global feedback `k = 4.5·RES`, solved linearly in one step through the cascade's `G⁴` and state sum;
  - input compensation `(1 + ½k)`, so the bass loss at full Q is −4.4 dB, where the uncompensated circuit loses −14 dB.
- **DIRTY:** the Chamberlin SVF at ×2, with `f` clamped per block to `0.95(√(q₊² + 4) − q₊)` and hard clamps of ±2 on `lp`, `bp` and the output. RES 1 means `q < 0`, which self-oscillates into the clamp.
- **Billing,** in the house manner:
  - a static count of each (type, mode) probe loop on thumbv7em, cortex-m7, opt-level 2, × 1.46 × 1.1, plus 12 per `vdiv`;
  - each is billed `max(0, that − old LP24)`, which the engine terms carry; LEVEL is its own term;
  - the budgets, in instructions a sample: SVF LP4 ≤ 46, PHZR ≤ 32, PROPHET ≤ 60, DIRTY ≤ 26, LEVEL ≤ +10.
- **Order:** implementation starts after `modal-stagger` merges and inherits its `saturate`.

## Alternatives considered
- **Newton (two fixed iterations) for PROPHET's loop:** about +100 instructions and a `vdiv` per iteration. That more than doubles the type's cost to fix a detuning at hard drive that the hardware also has.
- **A delay-free PHZR feedback:** it needs a per-sample divide or interpolated reciprocal. The one-sample delay costs nothing and leaves the notches exact.
- **PHZR from two SVF all-passes plus dry:** the notch spacing would follow Q, so Q could not also be feedback.
- **DIRTY with a clamped `f` alone, at ×1:** the cutoff tops out about 6 kHz at Q 0. ×2 costs about 12 instructions and doubles that.
- **A KNOB for PROPHET's bass compensation:** a machine shouldn't need one. The constant is an open question for the owner.
- **The SSM2040 as a Rev 3 CEM3320 (#126 as written):** the owner asked for Rev 1/2.

## Consequences
- The SVF's LP1, NT2 and PHZR sound different, and so does the top octave. Goldens are re-recorded, and old patches load safely, as the no-legacy-compat rule allows.
- PHZR's bill falls from 17 to an estimated 0. Removing the SVF's divides is headroom until the engine terms are re-benched.
- Voice state grows about 76 B (0.6 KB for eight voices), under ADR 0051's assert. `FilterParams` grows 2 B.
- New codes are append-only: KIND 4 and 6, MODE 9, Filter params 8 and 11.
- The MOOG, MS-20, SH-101 and TB-303 kinds remain their own issues.

## Sources
- `docs/superpowers/specs/2026-10-05-filter-types-design.md`.
- Intellijel *Cascadia Manual* v1.2 § 8.
- *SSM 2040* data sheet (1977), Figs. 1, 9 and 10.
- Zavalishin, *The Art of VA Filter Design* 2.1.2.
- Simper, "Linear Trapezoidal Integrated SVF".
- Huovilainen, DAFx-04.
- stmlib `dsp/filter.h` (MIT).
- Chamberlin, *Musical Applications of Microprocessors* (1985).
- Dattorro, JAES 1997.
- Lazzarini and Timoney (2021).
- Issues #57, #126, #128, #165, #177.
