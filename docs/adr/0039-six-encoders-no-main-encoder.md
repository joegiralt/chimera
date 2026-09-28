# 0039. Six encoders; no main encoder

- **Status:** Proposed
- **Deciders:** firmware (audit #92)

## Context
The HAL declared seven encoders, A–F plus a `Main` that only the
simulator's U/J keys could turn, so the sound browser scrolled on the
desktop by a control the chip does not have (ADR 0013 parity). ADR 0005
left open whether a main encoder exists on the hardware.

The PreenFM3's front panel is read through a chain of HC165 shift
registers, 24 bits per scan (`controls.rs`): the six encoders' quadrature
pairs on bits 8–19, and the twelve buttons on bits 0–7 and 20–23. Every
bit is taken; there is no seventh encoder to read.

## Decision
`NUM_ENCODERS` is 6 and `EncoderId` is A–F (`ALL_ENCODERS`). The firmware's
encoder arrays are sized from it, so the bit map must match at compile
time. Anything a main encoder would have done goes to A–F or the buttons:
the sound browser scrolls with A, and B1–B6 select the Part. This answers
ADR 0005's open question.

## Alternatives considered
- Keep `Main` as a desktop-only convenience: UI built on it would work in
  the simulator and be dead on the chip.

## Consequences
The simulator loses U/J. Design docs that assumed a main encoder (Part
select, fine-tune) are corrected to point here.

## Sources
`chimera-stm32/src/controls.rs` (ENC_BITS, BTN_BITS, the 24-bit scan);
ADR 0005; ADR 0013; audit issue #92.
