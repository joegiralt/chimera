# Algorithmic Engine — Design

**Date:** 2026-09-26
**Status:** Draft, awaiting user review
**Builds on:** `instrument-on-chip` (the Instrument on the STM32, measured costs, the bench).
**Replaces:** the Pizza, FM and VA engines. Modal stays.

## Intent

Replace the oscillator engines with one algorithmic 6-operator engine rooted in the Yamaha TX81Z sound: its algorithms, its waves, and optionally its arithmetic. The engine extends that with:
- a wide wave set;
- a per-operator modulation mode;
- a morph between two algorithms.

It must run 6 voices on the STM32H750 inside the audio budget, and it fixes #26, the current FM engine's cost of 6,600 cycles per voice.

**Done when:**
- `ChainType` is `{Algo, Modal}`;
- the factory Sounds play on the Algo engine on both the desktop and the chip;
- the bench shows the engine at 250 cycles per voice per sample or less;
- every parameter is editable from group pages with MIX gang edit.

## Principles (binding)

- **Functional core, imperative shell.** These are all pure, host-tested functions: wave recipes, operator maths, modes, envelopes, algorithm tables, ordering, morph blend, and the TX arithmetic. `AlgoEngine` is the thin per-sample loop.
- **Type-driven** (ADR 0012). For example:
  - `OpIndex` (0–5);
  - `WaveId` (0–63);
  - `ModMode`;
  - `PitchMode`;
  - `AlgoId`;
  - `Morph` (0.0–1.0);
  - `Character`.

  Illegal states are unrepresentable where it's cheap.
- **Cuttable.** One module per concern; the TX character path is behind a Cargo feature.
- **Parity (ADR 0013).** Desktop and chip run the same engine and the same budget.
- **Nothing snaps** (CLAUDE.md). Per-block parameters are ramped across the block.

## Voice model

Each voice has 6 operators. Per operator:

| Parameter | Range | Notes |
|---|---|---|
| WAVE | 64 waves | waves 1–8 are the TX81Z's W1–W8 |
| MODE | PM, FM, PWM, SYNC, PD-SQ, PD-SAW, PD-SP | How this operator acts on the operators it feeds. It has no effect on its own sound. |
| PITCH | RATIO or FIXED | RATIO uses the TX81Z's 64 coarse ratios (0.50–25.95), FINE in 1/16 steps, and DETUNE ±3. FIXED is 1 Hz–10 kHz. |
| LEVEL | 0–99 | TX81Z log scale, 0.75 dB per step. For a modulator it sets depth; for a carrier, volume. |
| ENV | AR, D1R, D1L, D2R, RR | TX81Z rate-based envelope, reusing `envelope_fm.rs`. |
| RATE SCALING | 0–3 | TX81Z keyboard rate scaling. |
| FEEDBACK | 0–7 | Self-modulation on any operator. |
| LEVEL SCALING | 0–99 | Keyboard level scaling, TX81Z-style. |
| VELOCITY | 0–7 | Velocity sensitivity of the level. |

**Voice-level parameters:**
- `ALG A` and `ALG B`, each a preset algorithm;
- `MORPH`, 0–100 %;
- `CHARACTER`: CLEAN or TX.

**The rest of the voice is unchanged:** drive → filter → wavefolder → VCA, the amp envelope, the LFO, the mod matrix, pitch bend and MPE sources.

**Modulation layers:**
- **Operator to operator:** audio-rate modulation inside the engine, wired by the algorithm.
- **The mod matrix:** drives the whole chain as before. The engine adds these matrix destinations, all read once per block:
  - `MORPH`;
  - each operator's `LEVEL`;
  - each operator's `FINE`;
  - each operator's `FEEDBACK`.

  This follows ADR 0010. MIX+PLUS priming works on the engine pages, e.g. `ALG OP3 LEVEL`.

### Modulation modes

Each operator's output `m` acts on each carrier it feeds, through that carrier's phase `φ` (0..1):

| Mode | Effect on the carrier |
|---|---|
| PM | `φ' = φ + m·depth` |
| FM | Through-zero linear FM: the carrier's phase increment becomes `inc·(1 + m·depth)` |
| PWM | Warps `φ` around a moving midpoint `0.5 + m·depth/2`, giving pulse-width style squeeze and stretch on any wave |
| SYNC | Resets the carrier's phase when the modulator's phase wraps. The amount scales with `LEVEL`, and the modulator's ratio sets the number of resets per carrier cycle. |
| PD-SQ | CZ-style phase distortion that switches between fast and slow playback within the cycle, with the contrast set by `LEVEL` |
| PD-SAW | CZ-style distortion that speeds playback up and then slows it abruptly within the cycle |
| PD-SP | PD-SAW then PD-SQ, applied back to back |

When a carrier has inputs in several modes, it applies them in a fixed order: SYNC, then PD, then PWM, then FM, then PM. That order is a pure, documented function, and the tests cover it.

## Waves

**64 waves, generated from our own recipes, in 8 families of 8:**
1. **TX:** the TX81Z's W1–W8, generated from the log-sine formula, not dumped from ROM.
2. **Classic:** sine, triangle, saw, square, 25 % pulse, 12 % pulse, trisaw, rounded square.
3. **Bent sine:** folded, skewed and clipped sines at increasing amounts.
4. **Skewed triangle and ramp:** the peak moving through the cycle.
5. **Additive:** odd harmonics only, even harmonics only, 1/n², organ drawbars, bell partials, stretched partials, and 2 hollow spectra.
6. **Formant:** single-cycle vowel-like shapes (a, e, i, o, u, and 3 nasal or voiced variants).
7. **Pulse / PW:** narrow pulses, double pulses, and pulses with a sagging top.
8. **Digital:** bit-crushed sines, stepped waves, and seeded pseudo-random single cycles.

**Storage:**
- Each wave is 256 samples of i16 with 8 band-limited mip levels. That's about 4 KB per wave, 260 KB in flash.
- `build.rs` generates the tables from pure recipe functions, which are unit-tested.
- A const assertion checks the flash budget.
- Each recipe family is its own module, so cutting a family frees about 32 KB.

**Playback:** each operator picks its mip level once per block from its fundamental, then reads samples with linear interpolation.

**Review:** `just waves` renders contact-sheet PNGs of all 64 waves at each mip level, for the user to review before voicing.

## Algorithms

An `Algorithm` is a const table:
- `weights: [[u8; 6]; 6]`, where 0 or 1 means modulator-to-carrier;
- `carriers: [u8; 6]`, the output gains;
- a short name.

**The 32 presets:**
- **T1–T8:** the TX81Z's 8 algorithms on operators 1–4 (matching the routing verified in #18), plus operators 5→6 as an extra 2-op stack mixed in. A TX81Z patch translates with operators 5–6 at level 0.
- **24 organised by carrier count:**

| Carriers | Algorithms |
|---|---|
| 6 | additive, all parallel |
| 5 or 4 | one or two modulators fanned out across the carriers |
| 3 | three 2-op stacks; 3-into-1 fan-ins |
| 2 | two 3-op stacks; cross-coupled stack pairs (a lattice) |
| 1 | a 6-deep chain; a 5-into-1 fan-in; diamonds |

The existing algorithm diagram draws every algorithm, with its layout computed from the table.

### Evaluation order and morph

- `plan(a: AlgoId, b: AlgoId) -> EvalPlan` is pure and cached until A or B changes. It takes the union of the two graphs and finds an order where modulators come before carriers. Any link that runs backwards in that order is marked to read the previous sample, which is the same one-sample delay self-feedback already uses. The result is an edge list.
- **Blending:** with MORPH = m, each edge weight is `(1−m)·A + m·B`, and carrier gains blend the same way. An operator that is a carrier in A and a modulator in B crossfades smoothly between the two roles.
- MODE and FEEDBACK are operator parameters, not part of the algorithm, so morphing doesn't change them.
- MORPH is ramped across each block, so modulating it never clicks.

## Rendering

Once per 64-sample block, per voice:
1. Refresh the `EvalPlan` if A or B changed.
2. Blend the edge weights by MORPH.
3. Advance each operator's envelope and level. The block ramps linearly from the old value to the new one.
4. Select each operator's mip level.

Per sample: go through the operators in plan order. For each, sum its weighted inputs by mode, apply the modes in the fixed order, look up its wave, and apply its level and envelope. Carriers are summed into the voice output.

**Character paths:**
- **CLEAN:** 32-bit float throughout.
- **TX** (feature `tx-character`, default on): the TX81Z's own arithmetic.
  - Phase accumulates in 32-bit fixed point.
  - Waves are read as log magnitude plus sign, from log tables generated alongside the waves.
  - Levels and envelopes add in the log domain.
  - A generated exponential table converts back to linear.
  - The output is truncated the way the chip truncates it.

**Budget:**
- The target is 250 cycles per voice per sample or less on the chip, measured with the existing `bench`.
- Six voices at about 1,500 cycles, plus the FX bus at about 3,200, come to 4,700 against the 7,000 budget.
- `AlgoEngine::COST` is committed from the measurement.
- If CLEAN or TX misses the target, the bench numbers decide what to cut: the mode set per carrier, or the mip interpolation.

**Memory:**
- Per-voice engine state is about 1 KB: 6 operators' phase, envelope, feedback history and the plan. That frees D2 SRAM compared with Pizza, FM and VA.
- The const assertions in `hw.rs` stay authoritative.

## UI

The pages are **grouped by parameter across all 6 operators**. On each page, encoders A–F edit operators 1–6.

| Page | Cells (operators 1–6) | Visualization band |
|---|---|---|
| WAVE | wave | the 6 waves as thumbnails |
| MODE | modulation mode | the algorithm diagram with each node's mode |
| PITCH | ratio or fixed frequency | the ratios as a harmonic ladder |
| FINE | fine and detune | the harmonic ladder |
| LEVEL | level | the algorithm diagram, node size following level |
| ENV (AR, D1R, D1L, D2R, RR) | one stage per sub-page | the 6 envelope shapes overlaid |
| FEEDBACK | feedback | — |
| SCALING | level scaling, rate scaling, velocity | — |
| ALGO | ALG A, ALG B, MORPH, CHARACTER | the A and B diagrams blended by MORPH |

- **Gang edit:** hold **MIX** while turning any encoder to move all 6 operators by the same relative amount, keeping their offsets and clamping each one at its own range. The focus band shows `ALL ×6`. Releasing MIX goes back to editing one operator.
- **Per-operator detail:** EDIT on a group-page cell opens a page with every parameter of that one operator.
- **Style:** Direction A (ADR 0016), with focus-band, dirty-region and screen-golden rules as for every page.

## Replacing the old engines

- `EngineType` loses `Pizza`, `Fm` and `Va`, and gains `Algo`. `ChainType` becomes `{Algo, Modal}`.
- These are deleted:
  - `pizza.rs`;
  - `engine_fm.rs`;
  - `fm_waveform.rs`;
  - `oscillator.rs` and any VA-only code;
  - their param blocks and pages;
  - their audio goldens, including the FM `KNOWN_BROKEN` entries.
- `envelope_fm.rs` and `fm_tables.rs` are kept, since the operators reuse them.
- There are no saved patches yet (storage is a later sub-project), so nothing needs migrating. The factory Sounds are re-voiced: TX81Z-style patches, plus simple 1–2-operator Sounds covering the old Pizza and VA roles.

## Testing

**Host tests (pure core):**
- **Wave recipes:** each is finite, has no DC offset beyond a tolerance, and has peak ≤ 1. The mip levels are band-limited: no energy above the level's Nyquist, checked with a small DFT. The flash total stays inside the budget.
- **Operator maths:** each mode is checked against a reference formula on known inputs. The order of mixed modes is checked.
- **Envelope:** TX81Z envelope behaviour is unchanged (the existing tests are kept).
- **Algorithms:** every table has at least one carrier and no self-links (self-modulation goes through FEEDBACK), and every weight is 0 or 1. T1–T8 equal the TX81Z routings.
- **`plan()`:** every forward edge runs modulator before carrier. A cyclic union is resolved with one-sample delays.
- **Morph:**
  - MORPH at 0 renders bit-identical to A, and at 1 bit-identical to B.
  - A sweep has no discontinuity between blocks.
- **Gang edit:** a pure `gang_apply(values, delta, ranges)` keeps offsets and clamps each value.
- **`init_in_place`** equals `new` for `AlgoEngine`.

**Goldens** (recorded deliberately, ADR 0011):
- the init patch;
- one patch per mode;
- a morph sweep;
- the TX character path;
- a 6-voice chord through the Instrument.

Modal's goldens are untouched. Screen goldens are added for the new pages.

**On the chip:**
- the bench, with the measured `Cost` committed;
- the loopback test (MOTU input): pitch, no clipping, no clicks.

## ADRs

- **0022:** one algorithmic 6-operator engine replaces Pizza, FM and VA; Modal stays.
- **0023:** the waves come from our own recipes. The TX81Z waves are generated from their formula, not dumped from ROM.
- **0024:** morph blends connection weights, and links that run backwards in the evaluation order read the previous sample.

## Out of scope (later)

- a custom algorithm editor (the table format already allows it);
- formant operators in the style of the FS1R;
- user waves loaded from SD;
- morph across more than 2 algorithms;
- per-operator pan and stereo operators.

## Risks

- **CPU:** mixed modes plus per-sample mip interpolation might exceed 250 cycles. The bench decides, and the fallbacks are listed under Budget.
- **Flash:** 260 KB of waves against about 628 KB free (more once the old engines are deleted). The const assertion guards it, and families can be cut.
- **TX character accuracy:** the formula-generated log-sine and exponential tables are verified against the published TX81Z behaviour: the waves' shapes, the 0.75 dB level step, and the algorithm routing. It is not claimed to be bit-exact against a real TX81Z.
