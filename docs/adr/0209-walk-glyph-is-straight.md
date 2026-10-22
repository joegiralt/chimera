# 0209. Draw WALK as STRAIGHT, simplified for its box

- **Status:** Proposed
- **Deciders:** owner (2026-10-22: picked style C, STRAIGHT, from the mockups; "the glyph is very small, so if you have too much going on, it's not going to translate very well… I trust you for the first round"; wants it in 0.2), firmware
- **Amends:** [0203](0203-tape-walk-replaces-oneshot-loop-ping-pong-and-tron.md) (WALK's glyph: footsteps until the owner picked)

## Context
ADR 0203 gave WALK a footsteps glyph and kept the other styles drawn for the owner to choose from. The owner wasn't happy with footprints, treads or a dotted trail. Three runner mockups followed: an oval, a stadium and STRAIGHT. The owner picked STRAIGHT. The mockup was drawn at 4×, and its detail doesn't survive the 64 × 48 box at 1×. It had a grandstand crowd, a dotted back lane, runners 12 px wide with faces, ponytails and caps, a stopwatch, cones and an oval with small runners on its far side.

## Decision
- **STRAIGHT is the default and the only style the firmware draws** (`StepStyle::DEFAULT`, `draw::straight`). It is drawn for its real size. It never shows more than two runners. Each runner is a 5 px head and seven limbs 2 px wide, from a joint table (`POSES`: stride, flight, cheer). The near limbs are in one ink and the far limbs in the next ink down, and no detail is under 2 px. The crowd, back lane, faces, hair, stopwatch and cones are gone.
- **What it shows:**
  - **DIRECTION:** the runners face the way the run plays. The strip's arrow points that way, with arrows at both ends on PING-PONG. LOOP adds the track's oval behind them, with an arrow on its far side heading back. PING-PONG has one runner heading out and one heading back, back to back.
  - **The window:** POSITION and LENGTH light the strip under the lane, and the runners stay on it. The leader is at the far end of the run.
  - **FADE:** dashes the end of the strip where the run fades out (the low end on REVERSE) and dims the second runner, never the leader.
  - **DRIFT:** nudges the second runner and changes both runners' strides.
  - **TRON:** puts a finish post with a 2 px chequered flag where the tape runs out. The leader stands there with arms up, and the strip past the post is grey.
  - **Speed:** WALK has no SPEED. It plays at the key's pitch, so the glyph draws none.
- **The other styles stay in the code but never link into firmware.** The footsteps (Prints, Treads, Trail), Signal, Treadmill, Oval, Stadium and the STRAIGHT mockup (`StraightMockup`) are behind `cfg(any(test, feature = "walk-styles"))`. That covers their `StepStyle` variants, `StepStyle::ALL`, `draw::footsteps` and `draw::people`. `walk-styles` is host-only, for the listening page's renders (`tape_walk_glyph_test`'s `write_variants`).
- **Bank 2:** the glyph is `draw`'s, as before (0173). Its poses are `static`s so they link under the module's name.

## Alternatives considered
- **Shipping the mockup as drawn:** at 1× its faces, hair and crowd turn into noise. That is the owner's concern.
- **Three runners:** in a 52 px run their limbs overlap, and at 1× they read as one shape.
- **A loop-back arc over the lane on LOOP, instead of the oval:** clearer at 1× in isolation. The owner's pick has the track take DIRECTION's shape, so the oval stays, simplified, behind the runners.
- **Leaving the other styles linked:** `render_style` takes the style at run time, so every style the match can reach would link into bank 2. The cfg gate makes sure they don't.

## Consequences
- **Flash:** plain release builds against main e9052561, with a `rust-objcopy` split. Bank 2 drops by 1,024 B on both sets: 313,632 → 312,608 on default and 341,312 → 340,288 on ORBIT. Bank 1 changes by +64 B (874,344 → 874,408 and 890,940 → 891,004), which is layout noise. No glyph code lands in bank 1, and both sets stay within their budget lines.
- WALK's screen goldens (`tape_deck_*`, `tape_page_*` and `tape_strip_*` of WALK's ways) and the glyph goldens (`*_still`, `loop_wow`, `loop_grain`, `tron_worn`) are re-recorded, and `docs/screens` is refreshed.
- A later round of owner feedback changes `straight.rs` alone. A mockup can be brought back by building with `walk-styles`.

## Sources
- The owner's mockups: (private notes) (STRAIGHT is style C). The 1× and 4× comparison is in (private notes).
- `chimera-engine/src/tape_glyph/{mod.rs,draw.rs,draw/straight.rs,draw/footsteps.rs,draw/people.rs}`.
- Tests: `chimera-core/tests/{tape_walk_glyph_test,tape_glyph_test,screen_golden_test}.rs`.
