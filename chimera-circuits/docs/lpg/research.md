# LPG circuit research: the Buchla 292 as a circuit, and its vactrol

2026-10-20. Papers, schematics and datasheets only. No GPL code read
(Surge's LPG stays unread). The arxiv MCP server was down, so the search used
WebSearch/WebFetch over DAFx, arXiv, AES, SMC, HAL and university pages.
The lab kept local copies of the 292B drawing, the 292C schematic excerpts,
the VTL5C3 sheet and the two DAFx papers for reading only. None is in the
repo (licence): each is cited by its source below. No schematic image is
copied.

## What exists

| Work | What it models | Licence / provenance | Used? |
|---|---|---|---|
| Parker & D'Angelo, "A digital model of the Buchla lowpass-gate", DAFx-13 (Aalto) | 292 audio path (canonical, 3 modes), TPT discretisation, control path (292C-style, Lambert-W approximations with a fitted γ = 0.001), LDR law eq. 39, a heuristic vactrol (one-pole, 12 ms up / 250 ms down) | no licence stated; equations and circuit facts used, own code | **yes**: audio-path equations (3–12) and the TPT solve (§2.2.2) for the firmware core; eq. 39's *form* for the LDR law. **Not** their vactrol (a heuristic, which they say themselves) nor their control-path fit (γ fudge) |
| Najnudel, Müller, Hélie & Roze, "Power-balanced dynamic modeling of vactrols: application to a VTL5C3/2", DAFx-23 (UVI / IRCAM) | Physical photoconductor: Shockley-Read-Hall carriers + traps (Iverson model), LED softplus law, optical power law, identified on a real VTL5C3/2 | **CC-BY 4.0** (credit: Najnudel et al., DAFx-23) | **yes**: eq. 4 (carrier ODE), eqs 7–8 (resistance), eq. 13 (LED), Table 1's identified ν±, qτ, µ±, Rℓ, Rd, Vt, Vs, Is. **Not** eq. 12 as printed (see below) |
| Najnudel, PhD thesis, Sorbonne 2022 ("Modélisation à bilan de puissance équilibré…") | the full derivation of the above, §5.3 method | thesis (HAL); full text not found online | cited only |
| Iverson & Smith, "Mathematical modeling of photoconductor transient response", IEEE TED 34(10), 1987 | the carrier physics Najnudel builds on | journal | background |
| Eichas & Zölzer, "Modeling of an optocoupler-based audio dynamic range control circuit", SPIE 9948, 2016 | signal-based VTL5C2 compressor model (static curves + low-pass filters) | paper | read the abstract; superseded by Najnudel for our purpose |
| Wright et al., DAFx-22 (grey-box compression); Simionato & Fasciani, DAFx-22 (neural optical compression) | LA-2A-style opto compressors, learned | papers | not used (black/grey-box, needs the device) |
| Rohs, McGill MUMT618 report (2018) | a student summary of Parker & D'Angelo | report | not used |
| Lanterman, Georgia Tech ACMS HW5 (2015) | homework on the 292C Sallen-Key core and the LED driver | course page | context only |
| low-pass-gate.readthedocs.io (a 292-style clone, after Lanterman/Bergman/White) | Sallen-Key Q and ωn for R1 = R2 = R, 470 Ω LED resistor, 3.9 V zener | no licence stated; facts only | cross-check only |
| **Buchla Associates drawing B21208 "Mod 292B Quad Lo Pass"**, 10/12/70, rev. 5/71 and 7-76 (copy via modularsynthesis.com/mems) | the complete schematic | a historical drawing; component values are facts | **yes: the reference circuit** |
| Buchla 292C schematic excerpts (modularsynthesis.com/roman, two drawings) | 292C core values, the three-way mode switch | facts | yes: COMBO and the cross-check of values |
| PerkinElmer VTL5C3 sheet (2001) and the dual-cell sheet (VTL5C3/2 row, via search; perkinelmer.com unreachable) | static R(If), response times, LED Vf | datasheet | **yes**: the static law and validation targets |
| 1N5228B sheet values (3.9 V at 20 mA, Zzt 23 Ω, Zzk 1.9 kΩ at 0.25 mA) | the 292B's limiter zener | datasheet | yes |
| Make Noise LxD and Optomix manuals; Doepfer A-101-2; effectsdatabase 292e | behaviour only (STRIKE, DAMP, combo mode, "292e functionally identical to the 292 plus velocity inputs") | manuals | behaviour context; no circuit used |
| Mutable Instruments (Plaits/Streams LPG) | code exists (MIT) | not read at first (papers only, by brief); Plaits' LPG was ported in the gate review, below | later: see below |

Nothing newer than Najnudel (2023) on LPGs or vactrols turned up in DAFx,
arXiv, AES or SMC. No WDF or state-space treatment of the 292 has been
published beyond Parker & D'Angelo's TPT form.

## What we follow, and why

**The audio path is the real 292B, not the "canonical" one.** Parker &
D'Angelo's Table 1 is a simplification, and three of its values differ from
the drawings:

| part (P&D name) | P&D Table 1 | 292B (drawing) | 292C (schematic) |
|---|---|---|---|
| middle node cap (C2) | 220 pF | C6 220 pF | C3 220 pF |
| output node cap (C1) | 1 nF | C4 **910 pF** | C5 **910 pF** |
| feedback cap (C3) | 4.7 nF | C3 4.7 nF | C4 4.7 nF |
| output shunt (Rα) | 5 MΩ | R15 **2.2 MΩ** | R26 **4.7 MΩ** (as legible) |
| gate shunt (VCA mode) | Rα → **5 kΩ** | R14 **15 kΩ** to ground | a CMOS-switched shunt |
| buffer | ideal op-amp | Q1, an **ITE4339 JFET source follower** (R16 33k to −15 V) | IC8 op-amp follower |
| input stage | "4–5 dB more in VCA mode" | R8 15k, R9 12k shunted in FILT, C2 0.47 µF, R10 47k, IC2 ×1.485: **+1.0 dB in GATE, −4.8 dB in FILT** (5.8 dB apart) | gain raised for GATE only |
| output stage | — | C5 0.47 µF, R17 47k, IC3 ×2 (+6 dB), R20 470 Ω | — |
| vactrols | VTL5C3 or /2 | VT1–8, one single cell per LDR, the two LEDs in series | one VTL5C3/2 a channel |

The 292B has only FILT and GATE. COMBO ("both") is the 292C's centre
position: C3 out and the gate shunt out, with the lower input gain (the 292C
raises the gain for GATE only). Our reference is therefore "the 292B as
drawn, plus the 292C's centre position", and it says so.

**The LED driver is the 292B's** (the 292C's driver is not fully legible in
the copies found; Parker & D'Angelo's Fig. 7 is that driver, approximated):
CV through R2 62k and OFFSET through R3 62k into IC1's − input; + input at
15·10k/(1M + 10k) = 0.149 V (R6 changed to 1 MΩ in the 7-76 revision); R4
15k feedback from the − input to the top of the LED string; R5 4.7k, C1 1 µF
and D1 (1N5228B, 3.9 V) from the top node to ground; IC1 (741 on 0/−24 V)
pulls the two series LEDs through R13 1k. The circuit is solved directly
(ideal op-amp, its output rail, the LED law, the zener's exponential knee,
C1's state). Nothing is fitted. It gives:
- a 0.77 V threshold (the bias keeps the LEDs off at rest);
- a linear 67.6 µA/V region, which comes from R4/(R2·R5) + 1/R2. The MEMS
  292B bench reports about 75 µA/V with a modified input resistor;
- a soft knee as the zener starts to conduct, so that 15 V gives 8 mA;
- a ceiling of about 15 mA, where IC1 rails against R13 1k. This is not
  Parker & D'Angelo's 40 mA, which comes from their 292C-style circuit;
- a derivative "kick" through C1 on rising CV. On falling CV the LEDs go
  dark until C1 has discharged.

**The vactrol is Najnudel et al.'s physical model**, eq. 4 with their
VTL5C3/2 Table 1, in the states p = q+ (holes) and x = q+τ (ionised traps,
electrons = p + x). The generation rate is their power coupling,
g = f/(q+ + q−). The resistance follows eqs 7–8, including Rℓ = 2 Ω and
Rd = 10 MΩ.

**The light–resistance law comes from the datasheet.** Najnudel's eq. 12
cannot be used as printed. With the 3-significant-figure P0 = −5.47e−5 and
P1 = 5.63e−5, f(PD) goes negative for every LED power below 56 mW, which
violates their own passivity condition, eq. 10. That range covers all of
the 292's operating range (the LEDs dissipate 1–25 mW). The two terms
nearly cancel, so the rounding error is larger than the result. We
therefore follow their §5.3 method with the datasheet in place of their
measurement:
1. Take the VTL5C3/2 static R(If) from the PerkinElmer sheet: 55 kΩ at
   1 mA, 2 kΩ at 40 mA, slope 19. Parker & D'Angelo's eq. 39 form,
   refit, gives R = 3.363/If^1.4 + 1695 Ω, with slope 19.5.
2. Invert eq. 4 in steady state for the drive f(If) that holds that
   resistance, and table it.
3. Below the sheet's 0.1 mA the law is extended log-log. This is our
   extrapolation, and it is labelled as such.

Nothing in the dynamics is fitted by us. Rise, fall and their level
dependence all come from Najnudel's identified constants.

**Discretisation.**
- Reference: f64 at 8× (384 kHz). Kaiser-sinc interpolation and
  decimation, 64 taps a phase, 21 kHz cutoff.
- Audio path: four node unknowns (middle node, output node, JFET source,
  IC3 input), trapezoidal capacitor companions and Newton on the JFET.
- Vactrol: backward Euler with Newton. The trapezoid's explicit half-step
  overshoots on the 1/q generation from dark.
- Driver: backward Euler on C1 with the loop's three regimes (closed, LEDs
  off, IC1 railed).

## Validation targets (results in notes.md)

| Target | Source |
|---|---|
| eq. 7 magnitude, all modes, Rf 1 kΩ–1 MΩ (Fig. 2's range) | P&D |
| eq. 12 DC gain; eq. 11 a_max (LOWPASS stable at a = 1) | P&D |
| COMBO "closer to −6 dB/oct than −12" | P&D §2 |
| static R at 1 mA / 40 mA, slope R0.5/R5 | VTL5C3/2 sheet |
| turn-on to 63 % ≈ 3 ms; turn-off to 100 kΩ ≤ 50 ms | VTL5C3/2 sheet |
| 10 kΩ → 1 MΩ in ≈ 200 ms | Najnudel Fig. 7a |
| LED Vf ≈ 1.65 V | VTL5C3 sheet, Lanterman |
| zener impedances 23 Ω / 1.9 kΩ | 1N5228B sheet |
| driver slope ≈ 75 µA/V, rising slope at the knee | MEMS 292B bench (modified input R) |

## Not modelled (each said where it matters)

- The 741's slew rate (0.5 V/µs): a 40 µs edge, which the vactrol does not
  see.
- The 910 pF across IC1.
- The OFFSET pot's source impedance (≤ 2.5 kΩ against 62 kΩ).
- JFET gate conduction above IDSS. Our levels don't reach it.
- The cells' 5 pF capacitance.
- The 292C's mystery 220 pF / 180 kΩ bleed parts.
- Op-amp noise.
- The vactrol's temperature and light history. Najnudel's model has no
  history term, and the sheet's adapt conditions differ by about ±30 %.
- Unit-to-unit spread. One part is modelled; ITE4339 and vactrol
  tolerances are wide.

## Added 2026-10-21 (the gate review)

| Work | Use | Licence / provenance |
|---|---|---|
| Buchla 292B drawing, parts list: **VT1–8 Vactec VTL2C3** (single cells, material type 3) | why the cell is a VTL5C3 (single, type 3, the nearest documented part) rather than the 292C's VTL5C3/2 | facts |
| PerkinElmer VTL5C3 sheet, "Response Time" plot, 40 mA curves, read off the plot: turn-off 11.6 kΩ at 6.75 ms, 97 kΩ at 20, 193 kΩ at 30, 295 kΩ at 40, 484 kΩ at 60, 684 kΩ at 80, 930 kΩ at 100 ms; turn-on R/R_final 11.6 at 0.67 ms, 3.1 at 2 ms, 1.3 at 4 ms | Najnudel's ν± refit to the turn-off (rms log error 0.048); turn-on reported only | datasheet |
| Plaits LPG: `plaits/dsp/fx/low_pass_gate.h`, `plaits/dsp/envelope.h` (LPGEnvelope), `plaits/dsp/voice.cc` (decay and colour mapping, kBlockSize 12), stmlib `dsp/filter.h` (Svf, FREQUENCY_DIRTY) | ported as a behaviour reference (`chimera-circuits/src/plaits_lpg/`, the MIT notice in the file; THIRD_PARTY.md) | **MIT**, Copyright 2014–2016 Emilie Gillet |
| SuperCollider LPG UGen (Mads Kjeldgaard's PortedPlugins v0.4.1), a port of Parker & D'Angelo's model and Max code; help page https://doc.sccode.org/Classes/LPG.html | run as a black box (SC 3.14.1, NRT), outside the repo, as an external listening reference only; source not read; no SC code, script or render is in the repo | **GPL-3.0** (not read, not linked) |
| Noise Engineering, "Getting started: lowpass gates" | behaviour: the bongo patch, trigger behaviour, early Buchla | blog, behaviour only |
| Make Noise Optomix manual | documents closed-gate bleed in vactrol LPGs | manual |
