# 0090. Store the master delay line as i16, full scale ±2.0 (supersedes in part 0014)

- **Status:** Proposed
- **Deciders:** owner (ruling 2026-10-04, on #337's budget), firmware

## Context
ADR 0014 sized the tape delay's line to 24 064 samples for its 10–500 ms
range, and kept it `f32`: 96 256 B of AXI, inside `FxBus`. That ADR
rejected 16-bit lines because they changed every locked delay and reverb
output. ADR 0028 has since stored the reverb as `i16`.

The sound pool budget (ADR 0087) sizes its arena from the AXI that is
really free. With the delay at `f32`, the arena stops at 112 KiB: 2.9 s of
24 kHz 12-bit audio beside NEW's 32 Sounds. The owner ruled on 2026-10-04
that the delay line goes to `i16`, so the arena can grow.

## Decision
- **`TapeDelay::buffer` becomes `[i16; MAX_DELAY_SAMPLES]`**, at full scale
  ±2.0 (1 LSB = 2⁻¹⁴), the same scale as the reverb (ADR 0028).
  - It is 48 128 B in place of 96 256 B, so 48 128 B of AXI is freed.
  - `MAX_DELAY_SAMPLES`, the 500 ms range, MECHANICS (ADR 0053) and the
    loop's order are unchanged.
- **Write:** `(x · 16384.0) as i16`. Rust's float-to-int cast truncates
  toward zero and saturates at the `i16` limits, so a write never wraps.
  The cast is one `vcvt` and an `ssat` on the M7.
  - **Truncation, not rounding,** as for the reverb's allpass state (ADR
    0028, Q3): a rounded loop can hold a limit cycle of a few LSB forever,
    and a truncated one decays to exact zero.
- **Read:** `read` loads two `i16` taps (four during a TIME crossfade) and
  converts each to `f32`. It interpolates as `(a + (b − a)·frac)·2⁻¹⁴`:
  two multiplies, as many as the `f32` read.
- **SAT is unchanged.** `tanhf(g·x)/g` stays in the loop at every SAT (ADR
  0038). It bounds a write at |dry| + FDBK, which is ≤ 2.0 for any send at
  or below unity, so in normal use nothing reaches the cast's clamp. A
  hotter send hard-clips at ±2.0 on the write, where it used to run to
  ±(1 + |dry|) unclipped.
- **The goldens are re-blessed** in the same commit (ADR 0011: they lock a
  refactor, not a sound), with the measured noise floor in the message.

## Sound cost
- **Grain on the repeats.** Each pass through the loop adds truncation
  noise of about 1 LSB, −84 dBFS against 1.0. That is about −89 dBFS RMS
  with a sign-correlated bias of ½ LSB.
- **Feedback sums it, and TONE decides how much.** Measured on the host
  as the deviation from the `f32` loop, with the repeat fitted out, under
  a 1 kHz tail:
  - at FDBK 0.9, about −83 dBFS;
  - at FDBK 0.99, about −77 dBFS at the default TONE (0.6). TONE's
    low-pass keeps the grain's highs from summing;
  - at FDBK 0.99 and TONE 1, −65 to −69 dBFS. That is the worst case, a
    flat loop, louder than the white estimate of power over 1 − FDBK²
    (−72), because truncation's error follows the signal's sign and sums
    partly in step from pass to pass.

  It is inaudible under a playing repeat. It shows as hiss only on a
  long, high-feedback tail, as on the tape delays the stage imitates.
- **Truncation is a dead zone.** Each pass shrinks a quiet repeat by
  about ½–1 LSB as well as by FDBK. Above about 60 LSB (−48 dBFS) a tail
  decays as on the `f32` line. Below it, a tail shrinks linearly to exact
  zero, and its last step is 1 LSB, −84 dBFS.
  - At FDBK 0.9, a 0.5 impulse at TIME 375 ms leaves the line all zeros
    after 16.25 s.
  - At FDBK 0.99, a tail ends early: a −12 dBFS burst stops where the
    `f32` tail would still be at −55 to −60 dBFS, about 100 repeats in at
    375 ms.
  - The `f32` line decayed toward denormals instead.
- **SAT doesn't hide the grain.** `tanh` has slope 1 at small signals, so
  the grain's level doesn't depend on SAT. A high SAT lowers the repeats'
  level, so the grain sits relatively closer under a decaying tail.
- **GRIT** is the reverb's control, not the delay's. REV SEND feeds the
  delay into the reverb (ADR 0030), whose writes land on GRIT's grid of
  2^(6·GRIT) LSBs at the same ±2.0 scale.
  - At GRIT 0 the two grids are the same, and the reverb adds nothing
    coarser.
  - At GRIT > 0 the reverb's grid is coarser, so it masks the delay's
    grain in the wet return.

  The delay's dry repeats keep its own 16-bit grain whatever GRIT is.

## CPU cost
- **Per sample:** one convert and saturate on the write, and two converts
  on the read (four during a TIME crossfade). That is about 3–7 cycles at
  48 kHz, 0.15–0.35 M cycles a second, about 0.03–0.07 % of the 480 MHz
  core.
- **Against that,** the line's D-cache footprint halves, and MECHANICS'
  moving reads touch half the lines.
- The bench (ADR 0013) records the delay's cycles before and after. The
  change is accepted if the delay's cost doesn't rise by more than the
  convert estimate.

## Alternatives considered
- **Keep `f32`.** The arena stays at 112 KiB: 2.9 s at 24 kHz in place of
  4.3 s.
- **Rounding on the write.** Its noise is unbiased, but a quiet tail
  stops shrinking at around a few LSB. That is the limit cycle ADR 0028
  measured on the reverb.
  - Measured on this loop at FDBK 0.99, a −12 dBFS burst still plays at
    −70 to −83 dBFS two minutes later, where truncation has reached
    exact zero.
  - That level is a residual tone, not a fade.
- **Full scale ±1.0.** Twice the resolution, but a loop bounded at |dry| +
  FDBK would clip at FDBK near 1 with a unity send.
- **A shorter line,** for example 250 ms as `f32`. Same bytes, but half the
  delay's range, and the owner kept 500 ms.
- **TPDF dither on the write.** It whitens the bias, but costs a random
  number per sample. The tail would never reach zero: a permanent hiss
  at the grain's level, under every pause, at any FDBK.

## Consequences
- AXI frees 48 128 B. ADR 0087's arena becomes 160 KiB, and this change
  is step 1 of its build order.
- Every delay golden changes once.
- A long FDBK ≈ 1 tail carries grain of about −77 dBFS at the default
  TONE, and −65 to −69 dBFS at TONE 1. A quiet tail ends at exact zero,
  sooner than on the `f32` line.
- Tests:
  - `full_feedback_with_no_saturation_stays_bounded` (ADR 0038) still
    passes;
  - a FDBK 0.9 tail reaches exact zero within a measured time;
  - a +6 dB send clips at ±2.0 and never wraps;
  - the grain at FDBK 0.9, at 0.99 and at 0.99 with TONE 1 is within
    3 dB of the measured figures above.

## Sources
- Owner ruling, 2026-10-04 (via the #337 review).
- `chimera-core/src/dsp/delay.rs` (`TapeDelay`, `read`,
  `MAX_DELAY_SAMPLES`).
- ADR 0011 (goldens), 0013 (bench), 0014 (memory map, delay line), 0028
  (`i16` reverb, truncation evidence), 0030 (REV SEND), 0038 (SAT at 0),
  0053 (MECHANICS), 0087 (sound pool budget).
- Spec: `docs/superpowers/specs/2026-10-04-sound-pool-budget-design.md` §
  The budget.
