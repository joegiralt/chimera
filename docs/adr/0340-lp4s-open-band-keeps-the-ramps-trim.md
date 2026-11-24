# 0340. LP4's open band keeps the ramp's trim

- **Status:** Accepted (2026-11-24)
- **Deciders:** firmware (the re-review of #1135; the owner's rule: filter sweeps are never steppy); amends ADR 0338

## Context
ADR 0338's band trimmed each sample at the tapped, damped `k` (`trim(ki)`). Under the band's lower edge `lo`, the moving loop trims at the ramp's `k`, as CLEAN always has: taps never move the trim. So a Q tap holding RES up stepped the gain wherever CUTOFF crossed `lo`.

A 750 Hz sine shows it (a period a block). LP4 at RES ½, swept 14 to 18 kHz under a steady Q tap of 0.4, which takes `k` from 0.95 to 0.1: one block's level stepped 3.6 dB as CUTOFF crossed `lo`. Untapped, the steepest block moved 0.0003 dB.

The re-review also found the band's place `u` dividing by `top − lo` on every sample of every tapped or FB LP4 block, and `TapShape::new` built twice per LP4 block, once each side of the door.

## Decision
1. **The band keeps the ramp's trim** (`Clean::moved`). Past `lo` a sample's trim is the ramp's, moved only by the band's damping of the ramp's `k`: `trim(ramp k) ramped + trim(damped(ramp k, u)) − trim(ramp k)`. The added term is exactly 0 at `u = 0`, so the gain is continuous across `lo`. The damping of the tapped `k` (ADR 0338) is unchanged; only the trim no longer follows the tap. A held block's trim (`trim(damped k)`) is the same value.
2. **`OpenTop` holds `1/(top − lo)`** (`per`, taken with the band's constants), so `u` multiplies.
3. **`TapShape` is built once a block.** `Clean::process` builds it for every mode but LP4; LP4 builds it behind the door, or for the closed door's path.

## Alternatives considered
- **Blending `trim(ki)` in over part of the band.** It is continuous at `lo`, but past the blend the trim still follows the tap, which nothing under the band does.
- **Trimming at the tapped `k` under the band too.** It would change bank 1's moving loop for every tapped CLEAN block: frozen code and new sound.

## Consequences
- `clean_open_test::a_q_tap_through_the_band_edge_never_steps_the_gain`: the tapped sweep's steepest block is 0.0003 dB, the untapped sweep's.
- **Goldens.** The reciprocal moves `u` by an ulp, and so every render whose CUTOFF sweeps through the band: `golden_test`'s `modal_lfo_cutoff` and `algo_lfo_cutoff` (their checksums by 2 and 3 ulps) and `fixed_columns_test`'s routed render. Nothing else moves.
- **Flash.** Bank 1 −80 B on every set (`process` builds no `TapShape` for LP4): ORBIT 912280. Bank 2 +128 B on default, +96 on ORBIT and bank2-gate, in `clean::open`'s S4 group; S0–S3 keeps its 64 B on ORBIT. Lines in `tools/bank1-budget.txt` and `tools/bank2-budget.txt`, measured on the merge with origin/0.4 0db4e3d41.

## Sources
private issue #695, #1135 (the re-review); ADR 0327, 0338.
