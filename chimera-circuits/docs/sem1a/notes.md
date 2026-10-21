# SEM circuit lab: notes (epic #720, second pass)

> Copied from the circuit lab's ledger (private notes, branch `sem-circuit-lab`). Lab paths are mapped to `chimera-circuits/src/sem1a/`. The `dl/` files it names (papers, datasheets, schematic renders) stay in the ledger and are cited, not copied. The WAV sets are not kept: `chimera-utils circuit render` renders any model again.

Desktop only: no firmware, no unit, no PR. Code was in `chimera-utils/src/sem_circuit/` on branch `sem-circuit-lab` (aa4cb6a2, cdb9bf2a, 9582667d); it is now `chimera-circuits/src/sem1a/`:
- `reference.rs`: the circuit.
- `validate.rs`: its checks.
- `fw.rs`: the firmware model.
- `ab.rs`: the A/B and the probes (now the ignored tests in `fw.rs`; the WAV writers are dropped).
- `export.rs`: the firmware's constants and fixtures (kept, with tests against the shipped KIND).

The lab's commands were `semc-validate`, `semc-probes`, `semc-ab [FILTER]`, `semc-cost`; their checks are now `cargo test -p chimera-circuits sem1a` (and `--release -- --ignored` for the slow ones). Their raw outputs are `validation.md` and `probes.md` here; the A/B table is `ab-table.md`; `cost.txt` read fw tick 16.7 ns/sample steady, 24.7 exact (host). Sources and the circuit reading are in `research.md`.

## Validation of the reference

| Check | Expected (source) | Reference | |
|---|---|---|---|
| DC: HP, BP, LP, S | 0 V (the loop nulls each OTA input; C15/C22 block DC) | −3.5 / −3.5 / +3.6 / 0.0 mV (base-current offsets, which T9/T10 would trim) | pass |
| DC: cap nodes | one V_GS below BP at 1 mA: −0.886 V | −0.890 / −0.883 V | pass |
| CA3080 gm at 500 µA | 6700 / 9600 / 13000 µS (D2); Wasp unit 7856 (L1) | 7794 µS | pass (L1's unit) |
| CA3080 peak I_out at 500 µA | 350 / 500 / 650 µA | 428 µA | pass |
| CA3080 swing, open load | +13.5 / −14.4 V | +13.50 / −14.40 V | pass |
| CA3080 R_out at 500 µA | 15 MΩ | 15.0 MΩ | by construction |
| LP passband | −R139/R155 = −6.85 dB, normalised 0 dB | +0.08 … +0.10 dB at fc/10 (fc ≥ 1 kHz) | pass |
| HP stopband to passband | 0 dB at 10·fc | +0.04 … +0.34 dB | pass |
| BP peak = 1/k (k from the parts: pot law + leak + C18 − A12 lag + C22) | predicted k | measured within +0.0021 … +0.0035 of predicted over fc 1–12 kHz and every RES (§5 of `validation.md`); at 100 Hz within 0.002 for RES ≥ 0.5 | pass |
| Cutoff, trimmed at 1 kHz | f0 = fc | 1.000 at 1 kHz; −0.6 % at 5 kHz, −2.0 % at 12 kHz (C18 and A12's lag) | circuit droop, modelled |
| MODE notch at fc·√((1−m)/m) | 3000 / 1732 / 1000 / 577 / 333 Hz | 2970 / 1715 / 990 / 572 / 330 Hz (the trim's 1 %), −54 to −74 dB deep; the centre −6.2 dB in both bands | pass |
| No self-oscillation (S4 in `sem-lab-research.md`; the SEM manual's "minimum resonance" at CCW, no mention of oscillation) | rings and decays at full RES | Q at RES 1: 59 (100 Hz), 47 (1 kHz), 24 (5 kHz), 13 (12 kHz); a 2 ms burst falls 40 dB in 382 / 55 / 8 / 3.5 ms at 100 / 1k / 4k / 10 kHz | pass |
| Antisaturator | Zavalishin Fig. 6.51 / L3: damping grows with BP | 0.01 / 0.08 / 0.35 / 1.26 V of extra damping at HP for BP 1 / 2 / 4 / 10 V | as derived |
| Numerics | ×8 converged | ×8 vs ×16 null −87.6 dB, LSD 0.001 dB; 3.3 Newton iterations a step, 0 unconverged; the Jacobian unit test agrees with finite differences to 1e-4 | pass |

**What could not be validated.** There is no hardware capture and no published SEM measurement. The schematic prints no calibration values: its trims are T9/T10 (offset), T11 (INIT FREQ) and T12 (V/oct), with no targets given. The checks are against the schematic's own algebra and the datasheets. The parts I had to assume are in Open questions.

## What the old prototype got wrong (`sem-lab-*.md`, `src/sem1a/ota_svf.rs`)

1. **The RES law was CLEAN's, not the circuit's.** That alone made it "identical to CLEAN up to RES 0.5": it was the same transfer function by construction. The circuit's PS16 into R157 gives k = 3.03 at RES 0 (Q 0.33, two real poles, much duller than CLEAN's Q 0.5), 1.10 at the middle and 0.27 at 0.9. All the high Q is in the last few percent of the knob.
2. **The JFET followers Q39/Q40 were missing.** The loop's BP and LP are 2N4302 source outputs, and their gain runs 0.87 at −12 V to 0.966 at +12 V. That is the circuit's only even-order term: H2 is −53 dB at BP at 0 dB and rises 6 dB per 6 dB. The old model had no even harmonics.
3. **The resonance ceiling was a constant invented floor** (K_MIN 0.04, Q 25). In the circuit it is set by the OTAs' output resistance (k 0.008), the pot's end, and C18's lead, which grows with cutoff. Q at full RES is 59 at 100 Hz and 13 at 12 kHz.
4. **Its reference was circular.** It was the same simplified equations at ×4, so "candidate vs reference" checked only the discretisation.
5. **Levels**: it assumed 5 V a unit. The VCO sheet gives a 7.2 V p-p saw (±3.6 V), 2.9 dB lower.
6. **Soft rails**: its 8th-power limit bent signals from about 10 V. At high RES and drive that moved its harmonics by up to 10 dB (seen here before the reference's limits were sharpened).
7. **Smaller misses**: C15/C23 (4.8 and 10.6 Hz, which set the low-frequency phase) and C22 (Q changes below about 30 Hz) were absent; the OTA used ideal 2V_T rather than a measured device. It did read the antisaturator's network and the MODE pot correctly. The fit of the diodes is kept.

Against the circuit, the old candidate nulls at −8 to −26 dB with LSD 0.3–2.8 dB (`ab-table.md`, last column).

## How different the SEM is from CLEAN, honestly

Every CLEAN file in the A/B is run **at the SEM's small-signal Q** (RES remapped per block through CLEAN's law), so the knob law is taken out. Nulls are least-squares-gain residuals, with CLEAN passed through the SEM's C15/C23 couplings so their phase does not count. Mean null, CLEAN against the circuit, without the MODE sweep:

| RES (SEM knob) | −12 dB | 0 dB | +12 dB |
|---|---|---|---|
| 0 (Q 0.33; CLEAN can only reach 0.5) | −15.4 dB | −15.4 dB | −9.9 dB |
| 0.5 (Q 0.9) | **−37.7 dB** | **−37.6 dB** | −10.6 dB |
| 0.9 (Q 3.5) | −30.7 dB | −22.6 dB | −7.3 dB |
| 1 (Q 47 at 1 kHz) | −10.7 dB | −7.9 dB | −3.3 dB |

- **At matched Q, ordinary levels and RES up to about 0.5–0.9, the SEM *is* a clean 2-pole SVF.** CLEAN nulls it to −30 … −47 dB. That was not the simplification: the circuit's OTAs see at most ±31 mV behind the 455:1 divider, so they bend by under 10 % even at the rails. The followers' H2 sits at −65 to −80 dB below a resonant peak. The old lab's "identical to CLEAN" came from its RES law. At the same *knob* the two are not identical, because the SEM is much more damped from RES 0 to about 0.8.
- **The real differences**, each audible:
  1. **RES's law and range.** Q runs from 0.33 to 13–59, ending in a ring that never holds. CLEAN cannot go below Q 0.5, and it self-oscillates from RES 0.95.
  2. **Headroom, then damping.** The SEM has about 18 dB of headroom above one VCO before anything clips. A full-RES peak at 1 kHz reads +25.8 dB at −24 dB input and +5.7 dB at +12 dB: the diodes take 20 dB off it gradually, and THD stays at 0.3–1.8 %. CLEAN at the same Q clips its states at digital ±1: +18.9 dB falling to −14.4 dB, with 2–4 % odd harmonics (`probes.md`).
  3. **The cutoff-dependent ceiling**: a full-RES ring is 382 ms long at 100 Hz and 3.5 ms at 10 kHz.
  4. **MODE**: the moving notch, −54 to −74 dB deep, −6 dB at the centre. CLEAN has no equivalent (MODE-sweep LSD 1.7–5.7 dB).
  5. **Even harmonics from the followers**: small, about −53 dB H2 at BP at 0 dB, −34 dB at +12 dB with RES 0.9. CLEAN has none.
  6. **At +12 dB CLEAN's own LEVEL clipping dominates** (−10 dB nulls even at RES 0.5): the SEM stays at 0.1–0.3 % THD there.

## Firmware model against the reference

`fw.rs`: f32, ×1, 48 kHz, block-ramped controls like CLEAN's.

| Measure | Result |
|---|---|
| Over all 75 A/B jobs | LSD mean 0.31 dB (worst 4.0 dB, in the quiet low bands of `brass_bp` at −12 dB, whose null is −29 dB); null mean −32.4 dB, worst −14.2 dB |
| By RES | null −30 dB at RES 0, −38 dB at 0.5, −36 to −38 dB at 0.9, −21 to −27 dB at RES 1 (Q ≈ 50: small k differences become phase) |
| Gain at fc, every RES × level (1 kHz) | within 0.2 dB, except +12 dB at RES 0.9/1 (+0.1 dB) |
| H2 / H3 vs drive, LP, RES 0 | H2 within 1–2 dB of the reference from −12 to +18 dB. H3 is small in both (−69 to −123 dB in the reference); the fw is off by up to 14 dB where H3 is under −80 dB, and 12 dB low at +12 dB (−81 against −69) |
| H2 / H3 at BP, RES 0.9 | H2 within 2 dB; H3 within 3 dB |
| Resonance against level, RES 1 | within 0.3 dB from −24 to +12 dB |
| RES ceiling (time to −40 dB) | 402 / 278 / 55 / 8.2 / 2.8 ms against 382 / 286 / 55 / 8.0 / 3.5 ms at 100 Hz … 10 kHz |
| MODE notch | 2980 / 1722 / 995 / 575 / 332 Hz against 2970 / 1715 / 990 / 572 / 330 Hz, −55 to −63 dB deep |
| The "exact" variant (followers folded into the solve, a divide a sample) | no better: the same 0.31 dB / −32.4 dB, so the cheap one stands |

## Cost

The probe crate was the ledger's `m7probe/` (not kept): cortex-m7, opt-level 2, assembly `m7probe/semc.s`. House method ceil5(instr × 1.606 + 12 × vdiv per sample).

| Path | Instructions | vdiv | Cycles/sample/voice |
|---|---|---|---|
| Steady, diodes quiet / conducting | 74 / 79 | 1 a block | **120 / 130** |
| Steady with the C15/C23 couplings | 84 / 91 | 1 a block | **135 / 150** |
| Moving (a divide a sample) | 88 / 93 | 1 a sample | **155 / 165** |
| Old lab tick, same probe | 48 / 53 | 1 a block | 80 / 90 |

- Clamps are compare/select: f32 min/max compiled to vcmp/vmrs/vsel plus NaN selects (8 instructions a clamp). Writing them as plain compares cut the loop from 136 to 86 instructions. The engine's `clampf` compiles the same way here, so check this on the bench ELF.
- The extra over the old tick: the followers' cubic (two 3-term Horners and two multiplies), three asymmetric state clamps, and the per-block parasitics (free per sample).
- The couplings could be the engine's own DC block instead (+0, if one exists on the path). Without them the sound is the same and only the sub-20 Hz phase changes.
- Host, for scale: 16.7 ns a sample, steady.

## The A/B set (the ledger's `ab/`, not kept)

148 MB of 24-bit mono WAVs, `ab/<stimulus>/r<RES×100>_<drive>_<x>.wav`:
- `a_ref`: the circuit.
- `b_fw`: the firmware model.
- `c_clean`: CLEAN at the SEM's Q, LP2/BP2, MODE by thirds LP2/NOTCH1/HP4.
- `d_2040`: the 2040 REV2, LP4, same RES knob.

Stimuli are the first lab's: bass, brass, brass_bp, pad, mode_sweep, cutoff_sweep, and res_sweep (RES 0 → 1 then the ring-out).

**Calibration**: digital ±1 = ±3.6 V at an SEM input (one VCO saw at full). Drive is LEVEL's −12 / 0 / +12 dB, so ¼, 1 and 4 VCOs' worth, applied to every filter as its own LEVEL.

**Levels**: one gain per stimulus, set so its loudest reference sits at −16 dBFS RMS. Every drive keeps that gain, so drive and the circuit's compression stay audible. The four files of a setting are RMS-matched to the reference.

What to listen for:
- `res_sweep` and every `r100` (the SEM's ring and its ceiling against CLEAN's oscillation).
- `r100_+12dB` (the diodes' squash against CLEAN's clipping).
- `r000` (Q 0.33: the SEM's dull bottom end).
- `mode_sweep`.
- `r050_-12dB` and `r050_+00dB`, to hear that SEM and CLEAN are the same filter there.

## Open questions

1. **Pot tapers and ends.** PS16's taper is not on the schematic. With a linear pot, everything above Q 3.5 sits in the top 10 % of RES. The firmware could keep the law, which is authentic, or spread it, e.g. map the knob so Q is exponential; decide by ear. The 100 Ω end resistance is an assumption (k_end 0.006).
2. **The 2N4302s.** IDSS and V_P are assumed (3 mA, −2 V). Over the spec's spread H2 at BP moves by ±3 dB (−56 / −53 / −50 dB at 1.5 / 3 / 5 mA). A real unit's parts would settle it.
3. **The VCO level** is my derivation of the comparator thresholds (§2 of research.md). If the real saw is 10 V p-p, everything level-dependent shifts by 2.9 dB.
4. **The 301A and 741 figures** are datasheet typicals from memory (D5). They only matter for the ceiling above 5 kHz and for clipping above +12 dB.
5. **Self-oscillation**: the circuit never holds a tone. If the owner wants it, it means going below the circuit (k < 0, held by the diodes), as the first lab's `d_sem_osc` did.
6. **C22 below 30 Hz** is in the reference exactly, but in the firmware only as a per-block correction. The worst fw nulls are at RES 1 with low cutoffs (bass, −14 dB).
7. **Not modelled**: the expo converter's own errors (cutoff is set in Hz), OFFSET trims (assumed trimmed), the VCA (A20, another CA3080 behind 455:1, which bends a 13 V peak by about 10 %), and the junction capacitances.
