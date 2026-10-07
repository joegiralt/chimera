# 0103. STRETCH POLY's wraps hold the output and crossfade from it (amends 0093)

- **Status:** Proposed
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
  moves, and at most an eighth, past the larger of the output's level and the
  grains'. A held value that stood still left a corner: the QA flagged
  6.6–7.2× on METAL and EPIANO STAB.
- **The bound.** POLY passes the source's peak by an eighth at most
  (`poly_wraps_stay_under_the_source_peak`: 1.04× worst).
- **No read.** The POLY wrap reads nothing: the slope comes from the output
  buffer.
- **MONO keeps the lift,** `y = x + w · carry`. A held value fading over 5 ms
  would duck a held tone at every MONO wrap. A short one bends it: MONO's tests
  measured 3.2× the sine's own bend.
- **Which kind.** The kind follows the block's lock (`hold` 1 unlocked, 0
  locked). A block whose lock came or went carries the live carry over as the
  new kind (`recarry`).
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
- **On the M7 (static count):**
  - the common path is 231 instructions a sample, against 235;
  - a MONO lift costs about 7–12 more a wrap;
  - a POLY hold costs about 80 in place of the lift's 108.
- **Host timing:** 1.08× LOOP's fade path, against 1.10×. The bill's worst
  case (MONO lifted under the floor) falls by about 3 static, under
  `COST_STRETCH` 450. The bench rows confirm it at ship.

## Sources
- ADR 0093.
- Spec `docs/superpowers/specs/2026-10-04-tape-core-design.md` § 2 STRETCH and
  § 5.
- private issue #384.
- `chimera-core/src/dsp/tape_engine/stretch.rs` (`hold_from`, `play`).
