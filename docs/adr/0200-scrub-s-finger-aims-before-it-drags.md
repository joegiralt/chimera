# 0200. SCRUB's finger aims before it drags, so a POSITION step never steps its speed

- **Status:** Proposed
- **Deciders:** owner (UAT, 2026-10-21: "the position jumps instead of moving smoothly"), firmware
- **Amends:** [0093](0093-tape-plays-12-bit-card-sources-through-six-ram-models.md) § SCRUB (the finger's slew), [0112](0112-every-parameter-declares-its-motion.md) point 9 (SCRUB's POSITION handed on at once)

## Context
SCRUB's POSITION reaches the deck unglided (ADR 0112 point 9): the FINGER's jog writes it once a UI pass, a CC writes it in 1/128 steps, and a matrix route or a jump writes it whole. The deck's finger followed it through one one-pole of DRAG, re-aimed once a block, so the finger's speed was proportional to the gap: every step of POSITION set the speed at once to its largest, then let it fall. Measured on a ramp source (the output is the head's place), DRAG 32 ms:
- the jog at 25 detents a second with a 33 ms UI pass moved the head at 0.14× to 0.36× within each pass, a sawtooth of its speed (pitch) at the pass rate; DRAG's one-pole alone swings ×2.8 at a 33 ms staircase;
- a CC sweep (a step every 23 ms) swung 0.27× to 0.43×;
- a jump of 0.7 of the window took the head from still to 19× in one 5 ms reading (the clamp's 16× plus the loop), a 4-octave zip that starts at once.

The head never skipped (its speed is bounded), but its speed, which is SCRUB's pitch, snapped at every step. That breaks the rule that every change is lerped and never snaps.

## Decision
- **The finger follows an aim, the aim follows POSITION.** `Scrub` gains `aim` (a `Pos`, in the finger's frame, so a turn leaves it as it is) and `aim_pull`, a one-pole of `AIM_S` = 40 ms a block. Each block the aim moves `aim_pull` of the way to POSITION's target, and the finger moves DRAG's `pull` of the way to the aim, as it moved to the target before. The first block of a note sets both to the target.
- **DRAG keeps its law and its bound.** The finger's speed stays bounded so the head's never passes `INC_MAX`.
- **Cost:** one multiply-add a block per SCRUB voice, and 12 B of `Scrub`. `COST_SCRUB` is unchanged.

## Alternatives considered
- **Glide POSITION with the Part's knobs** (undo 0112 point 9). It is one pole of 30 ms on the same staircase, and it would also glide the matrix's and CC's writes twice.
- **Make the jog hand the deck a speed, not a place.** Better for the jog alone, but CC, the matrix and a jump still write places. It fits the playability rework better than a fix.
- **Two poles of DRAG/2 each.** At DRAG's 10 ms floor it smooths nothing, and the UI pass's staircase is the same at every DRAG.

## Consequences
- A jump now starts the reel over several blocks. Measured on the ramp source, DRAG 32 ms, a jump of 0.7 of a 1 s window: the first 5 ms reading is 0.46× (was 7.9×) and the peak 7.2× (was 19×, past the 16× clamp with the loop). The head reaches 63 % of a jump later: at DRAG 32 ms, about 77 ms against 32.
- A staircase of POSITION at a 33 ms pass (the deck alone) moves the finger within +4 % and −8 % of its speed in a block-rate model of the two poles, where DRAG's one-pole alone swung it ×2.8; `scrub_finger_speed_never_steps` holds the deck under ×1.3.
- Through the real jog (25 detents a second, a 33 ms UI pass) the head's speed now runs 0.14× to 0.25× (was 0.13× to 0.36×), and a CC sweep's 0.23× to 0.36× (was 0.21× to 0.43×). What is left is the jog's own speed estimate, a count of the detents in the last 125 ms. Smoothing that is UI work for the playability rework.
- `scrub_drag_slews` now measures the two poles' 63 % time.
- The rest of SCRUB's playability (the dead FINGER at LENGTH 1, a key that places the finger, DRAG latched at note-on, a reel with real inertia) is the proposal on the TAPE rework, not this fix.
- **Flash:** bank 2 +192 B on the ORBIT build; bank 1 unchanged (its only difference is the build-revision string).

## Sources
- (private notes) (the measurements and the listening page).
- `chimera-engine/src/dsp/tape_engine/mod.rs` (`scrub_block`, `AIM_S`), `chimera-core/src/ui/jog.rs`, `chimera-core/tests/tape_models_test.rs`.
- Precedent: Morphagene Slide (a slewed position), a tape reel's inertia.
