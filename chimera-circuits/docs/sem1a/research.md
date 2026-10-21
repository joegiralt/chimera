# SEM-1A VCF, circuit-level: research (epic #720, second pass)

> Copied from the circuit lab's ledger (private notes, branch `sem-circuit-lab`). Lab paths are mapped to `chimera-circuits/src/sem1a/`. The `dl/` files it names (papers, datasheets, schematic renders) stay in the ledger and are cited, not copied. The WAV sets are not kept: `chimera-utils circuit render` renders any model again.

Date 2026-10-20. Desktop only. The owner rejected the first prototype (`sem-lab-research.md`, `sem-lab-notes.md`) as "an existing model played with a little". This pass models the circuit as built. No GPL code was read; papers, datasheets and the schematic only.

## 1. What the literature has

Searched with web search and fetch (the arXiv MCP server was down): the DAFx paper archive, AES/JAES, ICMC/SMC, arXiv, and Aalto, CCRMA, QMUL and Erlangen theses. Queries covered the SEM filter, CA3080/OTA state-variable filters, diode limiting in SVF damping, and the OB-X/OB-Xa two-pole (CEM3320 in SVF mode).

**No published circuit-level model of the Oberheim SEM filter exists.** All SEM "models" are commercial (Arturia SEM V / Filter SEM, Cherry Audio, GForce OB-E), and their pages give no method. The KVR thread "What makes SEM filter so special" is forum talk; it was not used. No paper models the OB-X/OB-Xa SVF either. Electric Druid's CEM3320 page says the OB-Xa runs a 3320 as a state-variable and uses only its LP; that is hobbyist and was not used.

What does exist, and how it was used:

| # | Source | Licence / provenance | Use |
|---|---|---|---|
| L1 | L. Köper, M. Holters, F. Esqueda, J. D. Parker, "A Virtual Analog Model of the EDP Wasp VCF", *Proc. DAFx20in22*, Vienna 2022, pp. 17–24. `dl/wasp-dafx22.pdf` | **CC BY 4.0** (stated on p. 1) | **Followed.** The closest published circuit model: an OTA (CA3080) state-variable, with diodes in the resonance network and white-box nodal state-space simulation (ACME, the Holters–Zölzer method). Taken from it: (a) the method, all parts in one nodal system solved by Newton; (b) its **measured CA3080 fit**, i = α·I_ABC·tanh(β·v/2V_T) with α = 0.8635 and β = 0.9408 (Table 3), used as the pair's emission factor n = 1/β and the mirrors' scale; (c) its finding that the OTA's behaviour at its output rails matters (Figs. 7, eqs. 36–41), here modelled as Ebers–Moll saturation of the output devices, not with their fitted surfaces; (d) Shockley diodes in the resonance path (eq. 42). Their Wasp-specific parts (CMOS inverters, fitted rail surfaces) were not used. |
| L2 | Ó. Bogason, K. J. Werner, "Modeling Circuits with Operational Transconductance Amplifiers Using Wave Digital Filters", *Proc. DAFx-17*, Edinburgh 2017, pp. 130–137. `dl/bogason-dafx17.pdf` | DAFx proceedings; no open licence stated. Read only. | Confirms the OTA macromodel terms (finite output resistance, input/output capacitance) that the reference includes, with datasheet values. The WDF route was not taken: nodal Newton is simpler for 15 nodes with six nonlinear devices. |
| L3 | V. Lazzarini, J. Timoney, "A Digital Model for the Prologue Voltage Control Filter", *JAES* 72(1/2), 2024, pp. 79–88. `dl/lazzarini-prologue.pdf` (Maynooth accepted manuscript) | Author manuscript; read only. | An OTA SVF with back-to-back diodes in the damping path, the same idea as the SEM's D11/D12, modelled at block level. It confirms that diode-limited damping, not clipping, bounds such a filter ("overload limiting built into the network"). Nothing was taken from it. |
| L4 | V. Zavalishin, *The Art of VA Filter Design*, rev. 2.1.0 (NI 2018), §4.4 TPT SVF, §6.9 OTA, §6.11 antisaturator. (the first lab's `dl/zav.pdf`) | Free download from NI; read only. | The firmware model's frame (TPT SVF, the antisaturator in the damping) and the old lab's basis. |
| L5 | M. Holters, U. Zölzer, "A generalized method for the derivation of non-linear state-space models from circuit schematics", EUSIPCO 2015. | Cited by L1. Not re-read this session. | The method behind L1's simulations; ours is its plain nodal-Newton form. |
| D1 | Oberheim Electronics, *SEM-1A schematic*, VCO/VCF/VCA/ENV/LFO/PSU sheets (PSU dated 11 Nov 1975, rev. 5-30-78), Wayback copy, the first lab's `dl/SEM-1A.pdf`; renders at 300/400/600 dpi in `dl/` (`hi-6.png`, `vhi-6.png`, `c1–c3.png`, `z1.png`, `vco.png`). | A service document; facts (part values, topology) only. | Every part value in the reference. |
| D2 | Intersil CA3080/CA3080A datasheet FN475. `dl/ca3080.pdf` | Manufacturer's datasheet; facts only. | gm 9600 µS typ at 500 µA; input bias 2 µA (so β_F ≈ 125); R_out 15 MΩ at 500 µA; C_out 5.6 pF; swing +13.5 / −14.4 V on ±15; peak I_out 350–650 µA at 500 µA. Internal: Q1/Q2 pair, D1/Q3 tail mirror, PNP/NPN Wilson mirrors (Q4–Q11, D2–D6). |
| D3 | 2N4302 JFET (onsemi PN4302 listing): IDSS 0.5–5 mA, V_GS(off) ≤ 4 V. | Facts only. | The ranges. The values used (IDSS 3 mA, V_P −2 V, λ 0.005 /V) are our typicals inside them; see §3. |
| D4 | 1N4148 SPICE card IS 2.52 nA, N 1.752, RS 0.568 Ω (the common published card, as in the first lab). | Facts. | D11/D12. |
| D5 | LM301A and 741, National datasheet typicals, **from memory**: 301A A_v 160 V/mV, 1 MHz GBW and 0.5 V/µs with 30 pF compensation, swing ±14 V into ≥ 10 K; 741 swing ±13 V or more. | Not re-fetched; flagged. | A12 (C19 = 5 pF scales GBW and slew by 6), A14/A16/A17. |

## 2. The circuit as read (corrections to `sem-lab-research.md` marked ★)

The VCF sheet was re-read at 600 dpi (`dl/z1.png`).

- **Input**: VCO saw (A4's output, directly) → panel pot (50 K, at full the wiper is the top) → C15 0.15 µF → R155 220 K → A12's summing node. The input gain is −100/220 = −0.4545, high-passed at 4.8 Hz.
- ★ **The VCO saw is 7.2 V p-p.** The first lab assumed 5 V peak. This is my derivation from the VCO sheet: Q5/Q6's reset comparator trips with node Y (R23 18.2 K from Q4's ramp, R28 16.2 K to −15, R29 28.7 K from Q6) at about 0 V. That puts the ramp between 8.0 V (Q6 off) and 16.7 V (Q6 on), and A4 (−75 K/91 K) makes the 8.7 V ramp a 7.2 V saw. **Calibration: digital ±1.0 = ±3.6 V at an input, one VCO at full.** The SEM's own two VCOs at full sum to +6 dB; LEVEL's +12 dB needs the EXT inputs too.
- **A12** (LM301A, C19 5 pF, R141 22 K on +): R139 100 K feedback; LP back through R138 100 K ‖ ★ **C18 10 pF**; the damping comes back through C22. A12's output is HP; the pot board's Q4 pad takes it directly (R140 1 K only feeds the J1 jack).
- **Integrators**: A13/A15, CA3080. The + input gets HP (or BP) through R142 (R148) 100 K over R144 (R150) 220 Ω; the − input is R145 (R151) 220 Ω, with R143 (R149) 1 M to the OFFSET trims. Pin 6 drives C20 (C21) 300 pF 2 % and ★ **the gate of a 2N4302 source follower, Q39 (Q40), drain at +15, R147 (R153) 15 K to −15**. Q39's source is BP and Q40's is LP, so **the loop's BP and LP are JFET-follower outputs, not cap voltages.** The first lab left the followers out.
- **Cutoff**: one expo pair (Q41/Q42; the notes call the PNPs 2N3905) feeds both pin 5s through R146/R152, 15 K each, so each OTA gets half. The expo itself (T11/T12, the FREQ and CV mixer) is not modelled: Chimera sets cutoff in Hz, and the trim is done as a tech would (§3).
- ★ **Damping**: BP is buffered by **A14 (741)**. A14's output, not Q39's source, feeds the RES pot PS16 (50 K to ground; wiper → R157 33 K) and the tap R159 47 K / R158 10 K. From the tap, D11/D12 anti-parallel and R156 100 K go to the R157 junction, then C22 0.33 µF into the summing node. A14 also drives BANDPASS OUT.
- **Outputs**: LP via A16 (741) to K3; the pot board's Q5 takes A16's output directly. NOTCH (MODE) is PS19 50 K between the HP and LP opamp outputs, its wiper into A17 (741). S504 takes either A17 or BP (A14). C23 0.15 µF into the VCA's R174 100 K + R175 220 Ω high-passes the output at 10.6 Hz. The VCA (A20, another CA3080 behind 100 K/220 Ω) is not part of this model.
- Rails ±15 V (regulator sheet, A26/A27 from ±18.5).

## 3. The reference model (`src/sem1a/reference.rs`)

15 node unknowns: S, A12's state, C15/R155, both OTAs' ±inputs, both cap nodes, BP, LP, the RES wiper, the tap, the diode node, and the R157/C22 junction. Trapezoidal companions for C15, C18, C22, C20, C21 and A12's state. Newton with an analytic Jacobian (a unit test checks it against finite differences) and partial-pivot LU, at ×8 (384 kHz).

- **CA3080**: an Ebers–Moll pair in forward-active, exactly solved for the tail split: I_C± = α_F·I·(1 ± tanh(v_d·β/2V_T))/2. Base currents I_C/β_F load the 220 Ω dividers. The mirrors carry the split to pin 6 at scale α. The output devices fall out of the active region at +13.5 / −14.4 V (an Ebers–Moll knee, `knee()`). r_o = 7500 V / I_ABC. α and β come from L1; the rest from D2.
- **2N4302**: Shichman–Hodges in saturation and triode, with λ, and a Shockley gate–source junction. IDSS and V_P are assumptions inside D3's spread. The follower needs IDSS > 2 mA to reach +13 V (it must source (13 + 15)/15 K), so a working SEM's parts sit in the upper half. Sensitivity is in notes.md.
- **D11/D12**: Shockley, D4's card.
- **A12**: one pole, GBW 6 MHz, A0 160 V/mV, slew 3 V/µs as a tanh-limited input stage, output limited at ±14 V. **A14/A16**: ideal followers limited at ±13.5 V. The op-amp limits are `x/(1 + (x/r)²⁴)^(1/24)`: within 1 % to about 0.5 V from the swing. ★ The first lab's 8th-power limit bent signals from 10 V on, which is not how op-amp output stages behave. This mattered for its high-level results.
- **RES pot**: PS16 linear, 100 Ω track at each end (our assumption: the pot's taper and end resistance are not on the schematic). **MODE pot**: linear.
- **Trim**: I_ABC = 2π·fc·C/(α·β/2V_T·A_div·g_follower), × 1/0.9901, so the BP peak sits at fc at 1 kHz. That is T11 set against a tuner at one point.

## 4. The firmware model (`src/sem1a/fw.rs`)

Zavalishin's TPT SVF in the circuit's sign and volts. Every constant is derived from `Parts`:
- A12's limit on HP. Each integrator's input through V0·tanh(v/V0), with V0 = 2V_T/(β·A_div) = 25.0 V.
- **The followers as each integrator's rate**: b = F(v_C), so db/dt = F′(v_C)·dv_C/dt. The step is scaled by r(b) = F′(G(b))/F′(0), a cubic fitted to the 2N4302 model with weight 1/(1 + (b/2.5 V)²). This is the circuit's even-order term.
- The antisaturator on the TPT state (the first lab's dead-zone fit; the network's values are the same).
- Asymmetric state limits: −12.6 / +13.8 V, where the followers run out (gate at the OTA's swing).
- Per block: the pot law, the integrators' leak, C18's lead, A12's lag, C22's coupling, and the trimmed cutoff's droop.
