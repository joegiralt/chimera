# CURTIS from the chip: a CEM3320 model built from its own circuit

> Ported into `chimera-circuits/src/cem3320_ob8/` (see `README.md`). Paths to `dl/`, `crops/`, `out/` and `../curtis/dl/` name the lab ledger (private notes), which holds the downloads and WAVs; none of them are in the repo.


Date 2026-10-20. Desktop and lab only. This replaces the rejected #720 CURTIS. That one was the 2040 REV2 structure with a resonance limiter, and nothing of its filter model is used here: no TPT-2040 loop, no constants, no `floor_neg`, no `limit_of`. I kept only the lab's stimuli (`curtis::{bassline, stab, pad}`, `circuit2040::{cutoff_sweep, res_sweep, saw}`), its WAV writer and its measurement helpers (`tone`, `bands`, `lsd`, `ac`, `pad_run`), plus the m7probe static-count method.

Code: lab branch `cem3320-lab` (19c87c8e), file `chimera-utils/src/cem3320.rs`; now `chimera-circuits/src/cem3320_ob8/` (`chip.rs`, `reference.rs`, `fw.rs`), its validation as tests (`tests.rs`). The lab's printers (`cem3320-validate | -probe | -ab | -onset | -cost`) were not ported; their tables are `validation.md` and `probes.md` here. The M7 count was in the ledger's `m7probe/`.

## 1. Sources and provenance

| Tag | Source | Status / use |
|---|---|---|
| [DS] | *CEM 3320 Voltage Controlled Filter*, Curtis Electromusic Specialties, © 1980, 6 pp. (curtiselectromusic.com reissue of the original CES sheet; `../curtis/dl/CEM3320_curtis.pdf`, pages `ds-1…6.png`) | Copyright CES. Read only. The numbers and equations I use are facts. Nothing is copied. |
| [PAT] | US 4,004,141, D. R. Curtis, "Linear/logarithmic analog multiplier", filed 1975, granted 1977. [DS] cites it ("Covered by U.S. Patent #4,004,141"). I read it on Google Patents (patents.google.com/patent/US4004141A). | Expired public patent. **Its fig. 3 is the CEM3320's cell topology.** |
| [OB8] | Oberheim *OB-8 Service Manual* (1983), archive.org `JL11369`. Voice board sheet 3 of 3, 1682B, PDF p. 21 (`../curtis/dl/ob8_sm.pdf`, crops `ob8vcf-21.png`, `ob8in-21.png`, re-cropped in `crops/`) | Copyright Oberheim. Read only. I read the values off the scan. |
| [OBXa] | Oberheim *OB-Xa Service Manual* (1982), archive.org `JL11372`: the 4-pole has no resonance trim | Read only |
| [ED] | T. Wiltshire, "CEM3320 Filter designs", electricdruid.net (2008, updated 2018), with its comment thread | Read only. Used for orientation: pin roles, Pro-One and P5 resonance tap after a buffer, and the remark that "the signal level when the filter is oscillating is much less at lower frequencies" in the plain data-sheet circuit. |
| [AS] | Alfa AS3320. The Erica and Alfa PDFs were HTML stubs or 403 on fetch, and the alldatasheet page refused. | **Not obtained.** Nothing here relies on it. |
| [Z] | V. Zavalishin, *The Art of VA Filter Design* 2.1.0 (TPT integrators) | Method only. Used in the firmware model. |

**Literature search** (owner's request: arXiv, DAFx, JAES/AES, ICMC, done by web search because the arXiv MCP server was down). I found **no published circuit-level model of the CEM3320, its current-mode cell, or a close relative.**
- DAFx: the nearest are *A Virtual Analog Model of the EDP Wasp VCF* (DAFx20in22, an OTA filter) and Esqueda/Kuznetsov/Parker, *Differentiable white-box VA modelling* (DAFx20in21). Neither treats a log/antilog current cell.
- JAES/AES/ICMC: nothing on the 3320.
- Log-domain filter literature (Frey 1993, *IEE Proc. G* 140(6); Mulder et al., *Dynamic Translinear and Log-Domain Circuits*, Springer) describes the class this cell belongs to, an externally linear translinear current integrator. It gives no instrument-level model.
- A KVR thread claims the 3320 and the Moog ladder "differ only in coefficients" once reduced to nonlinear equations. That is opinion with no derivation, and it was not used.
- MAME has `cem3320.cpp`. A search snippet calls it BSD-3; I did not open it, per the brief.
- No die photo or transistor-level teardown of the 3320 was found.
- **So the model below is derived here**, from [PAT], [DS] and textbook device equations (Shockley and Ebers–Moll, with Early effect). Nothing was used under any licence.

Prophet-5 rev 3: I could not get its service schematic in this session (sdiy.info links to manuals that I did not open). Published Rev 2 vs Rev 3 comparisons are listener opinion: "SSM more organic, darker; CEM cleaner"; "the Rev 3 filter seems to disappear as it closes". The GForce/Sequential remark that the Rev 3 cutoff sits an octave up comes from the board's capacitor values. None of these are measurements. So the instrument modelled is the **OB-8 4-pole**, as the brief allows.

## 2. What the cell actually is

From [PAT] fig. 3 (Curtis's "ganged quadruple-output multiplier") and [DS] p. 2–3:

```
            I_in (resistor currents)            G·I_REF (pnp mirror of the
                 │                              reference transistor's collector)
   pin ──────────┼──────┐                              │
                 │      │ base                         ▼
              diode ▲  Q_n ───────── collector ──── cap node ── C_P ── gnd
                 │      │ emitter                      │
                gnd     └── common emitter node ──     └── buffer ── OUT
                           (held by the exponential converter, shared by
                            all four cells and the grounded-base reference)
```

- **Log side.** The input pin is "a forward biased diode to ground … a low impedance summing node at a nominal 650 mV" ([DS]). The pin voltage is `V_T·ln(I_D/I_s)`.
- **Antilog side.** Q_n's base sits on that diode, and its emitter is on the node the exponential converter holds, so `I_C = G·I_D` with `G = A_IO·e^(−V_C/V_T)` ([PAT] eq. 33: `I_o,n = I1·e^(−V1/h)·(I_n/I_6 − 1)`).
- **Reference.** A grounded-base reference transistor fed by `I_REF` carries `G·I_REF`. A pnp mirror sources that current into Q_n's collector node. So the cap gets `I_OUT = G·(I_REF − I_D)`, which is exactly [DS]'s equation.
- **What follows from that structure:**
  - **Linear in current, by construction (translinear).** There is no tanh anywhere in the signal path. The cell is not an OTA and not a differential pair. It is a single-ended current-mode integrator.
  - **One-sided limit.** `I_D ≥ 0`, so `I_OUT ≤ G·I_REF`: the cap can rise no faster than `G·I_REF/C` but falls freely. When the resistor currents into the pin go below zero, the diode turns off and the pin leaves its virtual ground. [DS]'s absolute maximum allows the cell input to go to −6 V, which fits. The corner is sharp. The pin's own impedance (`V_T/I_REF` = 410 Ω against R_par of 37.7 K or 100 K) makes it ~0.4–1.1 % of I_REF wide.
  - **Swing.** The cap node saturates its output transistors. The buffer's swing is `V_CC − 3 V` p-p about 0.46·V_CC ([DS]). Each stage's rails sit about its own quiescent point.
  - **The "1 MΩ reflected".** Early effect in Q_n and the mirror gives an output conductance proportional to G, which [DS] states as "1 MΩ … regardless of control voltage". With V_A = 126 V on each, `1/(I_REF·(1/V_An + 1/V_Ap))` = 1.00 MΩ.
  - **Second harmonic.** The pin voltage moves as `V_T·ln(I_D)`, so the resistor currents are not exactly the signal. That plus Early × signal is the cell's even-order distortion ([DS]: "predominantly second harmonic").
  - **Ohmic `re`.** With r_e in both the diode and Q_n, `I_C = G·I_D·e^{r_e[(I_D − I_C) − (1−G)I_REF]/V_T}`. That is linear when G = 1, and otherwise a small-signal gain error `(1+ρ)/(1+Gρ)` (ρ = r_e·I_REF/V_T). This is [DS]'s "exponential error, most in the upper two octaves".
- **Resonance cell.** [DS] calls it "the traditional transconductance type". Its gm vs I_CR (fig. 6) bends toward a "max G_M" of ≈2.2 mmho. That is a **degenerated differential pair**: `v = 2V_T·atanh(x) + R_E·I·x`, `I_out = I·x`, so `gm = I/(2V_T + R_E·I)`. With R_E = 1/2.2 mmho = 455 Ω (my fit) this gives 1.03 mmho at 100 µA ([DS] 1.0 typ), 1.40 at 200 and 1.59 at 300 (fig. 6 ≈1.4, ≈1.6), with no other parameter. Pin 8 has 3.6 K to ground ([DS]). The output current goes into stage 1's diode node ([DS]: "a current output internally connected to the input of stage one").

## 3. The instrument: OB-8 4-pole ([OB8])

- **Input.** The mixer bus goes through CX12 1 µF and RX43 18 K into pin 1 (stage 1's diode node). One CEM3340 saw (10 V p-p) reaches the bus through RX31 180 K, so it is a Norton source of **25.3 µA peak** (5 V/198 K). The node is a virtual ground, so the source is a current. **This is one digital unit (0 dB),** the same anchor as REV2's "the Prophet's maximum saw".
  - Both saws at max: +5.3 dB. Saws and pulses: ≈+7.5 dB. The triangle path (39 K): ≈+10.8 dB.
  - The A/B drive range of −12 … +9 dB therefore runs from a quiet single oscillator to "all oscillators at max".
  - RX40 22 K on a 4016 labelled "4P R" may shunt the bus in one of the modes. If it is on in 4-pole, every level here is 4.3 dB lower. I read it as the 2-pole's, which is not verified.
- **Stages.**
  - R_F 100 K 1 % (RX46 and others).
  - 300 pF poles (CX15/17/18/19).
  - Stages 2–4: R_C 91 K and R_B 220 K to −15 V. These are the data sheet's unity values. Some of the OB-8's couplings read 100 K, which would mean −0.8 dB a stage, and the 4053/4016 2P/4P switching makes the exact routing hard to read.
  - V_CC 15 V, so the quiescent points are 6.95 / 7.14 / 6.93 / 7.14 V.
- **Resonance.**
  - Pin 10 goes through CX20 1 µF and RX64 130 K (scan "13ØK") to pin 8, with **CX16 1 nF** at pin 8. That is a pole at 45 kHz in the loop.
  - RES CV goes through RX49 33 K into pin 9, with RX50 330 K to −5 V.
  - Loop gain is 2442 Ω per mho, so k = 4 needs 333 µA. A 10 V RES CV gives ≈288 µA, k 3.85.
  - **As read, the OB-8's 4-pole would not quite self-oscillate at 1 kHz on a 10 V RES CV.** Chip tolerances (3.6 K ±25 %, gm ±20 %) and CX16 (below) bring it close: at high cutoff it does oscillate.
- **Output.** CX21 0.1 µF into RX62 75 K, the CEM3360's current input. It is modelled as a load. The heard output is pin 10, with the house 15.9 Hz blocker on every filter.

## 4. The reference (`Reference`)

- **Unknowns, per sample.** Four diode-node voltages, four cap-node voltages, four buffer outputs, and pin 8: 13 in all.
- **Equations.**
  - Diode-node KCL: resistor currents, the input or the OTA current, `I_s(e^{n/V_T} − 1)`, and the antilog transistor's base current `I_C/β` (β 100).
  - Cap-node KCL: `G·I_REF·(1 + Early)` minus `I_C·(1 + Early)`, plus the two saturating junctions `I_REF·e^{(v − v_hi)/V_T}` and `I_REF·e^{(v_lo − v)/V_T}`, against the trapezoidal cap current.
  - Buffer: an emitter follower, `y = v − V_BE0 − V_T·ln(1 + I_load/I_0)` with I_0 0.5 mA ([DS] "sink a nominal 500 µA"). It drives R_F, the next R_C, and for stage 4 the CX20/R_RI and CX21/R_load branches in closed-form trapezoidal form.
  - Pin 8: CX20's current, 3.6 K, and CX16.
  - The OTA: the degenerated pair, solved by its own Newton.
- **Solver.** Dense Newton with an analytic Jacobian, LU with pivoting, and step limits on the junction nodes. It runs in f64 at **8× (384 kHz)** and converges to 1e-10 V. It averages ~2–3 iterations a step and failed to converge 0 times in any run. Host speed is ~16.6 µs per 48 kHz sample (one core). (The lab cached renders by a hash of the case and the chip; the crate does not.)
- **Cutoff.** `g_set` inverts the small-signal gain errors analytically (β and r_e), then a measured calibration of 1.0093. The pole then sits on FREQ within ±0.3 % from 50 Hz to 12 kHz.
- **Noise (optional).** White shot noise of each diode, antilog transistor and mirror, plus thermal noise of the stage resistors.
- **Parameters that are mine, not the data sheet's:**
  - r_e 8 Ω, chosen so the exponential error is in [DS]'s range
  - β 100
  - V_A 126 V on each device (fitted to the 1 MΩ)
  - R_E 455 Ω (fitted to fig. 6)
  - where the saturations sit: placed so the swing is [DS]'s 12 V p-p; their internal location is not published

## 5. From the reference to the firmware model (`Fw`)

Same equations, normalised: currents in I_REF, voltages in `I_REF·R_EQ` = 5.73 V. Each step below drops or approximates one thing, and each was measured against the reference before it was kept.

1. **Pin node.** The ±0.1 % log term and the base current are dropped (the latter is folded into the cutoff map). The diode is `d = max(1 + ε, 0)`, ε being the resistor currents' change over I_REF. A rounded corner (`w` 0.01–0.08) was tried. w ≥ 0.03 changes the ring's shape (a −0.42 DC offset at w 0.08 against the reference's +0.05). w = 0, the physical ~1 %, matches. **The corner's sharpness is part of the sound.**
2. **Cap.** `du/dt = ω·[(1 − d) − ρ·u]`, with ρ = R_EQ/1 MΩ = 0.0909 and ω = 2π·FREQ.
   - Trapezoidal with TPT prewarp, implicit per stage.
   - With the hard corner, `m·z + g·max(z,0) = c` has its z on c's side of zero, so the exact implicit step is one multiply by `1/(m+g)` or `1/m`. There is no root and no iteration.
3. **Swing.** Clamp each stage at its own rails, `(0.9 − y_q)/U` and `(12.9 − y_q)/U`. These are lopsided because of the bias network, and that is what gives H2 at clip: −45 dBc against the reference's −47 at +9 dB. Holding the state at the rail made no measurable difference, so it was dropped.
4. **Loop.**
   - The stages' linear maps predict OUT 4 (the house's zero-delay trick).
   - Pin 8's CX16 is a TPT one-pole, its bilinear g warped with the cutoff's so its phase is right at FREQ. **Without it the fw's high-cutoff resonance is wrong.** The reference's onset k drops from 3.98 at 250 Hz to 3.29 at 15 kHz because of CX16 alone.
   - CX20's AC coupling is a one-pole on the predicted OUT 4.
5. **OTA.**
   - Solve `2V_T·atanh(x) + bb·x = rhs` (bb includes the loop's linear share) by one Newton step from last sample's x.
   - Write atanh(x) = Q(x)/(1 − x²), with Q the series polynomial to x⁹ (coefficients −2/((2n+1)(2n−1))). Q(1) > 0, so |x| < 1 is kept.
   - The Newton derivative's reciprocal is last sample's: one divide, off the critical path.
   - Against an exact tanh Newton this is within 0.1 dB on every probe.
   - **A linear OTA was tried and rejected.** At RES 0.25 and +9 dB its bass loss is −5.97 dB against the reference's −4.05, and it loses the drive's darkening.
6. **Rate.** 1× only. 2× was within 0.1 dB of 1× on every probe, and it doubles the cost.
7. **Dropped as inaudible.**
   - The passband H2 at −90 dBc (0 dB).
   - The buffer's V_BE modulation.
   - CX20's effect on bass: it sits at 1.2 Hz.
   - Noise.
   - Ring pitch pulled by CX16 is kept, since it falls out of item 4.
