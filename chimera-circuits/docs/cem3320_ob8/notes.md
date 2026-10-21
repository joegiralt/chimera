# CURTIS (CEM3320, from the chip): lab notes

> Ported into `chimera-circuits/src/cem3320_ob8/` (see `README.md`). Paths to `dl/`, `crops/`, `out/` and `../curtis/dl/` name the lab ledger (private notes), which holds the downloads and WAVs; none of them are in the repo.


Desktop only. Nothing touched the unit or the firmware crates, and nothing was pushed. Sources and the derivation are in `research.md`.
- **Code:** lab branch `cem3320-lab`, file `chimera-utils/src/cem3320.rs` (commit 19c87c8e); now `chimera-circuits/src/cem3320_ob8/`.
- **Commands (lab, not ported):** `chimera-utils cem3320-validate | cem3320-probe | cem3320-ab [FILTER] | cem3320-onset | cem3320-cost`, with env `CEM_OUT`. Their numbers are checked by `cem3320_ob8::tests` (`cargo test -p chimera-circuits --release -- --ignored cem3320` for the reference's).
- **Lab switches**, now plain fields with the lab's defaults:
  - `CEM_C8` (pin-8 cap) → `Chip::c8`
  - `CEM_RE` (data-sheet re) → `Chip::re`
  - `CEM_W` (rounded diode corner) → `Fw::w`
  - `CEM_NEWTON` (0 linear OTA, 1 poly, ≥2 exact tanh) → `Fw::newton`
  - `CEM_TRIM` (cutoff trim) → `render_ref`'s `fc_trim`
  - `CEM_NOCACHE` (the disk cache, not ported)
- **Outputs in `out/`:** `validate.md`, `probe.md`, `onset.md`, `cost.txt`, `ab/table.md` and the WAVs. `noCX16/ab/table.md` is the RES 0.85 set with CX16 removed. `out/cache` holds the reference renders (133 MB), which can be deleted.

## The bottom line

- **The cell is not an OTA and not a tanh stage.** It is Curtis's translinear log/antilog current integrator (US 4,004,141 fig. 3):
  - a diode-to-ground input
  - a transistor carrying G·I_D
  - a pnp mirror sourcing G·I_REF
  - so `I_OUT = G(I_REF − I_D)`.

  It is exactly linear in current until the input current reaches zero, then it starves one way.
- **Honest size of the difference from REV2.** At normal playing levels (one or two oscillators, −12 … +6 dB) and RES 0 or 0.5, CURTIS and REV2 **null to −21 … −43 dB, with third-octave differences ≤ 0.5 dB. They are the same sound.** At RES 0.85 the chip alone nulls at −12 … −19 dB, with ≤ 1 dB band differences.
- **Why, from the physics.** Both chips are four matched one-pole integrators in a negative-feedback loop, so both are `1/((1+s/ω)⁴ + k)` at the same k, and both are run well inside their limits.
  - The 3320 cell has no small-signal nonlinearity beyond the pin's `V_T·ln I_D`: H2 at −91 dBc at 0 dB.
  - Its limits are I_D = 0 (63 µA of error, 5.7 V of stage error) and ±6 V rails. At 0 dB one OB-8 saw puts stage 1 at ±2.3 V and its resets at 80 % of the starvation current.
  - The 2040 at its own calibration is just as linear.
  - Linear systems with the same poles and loop gain null. **No honest model of the 3320 can make it sound unlike a 2040 at those settings.** The rejected prototype sounded like REV2 because the chip does, not only because of how it was built.
- **Where the chip (and the OB-8 around it) really differs:**
  1. **Self-oscillation.** Stage 1 starving and hitting its rails limits the ring, not the OTA (the OTA's knee is ~9 V at the output). The ring is **+2.3 dB louder** than REV2's (0.94 vs 0.72 of full scale at RES 1), asymmetric (H2 −54 dBc, +4…6 % DC shift), and 0.2–0.3 % THD. REV2's ring is symmetric: H2 below −118 dBc.
  2. **The OB-8's CX16 (1 nF at pin 8).** It lowers the onset k from 3.98 at 250 Hz to 3.29 at 15 kHz. **At RES 0.85 the OB-8 squeals at high cutoff where REV2 is calm:** the cutoff sweep at RES 0.85 shows +8 … +19 dB above 3 kHz, LSD 4.8–8.5 dB. This is the board, not the chip. With CX16 removed the same sweep is within 0.6 dB LSD.
  3. **Clipping at +9 dB and up.** The rails are lopsided (the bias network leaves the stages at 6.95/7.14/6.93/7.14 V), so CURTIS clips with **H2 −47 dBc and compresses less**: at +9 dB −0.6 dB (vs REV2 −1.9) and H3 −29 dBc (vs REV2 −19).
  4. **Drive at low and mid RES.** The degenerated-pair OTA bends: the bass loss shrinks by 0.9 dB at RES 0.5, +9 dB, and by 2.1 dB at RES 0.25. This is about what REV2 does by clipping, so it is not distinct.
  5. **Knob law** (not used in the A/B; all filters were compared at the same k). Fig. 6 is "modified linear". With the pin-9 current linear in RES and RES 1 at k 4.21, k(0.25, 0.5, 0.75) = 2.55, 3.46, 3.93, against the house's 1.05, 2.1, 3.16, and the onset moves to RES ≈ 0.82. This is the chip's real feel and it is a free per-block choice.

## Calibration ("0 dB")

- One digital unit = one OB-8 CEM3340 saw (10 V p-p) at the bus through RX31 180 K + RX43 18 K = **25.3 µA peak into pin 1**. That puts stage 1 at ±2.29 V at DC. It matches REV2's "full scale = the Prophet's maximum saw".
- Drive range: −12 dB (a quiet single oscillator; the pulse via 470 K alone is ≈ −8 dB) through 0 (one saw) to +6 (both saws, +5.3 dB) and +9 (saws and pulses ≈ +7.5; the 39 K triangle path ≈ +10.8).
- If RX40 22 K shunts the bus in 4-pole, every level is 4.3 dB lower. My reading is that it does not (not verified).

## Validation against the data sheet (`out/validate.md`)

Reference = the OB-8/data-sheet circuit, f64 Newton on 13 nodes at 384 kHz.

| Check | Data sheet | Reference | |
|---|---|---|---|
| Quiescent buffer level | 0.46·V_CC = 6.9 V; input ~0.65 V | OB-8 6.955 / 7.123 / 6.955 / 7.123 V; inputs 0.650 V | ok |
| Control scale | 57.5 / 60 / 62.5 mV/decade | 60.4 mV/decade | ok |
| f_p vs formula, −25 … +155 mV | exponential error 4 % typ, 12 % max, mostly top octaves | −5.5 % (−25 mV) … +7.3 % (+155 mV); ±1.6 % from 0 to 90 mV | within max (the top end is r_e, 8 Ω, mine) |
| f_p after `g_set` (OB-8, 50 Hz – 12 kHz) | — | +0.01 … +0.31 % | renders sit on FREQ |
| Pole-control range | 3500:1 min | by construction (exponential) | — |
| Output swing at clip | 10 / 12 / 14 V p-p | 11.4 V p-p (−5.1/+6.3 V: the data-sheet circuit's DC-coupled R_I shifts stage 1) | ok |
| Passband distortion, 3 dB under clip | 0.1 % typ, 0.3 max, mostly H2 | 0.017–0.031 % (H2 −71 … −78 dBc) | under typ |
| Bandreject distortion | 0.3 % typ, 1 max | 0.11–0.12 % (H2 −58 … −59 dBc, H3 below −86) | under typ, H2-dominated as stated |
| Resonance gm at 100 µA | 0.8 / 1.0 / 1.2 mmho | 1.03 | ok |
| fig. 6 at 200 / 300 µA, max | ≈1.4 / ≈1.6, max ≈2.2 | 1.40 / 1.59, max 1/R_E = 2.2 | ok (fitted, one parameter) |
| Resonance input impedance | 3.6 K | 3.6 K | by construction |
| "From zero to oscillation" at ≤ 100 µA | yes | onset at 49.8 µA (k_max 6.15 at 100 µA) | ok |
| Resonance before oscillation | 20 min, 30 typ dB | peak +34 … +36 dB re DC at k 3.96–3.99 (+31 at 3.90) | ok |
| Oscillation distortion, unclipped | 0.5 % typ, 1.5 max | 0.13 % (1.1× onset) … 0.49 % (1.6×, stage 1 at 5.9 of 6 V) | ok |
| Frequency shift when clipped | > 5 % when clipped, < 0.5 % unclipped | < 0.5 % up to 2× onset (100 µA, stage 1 just at its rail) | the unclipped half checks; hard clipping is not reached at ≤100 µA |
| Noise re max output, LP at 20 kHz | −76 min, −86 typ dB | −90.7 dB (white shot + thermal only, no 1/f) | 4.7 dB quieter than typ; the missing 1/f and excess noise are the likely gap |
| Gain-cell output impedance | 1 MΩ reflected, 0.5–2 | 1.00 MΩ (V_A 126 V each) | fitted |

Not checked: CV and resonance feedthrough (trim-dependent and AC-coupled out), warm-up drift, and tempco.

## Firmware model vs reference (`out/probe.md`, `out/onset.md`, `out/ab/table.md`)

fw = f32, 1×, one TPT step a sample, per-block coefficients (`Fw` in `research.md` §5).

- **Musical A/B, fw vs ref:**
  - RES 0 / 0.5: **null −32 … −57 dB, LSD 0.03–0.17 dB**.
  - RES 0.85: −23 … −40 dB, 0.06–0.42 dB.
  - The RES 0.85 cutoff sweep self-oscillates in both, so its null is 0, with LSD 0.9–1.5.
  - RES 1 rings free-running, so the null is ~0; LSD 0.2–1.4 dB.
- **Passband sine 220 Hz at FREQ 2 kHz:**
  - gain −0.23 dB on both, −12 … +6 dB
  - at +9 dB: ref −0.63 / H2 −47 / H3 −29, fw −0.51 / −45 / −32
  - at +12 dB: −2.80 / −44 / −17 against −2.62 / −41 / −17
- **Resonance vs drive (110 Hz saw, FREQ 1 kHz), H9 (the peak) re H1:**
  - RES 0.5: ref −14.4 → −22.6, fw −14.5 → −22.6 (−12 → +12 dB)
  - RES 0.85: ref +3.6 → −2.0, fw +3.4 → −2.1
  - centroid within 6 Hz
- **Bass loss (55 Hz, FREQ 1 kHz):** within 0.05 dB at −12 and 0 dB at RES 0.25–0.85. At +9 dB fw keeps 0.2 dB more loss (e.g. RES 0.25 −4.23 vs −4.05). At RES 0.95 the reference is already oscillating (CX16), so that row is not a bass-loss number.
- **Self-oscillation onset k:**

  | FREQ | 250 | 1 k | 2 k | 4 k | 8 k | 12 k | 15 k |
  |---|---|---|---|---|---|---|---|
  | ref | 3.98 | 3.91 | 3.84 | 3.71 | 3.51 | 3.37 | 3.29 |
  | fw | 4.00 | 3.92 | 3.84 | 3.70 | 3.47 | 3.31 | 3.12 |
  | ref without CX16 | 4.00 | 4.00 | 4.00 | 4.01 | 4.04 | 4.07 | 4.09 |

- **Ring at RES 1 (k 4.21):**

  | FREQ | 250 Hz | 1 kHz | 4 kHz |
  |---|---|---|---|
  | ref: peak (units) / THD / H2 / f re FREQ | 0.92 / 0.2 % / −54 dBc / 1.003 | 0.94 / 0.2 % / −54 dBc / 0.996 | 1.00 / 0.3 % / −60 dBc / 0.972 |
  | fw: peak / THD / H2 / f | 0.94 / 0.3 % / −50 dBc / 1.003 | 0.96 / 0.4 % / −49 dBc / 0.996 | 1.02 / 0.3 % / −52 dBc / 0.982 |

  The DC shift is ref +4.5 … 5.8 %, fw +2.9 … 3.4 %.
- **2× vs 1×:** within 0.1 dB on every probe, so 1× is enough.
- **Known residuals:**
  - The fw's onset runs low in the top octave (−0.17 k at 15 kHz).
  - Its ring H2 is 4–8 dB high.
  - Its passband H2 is missing (the −91 dBc diode-node term).

## CURTIS vs REV2 and LADDER, in numbers (`out/ab/table.md`)

Null = residual after the best scale, dB. LSD = mean |Δ| of third-octave levels.

| Setting (music: bassline, stab, pad) | ref vs REV2 null / LSD / max band Δ | ref vs LADDER null / LSD |
|---|---|---|
| RES 0, −12 … +6 dB | −32 … −43 / 0.10–0.15 / ≤ 0.4 dB | −22 … −38 / 0.04–1.14 |
| RES 0, +9 dB | −22 … −40 / 0.15–0.72 / ≤ 1.6 | −17 … −20 / 0.86–1.99 |
| RES 0.5, −12 … +6 dB | −21 … −37 / 0.12–0.41 / ≤ 1.1 | −15 … −30 / 0.13–1.52 |
| RES 0.5, +9 dB | −20 … −32 / 0.73–3.11 / ≤ 2.2 | −13 … −19 / 1.66–3.27 |
| RES 0.85, −12 … +6 dB | −8 … −18 / 0.56–1.07 / ≤ 4.9 | −5 … −12 / 0.98–2.52 |
| RES 0.85, same, **CX16 removed** | −12 … −19 / 0.16–0.51 / ≤ 1.5 | — |
| RES 1 (ringing) | ~0 (free-running phase) / 0.98–3.14 / ≤ 9.3 | ~0 / 3.0–11.1 |
| Cutoff sweep RES 0 / 0.5 | −26 … −35 / 0.27–0.46 | −10 … −34 / 0.30–1.70 |
| Cutoff sweep RES 0.85 | −0 … −6 / 4.1–8.5 / up to 34 dB (CURTIS squeals above ~8 kHz) | −0 … −2 / 4.5–9.1 |
| RES sweep into oscillation | −0 … −4 / 1.1–1.6 / ≤ 1.9 | −0 … −3 / 4.1–4.9 |

- **Per region at RES 0.85 (chip and CX16), ref − REV2:** below 300 Hz −0.4 … −0.9 dB (slightly more bass loss), 300 Hz–3 kHz −0.5 … +1.6 dB, above 3 kHz +0.2 … +1.6 dB, at the peak up to +4.9 (stab). That is a slightly bigger, brighter peak at the same k. REV2's cubic stage trims its peak.
- **LADDER is the outlier at every setting:** its input pair bends from 0 dB and its ring is 12–18 dB quieter (0.11–0.23 of full scale).

## Cost (`out/cost.txt`, `m7probe/`)

- **M7 static count** (thumbv7em, cortex-m7, opt-level 2; steady loop as `K2040::steady`, with LEVEL ramp, LED peak and input sum):

  | | loop instructions | vdiv | measured / estimated cycles |
  |---|---|---|---|
  | REV2 | 109 | 0 | 173 (measured on the unit) |
  | EXP | 132 | 0 | 231 (measured) |
  | **CURTIS fw** | **144** | 1 (lagged, off the critical path) | **≈ 230–250** by REV2's / EXP's cycles-per-instruction |

  Over the ~200 target by 15–25 %. **2× is out (≈ 460+).**
- **Voices** (default VA Sound, `c2040-cost` at +57 / +80 over REV2): rev V **8 → 7**, rev Y 6 → 6. Rev V holds 8 only up to +32.
- **Host:** fw tick 55 ns/sample, REV2's whole `Filter::process` 48 ns, the reference 16.6 µs.
- **Getting under 200 (not done, each measurable):**
  - Fold CX16 into a per-block k(FREQ) and FREQ trim instead of the in-loop one-pole: about −8 instructions. It gets the onset right but not the ring pitch.
  - Shorten Q to x⁷ and the derivative to x⁴: about −4.
  - Keep the coefficients in registers: 28 `vldr` in the loop, mostly polynomial constants.
  - A bench on the unit would replace the estimate.

## WAVs

- **Path:** (private notes), 32-bit float, 48 kHz, mono. There are 340 files.
- **Stimuli:**
  - bassline (one saw, plucked filter envelope)
  - stab (two A m7 stabs, two saws a note)
  - pad (five notes, two saws a note)
  - cutoff_sweep (110 Hz saw, 40 Hz → 15 kHz)
  - res_sweep (RES 0 → 1, then the ring alone from 4.5 s; file tag `r0to100`)
- **Settings:** RES 0, 0.5, 0.85, 1 at the same k on all filters (`k = 4·RES/0.95`). Drive −12, −6, 0, +6, +9 dB.
- **Level matching:** within each stimulus × RES set, every filter is scaled so its 0 dB render is −24 dBFS RMS. The same gain is kept at the other drives, so **level changes across drives are the filter's own compression**, not gain. The output blocker is 15.9 Hz on all.
- **Start with:**
  - `cutoff_sweep/r085_+00dB` a vs c: the OB-8 squeal at high cutoff (CX16)
  - `res_sweep/r0to100_+00dB`: the onset and the louder, asymmetric ring
  - `bassline/r100_-06dB`
  - `stab/r000_+09dB` a vs c: lopsided clip vs REV2's symmetric one
  - For the null: `pad/r050_+00dB` a vs c. They should be indistinguishable, and that is the finding.

## Open questions

- **RX64 130 K** ("13ØK") and the **RES CV range**. As read, the OB-8 4-pole reaches k 3.85 on a 10 V RES CV and so does not self-oscillate at mid cutoff. A real OB-8 or OB-Xa recording would settle it: does the 4-pole ring, and at what cutoff does it start?
- **CX16 1 nF** is the single biggest audible difference from REV2. Confirm it on a second scan or a unit.
- RX40 22 K's mode, and the stage-2–4 R_C (91 K vs 100 K).
- **Prophet-5 rev 3:** its schematic was not obtained, so the canonical Rev 2/Rev 3 pair is untested. Its R_RI, input resistor and any pin-8 cap would move the onset and ring the same way CX16 does here.
- **Distortion and noise are below the data sheet's typicals** (0.02 vs 0.1 %, 0.11 vs 0.3 %, −91 vs −86 dB). Mismatch, β droop, 1/f and excess noise are not modelled. All are inaudible at these levels, but they are not verified.
- **Whether to adopt fig. 6's RES taper** (a feel choice, free), and whether the OB-8's no-oscillation-at-10-V is wanted or RES 1 should keep the house k 4.21.
- **The owner's ear.** Given the nulls, the case for a CURTIS TYPE rests on the high-RES and board behaviour (louder asymmetric ring, high-cutoff squeal, lopsided clip). Whether that is enough to justify ~60 extra cycles and a voice on rev V is a product call, not a modelling one.
