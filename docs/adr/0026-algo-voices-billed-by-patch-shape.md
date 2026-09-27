# 0026. Algo voices are billed by patch shape; heavy patches get fewer voices

- **Status:** Accepted (2026-09-27); extends [0022](0022-one-algorithmic-engine.md); partly superseded by [0027](0027-shedding-fades-tails-first.md) (shedding); rev V voice counts superseded by [0031](0031-six-voices-on-rev-v.md)
- **Deciders:** project owner

## Context
The chip bench (rev V, 480 MHz, 2026-09-27) measured, in cycles/sample per
voice: MODAL 391, FLOOR 5, 1 OP 436, ALT 556, 6 OP 720, CHAIN 776, CHN FB
777, WC (A14 ∪ A22, six operators with feedback) 790, KERNEL 478. The FX
bus: PLATE 3181, FDN 3066, MV 3300. The budget is 7,000; the FX bus takes
47% of it, leaving 3,700 for voices. Six voices of the worst case would need
about 617 each.

## Decision
- `AlgoEngine::cost` bills each voice from its patch's shape with measured
  terms, rounded high: `COST_BASE` 370, `COST_OP` 60, `COST_LINK` 8,
  `COST_FEEDBACK` 1, plus `Voice::CHAIN_COST` 10. `FxBus::COST` is 3,300
  (MV), `ModalEngine::COST` 390.
- A patch gets as many voices as fit: a one-operator patch gets six; the
  costliest patch (six operators with feedback on a pair whose union has
  the most links, 12, such as A16 ∪ A17: 842) gets four. Four is the
  floor, pinned by `cost_test`.
- Factory Sounds: SAW LEAD and SQR BASS 6; TX BASS, TX EPIANO, TX BRASS,
  TX BELL 5; MORPH PAD, MORPH KEYS 4.
- Past the budget the allocator steals the oldest held voice; it never goes
  over.

## Alternatives considered
- **Hold the release until six worst-case voices fit:** blocks the engine
  on optimisation work of unknown yield.
- **A flat five or four voices for every Algo patch:** wastes the budget on
  light patches.

## Consequences
- The spec's "Done when" of six worst-case voices is not met.
- The way back to six is
  [#16](https://github.com/joegiralt/chimera/issues/16) (the reverb diet),
  then the addendum's kernel optimisations, in that order.
- A knob turn that raises a patch's cost can now cut held notes, so
  [#31](https://github.com/joegiralt/chimera/issues/31) (a gentle shed on
  recost) matters more.
- The costliest patch's 12 links are priced by extrapolating `COST_LINK`
  from the 5-link CHAIN row; it has not been benched.

## Sources
`docs/superpowers/specs/2026-09-26-algo-engine-design.md` (addendum);
`chimera-stm32/src/bench.rs`; `chimera-core/tests/cost_test.rs`;
`.superpowers/sdd/2026-09-26-algo-engine-core/task-13a-report.md`.
