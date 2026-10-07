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
output on a −0.5 dBFS sine (`stretch-sine-c4-poly`), with METAL at +2.95 and
HARP at +2.37. That is at the +6 dBFS bound before the chain in spec § 9
(private issue #384).

## Decision
- A POLY wrap holds the output it left, `held`, and crossfades from it to the
  new grain under the same one-pole: `y = x + w · (held − x)`, with `w` going
  from 1 at the wrap and falling by `lift_k` a sample.
- The result is a convex mix of the grains' mix and a past output, so it never
  passes the source's peak (`poly_wraps_stay_under_the_source_peak`: 1.000×).
- The POLY wrap reads nothing.
- MONO keeps the lift, `y = x + w · carry`. A held value would leave a corner
  on a held tone, which MONO's tests forbid.
- The kind follows the block's lock (`hold` 1 unlocked, 0 locked). A block
  whose lock came or went carries the live carry over as the new kind
  (`recarry`).

## Alternatives considered
- **A third head reading past the window's end.** This is a true crossfade
  with no corner, but spec § 2 keeps every grain inside its window.
- **A shorter lift.** The overshoot shrinks only as the lift's fall becomes a
  step, which the click rule forbids.
- **A gain normalised by the lift (`(x + l)/(1 + |l|)`).** This needs a
  division a sample, and the gain jumps at each wrap unless it is compensated.
- **Hold for MONO too.** It is one fewer multiply, but it bends a held tone at
  every MONO wrap.

## Consequences
- STRETCH POLY's peak before the chain is the source's.
- POLY's corner at a wrap is a short hold rather than the lift's DC.
- The common path costs 2 more static instructions a sample on the M7. A MONO
  lifted wrap costs about 6 more. A POLY wrap costs about 14 in place of 108.
- The bill's worst case (MONO lifted under the floor) is about 447 against
  `COST_STRETCH` 450, to be confirmed by the bench rows at ship.

## Sources
- ADR 0093.
- Spec `docs/superpowers/specs/2026-10-04-tape-core-design.md` § 2 STRETCH and
  § 5.
- private issue #384.
- `chimera-core/src/dsp/tape_engine/stretch.rs`.
