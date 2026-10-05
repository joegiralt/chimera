# Filter Types: a Cascadia Home Filter, PROPHET and DIRTY: Design

Status: draft for the owner's approval. Docs only; no code is written until it is approved.
ADR: [0096](../../adr/0096-three-filter-types-cascadia-home_prophet-dirty.md) (Proposed).
Builds on the filter-routing spec ([2026-09-27-filter-routing-design.md](2026-09-27-filter-routing-design.md)) and ADR 0037. It answers private issue #128 for these three types, and redirects private issue #126 from the Rev 3 CEM3320 to the Rev 1/2 SSM2040.

**Implementation starts after `modal-stagger` merges.** That branch may replace `filter.rs`'s `saturate()` with stmlib's rational soft clipper. This work inherits whatever `saturate` it lands, and measures its budgets on it (§ Cost).

## Goal

- The SVF becomes the **home filter**, with the Intellijel Cascadia VCF's UX: its controls, its words and its modes, each mode doing what the Cascadia's does. The ZDF/TPT state-variable core stays. That core is the sound the owner likes.
- PHZR makes two notches that sweep with FREQ, and NT2 puts its notch an octave above FREQ, as the Cascadia's do. Today PHZR is silent as an effect and NT2's notch sits at FREQ.
- Two new TYPEs, each a "machine" with its own modes and panel:
  - **PROPHET** is the SSM2040 of the Prophet-5 Rev 1 and Rev 2.
  - **DIRTY** is a Chamberlin SVF: cheap, digital, and edgy near Nyquist.
- Each type and mode has a static instruction budget and is billed in the voice cost model. No `tan` runs per sample: coefficients are computed per block and interpolated per sample.

## The owner's words (2026-10-05)

> The "home filter" aims for Intellijel Cascadia's VCF UX. Keep today's clean ZDF/TPT state-variable filter, which "reminds me of a 101". The owner loves that it's state-variable, because it "allows you to move very very easily".

> Known bug: PHZR does nothing audible. Specify a proper double-notch phaser … It must sweep well with FREQ. Also verify NT2 against the Cascadia's "notch about an octave above cutoff".

> A PROPHET filter type: the SSM2040, as in the Prophet-5 Rev 1 and Rev 2 … four cascaded OTA-style one-pole stages, per-stage soft saturation, global resonance feedback, and the SSM2040's characteristic bass loss as resonance rises.

> A DIRTY filter type: the Chamberlin SVF … cheap, digital and edgy near Nyquist … bounded so it can't blow up.

> Cost and memory are expensive … The owner prefers "machines" that nudge you, so each type exposes the modes and labels that make sense for it.

## What exists today

| Claim | Evidence |
|---|---|
| One ZDF/TPT SVF (Simper/Zavalishin form). `saturate` acts on both integrator states. | `dsp/filter.rs` `tick_svf_nonlinear` |
| `g = fast_tan(π·fc/fs)` once a block, ramped linearly per sample. `k = 2(1 − RES) + 0.01` is **not** ramped. | `SvfFilter::process`, `g_at` |
| Each sample divides by `1 + k·g + g²` (every 2-pole stage), and LP6 divides by `1 + g`. | `tick_svf_nonlinear`, `tick_onepole` |
| `fast_tan` is a 5th-order Taylor series. A 20 kHz cutoff reads as about 18.3 kHz. | `dsp/mod.rs:115`, private issue #57 |
| **PHASER** outputs `ap2` alone, two SVF all-passes with no dry signal. That has unit magnitude, so there are no notches: it is a phase shift you can't hear on a held note. | `tick`, `FilterMode::Phaser` |
| **NOTCH** is `lp + hp` of one SVF, a notch exactly **at** FREQ, and RES narrows it. The Cascadia's NT2 notches **an octave above** FREQ, and Q makes a peak at FREQ. | `tick`; Cascadia manual § 8.7 |
| **LP6** is a plain one-pole and ignores RES. The Cascadia's LP1 "becomes emphasized … until it self oscillates". | `tick_onepole`; manual § 8.7 |
| The filter's DRIVE is a pre-gain `1 + 4·DRIVE` into the SVF, with no clip and no indicator. | `process` |
| The panel is SVF: CUTOFF · RES · MODE · ENV · KEY. FLT › MODE holds DRIVE and LFO. ENV, KEY and LFO are views of the routes ENV 1, NOTE and LFO 1 → CUTOFF. | `ui/filter_panel.rs`, ADR 0037 |
| `env_amount`, `key_track` and `fm_amount` were retired (Filter ids 4, 5 and 3) and are now matrix routes. RES (id 1) is already modulatable. | `params.rs` `FilterParams`, ADR 0037 |
| Billing: each mode is billed as its delta over LP24, which the engine terms carry. PHASER is billed 17, BP24 and HP24 2, the rest 0. | `FilterKind::cost`, `cost_test.rs` `each_svf_mode_bills_its_delta_over_lp24` |
| The chain is engine → DRIVE stage → filter → FOLD → DC blocker → VCA. | `voice.rs` `render` |

## Decisions

### 1. TYPE, MODE and the "machines"

- The screen calls KIND **TYPE**, as the ENV, LFO and reverb slots already do. The ParamId (6), the ident `KIND` and the disk codes are unchanged. Three types are built, in this order: **SVF** (the home filter), **PROPHET** and **DIRTY**.
- Each type is a machine. It shows only its own modes, under its own labels, and lays out its own panel. A type never shows a mode or a knob that does nothing on it (ADR 0037).

| TYPE | MODEs, in knob order (**default** in bold) | Labels on screen |
|---|---|---|
| SVF | LP1, LP2, **LP4**, BP2, BP4, HP4, NT2, PHZR | Cascadia's own words |
| PROPHET | **LP4** (single mode: the cell is fixed and dimmed) | LP4 |
| DIRTY | **LP2**, BP2, HP2, NOTCH | LP2, BP2, HP2, NOTCH |

- **Mode labels belong to the type.** `FilterKind::mode_label(mode)` replaces the global name tables, which closes private issue #165. The SVF's `Notch` is NT2 (offset), while DIRTY's is NOTCH (at FREQ). The two are the same `FilterMode` code, and each type runs its own DSP for it.
- **The default is a property, not position 0.** `FilterKind::default_mode()` lets the SVF list its modes in the Cascadia's knob order with LP4 in the middle. This amends ADR 0037's "`modes()[0]` is the default".
- **A TYPE change** keeps MODE if the new type has it, and otherwise takes the new type's default (ADR 0037). CUTOFF, RES and DRIVE are shared and keep their values; each type maps them through its own law (§ 3–5). Routes are never touched.
- **Never snapped.** A TYPE or MODE change crossfades the old output into the new one over one block (64 samples). For that block both run, each from its own state: on a TYPE change the new type starts from zeroed state; on a MODE change the old mode runs on a stack copy of the shared state. That one block's extra cost is not billed: it is brief and sits in the headroom, as ADR 0061 treats UI-only moves.
- **New codes,** append-only (ADR 0009): `FilterKind::Prophet = 4` (the number the routing spec's data model reserved for PROPHET-5) and `FilterKind::Dirty = 6`. `FilterMode::Hp12 = 9` is the code the routing spec reserved. An unknown code loads as SVF at LP4, as `from_disk_code → None` already does.

### 2. The SVF is the home filter: the Cascadia mapping

#### The controls

| Cascadia (manual § 8) | What it does there | Chimera | Screen label | Where |
|---|---|---|---|---|
| FREQ [8.5] | cutoff | CUTOFF (Filter id 0), 20 Hz – 20 kHz | **FREQ** | FLT, knob 2 |
| Q [8.6] | resonance, up to self-oscillation | RES (id 1), 0..1 | **Q** | FLT, knob 3 |
| MODE [8.7] | 8 filter types | MODE (id 7) | **MODE** | FLT, knob 4 |
| FM 1 [8.1] | attenuator, normalled to ENV B | route ENV 1 → CUTOFF (the old `env_amount`) | **FM1** | FLT, knob 5 |
| FM 2 [8.2] | attenuverter ±1 V/oct, normalled to keyboard pitch | route NOTE → CUTOFF (the old `key_track`); ±100 % is ±1 oct/oct exactly | **FM2** | FLT, knob 6 |
| LEVEL [8.8] | input gain; past unity the input clips for grit; a red LED grows with the distortion | DRIVE (id 2), gain `1 + 4·DRIVE`, then the input clipper and its LED (§ 2, LEVEL) | **LEVEL** | FLT › MOD, knob 1 |
| FM 3 [8.3] | attenuverter, no normal ("free") | route *FM3 IN* → CUTOFF (the old LFO knob and `fm_amount`) | **FM3** | FLT › MOD, knob 2 |
| FM 3 jack [8.C] | what's patched into it | new enum param **FM3 IN**, the source of FM3's route; default LFO 1 | **FM3 IN** | FLT › MOD, knob 3 |
| QM [8.4] | resonance-modulation attenuator, no normal | route *QM IN* → RES | **QM** | FLT › MOD, knob 4 |
| Q MOD jack [8.D] | what's patched into it | new enum param **QM IN**, the source of QM's route; default ENV 3 | **QM IN** | FLT › MOD, knob 5 |

Knob 1 of FLT is **TYPE**. The sub-page FLT › MODE is renamed **FLT › MOD**, after what it now holds, and its sixth cell is empty.

- **Route knobs show the matrix**, as today (ADR 0035, 0037): the cell `source → dest` with its amount from −100 to +100 %. Turning a knob whose route is absent creates the route. "—" means no route. The matrix always applies.
- **FM1, FM2 and FM3 are octaves,** like every CUTOFF route (`CUTOFF_OCTAVES`): 100 % from an envelope is +10 octaves at its peak, and 100 % from NOTE is one octave per octave. *Departures from the Cascadia:* its FM1 is unipolar (0 to 2× its input) and its FM3 is linear FM. Ours are one bipolar amount each (routing-spec decision 24), and every one is exponential. Audio-rate filter FM stays out (routing spec, Out of scope).
- **FM3 IN and QM IN** are the jacks. `FilterParams` gains `fm3_src: ModSource` and `qm_src: ModSource`, stored as new Filter ids **8** and **11**. Ids 9 and 10 stay unused, because the routing spec left them for the never-built SRC params. Both params are append-only; a patch without them loads with the defaults.
  - FM3 IN steps through the sources that don't already route to CUTOFF: LFO 1, LFO 2, LFO 3, ENV 2, ENV 3 and VEL. ENV 1 and NOTE belong to FM1 and FM2. QM IN steps through all eight sources.
  - **Repatching moves the cable.** Changing FM3 IN or QM IN moves the route's amount and its presence bit from the old source's cell to the new source's cell in one write. The old cell is left absent. A source whose cell in that column is already present elsewhere is skipped, so a move never merges two routes. The cutoff moves through the `g` ramp and Q through the new `k` ramp (§ 2, DSP), so the move doesn't click.
  - This is not a KIND change editing the matrix (ADR 0037 forbids that). It is the user patching a jack, on a cell that names the jack.
- **QM** is a route into RES (linear law, ADR 0010): `RES_eff = clamp(RES + Σ, 0, 1)`, read per block and ramped per sample (§ 2, DSP). The first turn of QM adds RES's column to the matrix, if it has none. If all 16 columns are taken, the knob stays "—" and shows "matrix full" in place, as today.
- **Hints (ADR 0086).** No new gesture is added. One existing gesture is extended: MIX + MINUS on FM1, FM2, FM3 or QM deletes that route (the cable pulled), as it does on a matrix cell. The touched cell's focus band then shows `MIX + MINUS DELETES THE ROUTE`, using `mod_grid::DELETE`. FM3 IN and QM IN are plain cells and need no hint.

#### LEVEL and its clip indicator

- **DSP.** `x' = sat_in(x · (1 + 4·LEVEL))`. The gain law is today's DRIVE law, and LEVEL 0 is unity. `sat_in` is the soft clipper `modal-stagger` lands (stmlib's rational clipper), or, if its common path divides, the divide-free cubic below. It runs, and is billed, only while LEVEL ≥ 0.001 or a route moves it, the same gate as the DRIVE stage (`Voice::stage_runs`).
- **The LED.** While the clipper runs, each voice keeps `peak = max|x · gain|` over the block (one `vabs` and one `vmax` a sample). At the end of the block it reports `clip = clamp((peak − 1) / 2, 0, 1)`: 0 means no clipping, and 1 means the clipper is flat-topping by 3×. Each Part publishes the maximum over its voices to `meter::FILTER_CLIP[part]`, one `Meter` per Part with one relaxed store a block, the same pattern as `MASTER_GR`.
- **On screen.** A 3 px ring in `theme::ALERT` (red) sits right of the LEVEL value. Its fill is a radius proportional to `clip`, held and falling over 150 ms on the UI side. That follows the house rule "palette colours only, so the fill is a radius" (`components::header_dots`). The same LED draws in the FLT viz's top-right corner, so you can see grit while you sweep FREQ on the main page. At LEVEL 0 the ring isn't drawn.
- *Departure:* the Cascadia's LEVEL also attenuates below unity. Ours starts at unity, because attenuation is OUT LEVEL's and the Part's job. (Open question 3.)

#### The modes, against the Cascadia

| Mode | Cascadia (§ 8.7) | Ours today | Change |
|---|---|---|---|
| LP1 | 1-pole 6 dB/oct; Q emphasises the cutoff up to self-oscillation | plain one-pole; RES ignored | **Resonant LP1:** `lp + 2·bp` of one SVF stage, `(1 + 2s)/(s² + ks + 1)`. It falls at 6 dB/oct and peaks at FREQ with Q, up to self-oscillation. At Q 0 its −3 dB point is 2.48 × FREQ, and FREQ is where it sings (open question 4). |
| LP2 | 12 dB/oct | one SVF `lp` | unchanged |
| LP4 | 24 dB/oct | two SVFs, the second at `k2 = 1.2k + 0.3` | unchanged |
| BP2 | 12 dB/oct BP | `bp × 2` | unchanged |
| BP4 | 24 dB/oct BP | two `bp`s × 2 | unchanged |
| HP4 | 24 dB/oct HP | two `hp`s | unchanged |
| NT2 | notch "approximately an octave above the cutoff FREQ, while the Q emphasizes the actual cutoff frequency … until it self oscillates" | `lp + hp`: notch **at** FREQ | **Fix:** `lp + ¼·hp` |
| PHZR | "a double notch with … phase shifting characteristics … responds particularly well to sweeping" | two SVF all-passes, no dry: no notches | **Fix:** dry plus four TPT all-passes, with feedback (below) |

#### The SVF's DSP

- **The core stays.** The TPT SVF, its in-loop `saturate` on both integrator states, the LP4 cascade and `k = 2(1 − RES_eff) + 0.01` are all unchanged.
- **Coefficients per block, interpolated per sample.** At each block end the filter computes:
  - `g = tan_accurate(fc/fs)`, stmlib's FREQUENCY_ACCURATE polynomial (`f·(π + f²·(a + f²·(b + f²·(c + f²·(d + f²·e)))))` with stmlib's coefficients, `f` clamped to 0.497). It replaces `fast_tan` for every filter, and so fixes #57's 18.3 kHz top.
  - `k` from RES_eff;
  - `d = 1 / (1 + k·g + g²)`, one `vdiv` a block;
  - `G₁ = g / (1 + g)` for PHZR, one `vdiv` a block.
- Each of `g`, `k`, `d`, `G₁` and the LEVEL gain then ramps linearly across the block from the last block's value, reaching the new one on the last sample (`g_at`, #53). A fresh voice starts unramped, as today.
- This removes every per-sample divide: two `vdiv`s a sample at LP4, about 24 cycles a voice.
- Lerping `d` instead of recomputing it is exact at block ends. In between it is within 0.2 % for a one-octave move in one block. Ramping `k` is new: QM and RES routes stepped per block would otherwise zipper.
- FAST (stmlib's 16 Hz – 16 kHz polynomial) is not used here, since nothing in the filter needs `tan` per sample. It is added beside ACCURATE for the Modal resonators' own `tan` (#57's second approximation), in a separate change.
- **NT2:** `out = lp + 0.25·hp` of one SVF stage, `(1 + ¼s²)/(s² + ks + 1)` normalised. Its zero is at 2 × FREQ (digitally, where `tan(πf/fs) = 2g`). The response is 0 dB at DC and −12 dB above the notch, and Q raises a peak at FREQ that self-oscillates. The cost is one multiply-add over LP2.
- **PHZR,** after the SSM2040 datasheet's own "Multiple Notch (Phase Shifter)" application (Fig. 10: four all-pass sections summed with the dry signal give "two deep notches"):

  ```
  u  = x + fb · w[n−1]                    // feedback, one sample late
  a  = AP(AP(AP(AP(u))))                  // four TPT first-order all-passes at G₁
  w  = a
  out = (1 − fb) · ½ · (u + a)            // dry taken after the feedback node
  AP(v): t = (v − s)·G₁; lp = t + s; s = lp + t; return 2·lp − v
  ```

  - **Two exact notches.** `out ∝ (1 + A)·u`, so the zeros are wherever the cascade's phase is −180° or −540°, whatever the feedback. Those are the analog frequencies FREQ·tan(π/8) = 0.414 × FREQ and FREQ·tan(3π/8) = 2.414 × FREQ, 2.54 octaves apart and centred on FREQ in log frequency. Digitally they sit at `(fs/π)·atan(g·tan(π/8))` and `(fs/π)·atan(g·tan(3π/8))`. They move with FREQ in octaves, which is why it sweeps well.
  - **Q is feedback:** `fb = 0.7 · RES_eff`. Peaks form where the loop phase returns to 0, which is near FREQ (and at DC and the top), so Q "emphasises the cutoff" between the two notches. The `(1 − fb)` keeps those peaks at 0 dB. The notches stay exact at every Q.
  - **Stable by construction:** the all-pass chain has unit gain, |fb| ≤ 0.7, and the one-sample delay leaves no delay-free loop to solve (no divide). Self-oscillation is impossible (open question 5).
  - State: four floats and `w`. The cost is about 30 instructions a sample against LP4's 44 (§ Cost), so PHZR becomes *cheaper* than LP4.

### 3. PROPHET: the SSM2040

#### What the SSM2040 is

From the datasheet (Solid State Music, July 1977) and its application figures:

- Four identical sections. Each is a variable-transconductance amplifier ("G", an OTA) driving a capacitor to ground, buffered by a high-impedance follower ("B"). The cutoff is exponentially controlled over 10,000:1.
- **Fig. 1 (low-pass real pole):** the input and the section's own output each meet the OTA's input through resistors into a 200 Ω attenuator, so each pole is `−K/(s + 1)` with `ω₀ = 200·G/(R₂C)`. The OTA sees the *difference* of input and output, as a one-pole's error. Its tanh-shaped transfer therefore bends the stage's slew, not its output level. That is the OTA one-pole, `ẏ = ω·tanh(x − y)`, unlike the Moog ladder's `tanh(x) − tanh(y)`.
- Distortion is 0.1 % THD at 30 mVpp at an OTA input and climbs steeply to about 1 % near 80 mV (datasheet, THD vs input level). That is soft and gradual: the "syrupy" overdrive.
- **Fig. 9 (the electronic-music VCF):** four low-pass sections in cascade, with resonance as global feedback from the fourth output to the first section's summing node, through an external OTA (a 3080). The chip has no resonance VCA of its own (Electric Druid). The datasheet says "at high resonance settings the filter will oscillate with a pure sine wave".
- **Bass loss.** Negative feedback of `k` around four unity-DC poles gives a pass-band gain of `1/(1 + k)`. At the oscillation threshold (`k = 4`) that is −14 dB. Fig. 9 has no compensation path; the 2020 SSI2140 reissue advertises "optional Q compensation schemes to overcome attenuation of passband frequencies as resonance increases".

#### Model

Per sample, with `G = g/(1 + g)` per block (`g = tan_accurate(fc/fs)`, one `vdiv` a block), `k = 4.5 · RES_eff`, `D = 1/(1 + k·G⁴)` per block (one `vdiv`), and each of them ramped per sample as in § 2:

```
x_c = α · x · gain · (1 + c·k)                 // α = ½ sets the saturation domain; c = input compensation
S   = (1 − G)·(s₄ + G·(s₃ + G·(s₂ + G·s₁)))     // the stages' state, through the linear cascade
u   = (x_c − k·S) · D                          // one-step ZDF: the global loop solved linearly
for i in 1..=4:                                // x₁ = u, xᵢ₊₁ = yᵢ
    t  = G · sat(xᵢ − sᵢ)                      // OTA: the nonlinearity on the stage's error
    yᵢ = sᵢ + t
    sᵢ = yᵢ + t                                // TPT state update
out = y₄ / α
sat(v) = c₁ − (4/27)·c₁³,  c₁ = clamp(v, −1.5, 1.5)   // cubic, C1, flat at ±1 from ±1.5, no divide
```

- **ZDF/TPT one-poles:** each stage is the trapezoidal one-pole, which is exact in frequency at FREQ and stable under modulation (Zavalishin, ch. 3 and 5).
- **The nonlinear loop, resolved in one step.** The global feedback is solved *linearly* and exactly, using the cascade's instantaneous gain `G⁴` and its state sum `S` (Zavalishin's ladder solution). The stage nonlinearities then run explicitly, each evaluated at `xᵢ − sᵢ` rather than at the implicit `xᵢ − yᵢ`. This is Zavalishin's "cheap" approach to an implicit nonlinearity (ch. 6), or a Huovilainen-style stage without his half-sample delay.
  - In the linear regime it is exact, so the self-oscillation pitch is FREQ, and 1 V/oct tracking is limited only by `tan_accurate`.
  - At high drive it detunes slightly, and the test bounds that.
- **Newton is rejected.** Two fixed Newton iterations would cost 8 `sat`s and their derivatives, a 4-stage Jacobian chain and a `vdiv` per iteration: about +100 instructions and +24 cycles of divides. That would more than double PROPHET's cost, to cure a detuning at hard overdrive that the Prophet itself has.
- **Bounded at every setting.** `sat` is bounded by ±1 and has slope ≤ 1. So each stage's state moves by at most `2G` toward its input per sample (`|1 − 2G·sat′| ≤ 1`), every state is bounded by the input's bound plus 3, and `u` is a bounded linear combination of them. k = 4.5 > 4 makes it oscillate, and `sat` sets that level.
- **Bass loss, with input compensation `c`:** the DC gain is `(1 + c·k)/(1 + k)`. The choices at k = 4:

  | `c` | DC at full Q | Character |
  |---|---|---|
  | 0 | −14 dB | Fig. 9 as drawn: authentic, the bass thins as Q rises |
  | **½ (proposed)** | −4.4 dB | the loss is there and audible, but Q stays usable on a bass |
  | 1 | 0 dB | the "Q compensation" of the reissue; no loss |

  `c` is a per-type constant, not a knob (open question 1).
- **Levels.** α = ½ puts a full-scale input at about 1 % third harmonic per stage at LEVEL 0 (the cubic's `A²/27`), which is the datasheet's "soft" region. LEVEL's ×5 drives it into the steep part of the curve. The LEVEL LED reads at the input, as on every type.
- **Mode:** LP4 only, as on the Prophet. The MODE cell shows "LP4", fixed and dimmed (ADR 0037).
- **Panel** (the routing spec § 6, kept): TYPE · CUTOFF · RES · ENV AMT · KEYBD · DRIVE. KEYBD is a stepped view of NOTE → CUTOFF (OFF, HALF, FULL at 0, 64 and 127). DRIVE is LEVEL with its LED. FLT › MOD holds FM3 · FM3 IN · QM · QM IN under those names, because the matrix always applies.
- **State:** four floats.

### 4. DIRTY: the Chamberlin SVF

```
per sample, run twice (×2 oversampling, the input held for both):
    lp += f·bp
    hp  = x − q·bp − lp
    bp += f·hp
    lp, bp = clamp(lp, ±2), clamp(bp, ±2)       // hard: the guarantee, and the dirt
out = clamp(½·(m₁ + m₂), ±2)                    // m = the mode's output at each sub-step
```

- **Modes:** LP2 = `lp`, BP2 = `bp`, HP2 = `hp`, NOTCH = `lp + hp` (the notch at FREQ, Chamberlin's own). The default is LP2.
- **Coefficients per block,** ramped per sample:
  - `f = 2·fast_sin(π·fc / (2·fs))`, the Chamberlin tuning at the doubled rate;
  - `q = 2 − 2.04 · RES_eff`, so RES 1 gives q = −0.04, a slight negative damping: DIRTY self-oscillates, hard-clipped at the clamp. That oscillation is the "edgy, digital" voice.
- **Why it can't blow up.** The linear update matrix is `[[1, f], [−f, 1 − fq − f²]]`. Its determinant is `1 − fq` and its trace `2 − fq − f²`. Jury's test gives stability exactly when `0 < fq` and `f² + 2fq < 4`, that is `f < √(q² + 4) − q` (consistent with Dattorro's and Lazzarini's analyses).
  1. **f is clamped per block** to `0.95·(√(q₊² + 4) − q₊)`, with `q₊ = max(q, 0)` (one `vsqrt` a block). So any setting with `q > 0` is stable as a linear system.
  2. **Oversampling ×2** halves `f` for a given FREQ. That lifts the top of the range: at Q 0 (`q = 2`), `f ≤ 0.79` reaches about 12 kHz. Without oversampling it would reach about 6 kHz. Above that, FREQ stops rising, which is part of the machine's edge.
  3. **The hard clamps** on `lp`, `bp` and the output make every state and output bounded for every input, every `f`, every `q` (negative included) and every ramp. The stable region isn't convex, so a one-block ramp between two stable points can cross out of it, and the clamp covers that. A finite input gives a finite output, with no NaN path.
- **The "×2 if cheap" check.** One sub-step is about 8 instructions with the clamps, so ×2 with the hold and the average is about 24 a sample, half of LP4. It is cheap enough, so ×2 is built and there is no ×1 variant.
- No in-loop saturation and no ZDF: DIRTY is deliberately the naive digital filter. The clamp and the Nyquist-near tuning error are the character. LEVEL drives it into the clamps, and the LED reads as on every type.
- **Panel:** TYPE · FREQ · RES · MODE · ENV · KEY, the SVF's words without Cascadia's FM numbering, because it is not the Cascadia. FLT › MOD: LEVEL · FM3 · FM3 IN · QM · QM IN.
- **State:** two floats.

### 5. UX: TYPE versus MODE, without a blank canvas

- **One knob picks the machine, the next picks its voice.** TYPE is FLT's knob 1. MODE (where the type has more than one) is on the same page, so a turn of TYPE and a glance show the machine's whole panel. Nothing needs setting up: every type opens at its default mode with the Sound's CUTOFF, RES and DRIVE, and all routes stay where they were.
- **Each type nudges you:**
  - SVF speaks Cascadia: FREQ, Q, FM1, FM2, LEVEL, FM3, QM, LP1 … PHZR.
  - PROPHET has one mode and the Prophet's words, and its bass falls as you turn RES up.
  - DIRTY has four plain modes, and it clips and screams near the top.
- **The viz** (`viz::Response`) gains NT2's shape (a peak at FREQ and a notch at 2 × FREQ), PHZR's (two notches at 0.414 and 2.414 × FREQ, peaked by Q; today it is flat), LP1's resonant 6 dB slope, and DIRTY's NOTCH at FREQ. PROPHET draws LP4 and lowers the pass band by its bass loss as RES rises, so you see the loss you hear.
- **Screen goldens:** FLT and FLT › MOD for each type; the MODE cell fixed and dimmed on PROPHET; the LEVEL LED at 0, ½ and 1; FM3 IN and QM IN after a repatch.

## Cost

### Method (the house manner, ADR 0056 and TAPE's ADR 0093)

- **Static count.** For each (type, mode), a `#[inline(never)]` probe `filter_probe::<T, M>` runs one 64-sample block on the ramping path. It is compiled for `thumbv7em-none-eabihf`, `-C target-cpu=cortex-m7`, `opt-level = 2` (the release profile), and disassembled with `llvm-objdump -d --mcpu=cortex-m7`.
  - The loop body is counted from its head to its back-edge.
  - Per-block work (the `tan`, the `vdiv`s and `vsqrt`s, the ramp set-up) is counted once and divided by 64.
- **Cycles** = instructions × 1.46 × 1.1, plus 12 for each `vdiv`/`vsqrt` left per sample, rounded up to 5.
- **Bill** = `max(0, cycles(type, mode) − cycles(old LP24))`. The baseline is *today's* LP24 loop, counted the same way before the change, because that is what the benched engine terms carry. A type cheaper than the old LP24 bills 0 until the engine terms are re-benched at ship time ("hardware checks only at ship").
- **LEVEL** bills its clipper and meter as a separate term, `FILTER_LEVEL_COST`, while it runs (`stage_runs`'s gate). That replaces nothing that runs today.
- Each constant's doc comment carries its count and arithmetic, as `COST_*` do.

### Budgets: estimated instructions a sample, ramping path; each is a cap the build must meet

| Type | Mode | Budget (instr) | `vdiv`/sample | Est. cycles | Est. bill over old LP24 (~50 instr + 2 `vdiv` ≈ 105) |
|---|---|---|---|---|---|
| SVF | LP4 | ≤ 46 | 0 | 74 | 0 |
| SVF | BP4, HP4 | ≤ 47 | 0 | 76 | 0 |
| SVF | LP1, LP2, BP2, NT2 | ≤ 27 | 0 | 44 | 0 |
| SVF | PHZR | ≤ 32 | 0 | 52 | 0 (was 17) |
| PROPHET | LP4 | ≤ 60 | 0 | 97 | 0 (≤ +10 if the count overruns) |
| DIRTY | any | ≤ 26 | 0 | 42 | 0 |
| any | LEVEL ≥ 0.001 (clipper + meter) | ≤ +10 | 0 | +17 | +20 |

- **Per block:** `tan_accurate` is about 14 instructions; at most 2 `vdiv`s and 1 `vsqrt` and the ramp set-up come to about 70 cycles a block, under 1.1 cycles a sample. That is included above.
- **The SVF rework pays for itself.** Removing the two per-sample divides frees about 24 cycles at LP4. That saving is not taken until a re-bench lowers the engine terms; it is headroom, not a lower bill.
- **The crossfade block** on a TYPE or MODE change runs two filters for 64 samples and is not billed (§ 1).
- `cost_test` replaces `each_svf_mode_bills_its_delta_over_lp24` with `filter_bills_each_type_and_mode`. It pins every (type, mode) bill, checks every bill is ≥ 0, checks every built type's every mode is listed, and checks that `Voice::cost` adds `FILTER_LEVEL_COST` exactly when `stage_runs` runs the clipper.
- The bench's SVF row (#177's "bench every SVF mode") gains PROPHET and DIRTY rows at ship time.

### Memory

- **Per voice:** the SVF keeps 4 floats and adds PHZR's 5. PROPHET adds 4 and DIRTY 2. The ramps hold endpoints for `g`, `k`, `d`, `G₁`, `D`, `f`, `q` and the gain, plus the block's clip peak. That is about 100 B against about 24 B today, ×8 voices ≈ 0.6 KB, under ADR 0051's `VOICE_CHAIN_BYTES` assert.
- Every type keeps its own state, flat rather than in an enum, so the one-block crossfade can run two at once.
- **Elsewhere:** `FilterParams` grows 2 B (`fm3_src`, `qm_src`), and `meter::FILTER_CLIP` is 6 × 4 B.
- **Flash:** about 1.5 KB for two loops and eight SVF mode loops, by `size` on the firmware, reported as one line (flash counting proportion).

## Constraints held

- **No alloc, no panic in audio.** The modes are matched once a block. Index paths are const or clamped, and `clamp` and `max` are branch-free (`vmaxnm`/`vminnm`).
- **Never snapped.** CUTOFF, RES, LEVEL and every coefficient ramp across the block. TYPE and MODE crossfade over one block. The UI lerps knob values as it always has.
- **Old patches load safely,** but needn't sound the same: the SVF's LP1, NT2 and PHZR change sound, and the `tan` change moves the top octave. Goldens that hit them are re-recorded and listed in the PR. The new params (Filter 8 and 11) and codes (KIND 4 and 6, MODE 9) are append-only, and unknown codes fall back to SVF at LP4.

## Tests the build must add

1. **Frequency response per (type, mode):** a log sweep, or the impulse's FFT, at FREQ 250 Hz, 1 kHz and 4 kHz and RES 0, checked against the analog prototype mapped by the bilinear transform:
   - the slope over the octave 2–4 × FREQ: LP1 −6 ± 1.5, LP2/HP2 ±12 ± 2, LP4/HP4 ±24 ± 3 dB/oct;
   - BP's peak at FREQ ± 3 %;
   - NT2's minimum at 2 × FREQ ± 3 % (digital formula), at least 30 dB below DC;
   - DIRTY's NOTCH at FREQ ± 5 % for FREQ ≤ fs/8;
   - PROPHET's −3 dB point within ±5 % of the linear SSM2040 prototype at α·input below 0.05.
2. **Self-oscillation, level and tuning,** at RES 1 for every mode that resonates (all but PHZR), excited by one impulse and read over 0.5 s after 0.25 s:
   - the level is steady (±1 dB over the window) and within a pinned band per type, measured at build: SVF and PROPHET between −12 and 0 dBFS, DIRTY at its clamp;
   - **1 V/oct:** FM2 = +100 % (NOTE → CUTOFF), notes C2 to C6, FREQ fixed. Each octave doubles the frequency within ±10 cents up to 5 kHz, and the absolute pitch is within ±25 cents of the target (`fundamental_hz`), for SVF LP2, LP4 and PROPHET. For DIRTY the same is required up to fs/8 only.
3. **Click-free sweeps** with the project's detector (`tests/common/sweep.rs`: `Tape::click`, `CLICK_RATIO`, `CLICK_FLOOR`; `common::clicks`):
   - FREQ swept 20 Hz → 20 kHz → 20 Hz over 1 s, and Q 0 → 1 → 0, on a held saw, for every (type, mode);
   - the same as UI steps through `Block::set` (`param_sweep_test`);
   - QM routed from a stepped source (a CLASSIC LFO square) at 100 %, to prove the new `k` ramp;
   - a TYPE change and every MODE change mid-note, to prove the crossfade;
   - FM3 IN and QM IN repatched mid-note.
4. **PHZR has two notches that move with FREQ:** white noise through PHZR at Q 0, averaged spectrum.
   - There are exactly two minima at least 25 dB deep, at the digital formula's 0.414 × and 2.414 × FREQ (±3 %), for FREQ 200 Hz, 400 Hz, 800 Hz and 1.6 kHz.
   - Each doubling of FREQ moves both notches up one octave (±0.05).
   - The notches stay within ±3 % at Q 0.5 and Q 1, and the peak near FREQ rises with Q.
5. **PROPHET loses bass as resonance rises:** the gain at 100 Hz with FREQ 2 kHz, at a low level, for RES 0 to 1 in tenths.
   - It falls monotonically.
   - At each point it is within ±1 dB of `20·log₁₀((1 + c·k)/(1 + k))`.
   - With `c` forced to 0 it reaches −14 ± 1 dB at k = 4.
6. **DIRTY stays bounded at every setting:**
   - a grid of FREQ (24 log points, 20 Hz – 20 kHz) × RES (0..1 in 11) × LEVEL (0, ½, 1) × the 4 modes;
   - inputs: silence, a full-scale saw, a square at fs/4, white noise ±1, and impulses of ±10;
   - plus a property test that changes FREQ, RES and MODE randomly every block for 10 s.
   - Every output is finite with |y| ≤ 2, and every state is within ±2.
   - The same grid runs on SVF and PROPHET, with bounds pinned at build.
7. **Per-type cost:** `filter_bills_each_type_and_mode` (§ Cost). Each constant's doc comment records its static count, and a reviewer re-runs the probe disassembly on the final binary.
8. **Kinds and modes, pure:**
   - `default_mode() ∈ modes()`;
   - `mode ∈ kind.modes()` after every write;
   - every `mode_label` fits its cell;
   - a TYPE change leaves the `ModState` byte-identical;
   - an FM3 IN or QM IN change moves exactly one route and its presence bit, and refuses an occupied cell;
   - KIND 4 and 6, MODE 9, Filter 8 and 11 round-trip through the codec, and unknown codes load as SVF at LP4.
9. **LEVEL's LED:** `clip` is 0 at LEVEL 0 and for any input ≤ 1, and rises monotonically with LEVEL on a full-scale saw. The Part's meter is the maximum over its voices. Screen goldens at 0, ½ and 1.

## Build order

The build starts after `modal-stagger` merges, as a plan with these tasks:

1. ACCURATE `tan` and the divide-free, `k`-ramped SVF. Goldens are re-recorded, and old-LP24 versus new counts are taken.
2. The NT2 fix and the resonant LP1.
3. PHZR.
4. LEVEL's clipper, meter and LED.
5. The Cascadia panel: labels, FLT › MOD, FM3 IN and QM IN, and route delete on the knobs.
6. TYPE crossfade and per-type labels.
7. PROPHET.
8. DIRTY.
9. Costs and `cost_test`.

Each task is QA'd on the desktop (param sweeps and per-pair levels) before the owner's ear.

## Out of scope

- The other kinds of the routing spec (MOOG private issue #123, MS-20 private issue #124, SH-101 private issue #125, TB-303 private issue #127).
- Audio-rate filter FM.
- The Cascadia's always-on LP4 and HP4 outputs.
- Moving the DRIVE node and FOLD onto the filter page with PRE/POST (private issue #278). LEVEL here is the filter's own input gain, the Cascadia's, and #278's DRIVE KIND stays its own spec.

## Open questions for the owner

1. **PROPHET's bass loss:** authentic (−14 dB at full RES), half (−4.4 dB, proposed), or compensated (none)?
2. **PROPHET's modes:** LP4 only (proposed, as on the Prophet), or also LP2, a tap after stage 2 that adds a cheap mode the Prophet never had?
3. **LEVEL below unity:** the Cascadia's LEVEL also attenuates. Keep unity at 0 (proposed: no change for existing Sounds), or re-centre the law so the knob's middle is unity?
4. **LP1:** the Cascadia's LP1 resonates. The proposal is resonant, with its peak at FREQ and its −3 dB point 1.3 octaves above. Should the −3 dB point sit at FREQ instead (the peak then falls 1.3 octaves below FREQ), or should LP1 stay a plain, non-resonant one-pole?
5. **PHZR at full Q:** the proposal can't self-oscillate (fb ≤ 0.7). Does the Cascadia's PHZR ring or oscillate at full Q? If it does, fb's cap rises to 0.95, and the bounded test covers it.
6. **QM IN's default source:** ENV 3 (proposed, the spare function generator) or something else? FM3 IN defaults to LFO 1, as today's LFO knob does.
7. **Names:** should the home type stay "SVF" on screen, or take another name? Is "PROPHET" the label you want, or "P-5" or "2040"?
8. **DIRTY's top:** at Q 0 its FREQ tops out about 12 kHz with ×2. Is that the edge you want, or should ×4 (about +24 instructions) reach about 20 kHz?

## Sources

- Intellijel, *Cascadia Manual*, firmware 1.2, rev. 2023-10-15, § 8 "VCF" (pp. 44–48): FM 1–3, QM, FREQ, Q, MODE, LEVEL and its LED, NT2, PHZR. https://intellijel.com/downloads/manuals/cascadia_manual_v1.2_2023.10.15.pdf. Used for behaviour and UX only.
- Solid State Music, *SSM 2040 Voltage Controlled Filter Circuit*, data sheet, July 1977: the block diagram, Fig. 1 (low-pass real pole), Fig. 9 (the VCF with voltage-controlled resonance), Fig. 10 (the multiple-notch phase shifter) and THD vs input level. http://bitsavers.trailing-edge.com/components/solidStateMusic/SSM2040.pdf
- Synth DIY Wiki, "SSM2040": https://sdiy.info/wiki/SSM2040 · "Sequential Circuits Prophet-5" (Rev 1/2 SSM2040, Rev 3 CEM3320): https://sdiy.info/wiki/Sequential_Circuits_Prophet-5
- T. Wiltshire (Electric Druid), "Multimode filters, Part 1: Reconfigurable filters" (the SSM2040 has no resonance VCA; its topologies next to the CEM3320's). https://electricdruid.net/multimode-filters-part-1-reconfigurable-filters/
- Synthtopia, "Sound Semiconductor Intros Updated Version Of Classic SSM2040" (2020), on Q compensation for pass-band loss. https://www.synthtopia.com/content/2020/05/27/sound-semiconductor-intros-updated-version-of-classic-ssm2040-analog-filter-chip/
- V. Zavalishin, *The Art of VA Filter Design*, rev. 2.1.2 (2020): the TPT one-pole and SVF, the ladder's zero-delay feedback solution, and nonlinearities and the "cheap" solving of implicit nonlinear loops. https://archive.org/details/the-art-of-va-filter-design-rev.-2.1.2 (rev. 1.1.1: https://www.native-instruments.com/fileadmin/ni_media/downloads/pdf/VAFilterDesign_1.1.1.pdf)
- A. Simper (Cytomic), "Linear Trapezoidal Integrated State Variable Filter", the form `filter.rs` uses. https://cytomic.com/files/dsp/SvfLinearTrapOptimised2.pdf
- A. Huovilainen, "Non-Linear Digital Implementation of the Moog Ladder Filter", DAFx-04, a cascade of one-poles with embedded nonlinearities. https://dafx.de/paper-archive/2004/P_061.PDF
- E. Pichon (Mutable Instruments), stmlib `dsp/filter.h`: `OnePole::tan<FREQUENCY_FAST | FREQUENCY_ACCURATE>`, MIT licence. https://github.com/pichenettes/stmlib/blob/master/dsp/filter.h
- H. Chamberlin, *Musical Applications of Microprocessors*, 2nd ed. (Hayden, 1985), ch. 14: the digital state-variable filter.
- J. Dattorro, "Effect Design, Part 1: Reverberator and Other Filters", JAES 45(9), 1997: the Chamberlin SVF's tuning and stability limits. https://ccrma.stanford.edu/~dattorro/EffectDesignPart1.pdf
- V. Lazzarini and J. Timoney, "Improving the Chamberlin Digital State Variable Filter" (2021), on its instability at high frequencies. https://arxiv.org/pdf/2111.05592
- Chimera: [2026-09-27-filter-routing-design.md](2026-09-27-filter-routing-design.md); ADRs 0009, 0010, 0026, 0035, 0036, 0037, 0051, 0056, 0061, 0086, 0089; issues #48, #53, #57, #123–#128, #157, #165, #177, #278.
