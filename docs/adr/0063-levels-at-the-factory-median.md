# 0063. INITs play at the factory median; BANK and the voice filter stop buzzing

- **Status:** Proposed
- **Deciders:** project owner (UAT 2026-09-30); firmware (Modal 2, task 19)
- **Supersedes:** [0058](0058-a-loudness-reference-modal-matches-algo-init.md)
  (the reference at ALGO INIT's −15 LUFS)
- **Relates to:** ADR 0049 (Algo INIT), ADR 0050 (the trim and the
  limiter), #231 (the bank's tanh)

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

ADR 0058 put every INIT at ALGO INIT's −15 LUFS, 7 dB over the factory
median, and left STRING's chord at the limiter's 3 dB edge. At the UAT
the owner heard both STRING v127 chords distort, and chose the factory
median (about −22 LUFS) as the reference by A/B. Measured then, the
limiter took 15.3 dB of gain from STRING's eight-note v127 chord, 5.95 dB
of its loudness. ALGO INIT's release (RR 8) was 0.11 s: it stopped short.

## Decision
- **The reference is the factory median, −22 LUFS:** ALGO INIT's C4 at
  velocity 100 on P1, measured as ADR 0058 measures it
  (`REFERENCE_LUFS`). ALGO INIT gets there by its OUT LEVEL, 0.8 to
  45/128 (−7.1 dB, on the knob's grid; `INIT_VOLUME`). The factory Sounds
  keep their levels: SQR BASS, which took INIT's, now sets 0.8.
- **BANK, BOWED and SYMP INIT sit within ±1 dB of it** by their
  `out_gain`, as ADR 0058 applies it at the VCA.
- **STRING is set by its peak:** its eight-note chord at velocity 127
  (`WIDE_8`: C2 G2 C3 E3 G3 C4 E4 C5) takes at most 3 dB of the limiter's
  gain reduction. Its C4 then sits 5.4 dB under the reference: a pluck
  peaks about 11 dB over a sustained tone of its loudness.
- **The limiter stays a safety ceiling:** no INIT's four-note or
  eight-note chord at velocity 127 loses more than 3 dB of its loudness to
  it (`MAX_CHORD_LIMITED_DB`), and the level sweep holds every INIT's
  eight-note chord under the ceiling.
- **ALGO INIT releases at RR 5,** a T60 of about 1.1 s, in
  `AlgoOpParams::default()` and the RR spec's default.
- **Old files decode as saved.** `Sound::neutral`, the frozen base, keeps
  v1's OUT LEVEL 0.8 and RR 8; so do the factory Sounds' silent
  operators.
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
- **Lower the output trim 7 dB:** every factory Sound, already at the
  median, would drop with INIT.
- **STRING at the reference like the rest:** its eight-note chord would
  take 8.1 dB of gain reduction.
- **The example curve, linear to ±0.5 and flat at ±1:** C1 as well, but
  it compresses every state from 0.5, so resonance lost its peak
  (`test_filter_resonance_mid_note`) and ALGO INIT fell 0.6 dB.
- **`tanh` in the filter:** a transcendental twice a sample on every
  voice.
- **The burst by pitch alone:** the tanh·2 still drove the filter past 1
  at C5.

## Consequences
- A new Sound plays 7 dB quieter than before; old files and the factory
  Sounds do not move. Every golden that renders INIT was re-recorded, and
  the golden harness's release grew to 1.6 s for RR 5.
- Every engine's filter changes where a state passed 1: the Algo goldens
  and SQR BASS (0.55 dB at most) were re-recorded, ALGO INIT reads 0.1 dB
  louder.
- BANK plays low notes louder and high ones softer than before.

## Sources
- Owner UAT, 2026-09-30 (PR #252); the probes in the UAT investigation.
- `a_high_bank_note_is_not_harsh`, `bank_loudness_is_even_across_the_keyboard`,
  `filter::tests::saturate_is_smooth_and_bounded`,
  `modal_models_match_the_loudness_reference`, `algo_init_releases_in_about_a_second`.
