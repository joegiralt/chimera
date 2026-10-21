# First LPG lab's research (#720, lpg-lab, superseded)

> The sources here stand. The "What we build" summary at the end is the
> first lab's behavioural model, which the owner rejected (made-up parts;
> `notes.md` lists what it got wrong). It is not in this crate.

## LPG research (#720): Buchla 292 / Serge / Make Noise, vactrol models

2026-10-20. Papers, datasheets and manuals only; no code read (Surge's LPG
is GPL-3.0 and stays unread, as the FX-in-filter spec already says). The lab
kept text copies for reading only; none is in the repo.

The DAFx reading list has no LPG or vactrol
entry; the two DAFx papers below should be added to its filter section.

## 1. Parker & D'Angelo, "A digital model of the Buchla lowpass-gate", DAFx-13

https://www.dafx.de/paper-archive/2013/papers/44.dafx2013_submission_56.pdf
(Aalto. Licence not stated; we take the equations and component values,
which are facts about a circuit, and write our own code.)

The primary source. What it says, and what we take:

- **Audio path (Fig. 1, Table 1).** A "canonical" 292: input → Rf → node
  Vx (C2 = 220 pF to ground) → Rf → node V+ (C1 = 1 nF to ground, Rα to
  ground) → unity buffer. Both Rf are the two LDRs of one VTL5C3 or
  VTL5C3/2. Three modes by a switch:
  - **BOTH** (the 292's "combi"): Rα = 5 MΩ, C3 = 0. A passive two-pole
    with non-coincident poles, "closer to −6 dB/oct than −12"; as Rf nears
    Rα a divider forms and the level falls too.
  - **VCA**: Rα = 5 kΩ. The poles move up and the divider starts at lower
    Rf: mostly clean attenuation. The real unit adds 4–5 dB of input gain
    in this mode to match loudness.
  - **LOWPASS**: Rα = 5 MΩ, C3 = 4.7 nF from Vfb = a·Vout back into Vx: a
    Sallen-Key-like two-pole, steeper, a small bump; the feedback gain `a`
    is a later-derivative addition (resonance), not Buchla's.
- **Equations (5)–(12).** KCL at Vx and V+ gives two states. Transfer
  function H = 1/(α1 + α2 s + α3 s²), α1 = 1 + 2Rf/Rα, α2 = Rf(2C1 + C2 −
  C3(a − 1) + (C2 + C3)Rf/Rα), α3 = Rf²C1(C2 + C3). DC gain Rα/(Rα + 2Rf)
  (the divider). Stability bound a_max = (2C1Rα + (C2 + C3)(Rα + Rf)) /
  (C3Rα); they suggest `a = a_norm · a_max` to decouple RES from Rf.
- **Discretisation.** Direct-form (bilinear on H) blows up under fast
  modulation: the states aren't the capacitor voltages. The
  topology-preserving form (trapezoidal integrators, delay-free loop solved
  in closed form, Zavalishin-style) is "stable under all physically
  reasonable parameter values and under any rate of modulation, without
  any oversampling". 2× oversampling only to tame warping near Nyquist.
  **We take this: TPT trapezoidal on the two capacitor states, one linear
  solve per coefficient update, no oversampling.**
- **Non-linearity.** The models are linear; older 292s had a transistor
  output buffer ("much more significant non-linear behaviour"); later
  derivatives put limiting diodes in the resonance path (tanh, linearised
  about the last sample). **We take: a cheap output-buffer soft clip for
  DRIVE.**
- **Control path (§3.1).** The CV goes through a first-order shelf and a
  log-amp-like stage with a zener limiter into the LED; their Fig. 8 shows
  LED current near-linear in CV up to a 40 mA ceiling (10 µA floor).
  Modelled with Lambert-W approximations: far too much for us, and the
  musical result is "CV → LED current, clamped 10 µA..40 mA".
- **LDR law (39).** Rf = A / If^1.4 + B, A = 3.464 Ω·A^1.4, B = 1136.2 Ω,
  fitted to the VTL5C3 datasheet: 1.45 kΩ at 40 mA, ~35 MΩ at 10 µA.
  **We take it as is.**
- **Vactrol memory (§3.2, Fig. 9).** Heuristic: a one-pole on the control
  whose time constant switches by direction, ≈ 12 ms rising and ≈ 250 ms
  falling (VTL5C3/2), "modulated further by the current output value of
  the vactrol model, so that it responds quicker when at high values, as
  also indicated on the datasheet". Full photoconductor physics "left for
  future work". **We keep the structure (fast up, slow down, faster when
  bright) and choose a recombination-shaped fall, §4.**

## 2. Najnudel, Müller, Hélie & Roze, "Power-balanced dynamic modeling of vactrols: application to a VTL5C3/2", DAFx-23

https://www.dafx.de/paper-archive/2023/DAFx23_paper_50.pdf (CC-BY 4.0)

A physical (port-Hamiltonian) vactrol: carrier generation by light,
Shockley-Read-Hall recombination through traps, identified against a real
VTL5C3/2 in an optical compressor. Too heavy for a voice (implicit solve,
Newton), but it gives the reason the fall is level-dependent:
recombination rates grow with the carrier density, so a bright LDR lets go
fast and a dim one crawls; times are "program-dependent". That is the
shape we want: **a fall rate that grows with the light level (a
bimolecular, n², term) plus a slow linear (trap) term for the long tail.**

## 3. PerkinElmer VTL5C3 / VTL5C3/2 datasheet (Photoconductive Cells and Analog Optoisolators, 2001)

https://www.radiolocman.com/datasheet/pdf.html?di=178415 ,
https://akizukidenshi.com/goodsaffix/vtl5c3.pdf

- VTL5C3: on-resistance ~1.5 kΩ at 40 mA; dark ≥ 10 MΩ (typ. ~20 MΩ dark
  adapted); turn-on 2.5 ms to 63 % of final; turn-off ≤ 35 ms to 100 kΩ
  (one bench measurement 18 ms, modularsynthesis.com/vactrols).
- VTL5C3/2: ~3 ms rise, ~50 ms decay to 100 kΩ.
- "Small light history memory"; times quoted after a 24 h 40 mA adapt:
  real parts vary a lot unit to unit (STABILITY, §5).

So the numbers we fit: **rise τ 2.5 ms; 40 mA → 100 kΩ in ~25 ms; the
rest of the way (100 kΩ → several MΩ, where the 292's cutoff falls from
~1 kHz to sub-audio) is the slow audible "ring", hundreds of ms.**

## 4. Make Noise LxD and Optomix manuals (behaviour, not circuits)

https://analoguehaven.com/make-noise/lxd/manual.pdf ,
https://www.makenoisemusic.com/wp-content/uploads/2024/03/optomixrev2manual.pdf

- LxD: two vactrol LPGs, CH 1 "12 dB/oct, mildly resonant", CH 2 "6 dB/oct
  non-resonant", CH 2 "moderate attack, slow decay". "As the Control
  Signal becomes less positive, the amplitude decreases, with the high
  frequencies being attenuated much sooner than the low frequencies" —
  the woody pluck. STRIKE: a gate "strikes, pings or plucks the vactrol";
  in a sequence STRIKE works as ACCENT.
- Optomix: DAMP "programs the way the LPG responds to a decaying control
  transient … from a long, slow ringing to a short, fast, muted response"
  (our DECAY); a touch of DAMP also cuts the closed-gate bleed. Rev 2 adds
  sidechain compression: the same vactrol used as an optical compressor
  (our negative FB, §5).
- Both: vactrol LPGs "will never have the speed or tight tolerances" of
  other VCAs; no very short clicks. (Our attack floor: ~2.5 ms.)

## 5. Serge VCFQ (and why it is not our model)

https://www.elby-designs.com/webtek/cgs/serge/cgs512/cgs112/cgs112.htm ;
Random*Source VCFQ listings.

The Serge "variable Q VCF" is not a vactrol LPG: a 12 dB multimode with
voltage-controlled Q, a trigger input that pings it (rings at high Q), and
an AGC input. Serge's pluck is a *resonant ring*; Buchla's is a *vactrol
decay*. The Serge flavour is already reachable in Chimera (CLEAN or 2040 at
high RES pinged by a short ENV; FB). We model the Buchla/Make Noise
vactrol gate; the 292's LOWPASS mode with RES gives a little of the Serge
ring.

## 6. Background

- Ken Stone / NRM "lopass" (public-domain 292 derivative), cited by
  Parker & D'Angelo: http://modularsynthesis.com/nrm/lopass/lopass.htm
  (schematic reference, not read for code — there is none).
- The FX-in-filter spec (docs/superpowers/specs/2026-10-17-fx-in-filter-design.md
  § LPG) already reserves KIND code 7 / ident `LPG` and mocks MODE (LP ·
  COMBO · VCA, default COMBO), OPEN, DECAY, STRIKE. This lab keeps those
  names.

## What we build from this (summary; detail in notes.md)

| Part | Source | Our choice |
|---|---|---|
| Audio path, 3 modes | Parker & D'Angelo Fig. 1, Table 1, eqs 3–12 | as given, TPT trapezoidal, 48 kHz, no OS |
| LDR law | P&D eq. 39 (datasheet fit) | as given |
| LED drive | P&D Fig. 8 (10 µA–40 mA) | an exponential converter u → current, so OPEN spans the audible range evenly (the 292's near-linear drive opens fully in its bottom third) |
| Vactrol memory | P&D §3.2; Najnudel et al.; datasheet | rise τ 2.5 ms; fall = bimolecular (n²) + linear trap term, fit to 40 mA→100 kΩ in 25 ms; DECAY scales the fall |
| Strike | LxD / Optomix STRIKE | a 4 ms LED pulse at note-on, height = velocity |
| Drive | P&D §2.3 (transistor buffer) | cubic soft clip after the gate |
| FB | ADR 0190 (output → cutoff) | the output into the LED: + rectified (bloom), − as an optical compressor (Optomix rev 2) |
| STABILITY | datasheet tolerances | per-voice LDR on-resistance and fall-time spread |
