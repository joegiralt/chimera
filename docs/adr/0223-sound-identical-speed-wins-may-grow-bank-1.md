# 0223. Let a sound-identical speed win grow bank 1 by its measured bytes

- **Status:** Accepted (2026-11-05)
- **Deciders:** owner; firmware

## Context
ADR 0198 froze bank 1: new code goes behind a bank-2 door, and an in-place rewrite may not grow the bank. The CLEAN filter's cascades (LP24, BP24, HP) now run their second stage a sample behind the first, so the M7 overlaps the two (#793, PR #855). Output and filter state are bit-identical to the per-sample loop, which `held_is_tick_to_the_bit` pins. On the unit, 8 voices, 6 OP reads 8178 → 8001 cycles a sample and 1 OP 5419 → 5246, about −22 a voice.

The loop is inlined into CLEAN's `run` for each of the 8 modes, and it grows bank 1 by about 1.7 KB in every set (default 809,312 → 811,112 B). Pipelining only the steady path still grows it by 936 B. An out-of-line second stage grows it by 864 B and loses the overlap. Moving CLEAN behind a door is a larger change than the win. With the change, the shipped ORBIT build's bank 1 is 827,044 B of 917,504 (896 KB): 90,460 B free.

## Decision
1. **A speed rewrite that is sound-identical** (goldens bit-identical, or a null under −120 dB) may grow bank 1. The budget lines rise by the measured bytes, each comment naming this ADR. Free bank 1 must stay above 40 KB in the ORBIT build; under that, the rewrite goes behind a door instead.
2. **The CLEAN cascade pipeline is the first:** about +1.7 KB in each set, measured on 2026-11-05 (efficiency branch, 6e80a3e0 plus the profiler fixes). The lines are in `tools/bank1-budget.txt`.

## Alternatives considered
- **Drop the win.** About 2 % of a voice for 1.7 KB: the owner judged 90 KB free enough to keep it.
- **CLEAN behind a bank-2 door.** It is VA's and ALGO's home filter, in every voice's block; a door is a real port for a 22-cycle win.
- **Free 1.7 KB elsewhere in bank 1.** No sound-identical cut of that size is known.

## Consequences
Bank 1's free room shrinks by each such win. Code-size waste is still not shaved byte by byte; the 40 KB floor is the escalation line. New features still go to bank 2 (ADR 0198 stands).

## Sources
- ADR 0198 (bank 1 frozen); #793; PR #855.
- `chimera-engine/src/dsp/filter/clean.rs` (`held`, `second`, `held_is_tick_to_the_bit`).
