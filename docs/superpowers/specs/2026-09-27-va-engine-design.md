# VA Engine — Design

**Date:** 2026-09-27
**Status:** Draft rev 2 (after adversarial self-review), awaiting user review
**Tracks:** epic #118.
**Builds after:** the FX diet (#36), for its CPU headroom.
**Supersedes in part:** ADR 0022's engine-set clause (`EngineType` and `ChainType` are `{Algo, Modal}`) and its note that no engine puts the amp envelope on the VCA. The rest of 0022 stands; the old VA placeholder it removed stays removed.

## Intent

A third engine, **VA**, that is only an oscillator section. It feeds the Part's existing chain, DRV → FLT → FLD → MOD, and the map reads **VA · DRV · FLT · FLD · MOD**. It holds only sounds Algo and Modal can't make.

**Done when:**
- `EngineType` and `ChainType` are `{Algo, Modal, Va}`, and a VA Part plays all 11 models on the desktop and the chip;
- the VA engine lives in the per-voice engine slot, and the voice pool is no bigger than before;
- the chip bench has one row per model, and each model's reading, minus `Voice::CHAIN_COST`, is committed in `VaEngine::COST`;
- every model gets 6 voices beside the FX bus on rev V and on rev Y;
- the host tests below pass, and the goldens are recorded after the sanity gate;
- the user has heard the loopback takes.

## Principles (binding)

- **Only what the other engines can't make** (ADR 0033). Plain PWM and generic hard sync are left out: Algo's modulation modes (#38) cover them. The P5 model's own sync is the exception, because it belongs to that model's poly-mod sweep.
- **One model per voice**, in the style of the Modal Cobalt8. Each model has three macros, A, B and C, and all three are modulation destinations.
- **Functional core, imperative shell.** Shape builders, BLEP and BLAMP residuals, macro mappings, detune tables, the noise filters' coefficients and the pitch maths are pure, host-tested functions. `VaEngine` is the per-block and per-sample loop.
- **`f32` only in the render path.** No `f64`, no libm per sample, no heap, no wavetables. Divides happen only per block or per waveform event (at most a few per cycle), never per sample. Trig uses the existing `sin_turns`, and `2^x` uses `algo::math::exp2`. The engine source scan (`engine_source_test.rs`) extends to `dsp/va/`.
- **Band-limited by construction.** polyBLEP corrects every step, and polyBLAMP corrects every corner. Every non-sine shape is built from linear segments, so the renderer knows each discontinuity exactly. The crusher and the noise grain alias on purpose, and they act after the band-limited oscillator.
- **Nothing snaps** (CLAUDE.md). This is detailed under Rendering.
- **Each family can be removed on its own.** A family is one module plus its `VaModel` variants, and no other family names it.

## Voice model

**`VaParams`**, the Sound's VA block (`BlockRef::Va`, voice-read, so `voice_reads` is true):

| Id | Param | Stored | Range | Display | Destination |
|---|---|---|---|---|---|
| 0 | MODEL | `u8` | 0–10, `VaModel` | `Names`, the model's short name | no |
| 1 | A | `f32` | 0–1, step 1/128 | `Uni`, labelled by model | yes |
| 2 | B | `f32` | 0–1, step 1/128 | `Uni`, labelled by model | yes |
| 3 | C | `f32` | 0–1, step 1/128 | `Uni`, labelled by model | yes |
| 4 | OCT | `i8` | −2…+2 | `Signed(2)` | no |
| 5 | FINE | `i8` | −100…+100 cents | `Signed(100)` | no |

- A, B and C are stored as `f32`, like Modal's, so a mod offset from `apply_offset` isn't rounded, and the macros don't zipper. The block is 16 bytes. `ParamSnapshot` grows by that much, which costs about 800 B across the pool and buffers (algo spec § Storage). The `AXI_RESIDENT` assertion checks it.
- **The note's frequency** is `f0 = 440 · 2^((note + 12·OCT + FINE/100 − 69) / 12)`, computed per block with `exp2`. It sets `inc = f0 / fs` (multiplied by the per-block `1/fs`). Every oscillator's increment is clamped to ≤ 0.45.
- **`VaModel`** is a `#[repr(u8)]` enum with fixed discriminants 0–10, in the order of the model table. An unknown stored byte reads as `Sweep`.
- **The amp envelope is the VCA for a VA Part.** VA is only an oscillator, so something must end its notes. On `EngineType::Va`, `Voice::render` multiplies the output by the amp envelope's per-sample value, which it already computes for the ENV mod source. The voice is active while the amp envelope is not idle. Algo and Modal are unchanged. The amp envelope's parameters stay off the destination list (ADR 0010).
- **The engine slot.** Today `Engines` holds `algo` and `modal` side by side in every voice. Because a voice sounds only one engine at a time (`note_on` fades before it switches), `Engines` becomes one slot: a `union` of the three engines, tagged by the voice's `active_engine`, and sized to the largest engine, Modal. `trigger` builds the new engine in place when the engine changes, replacing today's reset of the old one. `const` assertions keep `size_of::<VaEngine>()` no larger than the slot, and keep the slot within the existing `VOICE_RAM_BUDGET`. The voice pool shrinks by the size of `AlgoEngine` per voice, more than the 16 B that `played: ParamSnapshot` grows.
- **The `VaEngine` state target is ≤ 512 B.** Its largest part is Spread's 7 oscillators: phase, increment, drift state and BLEP carry for each.

## Shared definitions

These apply to every model.

**Classic shapes** on phase φ ∈ [0, 1), aligned so that any two can be blended:

| Shape | Definition |
|---|---|
| SINE | −cos 2πφ, which is `sin_turns(frac(φ + 0.75))` |
| TRI | −1 at φ 0, +1 at 0.5, linear between |
| SAW | 2φ − 1 |
| SQUARE | −1 on [0, 0.5), +1 on [0.5, 1) |
| PULSE(w) | raw +1 on [1 − w, 1), −1 elsewhere; then DC-removed and peak-normalised: `(raw − (2w − 1)) / (2·max(w, 1 − w))` |

- `blend(X, Y, x) = (1 − x)·X + x·Y`. The blend of two piecewise-linear shapes is piecewise linear, with the union of both shapes' breakpoints, so one segment walker renders it (§ Rendering). SINE is the one smooth shape: it is computed directly and blended with the walker's output, and it needs no correction.
- **Sums are peak-safe:** a model that sums oscillators outputs `Σ gⱼ·oⱼ / Σ gⱼ`, so |y| ≤ 1. The reciprocal is computed per block.
- **SUB** (on Sweep, PWM Dual, Saw Eraser and Triangle Pinch): a ±1 square at f0/2. It toggles at each wrap of the model's first oscillator and is BLEP-corrected at the same instant. It is mixed as `(main + C·sub) / (1 + C)`. C's curve is linear, and C = 1 gives equal parts.
- **Start phases:** the first oscillator of every model starts at φ = 0. Every other oscillator starts at a phase drawn from the note seed (§ Rendering).

## Models

Each model's row gives A, B and C with their exact mapping. "Pitch macros" are the values the pitch test uses: all 0, except Triangle Pinch B, which is 0.5.

### VA Sweep (`sweep`)

| Macro | Label | Mapping |
|---|---|---|
| A | SHAPE | Four equal zones, each a linear blend. From 0 to 0.25, SINE → TRI; from 0.25 to 0.5, TRI → SAW; from 0.5 to 0.75, SAW → SQUARE; from 0.75 to 1, PULSE with w from 0.5 to 0.08, linear. |
| B | SPREAD | A second oscillator, same shape. Its gain is `g₂ = min(1, 20·B)`. From 0 to 0.5, detune is `+50·(B/0.5)²` cents. Past the middle, four equal zones snap to exact intervals: +700 cents (5th) up to 0.625, +1200 (octave) up to 0.75, +1900 (octave + 5th) up to 0.875, +2400 (2 octaves) up to 1. |
| C | SUB | SUB, as defined above. |

The output is `(o₁ + g₂·o₂ + C·sub) / (1 + g₂ + C)`.

### VA Crushed (`crush`)

| Macro | Label | Mapping |
|---|---|---|
| A | SHAPE | From 0 to 0.5, SINE → TRI; from 0.5 to 1, TRI → SAW; linear blends. |
| B | BITS | `bits = 16 − 14·B`, continuous. The step is `q = 2^(1 − bits)`, per block. Quantisation is mid-tread: `round(x/q)·q`, rounded with an `as i32` conversion. |
| C | RATE | Sample and hold at `f_h = fs · 2^(−log₂(48)·C)`, which is 48 kHz → 1 kHz, exponential. A phase accumulator with `r = f_h/fs`: when `acc += r` reaches ≥ 1, it takes `acc −= 1` and a new sample. At C = 0, r = 1, which is an exact bypass. |

The order is oscillator (band-limited) → RATE → BITS.

### Spread Saw, Spread Square, Spread Triangle (`spread`)

- 7 oscillators of one shape (SAW, SQUARE or TRI), at frequencies `f0 · (1 + rᵢ·D)`.
- `r = [−0.110 023 13, −0.062 884 39, −0.019 523 56, 0, +0.019 912 21, +0.062 165 38, +0.107 452 42]`, the supersaw's measured detune offsets (Szabo 2010). Oscillator 3 is the centre.

| Macro | Label | Mapping |
|---|---|---|
| A | MIX | The centre's gain is 1, and each of the six sides has gain A. So A = 0 is the fundamental alone, and A = 1 is all 7 equal. |
| B | SPREAD | From 0 to 0.75, `D = 0.05 + 0.95·(B/0.75)²`, tight → wide. From 0.75 to 1: D = 1 for five oscillators, and oscillators 1 and 5 play an octave up at `2·f0·(1 + rᵢ·E)`, with `E = (B − 0.75)/0.25`. So the pair starts at a pure octave and widens across the zone. |
| C | DRIFT | Each oscillator's detune wanders by `dᵢ · 15·C²` cents, with dᵢ ∈ [−1, 1]. dᵢ glides linearly, per block, toward a random target. A new target is drawn when the glide arrives. Each oscillator's glide time is fixed per note from the seed, between 0.4 and 0.9 s. |

- All 7 start phases come from the note seed (the centre too: supersaw practice).
- The output is `Σ gᵢ·oᵢ / Σ gᵢ`.

### PWM Dual (`pwm`)

| Macro | Label | Mapping |
|---|---|---|
| A | WIDTH | A pulse over a 2-cycle period. Even cycles have width `0.5 + 0.45·A`, and odd cycles `0.5 − 0.45·A`. The levels are ±1, and the 2-cycle mean is 0 at every A. |
| B | SPREAD | A second oscillator of the same shape. `g₂ = min(1, 20·B)`, and detune `+50·B²` cents. |
| C | SUB | SUB. It toggles at each cycle start, so it is aligned with the 2-cycle period. |

### PWM Tri/Square (`pwm`)

A trapezoid on each cycle. With ramp fraction r: from 0 to r/2 it ramps −1 → +1; up to 0.5 it holds +1; from 0.5 to 0.5 + r/2 it ramps +1 → −1; then it holds −1. It is symmetric, so it has no DC.

| Macro | Label | Mapping |
|---|---|---|
| A | TRI W | r = A on even cycles. A = 0 is SQUARE, and A = 1 is TRI. |
| B | ASYM | r on odd cycles is `A + B·(1 − 2A)`. So B = 1 gives odd cycles the inverse width, 1 − A. At A = 0.5, ASYM has no effect. |
| C | SLEW | A one-pole low-pass, `s += k·(x − s)`, key-tracked. `fc = f0·2^(6·(1 − C))`, which runs from 64·f0 to f0. `k = 1 − (1 − k(fc))·min(1, 8·C)`, where `k(fc) = 1 − 2^(−2π·fc·log₂e / fs)`, per block. At C = 0, k = 1, which is an exact bypass. |

### PWM Saw Eraser (`pwm`)

Each cycle has a saw part, `s = 1 − 0.9·A` long, rising −1 → +1. Then comes a pulse part, 1 − s long: +1 for the first B·(1 − s), then −1.

| Macro | Label | Mapping |
|---|---|---|
| A | RATIO | As above. A = 0 is SAW, and A = 1 is 10 % saw and 90 % pulse. |
| B | WIDTH | The pulse part's high fraction, 0–1, linear, relative to the part A leaves. |
| C | SUB | SUB. |

- The DC `m = (1 − s)·(2B − 1)` is removed, and the result is scaled by `1/(1 + |m|)`.
- Both are computed per cycle, with the latched shape.

### PWM Triangle Pinch (`pwm`)

Each cycle has a triangle `w` long: −1 at 0, +1 at w/2, −1 at w. Then it holds −1.

| Macro | Label | Mapping |
|---|---|---|
| A | PINCH | Even cycles have `w = 1 − 0.95·A`. A = 0 is TRI. |
| B | ASYM | Odd cycles have `w_odd = clamp(w · 2^(4·B − 2), 0.0125, 1)`: ¼ as wide at B = 0, the same at B = 0.5, and 4× as wide at B = 1. It shows as 0–127, with 64 as neutral. |
| C | SUB | SUB. |

- The DC is removed over the 2-cycle period, where the mean is `−(1 − w̄)`, with w̄ the mean of the two widths.
- The result is scaled by `1/(2 − w̄)`, so the peak is ≤ 1. After the DC is removed the range is [−w̄, 2 − w̄].

### Noise (`noise`)

A white source feeds GRAIN, then COLOUR, then RES. The source is xorshift32, mapped to [−1, 1) by bit tricks.

| Macro | Label | Mapping |
|---|---|---|
| A | COLOUR | Two one-poles, always running. A dark low-pass: `k_d = 1 − 2^(−2π·f_d·log₂e/fs)`, with `f_d = 80·2^(8·A/0.5)` Hz. From A 0.4 to 0.5, k_d blends linearly to 1, so it reaches an exact pass at 0.5 without a step, and it stays 1 above 0.5. It is followed by a bright high-pass, `y = x − s_b`: `f_b = 5` Hz up to 0.5 (a DC blocker), then `5·2^(10·(A − 0.5)/0.5)` Hz, which is 5 Hz → 5.1 kHz. Level compensation is `min(4, √((2 − k_d)/k_d))`, which restores the low-pass's variance. |
| B | RES | A TPT state-variable band-pass (peak gain 1) at the key: f0, clamped to ≤ 16 kHz, with `g = fast_tan(π·f/fs)` per block. `Q = 0.5·2^(8.5·B)`, which is 0.5 → 181. Its output is scaled by `min(64, √(Q·fs/(π·f)))`, which equalises its variance with a white input's (the band-pass's noise bandwidth is π·f/(2Q)). The mix is `(1 − B)·colour + B·bp`. |
| C | GRAIN | Sample and hold before COLOUR, `f_h = fs · 2^(−7.3·C)`: 48 kHz → about 300 Hz, the same accumulator as RATE. C = 0 is an exact bypass. |

- The output is scaled to 0.25 RMS for white noise, and then clamped to ±1 as a guard. The clamp is reached only past 4σ.
- Noise has no pitch, except through RES.

### P5 (`p5`)

Two oscillators, A and B, mixed equally: `(o_A + o_B)/2`. B sits exactly 7 cents sharp.

| Macro | Label | Mapping |
|---|---|---|
| A | OSC A | Saw level `s` and pulse level `p`. From 0 to ⅓: s = 1, p = 3A, pulse width 0.5. From ⅓ to ⅔: s = 1 − 3(A − ⅓), p = 1. From ⅔ to 1: s = 0, and the width goes 0.5 → 0.1, linear. `o_A = (s·SAW + p·PULSE(w)) / (s + p)`. |
| B | OSC B | From 0 to 0.4, SAW → TRI; from 0.4 to 0.8, TRI → SQUARE; from 0.8 to 1, PULSE with w going 0.5 → 0.15. |
| C | PMOD | Poly mod: osc B → osc A pitch, exponential FM. `inc_A = inc_A₀ · 2^(3·C² · o_B)`, with o_B being B's most recent band-limited output (the carry form's emitted sample). `exp2` runs per sample, and the increment is clamped to ≤ 0.45. From 0.75 to 1, A is also hard-synced to B, and the FM sweep continues. |

- **Sync** resets A's phase at each of B's wraps, at its exact sub-sample time.
- The jump `A(0) − A(φ_A)` is BLEP-corrected there.
- A's shape is re-latched there, as at any cycle start.
- The filter-envelope poly-mod source is out of scope: the mod matrix can route an envelope to C.

## Rendering

**Band-limiting.**
- Residuals are 2-point, in the one-sample-latency carry form.
- For an event t samples before the current sample n (t ∈ [0, 1)):
  - a **step** of Δ adds `Δ·t²/2` to sample n−1 and `−Δ·(1 − t)²/2` to sample n (polyBLEP, Välimäki and Huovilainen 2007);
  - a **corner** whose slope changes by Δs (value per sample) adds `Δs·t³/6` to n−1 and `Δs·(1 − t)³/6` to n (polyBLAMP, the integral of the same kernel; Esqueda, Välimäki and Bilbao 2016).
- `t = (φ − breakpoint) · (1/inc)`, with 1/inc computed per block. Only P5's A, whose increment moves per sample under FM, divides, and it does so once per event.
- Every oscillator in a model shares the one-sample latency, so their sums stay aligned.

**The segment walker** (`seg`).
- Every non-sine shape is a list of at most 8 breakpoints over a period P of 1 or 2 cycles. Each breakpoint carries a value and a slope.
- Per sample: advance φ, evaluate the current segment's value at φ, and process **every** breakpoint crossed during the sample, in time order.
- At each breakpoint, Δ is the value after minus the value before, and Δs is the slope after minus the slope before, times inc. Both come from the latched shape, so two events in one sample, or edges closer than 2 samples, superpose correctly.
- A ramp shorter than half a sample is rendered as a step at its midpoint. This is the limit of its two corners, and it avoids huge slopes.
- The Spread models use dedicated fast paths (SAW: one BLEP; SQUARE: two BLEPs; TRI: two BLAMPs, with Δs = ±8·inc), because the generic walker costs more per oscillator. They must match the walker's output to 1e-5, and a test checks this.

**When things change.**
- **The macros** (after modulation) ramp linearly across each block from the previous block's values.
  - Gains, SUB and crush depth use the ramped value per sample.
  - Increments (SPREAD, DRIFT, OCT, FINE, PMOD's base) change per block. A frequency step moves the slope only, never the value, so it can't click.
- **Shapes latch per cycle.** At each period start, the walker rebuilds its breakpoints from the ramped macros at that sample. The same happens after a P5 sync reset. A width change mid-cycle therefore never skips or doubles an edge. The period-start event's Δ spans the old shape's end and the new shape's start, so the change itself is band-limited.
- **MODEL** changes while a note sounds use Algo's duck: the output ramps to 0 over one block, the model swaps, and the output ramps back over the next.
- **Retriggering.** A note-on to a sounding VA voice keeps its phases and drift, and only the amp envelope retriggers. Only a note from silence reseeds.

**The note seed.**
- It is `splitmix32(note | velocity << 8 | count << 16)`, where `count` is the engine's note-on count since it was built, as a wrapping `u16`.
- Two fresh engines given the same notes therefore render bit-identically, and two notes from silence get different phases.
- It seeds Spread's start phases and drift times, the second oscillators' start phases, and Noise's xorshift state.

## Budget

- On rev V the voice budget is 7,000 per sample. After the FX diet's 1,000-cycle bus, that leaves (7,000 − 1,000)/6 = 1,000 per voice; on rev Y it is (5,833 − 1,000)/6 = 805.
- The per-voice estimates below are paper counts for the engine part alone, including the VCA multiply. The bench decides.

| Model | Estimate | Why |
|---|---|---|
| Sweep | 40–60 | a walker, plus a sine below A 0.25, plus a second walker when B > 0, plus SUB |
| Crushed | 35–45 | a walker, a sine, S&H and quantise |
| Spread Saw | 100–120 | 7 × ~10 in fast paths, oscillator-major over the block |
| Spread Square, Spread Triangle | 110–140 | 7 × ~12 (two edges or corners per cycle) |
| PWM Dual | 40–50 | two walkers and SUB |
| PWM Tri/Square, Saw Eraser, Triangle Pinch | 30–40 | one walker, plus SUB or SLEW |
| Noise | 30–40 | RNG, S&H, two one-poles and an SVF |
| P5 | 60–80 | two walkers, `exp2` per sample, and sync events |

- The worst is about 150 per voice, plus `CHAIN_COST`. So every model fits 6 voices beside the bus on both revisions.
- **The per-cycle latch** costs about 40 cycles per period, so it matters only at high notes: about 4 per sample at C8. The bench rows play high notes to include it.
- **Billing:** `VaEngine::cost(&VaParams) = COST[model]`, one committed constant per model. It goes through `Engines::cost`, the per-patch path Algo uses (ADR 0026). MODEL isn't a destination, so a patch's cost is exact.
- **Bench:** one row per model (11 rows) at the macros below, with 6 voices from note 72 in steps of 5.

  | Model | A, B, C |
  |---|---|
  | Sweep | 0.2, 0.3, 1 |
  | Crushed | 0.25, 1, 0.5 |
  | Spread models | 1, 1, 1 |
  | PWM Dual | 1, 1, 1 |
  | Tri/Square | 0.5, 1, 1 |
  | Eraser | 0.5, 0.5, 1 |
  | Pinch | 0.5, 1, 1 |
  | Noise | 0.25, 1, 0.5 |
  | P5 | 0.5, 0.6, 1 |

  Each model's reading minus `CHAIN_COST` is committed as `COST[model]`, with the usual `// measured <date>, bench, rev V at 480 MHz` comment.

## UI

- **The map:** the VA chain is **VA · DRV · FLT · FLD · MOD**. VA is the engine's home: the first node, and where entering the chain lands. It has no sub-pages.
- **The VA page** (`BlockDef` id: the next free one at build time, 59 today; short `VA`; `CellGrid`, live-output viz band as other CellGrid pages) has these cells:

  | MODEL | A | B | C | OCT | FINE |
  |---|---|---|---|---|---|

  LEVEL isn't on the page: the Part mixer has it.
- **Model-specific labels:** A, B and C show `VaModel::macro_labels()`:

  | Model | A | B | C |
  |---|---|---|---|
  | VA Sweep | SHAPE | SPREAD | SUB |
  | VA Crushed | SHAPE | BITS | RATE |
  | Spread Saw, Spread Square, Spread Triangle | MIX | SPREAD | DRIFT |
  | PWM Dual | WIDTH | SPREAD | SUB |
  | PWM Tri/Square | TRI W | ASYM | SLEW |
  | PWM Saw Eraser | RATIO | WIDTH | SUB |
  | PWM Triangle Pinch | PINCH | ASYM | SUB |
  | Noise | COLOUR | RES | GRAIN |
  | P5 | OSC A | OSC B | PMOD |

  Every label is ≤ 6 characters, the length of the longest existing label (`INHARM`). The screen goldens check the fit.
  - `ParamSlot` labels are `&'static str` today, so the skeleton adds a label function for a slot, resolved against the Sound's blocks. The mod matrix names the destinations A, B and C with the same labels.
- **MODEL names** (`Names`, ≤ 8 characters, checked by the screen goldens): SWEEP, CRUSH, SPR SAW, SPR SQR, SPR TRI, PWM DUAL, TRI/SQR, ERASER, PINCH, NOISE, P5.
- `ChainType::Va` has the label "VA". `chain.rs` maps it to `VA_CHAIN`, and `FocusMemory` gains its page.

## Code layout

```
chimera-core/src/dsp/va/
  mod.rs      VaModel, VaParams (Block impl), macro_labels, pitch (f0, inc)
  blep.rs     BLEP and BLAMP residuals, the carry form
  seg.rs      breakpoint shapes, the walker, classic shapes and blends
  sweep.rs    VA Sweep
  crush.rs    VA Crushed
  spread.rs   the three Spread models and their fast paths
  pwm.rs      PWM Dual, Tri/Square, Saw Eraser and Triangle Pinch
  p5.rs       P5
  noise.rs    Noise
  engine.rs   VaEngine: note seed, macro ramps, MODEL duck, dispatch
```

- Each family module exports a pure shape builder (macros → breakpoints and gains) and a render function over the shared state.
- Removing a family means deleting its module, its `VaModel` variants and their match arms in `engine.rs` and `macro_labels`.
- `EngineType::ALL`, `ChainType::ALL`, `BlockRef`, `Blocks for ParamSnapshot`, `Engines`' matches and `voice_reads` each gain a `Va` arm. The compiler lists them.

## Testing

**Host tests.** Pure functions run directly. Renders are short, at the test profile's default opt-level.

- **Aliasing,** per model, at C6 (1046.50 Hz):
  - Render 2¹⁵ samples after a 2¹² settle, with a Blackman–Harris 4-term window.
  - The masked bins are those within ±4 bins of every multiple of each oscillator's period frequency: f0, or f0/2 for 2-cycle shapes, and SUB's f0/2. So a detuned sum (Sweep's SPREAD, PWM Dual's second oscillator, Spread's seven, P5's pair) masks each oscillator's own harmonic series. At C6, 48 kHz / f0 = 45.87, so each fold lands about 139 Hz from the nearest harmonic, well outside the mask.
  - The energy left between 20 Hz and 10 kHz is folded energy. It must be ≤ −45 dB relative to the total. Where the uncorrected render folds more than −60 dB (every case but a pure sine), it must also be ≥ 12 dB below the same shape rendered with corrections off (a test-only `Correction::Off` in the pure core), so the gate can't pass by accident.
  - It runs at every combination of A, B and C in {0, 0.5, 1}, except as follows.
  - Exempt or pinned, by design: Noise is exempt; Crushed runs at B = C = 0 only, since its crush stages alias on purpose; P5 runs at C = 0 only, since FM sidebands are unbounded (its sync has its own test below); Spread runs at C = 0 only, since DRIFT moves the frequencies the mask needs.
- **Pitch within 1 cent:**
  - The pure `pitch` function over OCT −2…+2, FINE −100, −37, 0, +100 and notes 21–108, checked against an `f64` reference in the test.
  - Rendered: every pitched model at A4 with its pitch macros. The nearest partial is measured by a 2¹⁷-point Hann DFT with quadratic interpolation on the log magnitude (bias under 0.01 bin, 0.004 Hz, which is inside 1 cent even at A0's 0.016 Hz).
  - Rendered: Sweep at OCT ±2 and FINE ±100, at A2 and A6.
  - For P5, both A and B's peaks are measured, with B at +7 ± 1 cents.
- **Each macro changes the output:** for every model, macro X at 0 and at 1, with the other two at 0.3, over 1 s from the same seed. `rms(y₀ − y₁) ≥ 0.1·rms(y₀)`. The 0.3 avoids the points where a macro is neutral by design (Tri/Square ASYM at A = 0.5, Saw Eraser WIDTH at A = 0).
- **Sweeping a macro doesn't click:** each macro goes 0 → 1 over 100 ms, per block as modulation does, at A4, for every model.
  - The largest second difference `|x[n] − 2x[n−1] + x[n−2]|` must be ≤ 1.5× the largest in held renders at 0, 0.25, 0.5, 0.75 and 1 (the FX-diet criterion; the crushed and noise models step on every sample by design).
  - A MODEL change mid-note passes the same test.
- **P5 sync doesn't step:**
  - C swept across 0.75 passes the click test.
  - The pure sync kernel, with FM depth 0 and A at 2.37× B's frequency, passes the aliasing gate at C6.
- **Spread seeds are deterministic:** two fresh engines given the same note sequence are bit-identical. Two notes from silence on one engine get different start phases.
- **Noise resonance tracks the key:** at B = 1, notes A3, A4 and A5 and OCT +1, the Welch PSD peak is within 10 cents of f0.
- **Walker unit tests:**
  - every classic shape and blend from the walker equals its closed form, away from events;
  - two events in one sample equal the sum of each alone;
  - the Spread fast paths equal the walker to 1e-5;
  - every shape's mean over its period is 0 within 1e-4, and its peak is ≤ 1.
- **Slot:** `init_in_place` equals `new` for `VaEngine`; switching a voice Algo → Va → Modal → Algo mid-note fades and restarts cleanly (`engine_switch_test`); the size assertions build.
- **No `f64` or libm** in `dsp/va/`, via the source scan.

**The ADR 0011 sanity gate,** for each model's init (Sweep's is A 0.5, B 0, C 0):
- it is finite, within ±1 and audible;
- it is silent after note-off, through the amp envelope VCA;
- pitched models are within one semitone.

**Goldens** (ADR 0011), recorded after the gate:
- each model holding A3 for 1 s at A = B = C = 0.3;
- a 6-voice Sweep chord through the Instrument, recorded after `VaEngine::COST` is committed.

**Screen goldens:**
- the VA page with Sweep, P5 and Noise selected, since the labels change;
- the VA chain's map.

**On the chip:**
- the bench rows;
- loopback takes (MOTU): pitch, no clipping, no clicks, one per model.

## ADRs

- **0033 (new): VA holds only what the other engines can't make.**
  - The principle, and why plain PWM and hard sync are left to Algo's modes (#38), with P5's sync as the exception.
  - The 11 models and their three macros.
  - The band-limiting method (polyBLEP, polyBLAMP, segment shapes, no wavetables) and its provenance. The code is our own, from the cited papers and Szabo's published supersaw measurements; no Mutable or other synth code is used (ADR 0002).
  - The amp envelope as the VA Part's VCA.
  - The shared engine slot.
  - It supersedes ADR 0022's `{Algo, Modal}` clause and its "no engine puts the amp envelope on the VCA" note.
  - 0028–0031 are reserved by the FX diet, and 0032 exists.
  - It goes in `docs/adr/README.md` with the template. It isn't written until the build starts.

## Out of scope

- Generic PWM and hard-sync models.
- Unison beyond Spread.
- Stereo voices.
- The P5 filter-envelope poly-mod source.
- Wavetables.
- A factory bank of VA Sounds, which comes after the build, by ear.
- Model-specific value display, such as SPREAD showing "5th". A, B and C show 0–127.

## Risks

- **2-point polyBLEP misses the −45 dB gate** on narrow pulses or Spread's octave pair at C6: bring the numbers to the user. The remedy is 4-point residuals (about +30 % per oscillator), not a looser gate.
- **The union slot** changes engine switching for Algo and Modal too: `engine_switch_test` and the fade tests must pass unchanged. If the slot proves unsafe to build, the fallback is a separate `va` field, costing about 3 KB of D2. Bring that to the user, since it breaks "voice RAM doesn't grow".
- **Loudness varies across models and macros,** because sums are peak-safe (Spread at MIX 1 is about 8.5 dB below MIX 0). The Part mixer compensates. Revisit after the loopback takes.
- **P5's per-sample `exp2`** pushes it past 80: use a cubic `2^x` over the bounded FM range (±3 octaves, 0.1 % error).
- **Per-cycle latch cost at high notes:** the bench's high rows measure it. If it matters, cache the breakpoints while the macros hold still.
- **Noise's RES normalisation** at high Q and low f hits the ×64 cap and gets quieter: accepted, documented.

## Defaults chosen

These values were chosen here, not by the user. Review them.

1. **Sweep SHAPE:** four equal zones; the pulse narrows 0.5 → 0.08, linear.
2. **Sweep SPREAD:**
   - the second oscillator fades in over B 0–0.05;
   - free detune is +50 cents max, quadratic, and only oscillator 2 moves;
   - the interval zones are 0.125 wide and exact, with no added beating.
3. **SUB:** mixed as `(main + C·sub)/(1 + C)`.
4. **Peak-safe sums** everywhere (`Σg·o/Σg`); no loudness compensation across SHAPE, so narrow pulses are quieter.
5. **Crushed:** BITS is 16 → 2, linear in bits, mid-tread; RATE is 48 kHz → 1 kHz, exponential; RATE comes before BITS.
6. **Spread:**
   - Szabo's seven offsets;
   - the centre stays at gain 1 and the sides get A;
   - SPREAD's tight end is D = 0.05, quadratic to D = 1 at 0.75;
   - the octave pair is oscillators 1 and 5, pure at 0.75, widening to D = 1;
   - DRIFT is ±15 cents at C = 1 (quadratic), with 0.4–0.9 s glides.
7. **PWM Dual:** widths 0.5 ± 0.45·A; SPREAD +50·B² cents.
8. **Tri/Square:** a trapezoid; ASYM moves the odd cycle toward 1 − A; SLEW is a one-pole from 64·f0 down to f0.
9. **Saw Eraser:** the saw part runs from 100 % to 10 %; WIDTH is the high fraction of the pulse part.
10. **Triangle Pinch:** the triangle narrows to 5 %; ASYM is ×¼ … ×4 around 0.5, and so neutral at 0.5.
11. **Noise:**
    - COLOUR runs dark 80 Hz → white at 0.5 → bright 5.1 kHz high-pass, compensated up to +12 dB, with the low-pass blending to an exact pass over A 0.4–0.5;
    - RES is Q 0.5 → 181, with variance-matched gain capped at ×64;
    - GRAIN is 48 kHz → 300 Hz;
    - the output is 0.25 RMS with a ±1 guard.
12. **P5:**
    - B is exactly +7 cents;
    - OSC A's zones are thirds, and its pulse narrows to 0.1;
    - OSC B goes saw → tri → square → pulse 0.15;
    - PMOD depth is 3·C² octaves;
    - sync engages at C ≥ 0.75;
    - the modulator is B's band-limited output.
13. **Pitch:** every increment is clamped to ≤ 0.45 cycles per sample; FINE is stored as integer cents.
14. **Start phases:** the first oscillator starts at 0, and the others are seeded. The seed mixes note, velocity and the engine's note count.
15. **Macros:** they ramp per block; shapes latch per cycle; a note-on to a sounding voice keeps its phases.
16. **UI:** the next free block id (59 today); the labels and MODEL names above; values show 0–127.
17. **Tests:**
    - the aliasing gate is −45 dB below 10 kHz at C6, plus ≥ 12 dB better than naive;
    - "audible" means `rms(Δ) ≥ 10 %` with the other macros at 0.3;
    - Noise's key tracking is within 10 cents.

## References

- V. Välimäki, A. Huovilainen, "Antialiasing oscillators in subtractive synthesis", IEEE Signal Processing Magazine, 2007 (polyBLEP).
- F. Esqueda, V. Välimäki, S. Bilbao, "Rounding corners with BLAMP", DAFx-16, 2016 (polyBLAMP).
- A. Szabo, "How to Emulate the Super Saw", 2010 (the supersaw's detune offsets).
- V. Zavalishin, *The Art of VA Filter Design* (the TPT state-variable filter).
