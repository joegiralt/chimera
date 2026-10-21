# LPG circuit lab: the Buchla 292 as a circuit

## 2026-10-21: "It sounds like a weak filter". Root causes, fixes, gate checks

The owner rejected the first page. The coordinator measured
`wav/marimba_ping_combo_d1_ref.wav`: a ~505 Hz centroid at onset, and a level
that sits at −31 dB from 200 ms instead of shutting. Every cause below is
measured in `gate.md` (the lab's `lpg-gate-ab gate`; now the tests in
`src/buchla292b/tests.rs` and `src/plaits_lpg/tests.rs`). No knob was
tuned by ear.

### Root causes

| # | Cause | Evidence | Fix |
|---|---|---|---|
| 1 | **The sources were dark.** The old harness's marimba and bongo are a near-sine bar and an FM'd triangle. | CLEAN on the same take also centres at ~530 Hz. | New takes: a band-limited saw and square, a sine folder (sin(4.5·sin)), and a 259-style FM tone, as a slow pluck and a 16th-note bongo. |
| 2 | **Every render used COMBO.** COMBO is P&D's "Both", the 292C's centre position: a passive −6 dB/oct network with Rα (2.2 MΩ in the 292B, 4.7–5 MΩ in the 292C/P&D) as its only path to ground. | The leak was re-derived from the drawing. R15 2.2 MΩ is confirmed, from Q1's gate (the output node) to ground. The DC gain is eq. 12, Rα/(Rα + 2Rf). With the cell climbing only to 3–6 MΩ within 1.5 s, COMBO sits at −22…−28 dB at 1 s and never reaches −40 dB. This is in the circuit, not a modelling error. Make Noise documents the "signal-bleed often associated with vactrol-based Low Pass Gates" (Optomix manual). The **292B has no COMBO**: its FILT (our LOWPASS: C3 in, −12 dB/oct) and GATE both close. | The default and the page use **LOWPASS, the 292B's FILT**. COMBO stays as a labelled extra. |
| 3 | **The cell decayed 2.5–5× too slowly.** The drawing lists VT1–8 as **Vactec VTL2C3**, a single cell of material type 3. That is not the dual VTL5C3/2 Najnudel identified. | On the VTL5C3 sheet's 40 mA turn-off points, Najnudel's VTL5C3/2 gives 19 kΩ at 20 ms (sheet 97 kΩ) and 366 kΩ at 100 ms (sheet 930 kΩ). | **Najnudel's model refit to the VTL5C3 sheet** (`vtl5c3::fit`): ν+ ×2.58 (348.7), ν− ×2.03 (3.634e8), turn-off RMS log error 0.048 over the seven points from 6.75 to 100 ms. The static law is P&D's eq. 39 (their VTL5C3 fit). The cell is a runtime choice (the `vtl5c3::Cell` each model is built with); VTL5C3/2 stays available. |
| 4 | **My brightness check was power-weighted.** A 220 Hz saw reads ~500 Hz even through CLEAN. | — | Magnitude-weighted centroid (CLEAN saw onset: 2.3 kHz). |

The open state was not a cause (the coordinator's point 2, checked):
- **Peak current.** The Env.perc ping (1 ms/10 ms × 15 V) lights the LEDs
  at up to 19 mA, IC1's rail through R13.
- **Minimum R.** The cell reaches 6.3 kΩ at 1.1 ms: LOWPASS −3 dB at
  14.7 kHz there, COMBO 13.0 kHz.
- **Longer pulses.** A 3 ms 15 V pulse reaches 4.3 kΩ (21.9 kHz), and 20 V
  (CV + OFFSET) reaches 2.3 kΩ.
- **Calibration.** 15 V is the Buchla 200 pulse and envelope level: the
  MEMS 292B page notes the Buchla 284 puts out 15 V. The 292B's own ceiling
  is about 15–19 mA (R13 1 kΩ from a −24 V rail), not P&D's 40 mA.

### Gate checks

One 220 Hz hit, the same Env.perc ping into every model. Levels are re the
loudest 5 ms.

| model | saw: onset centroid | centroid 50 ms | to −20 dB | to −40 dB | level at 1 s |
|---|---|---|---|---|---|
| CLEAN LP2, 300 ms pluck env | 2278 Hz | 573 Hz | 96 ms | 163 ms | silent |
| **ours ref, LOWPASS (292B FILT), VTL5C3** | 1804 Hz | 322 Hz | **101 ms** | **386 ms** | **−49.9 dB** |
| ours fw, LOWPASS | 1903 Hz | 301 Hz | 104 ms | 388 ms | −49.5 dB |
| ours ref, GATE | 3391 Hz | 1843 Hz | 23 ms | 179 ms | −52.6 dB |
| ours ref, COMBO (leaky mode) | 2455 Hz | 631 Hz | 312 ms | never | −26.7 dB |
| ours ref, LOWPASS, old VTL5C3/2 | 1955 Hz | 429 Hz | 206 ms | 779 ms | −42.6 dB |
| ours ref, COMBO, old VTL5C3/2 (the rejected page) | 2575 Hz | 891 Hz | 626 ms | never | −22.2 dB |
| SuperCollider LPG, lowpassmode 1 (P&D port) | 759 Hz | 726 Hz | 412 ms | 778 ms | −50.7 dB |
| SuperCollider LPG, lowpassmode 0 | 1710 Hz | 1685 Hz | 634 ms | 1230 ms | −31.1 dB |
| Plaits LPG (DECAY ½, COLOUR ½) | 2442 Hz | 1553 Hz | 493 ms | 1143 ms | −35.5 dB |

The folded sine gives the same ranking. Ours on LOWPASS: −20 dB at 78 ms,
−40 dB at 270 ms, −55 dB at 1 s.

**Verdict.** On the 292B's own modes the circuit now opens bright and then
shuts. It closes faster than both references.
- **Brightness.** Ours goes dark within ~20 ms (LOWPASS −3 dB from 14.7 kHz
  to ~2 kHz by 10 ms). Plaits holds 11.6 kHz at 10 ms and 4.4 kHz at 50 ms.
- **Level.** Ours reaches −40 dB at 0.39 s; Plaits at 1.14 s.
- **Firmware.** The firmware shape tracks the reference on every check
  (≤ 3 ms, ≤ 0.6 dB).

### What the references told us

- **Plaits** (MIT, ported in `src/plaits_lpg/`) is behavioural. One state does
  three jobs:
  - gain = state, a true VCA, so the gate truly closes;
  - cutoff = (0.003 + 0.3·state⁴ + 0.04·colour)·fs, so the highs collapse
    steeply;
  - the fall coefficient depends on the state.

  The closing in Plaits comes from its VCA term. In the 292 it comes from
  FILT's −12 dB/oct corner falling under the note, plus the divider. COMBO
  has neither strongly enough, which is why it plateaued.
- **The SuperCollider LPG** is Parker & D'Angelo's Max model, run as a black
  box (GPL-3.0; its .sc/.cpp were not read). Its help page documents a
  continuous `vca` amount, a `controloffset` bias, a `controlscale` and a
  `linearity` switch.
  - P&D §4 says the VCA mode "is implemented by providing a control which
    varies Rα", so "the response [can] be smoothly varied between filter-like
    and VCA-like". This is **not a 292 position** (the switch selects
    5 MΩ or 5 kΩ, i.e. 2.2 MΩ or 15 kΩ in the 292B).
  - Its `resonance` is P&D's added feedback amplifier `a`: "not present in
    the original Buchla versions". The 292B's follower gives a fixed
    a = 0.957.
  - Its default `vca` 1 with `lowpassmode` 1 (lowpass + VCA shunt at once)
    is likewise a combination the 292B's switch does not offer.
  - So its classic "bongo" comes from those parameters: the help file's
    patch uses vca 0.75, controlscale 0.3, res 1.1.
  - With defaults it is darker at onset (759 Hz on the saw) and slower to
    close than ours.
- **Noise Engineering's primer**: the bongo is a wavefolded sine through an
  LPG with a quick decay. Early Buchla needed an external envelope. "A
  trigger makes the gate jump instantly to its maximum." That describes
  strike-circuit LPGs (Plaits, LxD); a raw 292 with a pulse into its CV is
  bounded by the cell's turn-on. Ours reaches its minimum R at 1.1 ms.

### Open questions this leaves

- **Our close is faster than Plaits' and SC's.** That is the VTL5C3 sheet's
  decay with the 292B's FILT corner. If the owner wants a longer bright
  ring, that is a cell or DECAY choice, not a circuit fix.
- **COMBO's bleed is real,** but the cell's tail beyond 1 MΩ is outside both
  the sheet's curves and Najnudel's validation (≤ 1 MΩ, 200 ms). How fast a
  real cell climbs from 1 to 10 MΩ sets how much COMBO bleeds. R_dark
  10 MΩ (Najnudel's value) is the sheet's *minimum* at 10 s.
- **Turn-on.** The model turns on faster than the VTL5C3 sheet's curve,
  which starts near 190 kΩ rather than from dark. The rates can't fix it.
  Left as is.

The lab's listening page and WAVs are not in the repo.

---

2026-10-20. Desktop lab only: no firmware, no PR. Sources and every value:
`research.md`. Full tables: `validation.md` (circuit checks) and
`levels.md` (A/B, nulls, bands, brightness, strike timelines).

Code: the lab's `chimera-utils/src/lpg_circuit.rs` (branch
`lpg-circuit-lab`), now `chimera-circuits/src/buchla292b/` and
`src/vtl5c3/`. The lab's commands became tests (see `README.md`):

| lab command | now |
|---|---|
| `lpg-circuit-check` | the `buchla292b` and `vtl5c3` tests (`validation.md`) |
| `lpg-circuit-ab DIR` | dropped (wrote WAVs and `levels.md`); its timelines and fw/ref brightness are tests |
| `lpg-circuit-cost` | dropped (a printer); the numbers stay below |
| `lpg-circuit-vac` | `vtl5c3::tests::f32_firmware_vactrol_tracks_the_reference` |
| `lpg-circuit-gate` | dropped (a trace printer) |
| `lpg-circuit-fit` | `vtl5c3::fit::fit_vtl5c3`, test `refit_reproduces_the_vtl5c3_rates` |

## What was built

1. **Reference** (f64, 8× = 384 kHz): the Buchla 292B as drawn (B21208,
   10/12/70, rev. 7-76), plus the 292C's COMBO switch position.
   - Audio path: input attenuator, IC2, the LDR/220 pF/910 pF/4.7 nF/2.2 MΩ
     network, the 15 kΩ GATE shunt, Q1's JFET source follower (square law),
     C5/R17 and IC3 ×2 with its rails.
   - Solved by nodal Newton on four nodes, with trapezoidal capacitor
     companions.
   - LED driver: the 292B's IC1/R4/R5/C1/zener/R13 loop, solved in its
     three regimes (closed loop, LEDs off, IC1 railed).
   - Vactrol: Najnudel et al.'s VTL5C3/2 photoconductor ODE (DAFx-23,
     CC-BY), backward Euler. Its static law is refit to the PerkinElmer
     sheet.
2. **Firmware shape** (f32, 48 kHz):
   - The same driver equations and vactrol ODE, once every 16 samples.
   - The audio core is Parker & D'Angelo's topology-preserving trapezoidal
     solve (5 coefficients, ramped).
   - The follower is linearised (gain 0.957).
   - Two one-pole 7 Hz couplings, and a cubic rail at IC3.
   - Details under "Firmware model" below.

## What the old prototype (`lpg/`, `lpg-lab`) got wrong

Each point is checked against the drawings, the sheets and the papers.

| # | Old prototype | The circuit | Effect |
|---|---|---|---|
| 1 | **"Dark floor"**: a gain forced to 0 over the last 30 µA of light | No such thing. Shut, the 292B passes **−20 dB at DC with a ~66 Hz corner** in COMBO/LOWPASS (Rf → 10 MΩ against Rα 2.2 MΩ), and −62 dB in GATE | An invention. Removed. The leak is kept; see open questions |
| 2 | **Exponential converter** in front of the LED ("so OPEN moves in octaves") | The 292B drives the LEDs **linearly**: 67.6 µA/V above a 0.77 V threshold, then a 3.9 V zener's soft knee, then IC1's ~15 mA rail | An invention. The real law is linear, then steep (15 V gives 8 mA, 16 V gives 12.8 mA) |
| 3 | Vactrol as **a one-pole on "light"** with a hand-picked τ (2.5 ms rise, a bimolecular term **fit by hand** to "100 kΩ in ~50 ms", 250 ms trap tail) | A photoconductor. Najnudel's identified carrier/trap ODE gives the rise, the fall and their level dependence with no fitting by us | The old fall was a curve-fit guess. The real fall is set by two processes: holes drain through the traps (τ ≈ 7.6 ms), then electrons recombine bimolecularly, so R grows nearly **linearly in time**, ≈ 4–5 MΩ/s. Rise is ≈ 1.9 ms to 63 % from dark |
| 4 | P&D's **10 µA LED floor** (a model artefact) | Below the 0.77 V threshold the LEDs carry **zero** current (IC1's +0.149 V bias keeps them off) | The old 35 MΩ "rest" came from the floor; the real rest comes from the cell's dark resistance |
| 5 | P&D Table 1 values (**1 nF, 5 MΩ, VCA Rα 5 kΩ**) | 292B: **910 pF, 2.2 MΩ, GATE shunt 15 kΩ** (292C: 910 pF, 4.7 MΩ) | GATE is far more open than P&D's: at Rf 20 kΩ DC −11.3 dB against −19.1 dB. COMBO's leak and bass loss are deeper (−9.0 dB against −5.1 dB at Rf 2 MΩ) |
| 6 | "VCA mode **+4–5 dB makeup**", and every mode **normalised to unity** at full light | Fixed stage gains: input +1.0 dB in GATE, −4.8 dB in FILT/COMBO (R8/R9/R10, IC2 ×1.485), output ×2 (IC3) | Normalisation removed. The modes keep their real loudness relationship |
| 7 | **Cubic soft clip** as "the transistor buffer" | Q1's JFET follower is nearly clean (H2 −58 dB at 2.5 V in). The grit comes from **IC2/IC3 hitting ±13.5 V**: −21 dB THD at 10 V in, GATE, full light | The old drive curve was invented. The real drive is op-amp rail clipping after the gate |
| 8 | **STRIKE**: a 4 ms LED pulse at velocity height | The 292 has no strike input. **But C1 (1 µF across R5) differentiates the CV**: a fast rise pushes C1·(R4/R2)·dCV/dt into the LEDs, up to IC1's ~15 mA rail. A step gets a ~100 µs rail pulse whatever its height | A real built-in "strike" the old model missed (it was not in P&D's model either). See "Differences" for what it does to velocity |
| 9 | DECAY knob (×¼…×4 on the fall), FB into the LED, a STABILITY spread | Not in the circuit | Fine as product features later, but not part of a circuit model. Left out here |
| 10 | Sub-rate: a "light" state updated at 16 | — | The idea is kept, but done properly (below): the kick is the hard part |

## Validation (each check, against its source)

| Check | Source | Target | Reference | Pass |
|---|---|---|---|---|
| Core magnitude, 3 modes, Rf 1 kΩ–1 MΩ, 100 Hz–15 kHz | P&D eq. 7, Fig. 2 | eq. 7 | worst error ≤ 5 kHz **0.010 dB** (8×) | yes |
| DC gain | P&D eq. 12 | Rα/(Rα + 2Rf) | exact to 0.01 dB, 1 kΩ–10 MΩ | yes |
| LOWPASS stability | P&D eq. 11 | a < a_max | a_max 1.47…3.57 > 1 (op-amp), > 0.957 (JFET) | yes |
| COMBO slope | P&D §2 "closer to −6 than −12" | ≈ −6 dB/oct | −5.9 dB/oct at 3, 30, 300 kΩ | yes |
| R at 1 mA / 40 mA | VTL5C3/2 sheet | 55 kΩ / 2 kΩ | 55.0 kΩ / 2.0 kΩ | yes (fitted there) |
| Slope R(0.5)/R(5 mA) | sheet | 19 | 19.5 | yes |
| Turn-on to 63 % | sheet | 3.0 ms typ (VTL5C3: 2.5) | **1.87 ms** at 40 mA, 1.90 ms at 10 mA | **faster than the sheet**; the model's turn-on hardly depends on level, but the sheet's VTL5C3 curves are slower at 10 mA |
| Turn-off to 100 kΩ | sheet | ≤ 50 ms | 44.4 ms (40 mA), 39.2 ms (10 mA) | yes |
| 10 kΩ → 1 MΩ | Najnudel Fig. 7a | ≈ 200 ms (read off the plot) | 229 ms | yes (±15 %) |
| eq. 12 as printed | Najnudel Table 1 | f ≥ 0 (their eq. 10) | **negative below 56 mW** | **fails**: the printed constants are unusable; we used §5.3 with the sheet |
| LED Vf | VTL5C3 sheet / Lanterman | ≈ 1.65 V | 1.60 V at 20 mA (Najnudel eq. 13) | yes |
| Zener knee | 1N5228B sheet | 23 Ω at 20 mA, 1.9 kΩ at 0.25 mA | 23 Ω, 1.88 kΩ | yes (fitted there) |
| Driver slope | MEMS 292B bench (modified input R) | ~75 µA/V, then rising at the knee | 67.6 µA/V, rising 145 → 2900 µA/V from 8 to 15 V | consistent (their resistor was changed) |
| Oversampling | — | — | 8× against 48 kHz on a ping: −44 dB null | the 48 kHz core is fine below ~5 kHz |

## Firmware model

### What it does

| Part | Method |
|---|---|
| LED driver (every 16) | The 292B loop's static current at the block-end CV (zener via exp2), C1's charge to move, IC1's rail. The light over the block is the Simpson mean of f(If) when the CV moved (light ∝ If^4.5 at low current, so end-point light badly over-lights an attack). A kick runs at C1·(R4/R2)·(steepest CV slope), capped at the rail, for as long as C1's charge needs; the block's vactrol is then integrated piecewise. The CV's steepest slope per block is one compare a sample (the envelope stage could supply it). |
| Vactrol (every 16) | Najnudel eq. 4 in (u = s², x): the generation f/s becomes the constant 4f, so there is no singularity from dark. One ROS2 step (2-stage Rosenbrock, L-stable, one 2×2 inverse). 4/8/16 sub-steps only while turning on from near dark (a note's first one to three blocks). f32 equals f64 here (checked). |
| Static law | a 61-entry log2 f vs log2 If table (4 an octave, 1 µA..32 mA), from the reference's steady-state inversion. Conductance straight from the states: G = µ+p + µ−(p + x) + 1/R_dark. No powf, no per-sample divide. |
| Core (per sample) | P&D TPT, 2 states, 5 coefficients ramped over the 16 samples (2 divides an update). |
| Rest | follower gain 0.957 folded into the output gain; two one-pole 7 Hz couplings; a cubic knee from 80 % of IC3's rail. |

### Measured against the reference

All values from `levels.md`.

**Nulls (ref − fw):**
- marimba −39.0 dB (ping −41.1);
- bongo −46.3 (ping −47.7);
- bass COMBO −48.1, LOWPASS −42.5, GATE −38.4;
- pad −38.3;
- sweep COMBO −42.0, LOWPASS −37.8.

With drive (d2) the nulls are within 0.3 dB of d1.

**Octave bands:** fw within ±0.3 dB of the reference from 31.5 Hz to
4 kHz. At 8 kHz it is −0.3…−0.7 dB, and at 16 kHz −0.5…−3.5 dB (trapezoidal
warping at 48 kHz; LOWPASS +0.5…+1.0 dB at 4–8 kHz, the warped peak).

**Strike timelines** (3 ms ping, COMBO):
- 15 V: −3 dB within 1 % from 3 ms on. At 0.5–2 ms the firmware is
  6–11 % brighter (8.4 against 9.3 kHz at 0.5 ms).
- 7.5 V: within 4 % from 0.5 ms.

**Brightness against velocity** (C4 bar): the centroid matches to ≤ 3 Hz
and the level to ≤ 0.1 dB at every velocity.

**What's left:**
- the 48 kHz warping above ~8 kHz (prewarping the coefficients would cost
  nothing at sub-rate and is not done yet);
- low-light turn-on from dark: up to −6 % R in the first ms at 0.6 mA;
- the follower's linearisation (H2 −58 dB);
- the cubic against the smooth rail.

## The honest difference from CLEAN

CLEAN LP2 was driven with the same envelope, its cutoff the reference's own
static −3 dB at that CV (memoryless), its VCA the envelope × velocity
(Chimera's default routes). Files are RMS-matched per setting.

**Nulls LPG − CLEAN** (RMS-matched):

| Take | Null |
|---|---|
| bongo | **−1.1 dB** |
| bass COMBO / LOWPASS / GATE | −3.1 / −4.0 / −7.1 dB |
| marimba | −5.2 dB |
| pad | −11.1 dB |
| sweep | −10.6 dB |

**They are different sounds, not a tint of one another.** A −1 dB null
means the bongo waveforms share almost nothing sample for sample. The
pad and sweep, where the envelope is slow enough for the vactrol to follow,
are the only ones that come close, and even they are only 11 dB down.

What makes the difference, in order:
1. **The vactrol is the decay.**
   - After any note the cell's R climbs at roughly 4–5 MΩ/s, so the gate
     closes over **hundreds of ms**, whatever the envelope does. COMBO
     after a 15 V ping: 6.6 kHz at 10 ms, 1.8 kHz at 25 ms, 610 Hz at
     50 ms, 280 Hz at 100 ms, 170 Hz at 200 ms, and still ~100 Hz and
     −11 dB at 0.8 s.
   - CLEAN stops with its envelope. The bongo (70 ms decay) is where the
     two part furthest.
2. **The level and the cutoff move together, and the level moves little.**
   - Velocity 1 → 0.2 changes the LPG's level by **6.7 dB**, against
     CLEAN's 14.0 dB.
   - Velocity moves the centroid 562 → 371 Hz; CLEAN stays at 548 Hz.
   - The LPG trades dynamics for brightness.
3. **C1's kick flattens fast strikes.** A step CV drives the LEDs at the
   rail for a time set by the step's size. A ping's level changes only
   **1.0 dB** from velocity 1 to 0.2 (centroid 501 → 445 Hz). On the 292B a
   fast trigger is a nearly constant "pluck"; only slower envelopes play
   dynamics. A test on real hardware would be worth having.
4. **COMBO is a −6 dB/oct gate with a bass leak.** Even shut it passes
   bass at −20 dB with a 66 Hz corner: 31.5 Hz is +1…+12 dB against CLEAN
   on the plucks, and the pad's 31.5 Hz band sits 14 dB under CLEAN's.
5. **Drive is op-amp clipping after the gate.** At d2 (×4 into AUD IN),
   CLEAN's marimba is +22 dB in the 4 kHz band against the LPG. The LPG's
   rail clips the whole output after the gate, while CLEAN clips at its
   LEVEL stage, which lets the high harmonics through.

## Cost (M7, per voice)

**Per sample**, about 28 instructions, ×1.606 (the bill's factor):
**≈ 45 cycles**. The work:
- 5 coefficient ramps;
- the core: 3 muls + 4 FMAs + 2 state FMAs;
- the input HP (2) and the output HP (2);
- the gain and the rail compare (≈ 4);
- the slope compare (2);
- one load and one store.

**Every 16 samples**, about 300–350 cycles, **≈ 19–22 cycles/sample**:

| part | instructions | special ops |
|---|---|---|
| driver | ≈ 60–110 | 1–3 f(If) lookups (log2 with 1 divide, interp, exp2), 1–2 zener exp2s |
| vactrol ROS2 | ≈ 75 | 2 sqrt, 3 divides |
| coefficients | ≈ 20 | 2 divides |

**Total: ≈ 65–70 cycles/sample/voice** typical. The worst ordinary block
(a moving CV with a kick) is ≈ 72.

**Burst:** a note's first one to three blocks run 4–16 vactrol sub-steps:
up to +140 cycles/sample for that one 16-sample block. Eight voices
striking in the same block would spike. Capping at 8 sub-steps halves the
spike, at a few % of R in the first ms.

**Host:** the firmware shape runs at 20 ns/sample against CLEAN LP2's
7.7 ns (moving), 2.6×. The cost test moves the CV continuously, so the kick
branch runs every block; this is a pessimistic bound. If CLEAN LP2 is
~35–40 cycles on the M7, the host ratio suggests ≤ 100.

**Needs a DWT measurement on target before any firmware decision.**

**Memory:** RAM about 120 bytes a voice (states, coefficients, steps,
driver). Flash: the 244-byte table plus the kernel.

## Open questions (for the owner)

1. **The shut-gate leak and voice lifetime.**
   - The real 292B in COMBO/LOWPASS never closes. It passes bass at
     −20 dB DC (66 Hz corner) for seconds, as the cell creeps from ~1 MΩ
     to its 10 MΩ dark.
   - We kept it. A voice cannot end on "silence" in COMBO.
   - Options to decide on, not invent: end the voice when R passes some
     value and the output is under −60 dB (a policy, not the circuit); or
     only GATE (−62 dB shut) ends cleanly; or accept the leak as the
     sound. The 292C's 4.7 MΩ Rα leaks more: −14.4 dB DC at R_dark
     against the 292B's −20.1 dB.
2. **The C1 kick.**
   - A step CV fires a near-constant rail pulse, so fast triggers lose
     velocity (1 dB over 1 → 0.2). It is in the drawing, but it would be
     worth confirming on a real 292B or a clone.
   - In Chimera, ENV attack 0 is a step.
3. **Turn-on is faster than the sheet** (1.9 ms against 3.0 ms typical),
   and it hardly depends on level. That is Najnudel's model with their
   parameters (identified on one unit, a compressor-style measurement).
   Fitting it is possible but would be ours, not theirs. Left as published.
4. **Low light** (below 0.1 mA) is a log-log extension of the sheet curve;
   nobody has published below it.
5. **Signal levels are assumptions:** AUD IN 1.0 = 2.5 V, envelope peak
   15 V. They set where the knee and the rails bite. The Buchla 200's real
   levels would pin them.
6. **Not modelled:**
   - the 741's slew;
   - the 910 pF across IC1;
   - JFET gate conduction;
   - unit-to-unit spread (ITE4339 IDSS 0.5–1.5 mA, vactrol tolerances);
   - the 292C's 220 pF/180 kΩ "bleed" parts.
7. **Cost** needs a target measurement. Also: whether to prewarp the core
   at 48 kHz (it costs nothing at sub-rate and would close the 16 kHz
   −2…−3.5 dB).

## WAVs

The lab rendered 44 WAVs (`TAKE_SETTING_VER.wav`, ref / fw / CLEAN) for
`levels.md`; they are not in the repo.
