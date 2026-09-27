# 0022. One algorithmic six-operator engine replaces Pizza, FM and VA

- **Status:** Accepted (2026-09-26); supersedes [0003](0003-keep-faithful-tx81z-fm.md)
- **Deciders:** project owner

## Context
The chip bench measured the ported 4-op FM engine at 6,590 cycles per voice
per sample, over the whole 7,000-cycle budget, so every FM note was refused
(#26). Its envelope ran in `f64`, which the Cortex-M7 does in software (24
soft-double calls per sample), and its waves called libm's `sinf`, which is
soft-double inside. Pizza and the silent VA placeholder were two more engines
to maintain for roles a phase-modulation engine covers with one or two
operators. The ported code's licence was never verified.

## Decision
- One engine, Algo: six phase-modulation operators on 32 algorithms (the
  TX81Z's eight on operators 1–4 with a 6→5 pair, and 24 more), morphing
  between ALG A and ALG B. `EngineType` and `ChainType` are `{Algo, Modal}`.
- The render path is `f32` only, with no libm: a source scan fails the
  build's tests on `f64` or libm anywhere in `dsp/algo/`.
- Envelopes run per sample with TX81Z-style rates: every four steps of
  effective rate doubles the speed; AR 31 is about 12 samples.
- The engine does not use the amp envelope; the operator envelopes shape the
  sound, so release tails ring out. The amp envelope keeps running as the
  ENV mod source, and since no engine puts it on the VCA, its parameters are
  no longer modulation destinations (ADR 0010).
- Parameters are stored as bytes (`u8`/`i8`). MORPH and the six LEVELs are
  the destinations, applied unrounded to the gain and the blend.
- The budget: the engine targets 350 cycles per voice per sample.
  `AlgoEngine::COST` is an estimate (560) until the chip bench's worst case
  (six operators with feedback, six waves, MORPH halfway between A14 and
  A22) is measured; the cost is then that measurement minus
  `Voice::CHAIN_COST`. The spec's addendum rules that the allocator bills a
  patch-dependent cost from the patch's shape, with bench-measured
  coefficients, in place of one flat worst case.
- ADR 0003's rules, one by one: no libm in the render path — kept, and
  extended to `f64`; DC blocking — not added: only W3, W4, W7 and W8 carry
  DC, as on the TX81Z; waveform names from the ported code — replaced by the
  TX81Z's own W1–W8 and plain names for the classic waves (ADR 0023).

## Alternatives considered
- **Fix the FM engine's `f64` and keep it:** still four operators, one
  algorithm set, and code of unverified licence.
- **Keep Pizza and VA beside a new engine:** three engines where one covers
  their roles.
- **Per-block envelopes:** cheaper, but the TX81Z's fastest attack is about
  12 samples, and a 64-sample block would smear it.

## Consequences
The FM, Pizza and VA goldens are gone; Algo goldens were recorded after its
sanity gate. A factory bank of eight Algo Sounds fills the pool at start.
The Algo chain is `ALG · OSC · DRV · FLT · FLD · MOD`: ALG is the home page,
and every operator parameter is on an OSC group page. Modulation modes, the
full wave set, gang edit and the TX character path are sub-projects 2–5.

## Sources
`docs/superpowers/specs/2026-09-26-algo-engine-design.md`;
`docs/superpowers/plans/2026-09-26-algo-engine-core.md`; #26; the chip
bench (`chimera-stm32/src/bench.rs`); TX81Z owner's manual.
