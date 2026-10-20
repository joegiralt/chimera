# 0156. Model FUZZ as a Big Muff Pi from its circuit, anti-aliased at 1×

- **Status:** Proposed
- **Deciders:** owner (product, sound), controller (plan #614), builder

## Context
The FX slot (ADR 0155) has a FUZZ type, code 2. The spec asks for a fuzz built from the papers and a published schematic, not from the spike, with the stage gains checked. FUZZ has to fit one filter's cost per voice: `FUZZ_CEILING` 271, so 176 cycles a sample on top of the bare chain (bench, 2026-10-19).

The research step (private notes) costed the routes:
- first-order ADAA at 2× through the half-band runs 380–455 cycles;
- the same at 1× runs about 175.

## Decision
**The pedal.** FUZZ is the EHX Big Muff Pi, NYC reissue (V9). It uses the NYC column of PedalParts' Multi Muff BOM, which credits Kit Rae's values. `voice_fx/fuzz.rs` runs these stages:
1. **Input.** ×11.04: 1 FS is 0.5 V over `b = n·Vt = 45.29 mV`.
2. **Q1, linear.** 7.8× with a 1.0 kHz one-pole. Its rail clip isn't modelled.
3. **SUSTAIN.** `d(s) = 10^(−1.78·(1−s))`, a 35.6 dB span.
4. **Clipper 1 (Q2).** A shelf at 10.3× low and 1.45× high, pole 2,284 Hz, then the diode law.
5. **Clipper 2 (Q3).** A shelf at 12.0× low and 1.45× high, pole 1,268 Hz, then the diode law.
6. **Tone stack.** `(1−t)·LP(723 Hz) + t·(x − LP(1,855 Hz))`, from R19/C11 and C10/R18. It scoops the mids at noon, but only 3.7 dB at 1 kHz under the dB mean of the 200 Hz and 4 kHz bands (3.95 at its notch, 1.2 kHz): the circuit's, not the plan's 6 dB. `fuzz_tone_scoops_the_mids_at_noon` measures the stack alone (the clippers bypassed) against 3 dB.
7. **Q4 and VOLUME.** Folded into one gain, `0.092 · pot_level(P3)`. It's set by RF 4's bound, that every pot at 1 hands the 2040 at most 4 FS (VOLUME's ×4 top is by design; 3.99 measured on VA INIT), and brings the wet at SUSTAIN, TONE and VOLUME ½ to 1.6 dB under VA INIT C3's dry (it was 8 dB under at 0.044). The plan's ±1.5 dB and that bound can't both hold, by 0.1 dB; the bound wins. Task 14 makes the final pass.

**Where the gains come from.** They come from a small-signal nodal analysis of the NYC values (`fuzz-sim/muff.py`, with β 250–800 moving them ±0.6 dB). Two corrections to the spike:
- the spike's ideal Rf/Rin overstated the gain by 13 dB;
- the spike's drive was 14 dB too hot into clipper 1 and 8 dB too hot into clipper 2.

**The diode law.** It models a symmetric 1N4148 pair (Is 2.52 nA, n 1.752), one diode a side (D'Angelo, Gabrielli & Turchet, DAFx-19). It is the static law of the clipper ODE (Yeh, Abel & Smith, DAFx-07: the pre-filter plus the DC curve):
- the law is `ŷ + â·(e^ŷ − 1) = |x|`, with `â = R_eff·Is/b = 0.011128` and R_eff 200 kΩ, fitted to the NYC stage (`fuzz-sim/clipfit.py`);
- its antiderivative has the closed form `F̂ = (u + â + 1 − ŷ/2)·ŷ − u`, with F̂(0) = 0.

**The table.** Both F and f come from one 2,048 B table of per-interval Hermite cubics. Its layout:
- it's indexed by float bits on `|x| + 2⁻⁶`, four intervals an octave, from 2⁻⁶ to 2¹⁰;
- f is the cubic's exact derivative.

The table is built on the host from the closed forms in f64 and checked in. A test rebuilds it bit for bit. Its coefficients carry the powers of 2⁻²¹ of the raw interval bits, exactly, so `t` needs no scaling.

**The anti-aliasing.** Both clipping stages use first-order ADAA at 1× (Parker, Zavalishin & Le Bivic, DAFx-16):
- the output is `(F(x) − F(x₋₁))/(x − x₋₁)` when `|Δx| ≥ 2⁻¹³·(1 + F)`;
- otherwise it is `f(x)`. The research proposed the mean of the two f. The difference is under `ε·(1 + F)·max f′`, −80 dB, and it saves a state and an add a stage;
- on the fallback it divides by 1, not 0, so no NaN forms.

There is no oversampling and no half-band.

**The built loop.** The steady loop counts 93 instructions and 2 `vdiv` a sample (`tools/loop_count.py` on the ORBIT ELF, ADR 0056): **175 cycles against the 176**. The first build counted 116 (215). It came down by:
- folding Q2's high gain and Q1's one-pole weight into the drive, and the tone blend, VOLUME and MIX into three gains;
- the guard as one multiply-subtract;
- dropping Q3's clamp, since its input is bounded by `12 × F_MAX` < 2¹⁰, asserted at compile time;
- running two samples a time round, so the stages' memory alternates registers instead of moving.

V3 (the plain law on clipper 1) counted 160, but failed the aliasing gate (−47.6 dB at 1,760 Hz), so it isn't used.

**State.** `VoiceFx` holds one `Fuzz` (44 B) per place. A PRE↔POST fade runs two FUZZ sides on different signals, so each needs its own memory. The side fading in starts fresh, and a shut gate clears it.

**Cost.** `cost` 175, `ramp_cost` 20 and `glide_cost` 5 are placeholders until Task 13 bills them on the unit.

## Alternatives considered
- **ADAA at 2× through `dsp::halfband`:** 380–455 cycles, over the ceiling.
- **The spike's 8× half-band cascade:** 124 ns a sample on the host, far over.
- **Stateful ADAA (Holters, DAFx-19):** ill-defined above fs/3 at 1×, and the static law has no loop to protect. It's the upgrade path if the static model sounds dead at A/B.
- **Second-order ADAA (Bilbao et al., 2017):** it needs F2 and a second, worse-conditioned divide.
- **A WDF of the whole pedal (Werner et al., 2015):** iterative or multiport, well past the budget.
- **D'Angelo's ω3 for the law:** its zero below x₁ puts a crossover step at every zero crossing. The table has none.
- **An asymmetric pair, as in the spike:** the NYC pair is symmetric, and a two-sided table would be 4 KB.
- **The Ram's Head values:** about 5 dB more gain a stage. Every number is in the ledger for A/B.

## Consequences
- **Aliasing.** It is about that of plain 3× oversampling, not the spike's 8×. At SUSTAIN, TONE and VOLUME ½ it measures −53.1 dB at 1,760 Hz and −50.9 dB at 2,093 Hz, against −37.2 and −33.8 dB for the plain law. The `fuzz_aliasing_floor_at_high_notes` gate holds it at ≤ −50 dB and ≥ 12 dB under the plain law, and the floor is never moved looser than −45 dB.
- **No margin.** 175 of 176 cycles leaves nothing for a later addition. Task 13's unit bill decides.
- **Top end and delay.** The ADAA droop is `cos(πf/fs)` a stage, about −4 dB at 10 kHz over both. Each stage adds half a sample of delay. The dry isn't delayed to match: MIX 0 must stay the dry bit for bit (ADR 0155), and a one-sample comb's first notch is at fs/2.
- **Not modelled:** Q1's rail clipping, the diode cap's bias creep, and device mismatch.
- **Self-contained.** The kernel is one file with one table, so bank 2 (ADR 0157) can take it whole.

## Provenance
- Written from the papers and the published schematic values below.
- **No GPL code was read.** BYOD's and Surge's sources were never opened. ElectroSmash's analysis was unreachable and is not used.
- The spike (branch `drive-spike`) is the A/B baseline only. Nothing was copied from it.

## Sources
- D. T. Yeh, J. S. Abel, J. O. Smith, "Simulation of the diode limiter in guitar distortion circuits by numerical solution of ODEs", DAFx-07; and "Simplified, physically-informed models of distortion and overdrive guitar effects pedals", DAFx-07.
- J. Parker, V. Zavalishin, E. Le Bivic, "Reducing the aliasing of nonlinear waveshaping using continuous-time convolution", DAFx-16.
- S. Bilbao, F. Esqueda, J. Parker, V. Välimäki, "Antiderivative antialiasing for memoryless nonlinearities", IEEE SPL 24(7), 2017.
- M. Holters, "Antiderivative antialiasing for stateful systems", DAFx-19 (CC-BY 3.0).
- S. D'Angelo, L. Gabrielli, L. Turchet, "Fast approximation of the Lambert W function for virtual analog modelling", DAFx-19.
- K. J. Werner et al., "An improved and generalized diode clipper model for wave digital filters", AES 139, 2015 (abstract).
- "A case study: re-engineering the Big Muff π", guitarscience.net, 2014.
- PedalParts.co.uk, Multi Muff build document (NYC Reissue column, after Kit Rae); cross-checked against the Aion FX Halo, GGG Green Russian and Coda Effects tone-stack tables.
- The research ledger, (private notes), and its `fuzz-sim/` scripts; spec `2026-10-17-fx-in-filter-design.md` § FUZZ, § Cost, § Provenance; issue #614.
