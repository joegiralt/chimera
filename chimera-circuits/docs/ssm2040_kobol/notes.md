# 2040 EXP from the Kobol circuit: validation, differences, cost

2026-10-21. Desktop only; the unit was not touched and no firmware filter changed. REV2 is unchanged: the hash pin `k2040_rev2_is_bit_identical_to_before_the_make_up` passes on the lab branch.
- Sources, circuit reading and assumptions: `research.md`.
- Measures: `ab.md` (written by the lab's `chimera-utils kexp ab`; the numbers are now tests in `src/ssm2040_kobol/tests.rs`).
- WAVs and ngspice data: the lab's ledger (`wav/`, `spice/`), not in the repo; `chimera-utils circuit render` and `circuit::spice` regenerate them.
- Lab code: branch `exp-circuit-lab`; kept as `chimera-circuits/src/ssm2040_kobol/`.

## Headline

1. **The schematic exists, and the circuit is plain.** The RSF service manual shows a real SSM 2040 in the data sheet's standard connection: 10 K chain, **220 Ω** legs, 1 nF caps, R24 100 K in.
   - **Resonance:** OUT 4 → C6 4.7 µF → 4.7 K/220 Ω → a CA3080 (CI5), its (+) grounded, its current onto IN 1, its I_abc from T3 linear in the RES CV.
   - **Output:** a second CA3080 (CI3) as the VCA, into CI2A.
   - **Nothing compensates the bass.**
2. **The Kobol's "extra bass" is not the circuit. It is the takes.** Both of these come from the session transcript:
   - VCO1 OUT was **patched into VCF FREQUENCY IN** during all three sweeps (16:15–16:17 UTC). The owner mentions it at 16:19, and it was pulled before the drive takes.
   - Right after sweep1, the owner was asked to **"raise the MOTU's input 5 gain by about 5 dB, or turn up the Kobol's output"** (16:15:38).

   The sweeps' "bass loss" is a **perfectly flat** step: −3.27 / −6.81 dB, ±0.1 dB from H1 to H31, the VCO's notch pattern unchanged. That is what a gain change gives.
   - The audio-rate FM also explains why sweep_res10's ring is weak and inharmonic (~4 dB over H1, against a free ring 30 dB louder).
   - It also explains the "8 % pulse notches" seen only in the sweeps.
   - The circuit itself loses **1/(1+k)**:

   | Circuit bass loss | k 2.13 | k 5.6 |
   |---|---|---|
   | Measured (dB) | −10.3 | −18.7 |
   | Theory, 1/(1+k) (dB) | −9.9 | −16.4 |

     The extra 1.9 dB at k 5.6 is the ring compressing the forward path. The loss holds from 0.11 V to 6 V at the mixer.
   - **The audit's hypothesis (CI5 compressing loud bass) is refuted.** CI5 sees OUT 4/22.4, and OUT 4 can't pass about ±1 V, so CI5 never exceeds about 0.86 × 2V_T. A ring-saturated CI5 can at best halve its incremental gain, a floor of −9.5 dB at k = 4.

   **ADR 0191's make-up gain was fitted to a cable and a gain knob.** It should go.
3. **The circuit, with nothing fitted but one in-spec offset, reproduces the Kobol's ring** (table below): H2, level flatness, the pitch's straightness and the drive linearity. Its H3 is 3.5 dB short.
4. **EXP-C** (`fw.rs`), today's structure rebuilt from the circuit, tracks the circuit closely:
   - third-octave LSD 0.4–1.3 dB on the linear takes and 0.7 dB on the self-oscillation;
   - H2 right;
   - H3 short.

   It costs about **335–361 cycles** a sample on rev V, against the 285–300 target, so it is over. That is **6 voices** on rev V (today's EXP 7, REV2 8) and **5** on rev Y (6).

## Validation against the Kobol (clean takes only, after 16:19)

Calibration: the full VCO1 is 2.8 V peak at R24. The circuit's ring (0.367 V at OUT 4) is set against track2's −8.5 dBFS, with the VCA unchanged from drive_f. This is research.md A5, not a fit.

| Check | Kobol | Circuit (ngspice, unit) | EXP-C | Today's EXP | REV2 |
|---|---|---|---|---|---|
| Ring H2 (track2, mean of 15 notes) | **−39.3 dBc** | −38.7 | −37.7 | −150.6 | −150.0 |
| Ring H3 | **−43.6 dBc** | −47.1 | −54.2 | −54.6 | −59.7 |
| Ring level spread, 340 Hz–5.3 kHz | **0.7 dB** | 0.8 | 1.9 | 0.1 | 0.3 |
| Tracking: straightness of ring pitch against FREQ | slope 1.989 (KBD at full, ~2 oct/oct), residual **29 c rms**, bowed (−48 … +34 … −63 c) | slope 0.993, residual ±3 c | slope 0.988, ±1 c | — | — |
| Ring pitch against nominal FREQ, 340 / 1 k / 3 k / 5.3 k | (trimmed by Adj 15) | +46 / +28 / +18 / +13 c | +105 / +84 / +59 / +49 c (quintic); +54 c at 1 k (degree 7) | | |
| Drive ¼ → ½ → full: shape change, worst \|Δ\| (H1 step removed) | **0.15 / 0.37 dB** | 0.02 / 0.08 | 0.01 / 0.07 | 0.03 / 0.29 | 0.02 / 0.08 |
| Drive: does the filter squash across the VCO1 range? | no (linear) | no: the full VCO1 puts stage 1 at about 0.28 V, a quarter of its swing | no | no | no |
| Bass loss against RES at each drive | not measurable: the sweeps had the FM patch; the drive series is RES 7¼ only | 1/(1+k) at every level up to 6 V | the same | +4.2 / +7.1 dB of make-up (fitted) | 1/(1+k) |

Where they disagree, and why:
- **H2 is a per-unit tolerance, not a structure.** With the data sheets' typical offsets the circuit's ring H2 is −55 dBc; within spec it spans **−31.8 … −62 dBc** (`spice/v2/h2_*`). Even with zero offsets it is −50.5, from the asymmetric limits (collector saturation below, clamp above).
  - The Kobol's −39 is reached with CI5's offset at 1.5 mV (0.4 typical, 5 max). That is the one value read off a recording. Each Chimera voice slot could carry its own, as STABILITY's chip does.
- **H3 is 3.5 dB short in the circuit and 10 dB short in EXP-C.**
  - In the circuit, H3 rises with excess loop gain: −53 at k 4.3, −43.4 at OUT 4 for k 5.6, −40.5 at k 7. If the Expander's RES top is above the Kobol board's 646 µA (its R45 is 0 Ω, so the law is set on the programmer), the H3 rises.
  - The assumed high-side clamp shape also matters (vm 1.1 → 1.5 V moves H2 by 18 dB, H3 by 2 dB).
  - The MOTU and CI2A are not modelled.
  - EXP-C's OUT 4 has H3 at −47 (circuit −43.4); its CI3 cubic cancels about 1.4 dB more than the circuit's 3080. At high FREQ its 1× drops H3 further, as the Kobol's own does at 5.3 kHz (−52.8).
- **Tracking bow (Kobol ±30 c).** This lies outside the filter core: CI6/CI7, T4/T5 and the KBD at "2 V/oct" (full right). The netlist's tail current is an ideal exponential, so neither model has it. The ring itself tracks FREQ to within ±3 c in the circuit.
- **Absolute pitch.** The circuit rings 12–46 c sharp of its nominal FREQ: stage 1's own pole (+1.96 %), C6's lead, and the ring's nonlinear rise (+50 c from onset to k 5.6), less about 1.9 % of β/α/buffer loss. The Kobol trims this with Adj 15, so the A/B tunes every model per note. **No trim belongs in the model.**

## EXP-C against today's EXP and the circuit (A/B, `ab.md`)

Third-octave LSD (dB, mean |Δ| over bands within 50 dB of the top) and null (dB re the reference), same input, RMS-matched:

| Take | EXP-C vs circuit | EXP-C vs today's EXP | Today's EXP vs circuit |
|---|---|---|---|
| drive_q / _h / _f (RES 7¼) | 0.93 / 0.56 / 0.38 · null −1.2 / −4.1 / −12.1 | 2.28 / 0.97 / 0.27 · −12.8 / −19.3 / −24.9 | 7.98 / 2.85 / 0.61 |
| sweep1 / _res5 / _res10 | 0.50 / 1.33 / 1.06 · null −12.4 / −4.4 / (rings) | 0.92 / 1.17 / 2.79 | 1.19 / 0.69 / 1.74 |
| track2 (self-oscillation) | 0.72 | 0.67 | 3.13 |
| phrase_bass_vca8 / _bass / _acid_vca8 / _acid | 3.22 / 1.13 / 7.81 / 0.59 | 3.22 / 0.53 / 2.41 / 0.24 | 3.92 / 1.05 / 5.68 / 0.74 |

Honest closeness:
- **On linear material,** EXP-C and today's EXP are close to each other and to the circuit in spectrum (under about 1 dB). They sound alike there.
  - Their nulls are poor (−1 … −12 dB): a few percent of pole frequency between models moves the phase near cutoff.
  - Today's EXP is ~11 dB hotter inside the chip for the same input, but at these levels both are linear.
- **EXP-C differs from today's EXP** in four things the ear can find:
  1. **Bass at high RES:** EXP-C loses the circuit's 1/(1+k), up to −18 to −20 dB at RES max. Today's EXP keeps +4 to +7 dB of make-up.
  2. **H2 on the ring and when pushed:** −38 dBc against none.
  3. **The ring keeps ringing over a loud VCO,** as the circuit does: no squelch until stage 1's swing.
  4. **Resonance onset:** RES 0.72 on `k = 5.59·RES` (owner's call).
- **Where EXP-C is still off the circuit:**
  - ring H3, −54 against −47;
  - ring level spread, 1.9 against 0.8 dB (EXP-C's low-FREQ ring is 1.2 dB quieter);
  - hard drive (12 V at the mixer, +13 dB over full VCO1): H3 −26.8 against −34.9;
  - the phrases with RES near max (acid_vca8: 7.8 dB LSD, both self-oscillating at different phases).
- **Against the Kobol directly,** nothing separates the five on the phrase and drive spectra. Centroid errors are 0.37–0.42 oct for all, and harmonic shape errors 3.8–4.3 dB for all. The plain-saw input against the Kobol's own VCO, and the phrases' fitted patches, dominate. Only the self-oscillation (H2, H3, spread) and the drive linearity discriminate, and the table above has those.

## Cost and voices

The M7 static count of the steady inner loop (`lab/m7probe`, thumbv7em, cortex-m7, opt 2), billed the house way (instructions × 1.46 × 1.1, × 1.57 the REV2 unit/static ratio). The same method reproduces the earlier lab's 113 instructions for "e".

| Version | Instructions (common path) | Calibrated total | Over REV2's 173 |
|---|---|---|---|
| "e" (#711 lab) | 113 | ~285 | +110 |
| EXP-C, first cut | 172 | ~434 | +261 |
| EXP-C lean, quintic tanh | 133 | ~335 | +162 |
| **EXP-C lean, degree-7 tanh (the lab model)** | 143 | **~361** | **+188** |

"Lean" means: stages 2–4 share their constants, the swing knees are in physical sign, and the clamps are max/min (VMAXNM).

Voices, default VA Sound (`c2040-cost`, 752 cycles a voice, the filter's share 81):

| | REV2 | Today's EXP (+55) | EXP-C quintic (+159) | EXP-C degree 7 (+185) |
|---|---|---|---|---|
| rev V | 8 | 7 | **6** | **6** |
| rev Y | 6 | 6 | 5 | 5 |

7 voices on rev V needs ≤ 896 cycles a voice, i.e. EXP-C at ≤ ~320 total: about 20 instructions under the quintic version.

Candidates, each to be measured for what it changes:
- drop C6's coupling (3 instructions; DC then feeds back and H2 changes);
- the quintic tanh (−10; pitch +29 c and H3 lower);
- one shared swing curve without the FREQ widening (block rate only, about 0);
- Fold stage 1's pole into the others (−4).

These are static estimates, ±20 %. **Bench on the unit** before any decision; the ratio is calibrated only on REV2.

## Open questions for the owner

1. **Listen first** (`wav/`, 11 takes × 5 sources, 24-bit; per-take RMS-matched; the drive series keeps one gain per source so ¼ → full can be heard). Start with:
   - `track2_*`: the ring's dirt, circuit and EXP-C against today's;
   - `drive_f_*`;
   - `sweep_res10_*`: the bass at RES max;
   - `phrase_acid_*`.

   The Kobol's sweep takes are contaminated (FM patch). Listen to them as the instrument, not as a filter reference.
2. **The bass.** Do you want the circuit's honest 1/(1+k) on EXP, with ADR 0191's make-up removed? It would need a superseding ADR. Or is the fat low end something you liked that the Kobol never did? Then it is a FILTER feature, not a circuit model. One clean re-take would settle the Kobol when it is unpacked: VCO1 full, no cables, sweeps at RES 0/5/max, MOTU untouched.
3. **RES law.** The Kobol's 3080 is linear in its CV (k 0 → 5.6). Onset would move to RES 0.72 from the shared 0.95.
4. **Cost.** EXP-C at ~335–361 costs a voice against today's EXP (rev V 7 → 6). Squeeze toward ~320, or accept?
5. **H2 per voice.** Put CI5's offset (and the cells') into STABILITY's per-slot chip, so each voice rings a little differently, as real units do?
6. **Unexplained.** Today's EXP against the circuit at drive_q: 8 dB third-octave LSD, against 0.6 at drive_f. It is not a level effect, since both are linear there. Look before trusting that row.
