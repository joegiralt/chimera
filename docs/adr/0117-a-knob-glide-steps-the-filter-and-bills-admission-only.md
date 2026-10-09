# 0117. A knob glide steps the filter and bills admission only

- **Status:** Proposed
- **Deciders:** owner (ruling 2026-10-09: "cheaper glide + admit-only billing"), firmware
- Adds to [0116](0116-knob-changes-reach-the-audio-as-moves.md); supersedes nothing.

## Context
Since ADR 0112 a knob glides 30 ms (~22 blocks). Every block of it the filter's coefficients differ from the last, so each voice of the Part ran the per-sample moving loop (CLEAN 75, 2040 60, LADDER 70 cycles a sample over steady; fold and drive ramps 35 and 23) while the bill had them steady. STRESS MOVE ran 486,732 against a 448,000 budget. Billing the moving path honestly, shed included, drops a voice mid-turn in most of ADR 0040's setups (ALGO default 8/7 on rev V without/with the master tape, A16 ∪ A17 six).

## Decision
1. **Cheaper glide.** When no route or tap moves a stage at audio rate, a glide block runs in `STEPS` = 4 sub-blocks of 16 samples, each the steady loop at its own `g`, `k` (and trim, LEVEL's gain stays its ramp), set at the sub-block's midpoint on the block's linear ramp (`filter::step_to`). CLEAN, the 2040, LADDER, FOLD and DRIVE. DIRTY's one loop is unchanged. Routes and taps keep the per-sample moving path. The voice sets `FilterMods::glide` and the stage `glide` flags from the routes.
2. **Admission only.** The residual glide cost (`FilterKind::glide_cost` 5 for CLEAN, 2040, LADDER, 0 for DIRTY; `Voice::FOLD_GLIDE_COST`, `DRIVE_GLIDE_COST` 1) counts in admission (a new note is judged with its Part's glide on every voice) and in `sounding_cost`, never in shedding: `Allocator::shed` ignores it, so a turn never drops a sounding voice.
3. A path a route or tap already bills moving is not billed twice.

## Alternatives considered
- Bill and shed on the moving cost: loses a voice mid-turn in the reference setups.
- Admission only at the moving cost: still blocks notes during every glide for ~60 cycles a voice.
- A coarser step for the whole block: audible zipper on resonant sweeps.

## Consequences
Steady counts and shedding are unchanged. A note-on during a glide is judged ~5 cycles a voice dearer. A stepped glide differs from the per-sample ramp by under 5 % of a 0.8 saw on a 4-octave resonant sweep (test `filter_glide_test`); a block's cutoff step is at most 0.047 octave (3.3 %) per sub-block. Residual costs are predicted from the `filter-probes` ELF; the unit confirms them (`just flash-bench stress orbit`, STRESS MOVE and BILLED).

## Sources
ADR 0112, 0116; `tools/loop_count.py` on `filter-probes`; the knobs-rare report § Moving bill.
