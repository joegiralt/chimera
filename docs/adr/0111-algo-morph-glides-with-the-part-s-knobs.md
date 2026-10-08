# 0111. ALGO's MORPH glides 30 ms with the Part's knobs

- **Status:** Proposed; superseded in part by [0112](0112-every-parameter-declares-its-motion.md) (point 1: MORPH is one of the knobs every parameter declares; the generic `Knobs` glides it)
- **Deciders:** owner (UAT 2026-10-08: "the morph knob on the algorithm engine is very steppy", #417), firmware

## Context
Measured through the real path (panel acceleration, `nudge`, publish, Instrument; `chimera-core/tests/morph_smooth_test.rs`) on main:

- MORPH is stored 0..127, one tick a step. A slow detent moved the MORPH the voice plays by **1/127 (0.0079) in one block**; a fast turn (10 ticks a detent, one every 30 ms) by **10/127 (0.079) in one block**, 59 per mille of the range a millisecond. The engine ramps MORPH inside a block (`morph_from` to `morph_to`), so each detent is a ramp one block long, then a hold: a staircase, as the filter's was (ADR 0107).
- Inside the morph there is no discrete jump. Link weights and carrier weights are one linear `blend`; the output scale is continuous in the morph (`carrier_norm`, tested); the mip position follows the blended link depth and moves at most one mip a block through a crossfade; `silent` and the plan order do not depend on MORPH. A steady note at each of the 128 positions, over 40 algorithm pairs, changes at most 9 % of its level span between neighbouring ticks, no cliff anywhere. The stepping is the knob, not the DSP.

## Decision
1. **MORPH joins the Part's knobs** (`dsp/knobs.rs`, moved from `dsp/filter/knobs.rs`, which was never filter-only): one more one-pole of **τ = 30 ms** (`KNOB_EASE_S`), linear in its 0..1 place, once a block in `Instrument`. The voice plays the eased MORPH in place of the stored one (`AlgoLive::morph`) and adds its route's offset after, so a MORPH route (LFO, ENV) keeps its snap.
2. **Same landing rules as ADR 0107 point 4:** a silent Part's next note starts on the stored MORPH, a loaded Sound lands even under a ringing voice, a settled glide hands the voice the stored value bit for bit (any `u8`, so 255 still clamps to B in `Morph::from_param`).
3. **The tick stays 1/127.** Glided, a lone tick spreads over ~90 ms at 0.00034 of the range a block (23× under the 0.0079 step it was), and a fast turn peaks at 0.0041 of the range a millisecond, 14× under what it was and under the 2× the hand's own rate (0.0053) the test bounds. A finer tick would change the stored `u8` and the patch format for nothing the glide does not already give.
4. **The harness is shared:** the encoder-acceleration turns move to `tests/common/turn.rs` and the two-Part rig to `tests/common/part_rig.rs`; the filter and MORPH tests use both.

## Alternatives considered
- **Ease inside the AlgoEngine** (per voice): costs engine bytes a voice, each voice glides from its own start, and a route's offset would be eased with it. Per Part costs 8 B and moves every voice together.
- **A finer stored MORPH:** a patch-format change; not needed once eased.
- **Crossfade a discrete jump in the morph:** none found.

## Consequences
- CPU (estimated, not benched on the unit): one more `glide` a block per playing Part, a few cycles.
- RAM: 8 B a Part (`[(Knobs, Loads); 6]`).
- A MORPH turn lands 30 ms late (63 % in 30 ms, 95 % in 90 ms).
- A note started mid-glide begins its engine on the stored MORPH and ramps to the eased one in its first block (the voice's engine state is its own); the filter has no such state. Inaudible under an attack; not fixed here.
- Not covered, listed in #417's follow-ups: LEVEL, FDBK, FINE, DETUN and the Modal macros (STRUCT, BRIGHT, DAMP, POS, eased 0.3 a block, ~5 ms) step or glide short of 30 ms.

## Sources
- #417, ADR 0107, `chimera-engine/src/dsp/knobs.rs`, `chimera-core/tests/morph_smooth_test.rs`.
