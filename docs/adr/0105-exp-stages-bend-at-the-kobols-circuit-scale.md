# 0105. EXP's stages bend at the Kobol's circuit scale (supersedes in part 0096)

- **Status:** Accepted (owner, 2026-10-07)
- **Deciders:** owner, with Claude (DSP)

## Context
The owner finds the 2040's EXP voice "doesn't sound like a Kobol". An A-E probe (2026-10-07, `exp-probe.md`) varied EXP's stage bend (B), limiter shape (C), drive (D) and limiter law (E) against today's (A). The owner chose B as closest to the circuit as understood.

ADR 0096's stage law is `g·sat(v)` with `v` in model units, 1 V = `R_2040` = 0.2 units. Each SSM2040 stage's OTA input is a node of R_i 10 K, R_f 10 K and R_a to ground (Kobol 220 Ω; the data sheet draws 200 Ω). 1 V of stage error lands as 21.07 mV there, 0.405 × 2V_T, which in `sat` units is 0.608 against 0.2: 3.04×. So 0096 under-bends EXP's stages by that factor.

Honest size of it: B measures within 0.2 dB of A on every probe metric (unity, THD, squelch, self-oscillation level, renders). Stage errors stay under 0.85 V, so the circuit-scale tanh compresses at most 4 % at peak. The change is circuit-correct, not audibly large; the owner may not hear it.

## Decision
`K2040Voice::stage_scale()` is a multiple of REV2's bend: REV2 1.0, EXP `STAGE_SCALE_EXP` = 1.5·(node/R_f)/(2V_T)/`R_2040` = 3.04, derived in a const from 10 K/10 K/220 Ω and pinned by a const assert. The stage is `g·sat(S·v)/S`, i.e. `v·(g − ga·S²·v²)` with `v` clamped at ±1.5/S (±0.493 on EXP). `S` is a `VoiceSpec` const, so REV2 compiles to today's code (bit for bit) and EXP costs 0 cycles. REV2 does not change: the probe does not establish the Prophet's stage node.

Because the harder bend rings EXP's self-oscillation 11 cents flat, EXP gains `TRIM_EXP` = +11.5 cents (REV2's precedent, `TRIM_REV2`), keeping the 5-cent tracking test; residual under 1 cent.

## Alternatives considered
- Keep 0096's scale: simplest, but not the circuit.
- The data sheet's 200 Ω (2.79×): the Kobol's BOM says 220 Ω.
- C, D, E (rational-tanh limiter, +6 dB drive, circuit limiter law): audible, but not the owner's pick; D alone over-squelches. Left for a later decision.

## Consequences
- EXP's loud stage content is a little denser; the filter alone moves by at most 0.2 dB.
- EXP's cutoff is trimmed +11.5 cents: the RES 0 corner is that much sharp of FREQ (REV2's is 13 cents flat).
- REV2, the limiter, drive and output stage are untouched.
- If the Prophet's stage node proves the same circuit, REV2's `stage_scale` changes by a new ADR.

## Sources
- SSM 2040 data sheet (Digisound reprint, Aug 1979): OTA law p. 2, figs. 4/5/9, design hint d.
- Kobol Expander schematic (kobol-research.md [SCHEM]): 220 Ω.
- (private notes) step 1 and variant B.
- ADR 0096.
