# Algorithmic Engine — Design (sub-project 1: core engine)

**Date:** 2026-09-26
**Status:** Draft rev 2 (after adversarial review), awaiting user review
**Builds on:** `instrument-on-chip` (the Instrument on the STM32, measured costs, the bench, the AUDIO page).
**Replaces:** the Pizza, FM and VA engines. Modal stays.
**Supersedes:** ADR 0003 ("Keep the faithful TX81Z 4-op FM engine"), through ADR 0022.

## Roadmap

The engine lands in five sub-projects, each with its own spec, plan and build. The CPU risk is retired first.

1. **Core engine (this spec):** 6 operators, PM plus feedback, a 16-wave starter set, 32 algorithms, A↔B morph, the group pages, and a chip bench. Pizza, FM and VA are replaced.
2. **Modulation modes:** FM (through-zero), PWM, SYNC, PD-SQ, PD-SAW and PD-SP, each with a precise definition and an evaluation order. Also FIXED pitch mode.
3. **Full wave set:** 64 waves in 8 families, with mip selection that accounts for modulation bandwidth.
4. **Pages and gang edit:** MINUS+turn gang edit (anchored at the start of the gesture), SCALING pages, per-operator detail, and dedicated visualizations.
5. **TX character:** the log/exp fixed-point render path.

Decisions made while brainstorming the whole engine, and binding on later sub-projects:
- 6 operators, rooted in the TX81Z (its 8 algorithms and its 8 waves).
- The 7 modulation modes.
- 64 waves generated from our own recipes.
- A↔B morph.
- TX81Z-style rate envelopes.
- Group pages by parameter across operators.
- Gang edit is **hold MINUS + turn**, because MIX+turn is already coarse snap.

## Intent (sub-project 1)

One 6-operator phase-modulation engine becomes the voice's oscillator. It must:
- cover the old roles: TX81Z-style FM, and simple 1–2-operator Sounds for the Pizza and VA roles;
- play 6 voices on the chip inside the budget;
- fix #26.

**Done when:**
- `ChainType` is `{Algo, Modal}`;
- a new factory bank of Algo Sounds plays on the desktop and on the chip;
- the chip bench's **worst-case** patch measures within the budget below, and that measurement is committed as `AlgoEngine::COST`;
- the group pages edit every sub-project-1 parameter.

## Principles (binding)

- **Functional core, imperative shell.** These are all pure, host-tested functions: wave recipes, operator maths, the envelope, algorithm tables, `plan`, the morph blend and carrier normalisation. `AlgoEngine` is the per-sample loop.
- **`f32` only in the render path.** The chip has no double-precision FPU, and `f64` becomes software float. That is the cause of #26: `FmEnvelope::run` makes 24 `__aeabi_d*` calls per sample, and `libm::sinf` calls soft-double too. A test or lint guards the engine module against `f64`, and there is no `libm` trig in the render path.
- **Type-driven** (ADR 0012). The existing `Op` operator index extends from A–D to A–F, with no second index type. Also `WaveId`, `AlgoId`, `Morph`, and compact `u8`/`i8` stored parameters.
- **Parity** (ADR 0013). **Nothing snaps** (CLAUDE.md), as described under Rendering.

## Voice model

Operators are numbered 1–6 in the TX81Z convention: higher numbers modulate lower ones, and operator 1 is always a carrier.

**Per operator:**

| Parameter | Stored | Meaning |
|---|---|---|
| WAVE | `u8` | One of the 16 starter waves: TX81Z W1–W8 and classic. |
| COARSE | `u8` 0–63 | The TX81Z's 64 coarse ratios (0.50–25.95), a table of facts. |
| FINE | `u8` 0–15 | The TX81Z fine steps: 16 values toward a per-coarse maximum, 8 below coarse 4. |
| DETUNE | `i8` −3..+3 | TX81Z detune. |
| LEVEL | `u8` 0–99 | Scale of 0.75 dB per step; 99 is 0 dB and 0 is silent. |
| AR, D1R, D1L, D2R, RR | `u8` | TX81Z-style rates and level. |
| RATE SCALE | `u8` 0–3 | Keyboard rate scaling. |
| FEEDBACK | `u8` 0–7 | Self-feedback on any operator, averaged over the last two outputs. |
| VELOCITY | `u8` 0–7 | Velocity sensitivity of the level. |

**Per voice:**
- `ALG A` and `ALG B` (`AlgoId`, 0–31);
- `MORPH` (`u8` stored, `Morph` 0.0–1.0 in the render);
- a transpose.

**Storage:** all of it is `u8`/`i8`. Six operators come to about 80 bytes plus 4 per voice. The 13.6 KB of AXI headroom is checked by the existing `AXI_RESIDENT` assertion. Every byte of `Sound` growth costs about 50 bytes across the pool and the buffers.

**The voice chain:**
- The existing drive → filter → wavefolder → VCA chain is kept.
- Like the TX81Z, the Algo engine does **not** use the amp envelope: `uses_amp_env(Algo) == false`. The operator envelopes shape the sound, so release tails ring out.
- `is_active` is true while any operator that is a carrier in the blended graph has a live envelope.

**Mod-matrix destinations** (ADR 0010, all read once per block, ramped):
- `MORPH`;
- the 6 operator `LEVEL`s.

That's 7 destinations, within the 16-slot matrix. Level modulation is applied to the `f32` gain after conversion, so it can't zipper. FINE and FEEDBACK aren't destinations in sub-project 1: FINE's 104-cent steps would zipper.

## Waves (sub-project 1)

**16 waves:**
- **TX81Z W1–W8**, generated from their formulas: sine, sine², half-sine variants, and so on. W3–W8 contain DC by design, which is exempt from the DC test and documented.
- **8 classic waves:** triangle, saw, square, 25 % pulse, 12 % pulse, trisaw, rounded square and soft saw.

**Generation:**
- Each wave is 256 samples of i16 with 8 mip levels, each band-limited. The mip levels are constant length. That's about 64 KB in flash, within the budget.
- A separate host-only crate, `chimera-waves`, holds the recipes. `chimera-core`'s `build.rs` uses it, since a build script can't import the crate it builds. The recipes are unit-tested.

**Playback:**
- The mip level is picked per block from `fundamental × (1 + Σ incoming modulator level)`, a cheap bound on the PM bandwidth.
- The engine **crossfades between adjacent mip levels**, so a level change never steps the sound.
- Samples are read with linear interpolation, from a `u32` phase accumulator.

## Algorithms

An `Algorithm` is a const table, `mods: [u8; 6]`, a bitmask per operator of which operators it modulates, plus a carrier mask and a short name. Notation below: `a→b` means a modulates b; `[..]` lists the carriers.

**T1–T8: the TX81Z's 8 algorithms on operators 1–4**, as verified in #18, plus a 6→5 pair carried out alongside:

| Algorithm | Routing |
|---|---|
| T1 | 4→3→2→1, 6→5 [1,5] |
| T2 | 4→2, 3→2, 2→1, 6→5 [1,5] |
| T3 | 3→2→1, 4→1, 6→5 [1,5] |
| T4 | 4→3→1, 2→1, 6→5 [1,5] |
| T5 | 4→3, 2→1, 6→5 [1,3,5] |
| T6 | 4→1, 4→2, 4→3, 6→5 [1,2,3,5] |
| T7 | 4→3, 6→5 [1,2,3,5] |
| T8 | 6→5 [1,2,3,4,5] |

**24 more, by carrier count:**

| Algorithm | Routing |
|---|---|
| A1 | [1–6] (additive) |
| A2 | 6→1 [1–5] |
| A3 | 6→1, 6→2 [1–5] |
| A4 | 6→1, 5→2 [1–4] |
| A5 | 6→1, 6→2, 6→3, 6→4 [1–4] |
| A6 | 6→5→1 [1–4] |
| A7 | 4→1, 5→2, 6→3 [1–3] |
| A8 | 4→1, 5→1, 6→1 [1–3] |
| A9 | 6→5→4→1 [1–3] |
| A10 | 6→4, 6→5, 4→1, 5→2 [1–3] |
| A11 | 5→4, 6→4, 4→1, 4→2, 4→3 [1–3] |
| A12 | 5→3→1, 6→4→2 [1,2] |
| A13 | 3→1, 4→1, 5→2, 6→2 [1,2] |
| A14 | 3→1, 3→2, 4→1, 4→2, 5→3, 6→4 [1,2] |
| A15 | 6→5→3→1, 4→2 [1,2] |
| A16 | 3,4,5,6 → both 1 and 2 [1,2] |
| A17 | 6→5→4→3→2→1 [1] |
| A18 | 2,3,4,5,6 → 1 [1] |
| A19 | 4→2, 4→3, 2→1, 3→1, 6→5→1 [1] |
| A20 | 3→2→1, 5→4→1, 6→1 [1] |
| A21 | 6→5→4→1, 3→2→1 [1] |
| A22 | 6→4, 6→5, 4→2, 5→3, 2→1, 3→1 [1] |
| A23 | 6→2,3,4,5, and 2,3,4,5→1 [1] |
| A24 | 6→5, 5→2,3,4, and 2,3,4→1 [1] |

**The algorithm diagram** gets a layout computed from the table: operators are placed by depth (longest path to a carrier), and columns by carrier order. It must fit the existing viz band. For a deep chain (A17) it switches to a compact spacing, and a test asserts no node leaves the band. This replaces the hand-authored `ALG_EDGES`/`ALG_POS`.

### Plan and morph

- `plan(a, b) -> EvalPlan` is pure and cached per (A, B). `EvalPlan` is a fixed-size struct holding at most 30 edges, an order of 6, and delay flags. It lives in the voice.
- **Order:**
  - At `MORPH == 0` the engine uses A's own topological order. At `MORPH == 1` it uses B's.
  - Strictly between, it uses the union graph's order. A link that runs backwards in that order reads the previous sample.
  - So each endpoint renders exactly as that algorithm alone.
  - Entering or leaving an endpoint changes the plan only when the weight of the delayed edges is 0 on both sides of the switch, so it is continuous by construction.
- **Blend:** edge weight `w = (1−m)·A + m·B` for each edge. Carrier gains blend the same way.
- **Carrier normalisation:** the output is `Σ(gain·out) / sqrt(max(1, Σ gain))`. That gives constant loudness across algorithms with different carrier counts, and it's smooth through a morph.
- MORPH is ramped per sample across each block.

## Rendering

**Per block (64 samples):**
1. Refresh the plan if A or B changed.
2. Compute each operator's target gain: level, velocity, level modulation.
3. Compute each operator's increment from COARSE, FINE, DETUNE, transpose and pitch bend.
4. Pick each operator's mip level.

**Per sample,** for each operator in plan order:
1. `phase += inc`.
2. `pm = Σ w·in` (from the edge list), plus feedback.
3. `out = wave(phase + pm) × env × gain`.

The envelopes run **per sample** in `f32`: TX81Z rates as per-sample `f32` coefficients, computed once when a note or parameter changes. The TX81Z's fastest attack is about 12 samples, and a per-block envelope would smear it. Velocity sensitivity is applied once, in the gain, not in the envelope as well.

**No snapping:**
- A change to WAVE, ALG A or ALG B while notes sound ducks the affected output to zero over one block, swaps, then ramps back over the next block. That's a 2.7 ms dip, cheap and click-free.
- LEVEL and MORPH ramp across each block.

## Budget

- The voice chain costs about 220 cycles per voice (drive, filter, wavefolder and VCA; inferred from Pizza's measured 244 minus its oscillator of about 20).
- So each voice has 7,000 − 3,192 (FX) = 3,808 ÷ 6 ≈ 635 cycles, and the **Algo engine gets ≤ 350 cycles per voice per sample**, leaving margin.
- The bench gains a **worst-case Algo patch**: 6 operators, all with feedback, morph at 0.5 between A14 and A22, mip crossfades active, and all 6 voices. That patch's measurement is committed as `AlgoEngine::COST`.

**Retiring the risk first:** the plan starts with a kernel prototype (6 PM operators, `f32`, edge list, mip crossfade), benched on the chip before the rest is built. If it measures over 350 cycles, the fallbacks are, in order:
1. per-block kernel specialisation for the plan's shape;
2. dropping the mip crossfade for carriers above a pitch threshold;
3. 5 voices.

The operator tables for one voice's working set fit the 16 KB D-cache only if a voice uses at most about 3 distinct waves, so the bench patch uses 6 distinct waves to measure the worst case.

## UI (sub-project 1)

**Group pages:** on each page, encoders A–F edit operators 1–6.

| Page | Contents |
|---|---|
| WAVE | wave |
| COARSE | coarse ratio |
| FINE | FINE on A–F; a DETUNE sub-page |
| LEVEL | level |
| ENV | 5 sub-pages: AR, D1R, D1L, D2R, RR |
| FEEDBACK | feedback |
| ALGO | ALG A, ALG B, MORPH, transpose |

- The viz band shows the live output, as the existing CellGrid pages do. ALGO shows the A and B diagrams blended by MORPH.
- **Map:** the Algo chain has 3 top-level blocks, `OSC · ALG · (chain…)`, with WAVE, COARSE, FINE, LEVEL, ENV and FEEDBACK as OSC's sub-pages.
- **Sub-page navigation must reach every sub-page.** The map shows the current sub-page and its neighbours, scrolling. The existing two-row sub-page rendering gains scrolling if it lacks it. Map nodes must not overlap; a screen-golden test checks this.
- `FocusMemory` gets room for the new pages.
- Gang edit (MINUS+turn), SCALING and the per-operator detail pages are sub-project 4.

## Replacing the old engines

- `EngineType` becomes `{Algo, Modal}` and `ChainType` becomes `{Algo, Modal}`.
- **Deleted:**
  - `pizza.rs`, `engine_fm.rs`, `envelope_fm.rs`, `fm_tables.rs`, `fm_waveform.rs`;
  - `oscillator.rs` and anything only VA uses;
  - their param blocks and pages;
  - their goldens (all FM goldens, `pizza_lfo_cutoff`, `pizza_to_modal_switch`) and the FM `KNOWN_BROKEN` entries.

  The p81z-derived code's licence is unverified, so none of it is carried over. The Algo engine re-implements the TX81Z's published behaviour: the ratio table, the rate curves and the wave shapes.
- **Tests that used Pizza as their test engine** (about 83 references) move to the Algo init patch or to Modal, keeping each test's intent. Every change is listed in the plan.
- **Factory bank:** a new bank of 8 Algo Sounds:
  - 4 TX81Z-style patches (bass, e-piano, brass, bell);
  - 2 simple Pizza-role Sounds (saw lead, square bass);
  - 2 morph showcases.

  There are no user patches yet, so nothing needs migrating.

## Testing

**Host tests (pure core):**
- **Wave recipes:** each is finite with peak ≤ 1, and non-TX waves have no DC. Every mip level is band-limited, checked with a small DFT. The flash total stays in budget.
- **TX81Z tables:** the coarse ratios, the FINE steps and the 0.75 dB level step. Level 0 is silent.
- **The `f32` envelope:** stage timing across rates, attack no longer than 16 samples at AR 31, and no `f64` in the engine module.
- **Algorithms:** every one has operator 1 as a carrier, no self-links, and no modulator with a lower number than its target (the convention). T1–T8 equal the TX81Z routings.
- **`plan`:** forward order is respected, and an endpoint uses its own algorithm's order.
- **Morph:**
  - `MORPH = 0` renders bit-identical to A alone, and `MORPH = 1` bit-identical to B alone.
  - A sweep has no discontinuity above a set threshold.
- **Carrier normalisation:** equal loudness within ±1 dB across A1, T1 and A17 at unit levels.
- **The diagram layout:** it stays inside the band for all 32 algorithms.
- **`init_in_place`** equals `new` for `AlgoEngine`.

**The ADR 0011 sanity gate:**
- the init patch is pitched correctly (A4 = 440 Hz within 1 cent);
- a release tail decays;
- modulating `MORPH` and each `LEVEL` audibly changes the output (ADR 0010).

**Goldens** (recorded deliberately, ADR 0011):
- the init patch;
- a patch per T1–T8;
- a morph sweep;
- a 6-voice chord through the Instrument, recorded **after** `AlgoEngine::COST` is committed, so the allocator doesn't refuse it.

**On the chip:**
- the worst-case bench;
- the loopback test (MOTU input): pitch, no clipping, no clicks;
- the AUDIO page showing peak load under a 6-voice chord.

## ADRs

- **0022:** one algorithmic 6-operator engine replaces Pizza, FM and VA; Modal stays. This **supersedes ADR 0003**.
- **0023:** the waves come from our own recipes in `chimera-waves`. TX81Z W1–W8 are generated from their formulas, not dumped from ROM, and no p81z-derived code is carried over.
- **0024:** morph blends connection weights; endpoints use their own algorithm's order, and in between, links that run backwards read the previous sample.

## Out of scope (sub-projects 2–5 and later)

Sub-projects 2–5 cover:
- the FM, PWM, SYNC and PD modes;
- FIXED pitch;
- the full 64-wave set;
- gang edit, SCALING and the detail pages;
- the TX character path.

Later still: a custom algorithm editor, FS1R-style formant operators, user waves from SD, morph across more than 2 algorithms, and per-operator pan.

## Risks

- **CPU:** retired first by the kernel prototype and bench, with the fallbacks listed under Budget.
- **AXI headroom (13.6 KB):** compact `u8` parameters. The assertion fails the build if it's exceeded.
- **Test churn:** about 83 Pizza references plus goldens. Every changed test keeps its intent, and the plan lists each change.
- **Map and page count on this UI:** a screen-golden test for no overlap, and sub-page scrolling if needed.
