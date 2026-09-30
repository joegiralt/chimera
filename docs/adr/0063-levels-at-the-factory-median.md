# 0063. BANK and the voice filter stop buzzing

- **Status:** Proposed
- **Deciders:** project owner (UAT 2026-09-30); firmware (Modal 2, task 19)
- **Relates to:** [0058](0058-a-loudness-reference-modal-matches-algo-init.md)
  (`out_gain`), #231 (the bank's tanh)

## Context
At the owner's UAT BANK was harsh high up. Measured on P1, BANK INIT at
v100:
- Its loudness rose 18.6 dB from C2 to C6: the burst was the same at
  every pitch, and a mode's gain rises with its frequency.
- Its C5 carried energy over 8 kHz only 13.8 dB under its fundamental's
  band.
- Bypassing the bank's `tanh` left that energy: it came from the voice
  filter's `saturate`, which the bank's `tanh·2` drove past 1. That curve
  jumped from 1 to 0.83 at ±1 and from 0.96 to 1 at ±1.5: a step, heard
  as buzz, on anything that reached it, on every engine.

## Decision
- **BANK's output is `tanh·0.5`,** a quarter of Rings' `·2`: the filter
  after it stays under its knee. Its `out_gain` makes the level up at the
  VCA, as ADR 0058 applies it.
- **BANK's burst falls as 1/f0,** EXCITE's level at C3 (`BURST_AT_C3`):
  as loud a strike at every pitch.
- **The filter's `saturate` is C1:** linear to ±1, as it was, then
  `1 + u − u²/2` for `u = |x| − 1`, flat at ±1.5 from ±2. Value and slope
  are continuous. Below 1 nothing moved; a state past 1 now bends to 1.5
  where it was clipped to 1 by a step.

Measured after: BANK C5's energy over 8 kHz is 67.0 dB under its
fundamental's band, and C2 to C6 span 4.0 dB.

## Alternatives considered
- **The example curve, linear to ±0.5 and flat at ±1:** C1 as well, but
  it compresses every state from 0.5, so resonance lost its peak
  (`test_filter_resonance_mid_note`) and ALGO INIT fell 0.6 dB.
- **`tanh` in the filter:** a transcendental twice a sample on every
  voice.
- **The burst by pitch alone:** the tanh·2 still drove the filter past 1
  at C5.

## Consequences
- Every engine's filter changes where a state passed 1: the Algo goldens
  and SQR BASS (0.55 dB at most) were re-recorded, ALGO INIT reads 0.1 dB
  louder.
- BANK plays low notes louder and high ones softer than before.

## Sources
- Owner UAT, 2026-09-30 (PR #252); the probes in the UAT investigation.
- `a_high_bank_note_is_not_harsh`, `bank_loudness_is_even_across_the_keyboard`,
  `filter::tests::saturate_is_smooth_and_bounded`.
