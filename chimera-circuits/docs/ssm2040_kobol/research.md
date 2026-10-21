# 2040 EXP from the Kobol's own circuit: research

2026-10-21. Desktop and lab only. The unit was not touched, and no firmware filter code changed. Lab code was on branch `exp-circuit-lab` (commits f17f244b, bdfa3045); it is kept in `chimera-circuits/src/ssm2040_kobol/`:
- `circuit.rs` (lab `kcircuit.rs`): the netlist;
- `fw.rs` (lab `exp2.rs`): EXP-C;
- `takes.rs` (lab `kobol.rs`, `kab.rs`, `kexp.rs`): the measures and stimuli of the A/B and the probes; `tests.rs` holds the numbers;
- `lab/` on the branch: the probe script and the M7 count (not ported; see `README.md`).

The downloads (`dl/` in the lab's ledger) were untrusted, were read as images or text only, and none is in the repo.

## Sources

| Tag | Source | Use, licence |
|---|---|---|
| [SM] | RSF, *Expander Kobol* service manual (owner's link: urielbarthelemi.com `rsf_kobol_sm.pdf`, 28 pp.). The same drawings are at yusynth.net `gear/RSF/schema-RSF-KOBOL.pdf` (22 pp.). | Schematic and parts lists, read off the scans by me. © RSF; facts only, nothing copied. |
| [DS] | SSM 2040 data sheet, SSMT 1981 (electricdruid.net scan) | Cell specs, fig. 9, design hints |
| [PAT] | US 3,969,682 (Rossum, SSM), figs. 2–3 (Google Patents) | The cell: NPN pair, PNP mirror into the cap, FET-follower buffer |
| [RS] | R. Schmitz, discrete SSM2040-style filter, © 1999, after J. Haible's discrete 2040 (schmitzbits.de/rs2040.html; "fair use only with this copyright notice") | Transistor-level topology reference only. The image is not copied anywhere. |
| [AN] | Intersil/RCA AN6668, *Applications of the CA3080* | 3080 internals: mirror W basic, X/Y/Z improved (three-transistor) mirrors, PNP Y/Z; g_m = 19.2·I_ABC mS/mA; R_O ≈ 7.5 MΩ/I_ABC(mA) |
| [ED] | electricdruid.net "Comparing OTAs" (CA3080 against AS3080E and LM13600/13700) | Owner's link. Read via the coordinator's summary: no linearising diodes on the 3080, Widlar/basic bias mirror, PNP Darlington-Wilson output mirrors |
| [EZ] | E. Zumchak, "A Short Discussion of the OTA" (schmitzbits.de, saved as `schmitz_ota3080.html`) | 3080 mirrors; the PNP mirrors need Darlingtons |
| [AG] | A. Gratz, *OTA* (draft, "do not make publicly available copies") | Cited only, not quoted |

Tools: ngspice-45, the nixpkgs build already in the Nix store (`/nix/store/7j2jk46kl3j2ivsyklcmaxca517icxa5-ngspice-45/bin/ngspice`), the same binary the #711 lab used. It is not on PATH; nothing was rebuilt. No GPL code was read or used.

## 1. The Kobol's VCF, from the schematic

[SM] "Board" sheet, 15/6/79, VCF block (yusynth p. 8; SM pp. 22 and 24 are the same drawing, worse scans). Values are from the "Expander Kobol" résistances list (20/11/79, yusynth pp. 12–14) and the capacitor list (p. 13).

**Signal path**
- The VCO buses go to the mixer op-amp CI2 (R22 62 K feedback).
- From the mixer:
  - **R24 100 K** → junction → **C16 4.7 µF** (tantalum) → CI4 pin 6 (IN 1).
  - The external audio in, R25 10 K, joins at the same junction.
- Each VCO's own level is a CA3080:
  - VCO1's is CI26, its I_abc set by T10 and R157/R158;
  - its output current feeds the bus through R159 27 K.

  So "VCO1 volume" is a 3080 gain, and there is no separate mixer, as the owner said.

**CI4, SSM 2040**
- Chain pins 6→4→3→1→15→13→12→10:
  - IN→OUT feedback 10 K: R27, R30, R33, R36;
  - OUT→next IN 10 K: R28, R31, R34;
  - IN legs to ground **220 Ω**: R26, R29, R32, R35.
- Caps C2–C5 1 nF ("C1NF SIE"), on pins 5, 2, 14, 11.
- The data sheet's standard connection, with 220 Ω where fig. 9 shows 200 Ω.

**The resonance loop**
- OUT 4 → **C6 4.7 µF** tantalum → **R37 4.7 K / R38 220 Ω** → **CI5 (CA3080) pin 2 (−)**.
- **Pin 3 goes straight to ground** (checked at 600 dpi: no input reaches it).
- CI5's output current goes onto pin 6, IN 1.
- I_abc into pin 5 comes from **T3, a 2N3906 with its base grounded**. Its emitter is fed by R46 47 K from +15 and by R45 10 K from the RES CV (labelled −4/+4 V). So I_abc = 14.4 V/47 K + (V_cv − 0.6 V)/10 K:
  - 0 below V_cv ≈ −2.5 V;
  - 646 µA at +4 V.
- RES therefore sets the 3080's gain linearly.

**The output**
- The same C6 node → **R39 10 K / R40 220 Ω** → **CI3 (CA3080, "VCA linéaire")** pin 3. Pin 2 has R41 220 Ω to ground and R42 1 M from the Adj 2 "Cont Rej" trimmer.
- CI3's I_abc comes from T2/R47 22 K, driven by the ADS VCA (CI8, SSM 2050).
- CI3's output current goes into CI2A's virtual ground, R43 100 K, then R44 1 K and an output capacitor.

**Cutoff CV**
- CI6/CI7 with T4/T5 (2N3906) sum the CVF trim, keyboard (with the KBD amount, "2 V/oct" at full), the FM input, FREQ and the ADS2 amount.
- Adj 15 "F VCF" is the cutoff trim: the absolute cutoff is calibrated per unit.

**Page 1 note** ("La carte EXPANDER est la même que celle du KOBOL, hormis…")
- On the Expander, R17, R45, R50, R59, R72, R77, R84, R157, R162 and R225 are **0 Ω**.
- These are CV input resistors, whose scaling moves to the Expander's programmer:
  - R45 is the RES CV into T3;
  - R157 is VCO1's level CV.
- **None is in the filter's audio path.** The Expander's RES-knob-to-I_abc law is therefore set off this board and is not on the sheets; only I_abc ∝ RES CV is known. *(The coordinator's list read R43, R30, R39 and others; the page reads as above.)*

**Data sheet facts used**
- Cell: g_m 1/5 K typical at I_0 ≈ 10 µA, the bare pair's I_c/2V_T.
- Offset: 2 mV typical a cell, 5 max.
- Buffer: sinks 425/560/750 µA.
- Design hint 1: OUT swings ±1 V and sinks 500 µA DC.
- Fig. 9: "10 V p/p max" after its ×10, i.e. OUT 4 ≤ ±0.5 V when ringing.
- The THD-vs-level curve could not be reproduced by a bare pair under any reading of its x axis. Noted, not used.

## 2. The derived loop

- **Node gains** `a = (1/R_f)/ΣG`:
  - stage 1: 0.02148 (with R24);
  - stages 2–4: 0.02107.
- **Knees** `V_k = 2V_T/a`: 2.41 V and 2.45 V of stage error.
  - EXP's existing `STAGE_SCALE_EXP` (ADR 0105) already has this; it is right.
- **Pole** `ω = I_c·a/(2V_T·C)`. Stage 1 sits 1.96 % higher.
- **Stage 1's DC gain** from the mixer is R27/R24 = **1/10** (the Prophet's is 1/30).
- **CI5** in loop-gain terms:
  - the current into IN 1 is `I_abc·tanh(a₅·OUT4_ac/2V_T − o₅)`, a₅ = 220/4920;
  - equivalent to `k·L₅·tanh(OUT4_ac/L₅)` with **k = I_abc·R27·a₅/2V_T** and **L₅ = 2V_T/a₅ = 1.157 V** at OUT 4;
  - k = 4 at I_abc = 462 µA, and the schematic's top (646 µA) is **k = 5.59**.
  - The tanh is on OUT 4 alone. It is not a limiter on the summed loop, as today's EXP has it.
- **C6 into 3.32 K** (R37 + R38 ∥ R39 + R40) is a **10.2 Hz** high-pass on both the feedback and the output.
  - The loop does not feed DC back, so cell offsets build up as DC through the four stages. That is where the ring's lopsidedness comes from.
- **CI3**: `tanh(a₃·OUT4/2V_T)`, a₃ = 220/10220, so L₃ = 2.40 V.
- **CI5's bias current**, I_abc/2β into R37 ∥ R38 (210 Ω), is an offset that grows with RES: about 0.45 mV at k 5.6.

## 3. The reference netlist (`circuit.rs`), and what is assumed

Transistor level, Gummel-Poon BJTs:
- **SSM 2040 cell** ([PAT] fig. 2, built as [RS] but single-ended as [DS]'s block diagram):
  - an NPN pair, IN pin on the (−) side, (+) at the cell's offset;
  - a PNP mirror into the 1 nF cap;
  - an NMOS follower (the patent's FET follower) with a 560 µA sink;
  - an ideal exponential tail current, `I_c(FREQ)` exact.
- **CA3080, CI5 and CI3** ([AN], [EZ]):
  - an NPN pair on a basic-mirror bias (I_tail = I_abc/(1 + 2/β));
  - two PNP Wilson output mirrors (lateral PNPs, β 40);
  - an NPN Wilson return mirror;
  - no linearising diodes.
- **Network**: the Kobol's resistors and caps as in §1. CI2A is an ideal op-amp, gain 10⁵.
- **Solver**: Gear-2, reltol 1e-5, 192 kHz, Kaiser decimation to 48 kHz. Transients start from the cells' resting point (`uic`); a plain DC solve finds a spurious high-current state.

Assumptions (none comes from the recordings, except A6):
1. **The cell's high-side limit.** A diode from the cap to an internal 1.2 V, set for the data sheet's +1 V. The low side is the pair's own collector saturation. The die's mechanism is unpublished. Its DC curve (`circuit::dc_cell`; the lab's `spice/dccell`) bends from about +0.85 V up to +1.08 V and from −1.02 V down to −1.14 V (FREQ 1 kHz), widening about 55 mV a decade of I_c.
2. **No Early effect in the 2040 cell**, and the buffer stiff (the patent: "extremely low output impedance"). With VAF 100 and a bare NMOS follower, the netlist made 0.13 V offsets and a 2.6 % gain loss per stage. Those are model artefacts, already counted in the data sheet's offset spec.
3. **Offsets.** Cells +1.6, −2.1, +0.8 and +2.4 mV (one typical chip); CI5 0.4 mV (3080 typical); CI3 0 (trimmed).
4. **RES.** I_abc ∝ RES CV (T3), up to the Kobol's 646 µA (k 5.59). The Expander's knob law is off-board.
5. **The level.** Full VCO1 is 2.8 V peak of saw at R24.
   - Derived from the clean takes: the circuit rings at 0.367 V on OUT 4, and the Kobol's ring reads −8.5 dBFS on track2.
   - drive_f (same VCA state, sustain at max from 16:21:50 on) reads H1 −25.4 dBFS at RES 7¼ (k ≈ 2.3 by its shape).
   - So OUT 4's H1 is 52 mV, i.e. 1.78 V of H1 at the mixer, about 2.8 V peak for a saw.
   - This assumes the MOTU gain did not change between 16:22 and 16:29.
6. **The owner's unit: CI5's offset 1.5 mV** (in spec). This is the one value taken from a recording, from the ring's H2; see notes.md. As a tolerance, it is the per-voice spread STABILITY already models.

## 4. EXP-C, the firmware structure from the circuit (`fw.rs`)

Today's EXP structure, rebuilt term by term:
- **Stages.** Four TPT OTA stages with `t = g·V_k·tanh(v/V_k + off)`, the tanh a divide-free odd degree-7 polynomial with tanh's own −u³/3 (the earlier lab's quintic has 56 % of it), C1 and flat from |u| = 2: the circuit's knee and its offsets, stage 1 at its own pole (W₁ = 1.0196).
- **Swing.** A soft two-sided limit fitted to the reference cell's own DC curve (9–11 mV rms): above, knee 0.70 with bend 0.38; below, −0.80 with 0.31. It widens 5.5 % a decade of FREQ, once a block.
- **Loop.** Solved linearly in one step, through C6's coupling.
- **Resonance.** CI5 is `k·L₅·tanh(y₄p/L₅ − o₅)` on the predicted OUT 4, plus its bias-current offset.
- **Output.** CI3 is a cubic, since OUT 4 ≤ 1.2 V keeps it inside |x| ≤ 0.5.
- **RES law.** `k = 5.59·RES` (the 3080 linear in its CV, the Kobol's top). This needs the owner's decision: onset moves to RES 0.72, not 0.95.
- **Input scale.** Full scale is the Kobol's full VCO1 (`E_FULL` = 0.28 V at stage 1). That is about 11 dB under today's EXP drive (α 0.2 puts full scale at stage 1's 1 V swing).

**Dropped, because the circuit has no such element:**
- the make-up gain `1 + m(k)` (ADR 0191);
- `L = ½` on the summed loop;
- the +36-cent trim. The circuit's own pitch error is ±40 cents of its nominal FREQ, and the Kobol trims its cutoff with Adj 15 anyway.

REV2 is untouched; none of this is firmware.
