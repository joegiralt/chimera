# 0064. Bowed is the one-loop bow again, half-length and inverting, in tune

- **Status:** Proposed
- **Deciders:** project owner (UAT 2026-09-30); firmware
- **Supersedes in part:** [0056](0056-modal-resonators-share-four-macros.md)
  (§ Bowed, the two-delay bow; § Old patches, Bowed's translation)
- **Relates to:** #240 (the bow an octave low), #241, #248 (drift)

## Context
ADR 0056 replaced the one-loop bow (330298c) with a two-delay waveguide
bow: in tune, with POS as the bow and BRIGHT heard. At the UAT the owner
preferred the one-loop bow ("the new one sounds like the MS Cheetah"), and
has since heard this one as "lovely". The one-loop bow sounded an octave
low: its stick-slip ran a period of two passes of a whole-period loop, odd
partials of f0/2 (C3 at 65.4 Hz).

## Decision
- **The one-loop bow, restored** (`render_bowed`, `BowedString`): one
  ring, the friction `FORCE·4·tanh(slope·(v − x))` into the loop at 0.4, a
  bounded push, FORCE and SPEED eased a sample, a lifted bow ramping to
  DAMP's ring (`Release::lift`), the output's DC blockers kept (ADR 0060).
  Both `tanh`s are `fast_tanh`.
- **In tune: a half-length loop that inverts each pass** (`BOW_LOOPS`).
  Its stick-slip's two-pass period is f0; it keeps the one-loop bow's odd
  partials, an octave up. The allpass's fraction is exact at f0.
- **Smoothing that keeps the period between samples** (`BowHair`): a
  binomial on what the bow pushes, `[1, 2, 1]/4` under A4 and
  `[1, 6, 15, 20, 15, 6, 1]/64` from D#5, crossfaded between, all centred
  3 samples back: linear phase, its delay taken off the line and its
  gain at f0 made up. Without it the stick-slip locked to whole-sample
  periods, 26 cents sharp at C7. A note's first push primes it, so a low
  note sounds from its first block (#206).
- **A whole period's lock, stepped out of** (`unlocked`, `grip`): from
  C6 up a period within 0.18 samples under or 0.33 over a whole one is
  set at that edge, where the stick-slip plays the asked pitch within a
  cent. The share taken follows the bow as it is, re-taken each block
  from the eased force and bow velocity: all from INIT's effective force
  (0.447) to FORCE 0.5 at v127 (0.55) at SPEED 0.5 and up, none for a
  soft (0.3 or less), very hard (0.8 or more) or slow (SPEED 0.1) bow,
  whose windows differ. A lifted ring takes none: it plays its own
  period.
- **Once lifted the loop is linear** (no `tanh`): DAMP's T60 at any
  level, G1 to C7 within 10 % (`damp_sets_the_ring_at_every_pitch`).
- **BRIGHT is the output's:** the loop's three-tap low-pass stays at
  INIT's BRIGHT (in the loop BRIGHT sharpened the stick-slip's corner, so
  darker read brighter); BRIGHT sets the same low-pass on both output
  taps, and under INIT's 0.3 a one-pole down to 1 kHz at 0.
- **FORCE sets the friction's slope**, 8 at 0.5 (INIT's, as before),
  halved at 0 and doubled at 1.
- **POS** combs the output at the bow point, a third of the half-loop
  from the tap at POS 1, so the 3rd, 9th and 15th partials null there. It
  fades in over POS 0 to 0.03; a moved point glides across the block.
- **The taps' place is set once a block** when still; the loop reads the
  note's first pass as the write reaches it (#206), then the filtered
  taps.
- **A re-strike** sets the bow back on its ringing string (ADR 0062).
- **Old Bowed patches** load the one-loop bow's own values: DAMP for v1's
  0.12 s release, BRIGHT 1, POS 0.
- **`out_gain`** 0.93 puts BOWED INIT at the reference (ADR 0063).
- **`COST_BOWED` 640:** counted in the thumbv7em release build, 256
  instructions a sample on the bowed path and 14 more while POS moves,
  against the benched 620 of the one-loop bow at 143 and its two `tanhf`
  bodies, with `fast_tanh`'s two divides at 14 cycles: 620 + (270 − 143 −
  135) × 1.46 × 1.1 + 2 × 12.54 × 1.1. Eight voices on rev V (seven with
  the master tape), six on rev Y.

**Scope of the tuning:** G1 to C7 within 5 cents at velocity 20 to 127
for FORCE 0.1 at any SPEED and FORCE 0.5 at SPEED 0.5 and 1
(`the_bow_is_in_tune_within_its_scope`). Outside it the stick-slip itself
runs off: FORCE 1 up to 35 cents from F3 up (A6 at SPEED 0.5 +17.4), and
SPEED 0.1 at FORCE 0.5 up to 21 cents from G4 up.

Measured, INIT and v1's bow at velocity 100: G1 to C7 within 3.7 cents; a
lifted note keeps its pitch within 0.1 cent. BRIGHT 0 against 1 takes
4.3 dB off harmonics 8–24. FORCE 0.25 against 1 moves harmonics 8–24 by
0.8 dB at G1, 2.3 at C3, 4.4 at C6.

## Alternatives considered
- **Keep the two-delay bow:** the owner's ear chose the one-loop bow.
- **A one-pole in the loop** tracking f0: its delay is heard at 0.77 of
  its phase delay while bowed and whole once lifted; the lift moved the
  pitch up to 33 cents at C6.
- **Dither in the bow's velocity, an eleven-tap stage, linear
  interpolation for the fraction:** none unlocked the top octave.
- **`libm::tanhf`:** two bodies a sample cost two more voices.

## Consequences
- A heavy or slow bow is out of tune (see the scope above): the grid of
  `bowed_plays_clean_across_the_instrument` (its C notes) is clean in
  every case but C7's SPEED 0.1 at FORCE 1 or v127, 6 to 37 cents sharp.
- A settled bow's drift is 49.2 dB under its RMS at worst (C6, SPEED
  0.1), where the two-delay bow's was 69.
- SPEED moves the tone little at G1 (the partials' shares by 0.014).

## Sources
- Owner UAT, 2026-09-30 (PR #252); 330298c (the one-loop bow), 96169dd,
  24c1882, 2be21bd (the two-delay bow).
- `bowed_is_in_tune`, `bowed_is_stable_and_in_tune_at_every_corner`,
  `bowed_bright_is_heard`, `force_and_speed_move_the_bows_tone`,
  `a_v1_bowed_patch_bows_in_tune`, `bowed_plays_clean_across_the_instrument`,
  `the_bows_smoothing_moves_smoothly_with_pitch`.
- J. O. Smith, *Physical Audio Signal Processing*, bowed strings (the
  inverting reflection).
