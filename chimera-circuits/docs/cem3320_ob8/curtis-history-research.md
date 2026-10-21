> **History only.** The CURTIS lab (branch `curtis-lab`, cec352e9) voiced the CEM3320 on the 2040 REV2's structure with a resonance limiter. The owner rejected that model, and it is not in the crate; only its stimuli (`cem3320_ob8::stimuli::{bassline, stab, pad}`) came over. The chip-level model that replaced it is `research.md`. `dl/` names the ledger's downloads (service manuals, data sheet), which are not in the repo.

# CURTIS (CEM3320, OB-8 / OB-Xa 4-pole): research (#720)

Date 2026-10-20. Desktop/lab only. I read the downloads in `dl/` as images and text. None of them go in the repo.

## Sources (and licences)

| Tag | Source | Licence / status |
|---|---|---|
| [DS] | *CEM 3320 Voltage Controlled Filter* data sheet, Curtis Electromusic Specialties, © 1980, 6 pp. From curtiselectromusic.com (the product page's "Data Sheet PDF", wsimg host). Saved as `dl/CEM3320_curtis.pdf`, pages as `dl/ds-*.png`. | Copyright CES. Read only. Facts and equations are not copyrightable, and nothing was copied. |
| [PAT] | US 4,004,141, D. R. Curtis, "Linear/logarithmic analog multiplier", 1977 (expired). The data sheet cites it. Read on Google Patents. | Public patent |
| [OB8] | Oberheim *OB-8 Service Manual* (1983-01), archive.org `JL11369`, `dl/ob8_sm.pdf`. Voice-board schematic "OB-8 VOICE BOARD, sheet 3 of 3, 1682B", PDF p. 21. Text p. "4-POLE FILTER". | Copyright Oberheim. Read only. I read the values off the scan myself. |
| [OBXa] | Oberheim *OB-Xa Service Manual* 3rd ed. (1982-06), archive.org `JL11372`, `dl/obxa_sm.pdf`: spec page, filter and resonance calibration (pp. 15–20). It has no voice-card schematic, only the PCB composite (p. 64). | Copyright Oberheim. Read only. |
| [ED] | T. Wiltshire, "CEM3320 filter designs", electricdruid.net (2016). Pin roles, the OB-Xa 2-pole SVF, Pro-One and P5 rev 3 resonance taps. | Read only, for orientation |
| [Z] | V. Zavalishin, *The Art of VA Filter Design* 2.1.0: TPT, OTA stage §6.9, saturating integrator §6.11. Notes in `../../nonlinear-filter-papers.md`. | Methods only |
| [2040] | Our own 2040 research and lab, `../../circuit-filters/2040/research.md`, `ab/notes.md` (branch `circuit-2040`). | Ours |

I found no DAFx, JAES or AES paper that models the CEM3320. The reading list (`../../dafx-reading-list.md` §4) has none, and a web search found none. The nearest methods are the 2040 lab's: [Z] plus Huovilainen, D'Angelo–Välimäki, Oyama and the Wasp paper (OTA cascades, distributed nonlinearity). I read no GPL code. MAME has a `cem3320.cpp`, and I deliberately did not open it.

## 1. The chip ([DS], [PAT])

- **The cell is not an OTA.** Each section is "a variable gain cell … a current-in, current-out device (as opposed to the traditional voltage-in, current-out device)", followed by a high-impedance buffer:
  `I_OUT = (I_REF − I_IN)·A_IO·e^(−V_C/V_T)`, A_IO ≈ 0.9 (0.7–1.3), I_REF = 63 µA typ (45–85, "±25 %").
  It is Curtis's log/antilog multiplier ([PAT] fig. 2: `V_o = R·e^(−V1/h)·(I4 − I1)`), and [PAT] says signal currents into the log side "must stay one polarity".
- **The input pin is a diode.** "As the input to the variable cell is a forward biased diode to ground, it presents essentially a low impedance summing node at a nominal 650 mV." Every stage is therefore a current summer: R_F from its own buffer (100 K), R_C from the previous stage (91 K), R_B 220 K to −15 V for bias, and an internal ~1 MΩ (the cell's output impedance reflected back).
- **Pole:** `f_p = A_IO·e^(−V_C/V_T) / (2π·R_EQ·C_P)`, `R_EQ = R_F ‖ 1 MΩ` (90.9 K). The datasheet's caps are 300 pF. Control is 60 mV/decade (18 mV/oct) and temperature compensated, apart from 1/V_T (the Q81 tempco).
- **What the class A cell implies (our derivation).** In quiescence `I_IN = I_REF`. A stage's error current (the sum of the resistor currents' deviations) moves `I_IN`. The cap current is `−G·ΔI_IN`, so the stage is linear in current. However, `I_IN` cannot drop below 0: the diode turns off, the node leaves its virtual ground, and the cell's output pegs at `+I_REF·G`.
  - So the stage **slews one way only**. The output can rise no faster than `I_REF·G/C`, and it can fall without a matching limit.
  - In volts of stage error the knee is `I_REF·R_EQ` = **5.73 V**. That is about the same as the buffer swing (±6 V), unlike the 2040, whose OTA knee (2.7 V) sits far outside its ±1 V swing.
  - Signals below cutoff never get near the knee (at DC the error is ~0). Content above cutoff lands on the stage error almost in full: steps, saw resets, fast envelopes, loud highs. So the **asymmetry shows on edges and bright content first**, and on alternate polarities stage by stage, since each stage inverts.
- **Small-signal H2.** The node voltage is the diode's `V_T·ln(I_IN/I_s)`, and it moves with the signal. Solving `x + β·ln(1+x) = E` (x = ΔI_IN/I_REF, β = V_T/(I_REF·R_par)) gives `x ≈ E − β·E + β·E²/2`. Stages 2–4 have R_par = 37.7 K and β = 0.011, so H2 ≈ β·A/4 = **0.14 % at half the knee**. This matches [DS]'s "distortion in passband 0.1 % typ … predominantly second harmonic" (note 7). The same mechanism gives the sharp starvation corner: x → −1 within ~β of I_REF.
- **Buffers:** "Output swing at clipping 10/12/14 V p-p". `V_OUT(p-p) = V_CC − 3 V` around a quiescent `V_ODC = 0.46·V_CC` = 6.9 V, so ±6 V. They are class A emitter followers (source 10 mA, sink 0.5 mA nominal).
- **Resonance cell:** "the traditional transconductance type of amplifier". It has a signal input at pin 8, an input impedance of 3.6 K ± 0.9 K to ground, a current control at pin 9 ("modified linear", ≤ 100 µA spec, fig. 6 to 300 µA), and a current output internally into stage 1's input.
  - Fig. 6, gm vs I_CR: 1.0 mmho at 100 µA (spec 0.8–1.2), ~1.45 at 200, ~1.65 at 300, "max G_M" ≈ 2.2 mmho. **Fit:** `gm = 2.2 mmho·I/(I + 120 µA)`. `120 µA/2.2 mmho` = 54.5 mV, a bipolar pair's 2V_T, so this is a pair degenerated by ~450 Ω. Its large-signal knee at pin 8 is `I/gm = (I + 120 µA)/2.2 mmho`: 55 mV at low current, 210 mV near the onset. The tanh shape of that knee is an assumption; a degenerated pair is a little softer.
  - The data sheet's own design point: "When the resonance control is advanced until sustained oscillations are produced, advancing … further will merely increase the amplitude … the oscillation amplitude should be kept below the clipping level of the first stage". If it clips, "frequency shifts in excess of 5 %". Oscillation distortion is 0.5 % typ: the OTA is meant to be the ring's amplitude limiter.
  - "Amount of resonance obtainable before oscillation 20/30 dB." The 4-pole low pass needs `A_OSC` = 12 dB (×4). `R_RI = 3.6 K·(Gm_osc·R_EQ − 1)/A_osc`.

## 2. The OB-8 / OB-Xa 4-pole ([OB8], [OBXa])

- [OB8] text: "The OB-Xa had separate 2-pole and 4-pole filters for each voice, each filter utilizing a CEM3320 with different external components … In the OB-8 there is one 3320 per voice, and the external components are electronically switched (4053/4016) to generate either a 2-pole or a 4-pole slope." So the OB-8's 4-pole is the OB-Xa's 4-pole topology on one chip.
- **Values from the OB-8 voice board (sheet 3, 1682B, UX06 CEM3320):**
  - Input: the mixer bus → CX12 1 µF → **RX43 18 K** → pin 1 (stage 1's node). Stage 1's R_F is RX46 100 K 1 %. Stage 1's gain from the bus is therefore **~5 (90.9 K/18 K)**, against the data sheet's 1 (R_I 91 K). The bus is passive: oscillator outputs switched by 4016s through RX31 180 K (saw), RX32 39 K (triangle), RX35 470 K (pulse) for VCO 1, plus RX40 22 K to ground on a 4P switch. One CEM3340 saw (10 V p-p) gives ~5 V p-p at stage 1's output-equivalent (±2.5 V); both oscillators give ~±5 V; the triangle alone is hotter. So **the OB-8 runs the 3320 hot**: one oscillator sits ~8 dB under the ±6 V swing, two sit 1–2 dB under it.
  - Couplings: RX47 100 K, RX54 100 K, RX60 91 K, RX68 100 K, with R_B 220 K (RX53, RX56, RX58) to −15 V. All stages after the first are unity.
  - Poles: CX15, CX17, CX18, CX19 = 300 pF (the data sheet's). R_EE: RX63 270 Ω to −5 V (pin 13).
  - Resonance (4P): the output (pin 10) → CX20 1 µF → **RX64 130 K** (read "13ØK" on the scan; 130 K is the only reading that fits [DS]'s design rule, see below) → 4053 UX07 (Y, 4P) → pin 8, with CX16 0.001 µF to ground (a 56 kHz pole: ignored). RES CV → **RX49 33 K** → pin 9, **RX50 330 K to −5 V** (−15 µA, so RES 0 is truly off).
  - Output: → CX21 0.1 µF / RX62 75 K into the CEM3360 VCA (a 21 Hz high-pass). In our renders a 15.9 Hz blocker stands in for it, on all three filters.
- **Loop gain:** `k = gm·A8·R_EQ1`, with A8 = 3.6/133.6 = 0.0269 and R_EQ1 = 90.9 K, so k = gm·2449 Ω. The onset k = 4 needs gm = 1.63 mmho (I_CR ≈ 346 µA). At max gm (2.2), k_max = 5.39, i.e. **35 % past the onset at most**.
  - With the data sheet's 51 K, A8 = 0.066 and k_max = 13.2, 10 dB more. The OB-8's 130 K is consistent with [DS]'s `R_RI` rule for an oscillation that starts near the top of fig. 6: `3.6 K·(1.63 m·90.9 K − 1)/4` gives the onset at 130 K for gm ≈ 1.63, and fine control at the top ("the slope … becomes more gradual … finer resolution as the critical point is approached").
  - The ring's level: the OTA's knee at pin 8 is ~210 mV near the onset, which is **~7.9 V at the output**. A 1.25 % excess (k 4.05) rings at about 0.22 of that knee: ~1.8 V at the output and ~5 V at stage 1, just under stage 1's ±6 V. That is a clean sine, as [DS] intends. With the house's k(1) = 4.21 (5 % excess) the ring would reach ~4 V out and ~11 V at stage 1, which clips.
- [OBXa] calibration: the 2-pole is "trimmed until a low frequency oscillation is heard, then turned back just to the point at which the oscillation stops" (so the 2-pole never self-oscillates). "There is not a resonance adjustment for the 4-pole filter." The 4-pole has an initial-frequency trimmer and a V/oct trimmer, and voices are matched by ear against a reference voice (spread is expected).
- The OB-Xa's 12 dB (2-pole) mode is a state-variable filter built from three 3320 sections ([ED]), with a diode clipper (DX03/DX04, OB-8) in its resonance path. It is the same family as the SEM, and SEM is a separate TYPE in #720, so the OB 2-pole sound is covered there. CURTIS is the 4-pole.

## 3. How it differs from the SSM2040 (our 2040) and the Moog ladder (LADDER)

| | CEM3320 as the OB-8 runs it | SSM2040 (Prophet rev 1/2) | Moog ladder |
|---|---|---|---|
| stage | class A current-gain cell (log/antilog), linear in current | OTA (diff pair) with an attenuator: tanh knee at 2.7 V of error | transistor ladder: `tanh(x) − tanh(y)` |
| stage nonlinearity | **one-sided** (starvation at −I_REF), alternating by stage: even harmonics, asymmetric slew | symmetric (odd) unless offsets | symmetric (odd) |
| where it bends vs the swing | knee 5.73 V ≈ swing ±6 V: edges and highs hit it as the swing nears | knee 2.7 V ≫ swing ±1 V: the swing clips first | saturates smoothly everywhere |
| resonance element | OTA, `tanh`, knee rising with RES (2→8 V at the output) | SSM2020, linear | input pair's tanh |
| feedback under signal | **limits**: loud input squashes the peak; the bass loss shrinks with level | linear: peak and bass loss level-independent until the clip | soft |
| bass loss | 1/(1+k) small-signal, less when loud | 1/(1+k) at every level | 1/(1+k) |
| self-oscillation | OTA-limited sine, ~1 % past the onset | clip-limited | tanh-limited |
| RES taper | "modified linear" (fig. 6): fine near the onset | linear | linear |

## 4. Modelling decisions (lab `chimera-utils/src/curtis.rs`, branch `curtis-lab`)

- **Structure:** the house's 2040 structure ([Z] TPT one-poles, the global loop solved linearly in one step, each stage's nonlinearity on its linear error, a state clamp per stage), with the resonance OTA as a limiter on the predicted feedback, exactly like EXP's 3080: `u = x − L'·tanh((x − u_l)/L')`.
- **Units:** 1 = 5.73 V (the cells' knee). Full scale is 3 V (0.524 units), half the swing, matching REV2's α relation. 0 dB is therefore a little over one OB-8 oscillator, and +6 dB is both.
- **Stage:** `t = g·φ(v)`. φ is linear above −0.8, a C1 quadratic to the floor −1 at −1.2, and flat below. It alternates polarity per stage. The real corner is sharper (~1 % of I_REF); 0.8/1.2 is softened for 1× aliasing. **Dropped:** the β·E²/2 small-signal H2 (−60 dB, inaudible).
- **Swing:** a hard clamp at ±6 V (±1.0477 units), as REV2 clamps. The 3320's buffers are emitter followers into rails. I assume they are symmetric about V_ODC; a real chip's ±30 % V_ODC spread would make them lopsided.
- **Resonance OTA:** `L'(k) = k·IH/(A8·(GMAX − k/RK))/U_K`, a per-block constant on the unit. Its knee rises with RES, so at low and mid RES the feedback saturates at normal levels.
- **RES law:** the house `4·RES/0.95` to the onset (so A/Bs match k), then +1 % to k 4.05 at RES 1, the OB-8's place just past the onset (the data sheet's "below the first stage's clip" design). The chip's own fig. 6 taper (k = 5.39·r/(r + 0.33) for a linear RES CV) puts k 3.25 at half RES. It is noted as a voicing choice and not adopted for the A/B.
- **Assumptions, not in any source:** the OTA's tanh shape (a degenerated pair is softer), the starvation knee's width, symmetric swing, the 130 K reading of RX64, and that the OB-Xa's 4-pole matches the OB-8's (stated by [OB8]'s text, not checked on a schematic).
