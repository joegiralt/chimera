> Kept from the circuit-2040 lab ledger (private notes, 2026-10-20). Paths and commands below are the lab's, on branch `circuit-2040`; the models now live in `chimera-circuits/src/ssm2040_prophet/` (see `README.md`). The WAVs it names stay in the ledger.

# SSM2040 / Prophet-5 rev 1/2 filter: component-level research (#711, circuit 1)

Date 2026-10-20. Desktop/lab only. Owner: "The 2040 to me still doesn't sound like a Prophet." Filters only.
Downloads (untrusted, read as images/text only) are in `dl/` beside this file; none go in the repo.

## Sources (and licences)

| Tag | Source | Licence / status |
|---|---|---|
| [DS] | SSM 2040 data sheet, Solid State Micro Technology (orig.), bitsavers `components/solidStateMusic/SSM2040.pdf`; Digisound reprint Aug 1979 (digisound80.co.uk `SSM2040.pdf`) | Copyright SSM/Digisound; read only. Facts and equations are not copyrightable; nothing copied. |
| [DS20] | SSM 2020 data sheet (bitsavers `SSM2020.pdf`) | as above |
| [TM] | Sequential Circuits, *Prophet-5 Technical Manual*, Jan 1980, archive.org `Sequential_Circuits_Prophet_5_Service_Manual` (§§ 2-7, 2-23, 4-17, 5-13; Voice schematic SD411 p. 3-21) | Copyright Sequential; read only. Values read off the scan by me. |
| [RS] | R. Schmitz, RS2040 discrete SSM2040-style filter, schmitzbits.de/rs2040.html (Haible-derived) | "Fair use only with this copyright notice"; read only, for the cell topology (diff pair, PNP mirror, follower). Not copied. |
| [Z] | V. Zavalishin, *The Art of VA Filter Design* rev 2.1.0 (TPT, OTA stage §6.9, nonlinear ZDF §6.4–6.10, aliasing §6.13) — read for ladder work, notes in `../../nonlinear-filter-papers.md` | free PDF from Native Instruments; methods only |
| [H] | A. Huovilainen, "Non-linear digital implementation of the Moog ladder filter", DAFx-04 | methods only |
| [DV] | S. D'Angelo, V. Välimäki, "Generalized Moog ladder filter, Part II", IEEE TASLP 22(12), 2014 | author post-print; methods only |
| [O] | H. Oyama, "Quantifying nonlinear behavior in digital Moog ladder filters", DAFx-26 paper 29 | CC-BY 4.0 |
| [W] | L. Köper, M. Holters, F. Esqueda, J. D. Parker, "A virtual analog model of the EDP Wasp VCF", DAFx20in22 paper 34 | CC-BY 4.0 |

No DAFx paper models the SSM2040 itself (searched the DAFx archive list in `../../dafx-reading-list.md` and the web; the nearest are [W] for OTA extended models, [H]/[DV]/[O] for distributed-nonlinearity cascades). No GPL code was read or used.

## 1. The chip ([DS])

- Four identical sections, one exponential function generator: `I_c = I_0·exp(qV_ctl/kT)`, `I_0 = 10 µA`, −18 mV/oct at pin 7, 10,000:1 range. Fig. 3 control summer (100 K inputs, 54.9 K + 1 K Tel Labs Q81 tempco; 91 K/20 K V/oct trim).
- Each section: a variable transconductance amplifier G (`I_o = (V₊ − V₋)·G`, `G = I_c/52 mV` at 25 °C, i.e. the plain differential pair's small-signal gm `I_c/2V_T`) into an external cap, then a high-impedance buffer B. Its (+) is grounded inside; the IN pin is (−).
  - Large-signal it is a bipolar pair: `I_o = I_c·tanh((V₊ − V₋)/2V_T)` (the cell is a bare pair with a mirror; [RS] rebuilds it so, and the data sheet's G = I_c/52 mV is that pair's slope). Nothing in [DS] says it is linearised.
- Generalised connection (Fig. 4): `ω₀ = G·R_a·R_i / (C(R_aR_i + R_aR_f + R_iR_f))`, gain `K = R_f/R_i`, inverting lowpass `V_o = −K·V_i/(1 + s/ω₀)` (Fig. 5). The 200 Ω attenuator R_a is "chosen for optimum control rejection" (hint d).
- **The node attenuates the stage error ~52:1.** With 10 K/10 K/200 Ω the OTA sees `a = 1/52` of `(V_in + V_out)`; so the OTA's tanh knee (`2V_T` = 52 mV) sits at **`V_k = 2V_T/a` = 2.70 V of stage error** (2.65 V on the Prophet's stage 1, whose input is 300 K). Below ~1 V of error the cell is close to linear (tanh 0.377 → 4.6 % gm loss at 1 V); above ~2–3 V it slews: the cap current saturates at `±I_c`, so the slew limit is `I_c/C = ω_c·V_k` and scales with cutoff.
- Design hints: (a) outputs swing only **±1 V** and sink 500 µA DC; (c) C > 1000 pF. Specs: input offset **2 mV typ / 5 mV max per cell**, 0.6 mV typ (3 max) for four in series; THD 0.1 % at Ein 30 mV p-p; gm tempco +0.5 %/°C.
- Fig. 9 (the "electronic music lowpass"): 100 K signal input, 10 K/10 K/200 Ω per stage, 1000 pF, output op amp ×10 (10 K/100 K), resonance through **½ SSM 2020** whose current goes straight into stage 1's node, its gain set by a transistor pair from RES CV via 15 K.

## 2. The SSM 2020 in the loop ([DS20])

- "Dual linear-antilog VCA", two-quadrant multiplier with differential signal inputs and a **current output**: `I_out = I_c·(V₊ − V₋)/14 V`, gm 1/14 kΩ at I_c = 1 mA. 0.1 % THD at 6 V p-p open loop (0.02 % in a VCF loop). So it takes volts directly and is linear across ±7 V; it is not an OTA on a 26 mV knee.
- In the loop it sees U479's output, `−10 × OUT 4`, at most ±10 V; in self-oscillation OUT 4 is about a quarter of OUT 1 (four poles at −3 dB each), so the 2020 runs at ≤ ±2.5 V, i.e. **linear**. The ADR 0096 reading (REV2's 2020 linear, no limiter) holds.

## 3. The Prophet-5 rev 1/2 voice filter ([TM], SD411 "VOICE 1", 3-21)

Read off the schematic (values legible unless marked):
- U478 SSM 2040. Caps C414, C410, C412, C411 = .001 µF.
- Stage 1 node (pin 6, I1): **R476 200 K** from the mixer bus, **R480 10 K** feedback from OUT 1, **R475 200 Ω** (scan reads 200 or 230; taken as the data sheet's 200) to ground, and the **RES FDBK VCA's output (½ U490, SSM 2020 pin 1) wired straight onto the node** as a current.
- Stages 2–4: R483/R484, R462/R465, R466/R467 = 10 K/10 K; R485, R464, R463 = 200 Ω.
- OUT 4 → R461 10 K → U479 (½ TL082) inverting, R478 100 K: **×−10** ("F", the voice's filter out). U479's output drives the 2020's (+) input (pin 3, DC-coupled) and, through C413 0.1 µF / R471 100 K (15.9 Hz high-pass), the FIN VCA (other half of U490).
- The mixer bus: MIX OSC A/B AMT VCAs (2020 current outputs, U473), and noise through R460 100 K, meet on one node that feeds R476. So the bus is a current summing node loaded by 100 K: seen from the filter, a Thévenin source behind **300 K**. Stage 1's DC gain is −10 K/300 K = −1/30 of that source.
- RES: FILT RES CV → R468 30 K into the 2020's control: **I_c (and so k) linear in RES CV**. `k = 10·R_f·I_c/14 V` → k = 4 at I_c = 560 µA.
- Cutoff: U435 (348) summer: FILT SUM CV (R4107 100 K), KEY CV (R4106 100 K through a 4066 switch: keyboard tracking is all-or-nothing at full 1 V/oct), P-MOD FILT (R4109 100 K), filter env (R4105 54.9 K), R4108 102 K to −15 V via R406 V/OCT trim; R407 INIT FREQ (2.2 M). Out through R477 54.9 K and **R479 1 K, a plain 1 % resistor — no tempco resistor** (§4-17: "The filters are not temperature-compensated, so they will drift and they will never track as well as the oscillators").
- Levels: §5-13 "use a maximum-level sawtooth and no resonance. Adjust CUTOFF near sawtooth frequency. Each OUT pole should show a 500-mV level, with progressive rounding of the waveform." Tuning §4-17: at full resonance "they oscillate with a pure sine wave", trimmed A440 at C3 and two octaves up at C5 (R407/R406).
- §2-7: four audio inputs to the filter: noise, OSC A, OSC B, and the filter output fed back by the RES FDBK VCA.

## 4. Equations the lab uses (our derivation)

Per stage i, cap voltage `V_i` (= OUT i through a unity buffer), node `n_i`:
- `n_1 = (V_in/R_in + V_1/R_f + I_res)/(1/R_in + 1/R_f + 1/R_a)`; `n_i = (V_{i−1}/R_i + V_i/R_f)/(1/R_i + 1/R_f + 1/R_a)`.
- `C·dV_i/dt = I_c·tanh((v_os,i − n_i)/2V_T) − I_clip(V_i)`.
- `I_res = (I_c2/14 V)·V_o·(1 − 0.087·(V_o/14)²)`, `V_o = −10·V_4` (cubic fitted to [DS20]'s 0.1 % at 6 V p-p).
- Pole: stages 2–4 `ω_c = I_c·a/(2V_T·C)`, `a = 1/52`; stage 1 1.9 % higher (`a₁` = 0.019595).
- Loop gain `k = 10·R_f·I_c2/14 V`; DC gain `1/(1+k)` (−14 dB at k = 4: the Prophet's bass loss, uncompensated).
- Calibration: digital 1.0 at the bus Thévenin source = −15 V, so 0.5 V at OUT 1 at DC: the manual's "500-mV level", read as peak (as ADR 0096 does). Our ±12 dB drive renders cover the p-p reading (−6 dB) and a hotter mixer.
- **Assumptions (not in any source):** the swing limit's shape (modelled as a diode soft clamp of the cap node at ≈ ±1 V, IS = 1e-14 into ±0.447 V; its level moves ±60 mV a decade of I_c); the cells' offsets (+1.2, +0.9, −0.4, −0.7 mV: a "typical" chip whose four-in-series alternating sum is the data sheet's 0.6 mV). [W] shows OTA output behaviour near the rails is decisive for a CA3080 SVF; the 2040's equivalent is this ±1 V limit, and only a measured chip can pin it.

## 5. What today's Chimera 2040 (REV2) misses

From `chimera-engine/src/dsp/filter/k2040.rs` (origin/main 5c216cdd) and ADRs 0096/0105:

1. **The OTA knee is 2.8× too far out.** REV2's stage is `g·sat(v)` with 1 V of stage error = 0.2 `sat` units, and `sat(c) ≈ tanh(0.667c)/0.667`: a tanh argument of 0.133 per volt. The circuit's is `a/2V_T` = 0.377 per volt (ADR 0105 already found this for EXP's 220 Ω node and fixed it there; REV2 was left at 1× because "the Prophet's stage node is not in the probe notes" — SD411 shows it is the same 10 K/10 K/200 Ω node as the data sheet, so REV2's scale should be 2.83×, a hair under EXP's 3.04×). **Measured (ab/notes.md): this matters far less than expected.** At the Prophet's levels the stage error stays well under the 2.7 V knee, so the circuit's OTAs add only −80…−97 dBc of distortion; the circuit's audible nonlinearity is the ±1 V swing (shared by input and resonance at stage 1), not the OTA. Fixing the knee is free and correct, but it is not what makes the difference.
2. **Hard clamp vs a soft swing limit (the measured main difference when pushed).** `clampf(d, ±R)` on every stage, at 1× rate: a hard clip with aliasing. The chip limits softly (assumption above) and the drive renders show the circuit compressing well before the hard rail would.
3. **The shaper.** The cubic `c − 4c³/27` (flat past ±1.5) has 1/2.25 of tanh's low-level cubic term and no tail; [O] shows saturator *shape* moves cutoff/drive behaviour by up to an order of magnitude in ladders.
4. **No offsets.** A real 2040 has 1–5 mV per cell × 52 of node gain: tens of mV of DC at each pole, so clipping and slewing are asymmetric (even harmonics) and the self-oscillation is lopsided. Today's model is perfectly odd-symmetric.
5. **1× rate with a one-step linear solve.** Fine at low cutoff (loop coupling `kG⁴` small, [Z] §6.6), but the OTA's operating point is under-estimated at high cutoff/drive (`nonlinear-filter-papers.md` §7 item 2). Measured below.
6. **Stage 1 is a different node** (300 K in, 1.9 % higher pole). Inaudible; free in a component model.
7. **Not misses** (checked): topology (4 inverting OTA-C poles, feedback current into stage 1's node), linear 2020, k linear in RES, bass loss 1/(1+k), no make-up gain, 15.9 Hz output coupling (C413/R471, a DC blocker exists in our voice chain).

Outside the filter core (noted, not in scope): the Prophet's cutoff is not temperature-compensated (R479 is a plain resistor: per-voice drift); keyboard tracking is a full-or-nothing switch; the 2050 envelopes and the 2030 VCOs shape "a Prophet" at least as much as the filter.

## 6. Modelling methods (from [Z], [H], [DV], [O], [W])

- The 2040 stage is Zavalishin's OTA stage (§6.9): saturator at the integrator input, `y' = ω·V_k·tanh((x − y)/V_k)`; it does not bound the state, so the swing limit is a separate element ([Z] §6.11 antisaturator / saturating integrator; [W]'s rail model).
- Distributed nonlinearity (all four stages) is what [O] finds matters; keep it.
- Reference: a converged implicit solve of the node equations at high rate, or SPICE ([O] uses SPICE; [W] measures hardware). We use ngspice (below) plus our own Newton/trapezoidal model.
- Cheap paths: [Z] §6.6 linearise-at-previous-point (one Newton step), Padé tanh; whole loop oversampled, not just the shaper ([Z] §6.13).

## 7. Reference method used

ngspice-45 (nixpkgs build, user-space; lab tool only, never in the repo), on our own netlist written by `chimera-utils/src/circuit2040.rs::netlist` from §§ 3–4: per stage a bipolar NPN pair (generic model is=1e-14 bf=200 vaf=100) on an ideal tail current (the expo converter as an exact `I_c(f_c)`), collectors sensed and mirrored into a 1 nF cap, diode soft clamp, unity buffer; the Prophet's resistor network; U479 as an ideal ×−10; the 2020 as a behavioural current. Gear-2, reltol 1e-5, max step 1.3 µs, output at 192 kHz decimated to 48 kHz (Kaiser polyphase). Inputs at 192 kHz (band-limited upsample of the 48 kHz stimuli).
