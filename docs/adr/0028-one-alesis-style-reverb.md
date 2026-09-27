# 0028. Use one Alesis-style ring for the reverb

- **Status:** Accepted (2026-09-27); supersedes in part [0014](0014-audio-memory-map.md) (its rejection of 16-bit delay lines, and its FX bus and AXI totals)
- **Deciders:** project owner

## Context
The FX bus took 3,310 cycles per sample on rev V, leaving the voices too
little. Three reverbs (Plate, FDN, MidiVerb), chosen by TYPE, sat within
234 cycles of each other; MidiVerb alone read 361 cycles on the per-effect
bench, so most of the cost was never the reverb. The sound wanted is the
early Warp records' reverb: grainy, dark, wide, metallic in a musical way,
after the Alesis Quadraverb. GitHub issue #16 asked for a shared-arena,
low-rate, 16-bit Quadraverb-style reverb; this decision supersedes it with
the ring built here.

## Decision
- One reverb, `dsp::ring`: four stages in a ring S1 → S2 → S3 → S4 → S1,
  each allpass → allpass → delay → one-pole DAMP → gain g_k, after Sean
  Costello's description of the Quadraverb. The send enters +0.5 at S1
  and −0.5 at S3; each side sums three delay taps spread across
  different stages.
- Half rate: the ring runs at sample_rate / 2 (24 kHz on the chip) behind
  one 35-tap half-band decimator and two interpolators (L, R), the same
  filter shared by all three. The band limit is the dark top.
- i16 storage, full scale ±2.0. GRIT is a continuous bit depth, not a
  switch between fixed values: every write lands on GRIT's grid,
  Δ = 2^(6·GRIT) LSBs (16 bits down to 10 as GRIT runs 0 to 1).
- Q3 quantisation: the eight allpass state (`v`) writes truncate toward
  zero; the delay and DAMP writes round. No silence gate. Evidence is
  `.superpowers/sdd/2026-09-27-fx-diet/task-3-experiment.md`: on a unit
  impulse, truncation looked like it cut T30 by 79%, but that was the
  impulse hitting the quantisation floor, not the design. On a 0.5 s noise
  burst (Schroeder EDC, least-squares fit) every quantiser tried passes
  within ±35% of the TIME target at GRIT 0. Rounding everywhere (Q1)
  never reaches silence on its own — it sits at 12–32Δ output with no
  input, 6–16× a 2Δ gate's threshold — while truncation (Q2, Q3) reaches
  exact zero unaided in 3–11 s. RT60 is measured on that noise burst, not
  an impulse.
- SIZE is quantised to 32 steps, each a `const fn` table of distinct prime
  lengths; a step crossfades every allpass, delay and tap read over 720
  ring samples (30 ms at 24 kHz) and never moves a read pointer.
- TIME, DAMP and GRIT are one-pole smoothed per block. `WET_GAIN` (4.54)
  matches the old plate's RMS within 1 dB.
- Plate, FDN, MidiVerb and TYPE retire. GRIT takes `ParamId(5)`; TYPE's
  `ParamId(0)` is retired and never reused (ADR 0009).
- Memory: the ring is 46,440 B. `FxBus` = 160,032 B (was 253,612). AXI
  holds 419,068 of 524,288 B.

## Alternatives considered
- **Keep three reverbs:** three times the code, none of them cheap.
- **f32 storage** (92,880 B, fits): no grain, and twice the D-cache
  footprint. It stays the fallback if the i16 grain is too strong at
  GRIT 0.
- **A full-rate ring:** twice the cost, and no dark band limit.
- **Rounding everywhere, with a silence gate** (the plan's original
  ruling): rounding sustains a 12–32Δ tail with no input, which no gate
  catches without also cutting off a live tail early.

## Consequences
- The reverb is billed in `FxBus::COST` (Task 14 records the final
  reading).
- ADR 0014 rejected 16-bit lines because they changed every locked
  output; the ring's output was new, so there was nothing to keep.
- At high GRIT the truncation shortens long tails (to about 0.34–0.92 of
  the TIME target at GRIT 1): part of the grain, not a defect. Nothing
  may gate work on `Ring::is_silent`, which the rounded DAMP state can
  keep false indefinitely.
- Issue #16 is superseded by this ring rather than built separately.

## Sources
- `docs/superpowers/specs/2026-09-27-fx-diet-design.md`, `docs/superpowers/plans/2026-09-27-fx-diet.md`.
- Sean Costello (Valhalla DSP), KVR Audio forum thread 349039: "4 parallel loops (2 x AP + 1 delay), outputs from delay taps".
- `.superpowers/sdd/2026-09-27-fx-diet/task-3-experiment.md` (Q3 quantiser evidence).
- `chimera-core/src/dsp/ring.rs`, `halfband.rs`, `tests/reverb_test.rs`.
- https://github.com/joegiralt/chimera/issues/16
