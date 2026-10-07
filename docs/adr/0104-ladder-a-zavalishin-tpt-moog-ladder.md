# 0104. LADDER: a fourth filter TYPE, the Moog transistor ladder as Zavalishin's TPT ladder at 1×, a Padé tanh on the input pair and stage 1

- **Status:** Proposed
- **Deciders:** owner (the type, its params, "with tanh", no make-up gain, 2026-10-07), firmware

## Context
ADR 0096 built CLEAN, the 2040 and DIRTY and left the Moog kind (#123) for later; the routing spec kept KIND code 1 for it. The owner wants a Moog transistor ladder beside the 2040: same params as the other types (FREQ, RES, LEVEL, MODE, the fixed columns), no VOICE, Xpander-style modes, LEVEL as drive, the bass loss kept.

The research (ledger `nonlinear-filter-papers.md`) weighed three published models:
- **Huovilainen (DAFx-04):** five tanh on forward-Euler stages; needs 2× oversampling, a half-sample delay in the feedback and a tuning table the paper doesn't give. About twice the budget at 2×.
- **D'Angelo & Välimäki (ICASSP 2013; TASLP 2014, Part II):** exact linear response through FIR compensation; 26 multiplies, per-sample coefficient math under modulation, and the authors want oversampling against output noise anyway.
- **Zavalishin (*The Art of VA Filter Design* 2.1, §§ 5.3–5.5, 6.6, 6.9–6.10):** the TPT ladder, prewarped and exact at small signal, its global loop solved in closed form (eq. 5.8), the stage nonlinearities "linearised at zero, then pushed through" (§ 6.6), which the 2040 already runs.

Oyama (DAFx-26, paper 29, CC-BY 4.0) benchmarks Moog models against an ngspice BJT ladder. Distributed saturators (input pair plus all four stages) match SPICE an order of magnitude better than boundary saturation, and the saturator's shape matters as much as its place; linearising early stages costs more than late ones.

## Decision
- **TYPE.** `FilterKind::Ladder`, disk code **1**, ident "LADDER", screen name LADDER, last on TYPE's knob. CLEAN's panel and words (DIRTY's precedent); FLT › MOD reads LADDER MOD. Unknown codes still load as CLEAN LP4.
- **Model.** Zavalishin's TPT four-pole at 1× with the 2040's solve: shared `G = g/(1 + g)`, `D = 1/(1 + k·G⁴)`; the loop linearly in one step, `u = x·D − k·D·S`; then each stage on `ẏ = ω(tanh x − tanh y)` (§ 6.9, Fig. 6.33): `t = g·(w − σ(e))`, `e` the stage's linear TPT estimate, `σ(e)` both the stage's own feedback and the next stage's input (five saturators, Huovilainen's topology). Each stage output is clamped at its saturator's flat point (3 on stage 1, 1.5 on 2–4) and the state is `y + t`, Zavalishin's saturating-integrator form (Fig. 6.40), as the 2040's.
- **Saturators (owner: "with tanh").** A Padé 3/2 tanh, `x(27 + x²)/(27 + 9x²)` held at ±1 from ±3 (within 0.024 of tanh, one `vdiv`), on the input pair and stage 1, where Oyama finds the harmonic character lives; the 2040's divide-free cubic `sat_2040` on stages 2–4.
- **Units and drive.** A model unit is the pair's knee, 2V_T. `ALPHA_LADDER = 0.5`: a full-scale input at LEVEL ½ peaks at half the knee, reaches it at LEVEL 0.75. LEVEL's gain feeds the loop, so drive changes the sound; no clipper runs. The LED reads `|u|` at the input pair, knee 1, full at 3.
- **Resonance.** `k = 4·RES/0.95` (`k2040_k`), onset at RES 0.95. `TRIM_LADDER` = +15.8 cents on FREQ: the stages' saturators slow the ring at RES 1 by 15.4–16.5 cents over C2–C6 untrimmed; trimmed it reads −0.7 to +0.4.
- **No make-up gain.** DC gain `1/(1 + k)`, −14 dB at the onset, as the circuit has it (the owner's 2040 ruling).
- **Modes (derived, not from a source).** From the stage outputs `y₀ = u … y₄` by Zavalishin's `s → s − 1` rule (§ 5.5), the Xpander approach: LP2 `y₂`, LP4 `y₄`, BP2 `y₁ − y₂`, BP4 `y₂ − 2y₃ + y₄`, HP4 `y₀ − 4y₁ + 6y₂ − 4y₃ + y₄`; LP4 the default, CLEAN's knob order. `modes_respond_as_derived` holds each to the analytic transfer function within 0.3 dB at RES 0 and ½ and checks slopes and peaks. Each mode's gain lerps in `k` from 0 dB at RES 0 (BP2 ×2, BP4 ×4) to its ring at LP4's level (LP2 and BP2 ×½) at the onset. A mode LADDER lacks plays LP4 (`ladder_index`, explicit).
- **State.** LADDER and the 2040 share one slot (`Poles`: four stages and the ramp ends), so the voice chain stays **2,104 B**, under ADR 0101's 2,112. A change between them warm-starts the new type in the slot and runs the old one on a stack copy, its stages carried in `Fade.carry`, as a VOICE flip does.
- **Bills** (counted on the bench ELF, `ladder_probe`, `tools/loop_count.py`): steady LP4 83 instructions + 2 `vdiv`, 165 cycles; HP4 87, 170. `LADDER_COST` 75 over `OLD_LP24`; `LADDER_MOVE_COST` 55. The steady loop's two `vdiv` are the Padé tanhs, chosen by the owner; the other types' "0 `vdiv` steady" guard doesn't apply to LADDER.

## Alternatives considered
- **Huovilainen 2004:** the best SPICE match in Oyama's benchmark, but only at 2× with the half-sample delay; the whole loop must be oversampled (Zavalishin § 6.13), about 2× our budget, and its tuning table is unpublished.
- **D'Angelo & Välimäki 2014:** exact linear response and decoupled controls, at 26 multiplies plus per-sample coefficient math when FREQ moves, and oversampling for its noise.
- **Newton–Raphson on the four stages:** +4 `vdiv` a sample (~190–230 cycles), for an operating-point correction that matters above fs/4.
- **Padé on all five saturators:** ~190–210 cycles; the owner chose Padé where Oyama finds it matters.
- **Cubic everywhere:** cheapest (~110–125), but the cubic's x³ term is 4/27 against tanh's 1/3, and Oyama's soft-clip row shows a wrong-shaped saturator in a distributed ladder is worse than boundary saturation.
- **A seventh field for LADDER's state:** 32 B more, 2,136 B, past ADR 0101's bound.
- **Make-up gain at high RES:** overruled for the 2040; held here.

## Consequences
- A fourth TYPE on every engine. Old patches keep their TYPE; KIND 1 now loads as LADDER (no file wrote it before).
- One more steady loop with a `vdiv` pair: LADDER is the costliest steady type (75 over LP24, against EXP's 40).
- Flash: +6.7 KB (default 134,508 → 127,840 B free; orbit 112,388 → 105,800 B), almost all `Ladder::process`'s ten monomorphised loops.
- The A/B harness and the SPICE reference stay out of the repo (below).

## A/B against SPICE
SECTION_PLACEHOLDER

## Sources
- V. Zavalishin, *The Art of VA Filter Design*, rev. 2.1.2 (2020): §§ 3.9, 5.1–5.5 (eq. 5.8, multimode taps), 6.2–6.10 (§ 6.6 approximations, § 6.9 transistor ladder, Fig. 6.40). https://archive.org/details/the-art-of-va-filter-design-rev.-2.1.2
- A. Huovilainen, "Non-Linear Digital Implementation of the Moog Ladder Filter", DAFx-04, pp. 61–64. https://dafx.de/paper-archive/2004/P_061.PDF
- S. D'Angelo and V. Välimäki, "Generalized Moog Ladder Filter: Part II", IEEE/ACM TASLP 22(12), 2014, doi 10.1109/TASLP.2014.2352556; "An improved virtual analog model of the Moog ladder filter", ICASSP 2013 (second-hand).
- H. Oyama, "Quantifying Nonlinear Behavior in Digital Moog Ladder Filters: Cross-Implementation Comparison and Common-Core Ablation", DAFx-26, paper 29, CC-BY 4.0. https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_29.pdf
- Oyama's testbench, https://github.com/oyama/ladder-filter-testbench (MIT, commit 7e3e3464): the ngspice netlist and the metrics, run host-side from a private directory, never vendored.
- Ledger (private notes) (2026-10-07); report (private notes).
- ADRs 0096, 0101; spec `docs/superpowers/specs/2026-10-05-filter-types-design.md` § 4a; issue #123.
