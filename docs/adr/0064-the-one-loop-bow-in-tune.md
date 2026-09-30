# 0064. Bowed is the one-loop bow again, half-length and inverting, in tune

- **Status:** Proposed
- **Deciders:** project owner (UAT 2026-09-30); firmware (Modal 2, task 19)
- **Supersedes in part:** [0056](0056-modal-resonators-share-four-macros.md)
  (§ Bowed, the two-delay bow; § Old patches, Bowed's translation)
- **Relates to:** #240 (the bow an octave low), #241, #248 (drift)

## Context
ADR 0056 replaced the one-loop bow (Task 13, 330298c) with a two-delay
waveguide bow: in tune, with POS as the bow and BRIGHT heard. At the UAT
the owner preferred the one-loop bow ("the new one sounds like the MS
Cheetah"). That bow sounded an octave low: its stick-slip ran a period of
two passes of a whole-period loop, odd partials of f0/2 (C3 at 65.4 Hz).

## Decision
- **The one-loop bow, restored** (`render_bowed`, `BowedString`): one
  ring, the friction `FORCE·4·tanh(8·(v − x))` into the loop at 0.4, the
  loop's tap through BRIGHT's three-tap low-pass, FORCE and SPEED eased a
  sample, a lifted bow ramping to DAMP's ring (`Release::lift`), the
  output's DC blockers kept (ADR 0060).
- **In tune: a half-length loop that inverts each pass** (`BOW_LOOPS`).
  Its stick-slip's two-pass period is f0; it keeps the one-loop bow's odd
  partials, an octave up. The allpass's fraction is exact at f0.
- **Smoothing that keeps the period between samples** (`BowHair`): a
  binomial on what the bow pushes, `[1, 2, 1]/4` under C5 and
  `[1, 6, 15, 20, 15, 6, 1]/64` from C5, linear phase, its delay taken off
  the line and its gain at f0 made up. Without it the stick-slip locked
  to whole-sample periods, 26 cents sharp at C7; a one-pole's delay was
  heard differently bowed and lifted, so the lift moved the pitch.
- **Once lifted the loop is linear** (no `tanh`): DAMP's T60 at any
  level, G1 to C7 within 10 % (`damp_sets_the_ring_at_every_pitch`).
- **POS** combs the output at the bow point, a third of the half-loop
  from the tap at POS 1, so the 3rd, 9th and 15th partials null there. It
  fades in over POS 0 to 0.03 (Task 13's hard switch), and both taps pass
  BRIGHT's low-pass.
- **The taps' place is set once a block;** the loop reads the note's
  first pass as the write reaches it (#206), then the filtered taps.
- **A re-strike** sets the bow back on its ringing string (ADR 0062).
- **Old Bowed patches** load the one-loop bow's own values again: DAMP
  for v1's 0.12 s release, BRIGHT 1, POS 0.
- **`out_gain`** 0.93 puts BOWED INIT at the reference (ADR 0063).
- **`COST_BOWED` 980:** 330298c's 860 × 1.13, the restored bow's host
  time over the old one's, each against STRING's on its own build. A host
  estimate until the chip bench.

Measured: G1 to B5 within 3.7 cents at INIT, v1's bow and BRIGHT 0 and 1;
C6 to C7 within 6.3 (the stick-slip still leans toward whole-sample
periods up there). A lifted note keeps its pitch within 0.1 cent.

## Alternatives considered
- **Keep the two-delay bow:** the owner's ear chose the one-loop bow.
- **A one-pole in the loop** tracking f0: its delay is heard at 0.77 of
  its phase delay while bowed and whole once lifted; the lift moved the
  pitch up to 33 cents at C6.
- **Dither in the bow's velocity:** it did not unlock the top octave.
- **Linear interpolation for the fraction:** no better than the allpass.

## Consequences
- BOWED costs 980: five voices on rev V, four on rev Y (the two-delay bow
  fitted eight).
- The top octave's tuning gate is 7 cents, not 5.
- BRIGHT is weak on this bow, as at Task 13: BRIGHT 0 against 1 moves
  harmonics 8–24 by 4.5 dB, the darker loop sharpening the stick-slip's
  corner. FORCE and SPEED move the tone a little (the partials' shares by
  0.020 at least).
- A settled bow's drift is 49.9 dB under its RMS at worst (C6), where the
  two-delay bow's was 69.

## Sources
- Owner UAT, 2026-09-30 (PR #252); 330298c (the one-loop bow), 96169dd,
  24c1882, 2be21bd (the two-delay bow).
- `bowed_is_in_tune`, `bowed_is_stable_and_in_tune_at_every_corner`,
  `force_and_speed_move_the_bows_tone`, `a_v1_bowed_patch_bows_in_tune`,
  `bowed_plays_clean_across_the_instrument`.
- J. O. Smith, *Physical Audio Signal Processing*, bowed strings (the
  inverting reflection).
