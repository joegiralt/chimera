# 0103. STRETCH POLY's wraps hold the output and crossfade from it (amends 0093)

- **Status:** Accepted (owner, 2026-10-07)
- **Deciders:** firmware (from the TAPE Task 19 desktop QA, #384)

## Context
ADR 0093 has a grain that reaches its window's end wrap in the render loop
under a one-pole lift: the jump rides on the output and falls over 5 ms (or
an eighth of the wrap interval). MONO wraps by whole periods, so its jump is
near 0. POLY wraps by the window, which isn't whole periods, so its jump can
be up to twice the peak. While the lift falls the new grain swings to its own
peak, and the two add. The factory QA measured +6.06 dBFS at the engine's
output on a −0.5 dBFS sine (`stretch-sine-c4-poly`), against the +6 dBFS
bound before the chain in spec § 9. A C4 sine test measured 2.4×
(private issue #384).

## Decision
- **The hold.** A POLY wrap holds the output it left and crossfades from it to
  the new grain under the same one-pole: `y = x + w · (held − x)`, with `w`
  going from 1 at the wrap and falling by `lift_k` a sample. The result is a
  convex mix of the grains' mix and the held value.
- **The held value moves.** It leaves at the output's slope (`o − prev`) and
  slows to rest at most 12 samples' move on. Its rest point is at most 4
  moves, and at most an eighth, past the grains' level, or `|o|` if that is
  further out. The level is the grains' mix, or their last block's RMS if
  that is higher (a multiply-add a sample): a loud wave crossing 0 at a wrap
  then moves on rather than resting at once. A held value that stood still left a corner: the QA flagged
  6.6–7.2× on METAL and EPIANO STAB.
- **The bound.** POLY passes the source's peak by an eighth at most
  (`poly_wraps_stay_under_the_source_peak`: 1.04× worst).
- **No read.** The POLY wrap reads nothing: the slope comes from the output
  buffer.
- **MONO keeps the lift where whole periods hold,** `y = x + w · carry`. A held
  value fading over 5 ms would duck a held tone at every MONO wrap. A short one
  bends it: MONO's tests measured 3.2× the sine's own bend. Whole periods hold
  when all three of these are true:
  - the grain was latched locked, so its span is whole periods;
  - no hold is still in play (`m · w` under 0.01);
  - the jump it reads is within an eighth of the level.
- **Every other MONO wrap holds, as POLY's does.** That covers:
  - a grain started unlocked that wraps in a block that locks (SIZE or END
    moved mid-note);
  - a run-up grain moved to its span's start as the window moves;
  - a wrap with a hold still in play;
  - a jump read past the source's edge.

  Lifted, those jumps rode on the new grain's swing: the tape-spec final review's
  mid-note fuzz measured up to 2.15× a 0.95 sine.
- **Which kind.** A carry's kind is its grain's. It is latched with the grain's
  span at the splice (`hold` 1 unlocked, 0 locked) and checked at each wrap as
  above. A lift's wrap takes on what the carry in play adds; a hold's starts
  from the output.
- **The loop.** The carry's terms (`1 − hold · w`, `w · carry`) are formed
  before the reads, so the output waits on one multiply-add after them.

## Alternatives considered
- **A held value that stands still.** It stays exactly under the peak, but the
  wave stalls for a sample, and the QA flags that as a click on slow
  material.
- **A third head reading past the window's end.** This is a true crossfade,
  but spec § 2 keeps every grain inside its window.
- **A shorter lift.** It gave 1.5–1.9×, and steps past the click rule.
- **A clip at full scale.** It flattens the overshoot instead of avoiding it.
- **One hold for both kinds.** It ducks or bends MONO, as above.

## Consequences
- STRETCH POLY's peak before the chain is within an eighth of the source's.
- The desktop QA passes 241 of 268 renders, against 238.
  `stretch-sine-c4-poly`, `stretch-brass-c4-poly` and `stretch-kit-16-poly` now
  pass, and no render newly fails.
- No STRETCH wrap, MONO or POLY, passes the peak by more than a hold's eighth
  plus the Hermite's own overshoot. The TAPE mid-note fuzz holds every machine,
  STRETCH included, to 1.1 on a 0.9 sine. MONO's held-tone tests (no duck, no
  bend) are unchanged.
- **On the M7 (static count):**
  - the common path is 226 instructions a sample, against 235 (the loop
    indexes its output);
  - a wrap's carry is out of line, one copy for both grains (`carry_at_wrap`),
    so a MONO lift is about 190 a wrap, against 108;
  - a POLY hold is about 125;
  - a MONO wrap's checks (the hold in play, the jump against the level) add a
    compare or two on that cold path. The common path is unchanged.
- **The bill's worst case** (MONO lifted under the floor) is −9 on the common
  path and about +10.6 at its wraps (two in 16 samples): +1.6 static, about
  445 against `COST_STRETCH` 450. The bench rows confirm it at ship.
- **Flash:** about +1 KB, with bench,orbit at 2,108 B free.

## Sources
- ADR 0093.
- Spec `docs/superpowers/specs/2026-10-04-tape-core-design.md` § 2 STRETCH and
  § 5.
- private issue #384.
- `chimera-core/src/dsp/tape_engine/stretch.rs` (`hold_from`, `play`).
