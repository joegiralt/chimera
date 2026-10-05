# 0097. BANK and SYMP clip with stmlib's curve (amends 0063)

- **Status:** Proposed
- **Deciders:** project owner (asked for it; cut the stagger after review); firmware

## Context
BANK's and SYMP's outputs ran through `libm::tanhf`. On its shortest path
that is about 60 instructions and two `vdiv.f32`s a sample, with an
`expm1f` call inside (thumbv7em, opt-level 2).

stmlib's `SoftClip` (`stmlib/dsp/dsp.h`) is `x(27 + x²)/(27 + 9x²)`,
clamped to ±1 outside ±3. It has the same slope at 0 and the same ceiling
for one divide. Chimera already had it as `fast_tanh`, used by DRIVE and
the bow.

## Decision
- **BANK's and SYMP's out clip is stmlib's `SoftClip`** (`fast_tanh`),
  BANK's still at half scale. This amends 0063's "`tanh·0.5`".
- **The bills drop on the counted saving, taken at 90 %:**
  - `COST_BANK` goes from 460 to 400: (44 × 1.46 + 12.54) × 0.9 = 69.1.
  - `COST_SYMPATHETIC` goes from 540 to 470: (45.5 × 1.46 + 12.54) × 0.9
    = 71.07.
  - BANK at 32 modes gets a third voice on rev V.
  - SYMP gets one more voice on most rows.
- **Kept, not replaced:**
  - The voice filter's `saturate`. It is linear to ±1, cheaper (no
    divide), and its ring is load-bearing: the rational curve, scaled to
    the same slope and ceiling, put full resonance's ring 14 to 24 dB
    lower. `full_resonance_rings_at_its_level` pins it.
  - The delay's feedback `tanhf`. ADR 0038 and 0031 chose it over this
    same curve, and its cost sits in the benched `FxBus::COST`.
  - TAPE's quintic `soft_clip`: divide-free, so cheaper.
  - DRIVE and the bow already use it.

## Alternatives considered
- **Elements' staggered re-tune (`elements/dsp/resonator.cc`,
  `ComputeFilters`): tried, measured, dropped.** Modes 0–15 re-tuned every
  block, 16–31 every other block and 32–47 every fourth, as a rotating mask.
  - It saved about 3 cycles a sample at MODES 32 (11 at 48) and cost
    about 2 at 16 and 24. No bill moved.
  - A high mode three blocks behind mistunes on fast pitch moves. The
    review measured a 13–20 dB octave-band error against the
    every-block bank, with STEAL GLIDE at short glide times or a PITCH
    mod route. Macro sweeps stayed within a block-late sweep's own error,
    but pitch moves did not.
  - BANK re-tunes every mode every block, as before.
- **The rational curve in the filter.** See above: it is the resonance.

## Consequences
- The clip saves 69 to 71 cycles a sample a voice.
- `modal_sympathetic`'s golden is re-recorded. No golden renders BANK.
- BANK and SYMP clip slightly harder in the knee: up to 0.024 above
  `tanh`, at |x| ≈ 1.6. A hard-driven BANK sweep plays about 0.85 dB
  louder.
- The bench's MDL rows should re-read BANK and SYMP. The delay's `tanhf`
  remains a costed option for the FX bus, if a re-bench and the owner's
  ear allow.

## Sources
- stmlib, `dsp/dsp.h` (`SoftLimit`, `SoftClip`), Copyright 2012 Emilie
  Gillet, MIT: https://github.com/pichenettes/stmlib
- Mutable Instruments Elements, `elements/dsp/resonator.cc`, Copyright
  2014 Emilie Gillet, MIT: https://github.com/pichenettes/eurorack (the
  stagger tried; nothing of it ships).
- The notice: ADR 0032; `THIRD_PARTY.md`.
- Instruction counts: `llvm-objdump --mcpu=cortex-m7` of the release
  firmware, with `render_modal` and `render_sympathetic` kept out of line
  for the count; `cost_test.rs`, `filter_test.rs`.
