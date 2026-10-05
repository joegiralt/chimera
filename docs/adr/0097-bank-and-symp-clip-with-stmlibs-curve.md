# 0097. Stagger BANK's re-tune as Elements does; clip BANK and SYMP with stmlib's rational curve (amends 0063)

- **Status:** Proposed
- **Deciders:** project owner (asked for both); firmware

## Context
BANK re-tuned every mode's SVF each block: a `tan` polynomial and two
`vdiv.f32`s a mode, 41 instructions in all (thumbv7em, opt-level 2). That
is about 2 % of the bank's bill at 32 modes, against 52 instructions a mode
a sample in the render loop. Mutable Instruments Elements
(`elements/dsp/resonator.cc`, `Resonator::ComputeFilters`) re-tunes its
first 25 modes every call and the rest every other call.

BANK's and SYMP's outputs ran through `libm::tanhf`: about 60 instructions
and two `vdiv.f32`s a sample on its shortest path, an `expm1f` call inside.
stmlib's `SoftClip` (`stmlib/dsp/dsp.h`), `x(27 + x²)/(27 + 9x²)` clamped to
±1 outside ±3, has the same slope at 0 and the same ceiling for one divide.
Chimera already had it as `fast_tanh`, used by DRIVE and the bow.

## Decision
- **BANK re-tunes in tiers (`DUE`).** Modes 0–15 every block (750 Hz),
  16–31 every other block, 32–47 every fourth, each tier's modes in turn:
  28 of 48 a block at most. The recurrence (stretch, Q, the Nyquist cut)
  still runs for every mode; only `Svf::set` is skipped. A note's first
  block, and its note-on, re-tune every mode.
- **Skipped modes are not interpolated.** The SVF's states stay
  continuous when its coefficients step, and a skipped mode is at most
  three blocks (4 ms) behind macros that already ease 0.3 a block. The
  tests hold it to that:
  - a note's first block is the unstaggered bank's bit for bit;
  - a still chord is identical;
  - a swept one moves no octave band more than 1 dB, or than the same
    sweep a block late does;
  - no sweep clicks under the sweep test's measure.
- **BANK's and SYMP's out clip is stmlib's `SoftClip`** (`fast_tanh`),
  BANK's still at half scale. This amends 0063's "`tanh·0.5`".
- **The bills drop on the counted saving, at 90 %:**
  - `COST_BANK` goes from 460 to 400: −69.1 for the clip, and +3.0 for the
    stagger's worst case, at MODES 24, where every mode still re-tunes and
    each pays the mask's few instructions.
  - `COST_SYMPATHETIC` goes from 540 to 470: −70.4.
  - BANK at 32 modes gets a third voice on rev V. SYMP gets one more voice
    on most rows.
- **Kept, not replaced:**
  - The voice filter's `saturate`. It is linear to ±1, cheaper (no
    divide), and its ring is load-bearing: the rational curve, scaled to
    the same slope and ceiling, put full resonance's ring 14 to 24 dB
    lower.
  - The delay's feedback `tanhf`. ADR 0038 and 0031 chose it over this
    same Padé curve, and its cost sits in the benched `FxBus::COST`.
  - TAPE's quintic `soft_clip`: divide-free and cheaper.
  - DRIVE and the bow already use it.

## Alternatives considered
- **Elements' exact rule (25 every block, the rest every other).** It
  saves less: 4 of 32 modes a block, against 8. Both pass the tests; the
  three tiers stay within the block-late bound everywhere.
- **Interpolating skipped coefficients across the blocks they skip.** That
  is a per-sample ramp on every mode, which costs more than the stagger
  saves.
- **A `u64` shift by `i` per mode.** On thumbv7em it is eight
  instructions; shifting the mask by one each mode is four. At MODES 32
  the first left the stagger dearer than none.
- **The rational curve in the filter.** See above: it is the resonance.

## Consequences
- The stagger saves little: 2.9 cycles a sample at MODES 32 and 11 at 48.
  It costs 1.9 to 2.7 at 16 and 24, where nothing is skipped. The clip is
  the saving: 69 to 70 cycles a sample a voice.
- `modal_sympathetic`'s golden is re-recorded for SYMP's clip. No golden
  renders BANK.
- BANK and SYMP clip slightly harder in the knee: up to 0.024 above
  `tanh`, at |x| ≈ 1.6.
- The bench's MDL rows should re-read BANK and SYMP. The delay's
  `tanhf` → `fast_tanh` remains a costed option for the FX bus, if a
  re-bench and the owner's ear allow.

## Sources
- Mutable Instruments Elements, `elements/dsp/resonator.cc`
  (`ComputeFilters`, the `update` rotation), Copyright 2014 Emilie Gillet,
  MIT: https://github.com/pichenettes/eurorack
- stmlib, `dsp/dsp.h` (`SoftLimit`, `SoftClip`), Copyright 2012 Emilie
  Gillet, MIT: https://github.com/pichenettes/stmlib
- The notice and the parts it covers: ADR 0032; `THIRD_PARTY.md`.
- Instruction counts: `llvm-objdump --mcpu=cortex-m7` of the release
  firmware, with `compute_filters`, `render_modal` and `render_sympathetic`
  kept out of line for the count; `cost_test.rs`, `modal_stagger_test.rs`,
  `filter_test.rs` (`full_resonance_rings_at_its_level`).
