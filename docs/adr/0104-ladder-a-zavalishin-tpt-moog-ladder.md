# 0104. LADDER: a fourth filter TYPE, the Moog transistor ladder as Zavalishin's TPT ladder at 1×, five divide-free tanh saturators

- **Status:** Proposed
- **Deciders:** owner (the type, its params, "with tanh", no make-up gain, 2026-10-07), firmware (the saturator's form after the SPICE A/B, for the owner's ruling)

## Context
ADR 0096 built CLEAN, the 2040 and DIRTY and left the Moog kind (#123) for later; the routing spec kept KIND code 1 for it. The owner wants a Moog transistor ladder beside the 2040: same params as the other types (FREQ, RES, LEVEL, MODE, the fixed columns), no VOICE, Xpander-style modes, LEVEL as drive, the bass loss kept.

The research (ledger `nonlinear-filter-papers.md`) weighed three published models:
- **Huovilainen (DAFx-04):** five tanh on forward-Euler stages; needs 2× oversampling, a half-sample delay in the feedback and a tuning table the paper doesn't give. About twice the budget at 2×.
- **D'Angelo & Välimäki (ICASSP 2013; TASLP 2014, Part II):** exact linear response through FIR compensation; 26 multiplies, per-sample coefficient math under modulation, and the authors want oversampling against output noise anyway.
- **Zavalishin (*The Art of VA Filter Design* 2.1, §§ 5.3–5.5, 6.6, 6.9–6.10):** the TPT ladder, prewarped and exact at small signal, its global loop solved in closed form (eq. 5.8), the stage nonlinearities "linearised at zero, then pushed through" (§ 6.6), which the 2040 already runs.

Oyama (DAFx-26, paper 29, CC-BY 4.0) benchmarks Moog models against an ngspice BJT ladder. Distributed saturators (input pair plus all four stages) match SPICE an order of magnitude better than boundary saturation, and the saturator's shape matters as much as its place; linearising early stages costs more than late ones.

## Decision
- **TYPE.** `FilterKind::Ladder`, disk code **1**, ident "LADDER", screen name LADDER, last on TYPE's knob. CLEAN's panel and words (DIRTY's precedent); FLT › MOD reads LADDER MOD. Unknown codes still load as CLEAN LP4.
- **Model.** Zavalishin's TPT four-pole at 1× with the 2040's solve: shared `G = g/(1 + g)`, `D = 1/(1 + k·G⁴)`; the loop linearly in one step, `u = x·D − k·D·S`; then each stage on `ẏ = ω(tanh x − tanh y)` (§ 6.9, Fig. 6.33): `t = g·(w − σ(e))`, `e` the stage's linear TPT estimate, `σ(e)` both the stage's own feedback and the next stage's input (five saturators, Huovilainen's topology). Each stage output is clamped at the saturator's flat point, 3, and the state is `y + t`, Zavalishin's saturating-integrator form (Fig. 6.40), as the 2040's.
- **Saturators (owner: "with tanh").** One tanh on all five: `tanh_poly`, the odd degree-11 polynomial with `p(3) = 1`, `p′(3) = 0` that fits tanh best on [0, 3] (least worst error 0.005, monotone to f32 rounding; our fit, not from a source), held at ±1 past ±3. No divide.
  - The owner's design was a Padé 3/2 tanh (`x(27 + x²)/(27 + 9x²)`, 0.024 off tanh, one `vdiv`) on the input pair and stage 1, the 2040's cubic on stages 2–4. Measured against SPICE (below) it misses the large-signal cutoff shift by 38 cents RMS (pass bar ~20) and the drive sweep by 1.0 % of H1: the cubic's x³ term is 4/27 against tanh's 1/3, and where a Padé stage hands a cubic stage its output the two saturators' inverses differ, a static compressor the circuit doesn't have. Padé throughout fixes it (14 cents, 0.19 %) at five `vdiv`, 215–220 cycles; `tanh_poly` throughout does better (10 cents, 0.16 %, spectra 0.04 %) at 200–210 and no divide. Ruling asked of the owner.
- **Units and drive.** A model unit is the pair's knee, 2V_T. `ALPHA_LADDER = 0.5`: a full-scale input at LEVEL ½ peaks at half the knee, reaches it at LEVEL 0.75. LEVEL's gain feeds the loop, so drive changes the sound; no clipper runs. The LED reads `|u|` at the input pair, knee 1, full at 3.
- **Resonance.** `k = 4·RES/0.95` (`k2040_k`), onset at RES 0.95. `TRIM_LADDER` = +20.8 cents on FREQ: the stages' saturators slow the ring at RES 1 by 20.2–21.4 cents over C2–C6 untrimmed (SPICE's ladder rings 19 cents flat at k = 4.3); trimmed it reads within ±0.6 cents, and so tracks 1 V/oct. The trim costs small-signal accuracy (below); dropping it is the circuit-faithful choice.
- **No make-up gain.** DC gain `1/(1 + k)`, −14 dB at the onset, as the circuit has it (the owner's 2040 ruling).
- **Modes (derived, not from a source).** From the stage outputs `y₀ = u … y₄` by Zavalishin's `s → s − 1` rule (§ 5.5), the Xpander approach: LP2 `y₂`, LP4 `y₄`, BP2 `y₁ − y₂`, BP4 `y₂ − 2y₃ + y₄`, HP4 `y₀ − 4y₁ + 6y₂ − 4y₃ + y₄`; LP4 the default, CLEAN's knob order. `modes_respond_as_derived` holds each to the analytic transfer function within 0.3 dB at RES 0 and ½ and checks slopes and peaks. Each mode's gain lerps in `k` from 0 dB at RES 0 (BP2 ×2, BP4 ×4) to its ring at LP4's level (LP2 and BP2 ×½) at the onset. A mode LADDER lacks plays LP4 (`ladder_index`, explicit).
- **State.** LADDER and the 2040 share one slot (`Poles`: four stages and the ramp ends), so the voice chain stays **2,104 B**, under ADR 0101's 2,112. A change between them warm-starts the new type in the slot and runs the old one on a stack copy, its stages carried in `Fade.carry`, as a VOICE flip does.
- **Bills** (counted on the bench ELF, `ladder_probe`, `tools/loop_count.py`): steady LP4 121 instructions, 0 `vdiv`, 200 cycles; HP4 126, 210. `LADDER_COST` 115 over `OLD_LP24`; `LADDER_MOVE_COST` 85 (2 `vdiv`, one out-of-line loop for every mode). The research estimated 140–165 for the owner's Padé/cubic mix, which counted 165–170.

## Alternatives considered
- **Huovilainen 2004:** the best SPICE match in Oyama's benchmark, but only at 2× with the half-sample delay; the whole loop must be oversampled (Zavalishin § 6.13), about 2× our budget, and its tuning table is unpublished.
- **D'Angelo & Välimäki 2014:** exact linear response and decoupled controls, at 26 multiplies plus per-sample coefficient math when FREQ moves, and oversampling for its noise.
- **Newton–Raphson on the four stages:** +4 `vdiv` a sample (~190–230 cycles), for an operating-point correction that matters above fs/4.
- **The owner's mix, Padé on the input pair and stage 1, the cubic on 2–4:** 165–170 cycles, but 38 cents off SPICE's cutoff shift and 1.0 % on the drive sweep (above).
- **Padé on the input pair and stages 1–2, or 1–3:** 25 and 14 cents; the drive sweep stays 0.8 and 0.7 % while any cubic stage is left.
- **Padé on all five:** 14 cents, 0.19 %; 215–220 cycles, five `vdiv`.
- **Lower-degree polynomials:** degree 7 or 9 flat at ±2.5 are monotone and divide-free but miss the drive sweep (0.95, 0.74 %); degree 9 flat at ±3 is not monotone.
- **Cubic everywhere:** cheapest (~110–125), but Oyama's soft-clip row shows a wrong-shaped saturator in a distributed ladder is worse than boundary saturation.
- **One divide-free Newton step per stage** (σ′ from the saturator, `1/(1 + g·σ′)` by its series): no better on the cutoff shift (40 against 38 cents); the shape, not the solve, was the error.
- **A slot of its own for LADDER's state:** 32 B more, 2,136 B, past ADR 0101's bound.
- **Make-up gain at high RES:** overruled for the 2040; held here.

## Consequences
- A fourth TYPE on every engine. Old patches keep their TYPE; KIND 1 now loads as LADDER (no file wrote it before).
- LADDER is the costliest steady type: 115 over LP24, against EXP's 40.
- Flash: −5,444 B default (134,508 → 129,064 B free), −5,356 B orbit (112,388 → 107,032 B), most of it `Ladder::process`'s five steady loops; the moving loop is shared by every mode. The `bench,master-tape` build had 7,688 B free on main; LADDER's probes stay out of it.
- The A/B harness and the SPICE reference stay out of the repo (below).

## A/B against SPICE
**The reference.** Oyama's testbench (MIT, github.com/oyama/ladder-filter-testbench at 7e3e3464), run unmodified from a private directory with ngspice 45. Its netlist (`models/spice/ladder_filter_core.cir`) is a BJT ladder: generic SPICE NPN (β = 10,000, `VT` ≈ 26 mV at 300 K), 68 nF stage capacitors, ±10 V, an input differential pair and four ladder pairs, feedback `+k·(V(s1_r) − V(s1_l))`, simulated at 8 × 96 kHz. The setup reproduces his published figures: SPICE self-oscillates at 4.803 mV RMS (k = 4.3), Huovilainen at 4.262, Huovilainen's linear error 0.116 dB, D'Angelo 2014's 0.363 dB. Huovilainen 2004 is the testbench's own from-the-paper implementation (2×, half-sample delay, no tuning table), our cross-check that a gap is the model's, not the SPICE setup's. Nothing of the testbench, the netlist or the adapter enters the repo.

**Our side.** LADDER's core per sample (`Ladder::tick_raw`, test-support) through a host-only C ABI: volts in and out at 2V_T = 52 mV per model unit, the testbench's `k` passed through, FREQ trimmed as shipped (`chimera`) and not (`chimera_untrimmed`). Run at 96 kHz as every model there is (Chimera runs at 48 kHz; the 1× model is rate-agnostic).

**Results**, Oyama's Table 3 metrics (lower is better):

| Model | Spectra (% H1) | Cutoff sweep (% H1) | Drive sweep (% H1) | Linear (dB) | Large-signal cutoff shift RMSE / max (cents) | Self-osc (mV, SPICE 4.803) | Small-signal cutoff RMSE (cents) |
|---|---|---|---|---|---|---|---|
| Huovilainen 2004 (2×) | 0.04 | 0.17 | 0.05 | 0.116 | 11.6 / 44.5 | 4.262 | 7.5 |
| Zavalishin 2012 (Newton, exact tanh) | 0.05 | 0.14 | 0.06 | 0.344 | 12.3 / 44.6 | 4.857 | 8.1 |
| D'Angelo 2014 | 0.06 | 0.22 | 0.18 | 0.363 | 20.3 / 54.6 | 4.848 | 9.4 |
| Mystran 2012 | 0.04 | 0.19 | 0.08 | 0.338 | 13.0 / 46.8 | 4.754 | 7.7 |
| **LADDER untrimmed** | **0.04** | **0.06** | **0.16** | 0.340 | **10.4** / 42.3 | 5.002 | **6.6** |
| **LADDER as shipped (+20.8 c)** | 0.11 | 0.38 | 0.15 | 0.300 | 16.2 / 38.7 | 5.003 | 14.2 |
| LADDER, owner's mix (Padé on the input pair and stage 1, cubic on 2–4), untrimmed | 0.19 | 0.81 | 1.01 | 0.341 | 38.5 / 101.6 | 5.061 | 2.2 |
| D'Angelo 2013 | 0.76 | 1.16 | 0.18 | 1.517 | 30.6 / 64.7 | 6.931 | 71.7 |
| FAUST (linear) | 0.54 | 19.69 | 8.46 | 0.392 | 510 / 1638 | — | 17.8 |

- The model with `tanh_poly` sits with the distributed-nonlinearity models (Oyama's 0.04–0.10 / 0.17–0.33 / 0.05–0.24) and passes the ~20-cent large-signal bar, at 1× and with no divide. The trim moves it 6 cents off the circuit's cutoff, which shows in every metric read at a fixed cutoff; untrimmed, it rings 28 cents flat at k = 4.3 where SPICE rings 19.
- Q against SPICE (resonance-gain case): RMSE 0.018, between Huovilainen's 0.049 and the ZDF models' 0.005–0.007.
- Self-oscillation is 4 % hotter than SPICE's, as Zavalishin's and D'Angelo's are 1 %.


## Sources
- V. Zavalishin, *The Art of VA Filter Design*, rev. 2.1.2 (2020): §§ 3.9, 5.1–5.5 (eq. 5.8, multimode taps), 6.2–6.10 (§ 6.6 approximations, § 6.9 transistor ladder, Fig. 6.40). https://archive.org/details/the-art-of-va-filter-design-rev.-2.1.2
- A. Huovilainen, "Non-Linear Digital Implementation of the Moog Ladder Filter", DAFx-04, pp. 61–64. https://dafx.de/paper-archive/2004/P_061.PDF
- S. D'Angelo and V. Välimäki, "Generalized Moog Ladder Filter: Part II", IEEE/ACM TASLP 22(12), 2014, doi 10.1109/TASLP.2014.2352556; "An improved virtual analog model of the Moog ladder filter", ICASSP 2013 (second-hand).
- H. Oyama, "Quantifying Nonlinear Behavior in Digital Moog Ladder Filters: Cross-Implementation Comparison and Common-Core Ablation", DAFx-26, paper 29, CC-BY 4.0. https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_29.pdf
- Oyama's testbench, https://github.com/oyama/ladder-filter-testbench (MIT, commit 7e3e3464): the ngspice netlist and the metrics, run host-side from a private directory, never vendored.
- Ledger (private notes) (2026-10-07); report (private notes).
- ADRs 0096, 0101; spec `docs/superpowers/specs/2026-10-05-filter-types-design.md` § 4a; issue #123.
