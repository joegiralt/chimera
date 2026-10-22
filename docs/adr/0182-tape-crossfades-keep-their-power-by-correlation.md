# 0182. TAPE's long crossfades keep their power, the law picked by the material's correlation

- **Status:** Proposed
- **Deciders:** owner (#698), firmware

## Context
Musical finding 9. LOOP's fades and STRETCH's splices used gains that sum to 1 (a smoothstep on a linear ramp). That is right for correlated material (a held tone, a short loop) and leaves −3 dB at the middle of uncorrelated material: LOOP on a choir or noise bed dipped once a pass ("at some lengths it pulses"), and STRETCH POLY at D-TIME 50 ms fluttered by up to 3 dB at 40 Hz. Equal-power gains on correlated material peak at +2.8 dB instead, past the bounds `tape_bounds_test` and ADR 0103 hold.

## Decision
- `heads::xgains(g, c)`: the outgoing and incoming gains `y·(1 + c·(1 − y²))` and `x·(1 + c·(1 − x²))` on the smoothstep `x` (`y = 1 − x`). `c` 0 is the old sum to 1; `POWER` (½) sums the powers to 0.945–1.
- `heads::power_of` picks `c` per fade from what its two heads will read: the normalized correlation `ρ` of 64 sample pairs (a random place in each stride, so no tone aliases to one phase) over the fade, and `c` holding constant power at the middle for that `ρ`, `(2/√(2 + 2ρ) − 1)·4/3`, clamped to 0..`POWER`. A held tone sums to 1 and never peaks past its level; a bed keeps its power.
- **LOOP:** fades of 20 ms and longer (`POWER_FADE_S`), read forward, measure; shorter ones, mirrored or SCRUB's, sum to 1. Too few frames held (a streamed span) sums to 1.
- **STRETCH:** splices on an unpitched source between grains going one way and clear of a wrap measure, at most `POWER / 2` (−1.5 dB at worst on a bed, against −3); a locked or pitched grain, a flip or a wrap inside the fade sums to 1, which the holds' and lifts' bounds (ADR 0103) assume.

## Alternatives considered
- **Equal-power always on long fades.** +2.8 dB on correlated loops, past the bounds.
- **By source label only (unpitched equal-power).** Many sines and pads carry no period; the measured law follows the material.
- **Full equal-power in STRETCH.** It broke the hold/lift peak bound on grains that wrap or flip; reworking ADR 0103's carry for it is left for a STRETCH round of its own.

## Consequences
- About 8 flops a sample more in a fade's loop, and 128 decodes once a fade (cold path); LOOP's and STRETCH's bills hold.
- Saved LOOP and STRETCH Sounds on uncorrelated material sound steadier; they load safely.

## Sources
- Musical finding 9; #698. Kontakt manual (crossfade curves); the constant-power pan law.
- `chimera-engine/src/dsp/tape_engine/{heads,mod,stretch}.rs`.
