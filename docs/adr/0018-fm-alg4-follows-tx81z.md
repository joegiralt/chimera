# 0018. FM algorithm 4 follows the TX81Z, not p81z

- **Status:** Accepted (2026-09-26)
- **Deciders:** project owner

## Context
The FM engine's routing was ported from p81z's `FMArrangement.cpp`. For TX81Z
ALG 4 (index 3), p81z modulates operator 2 with operator 3's output
(4 → 3, 3 → 2, (3 + 2) → 1). The engine's own design spec said something
different again (4 → 3, (3 + 4→2) → 1). Both disagree with the TX81Z (#18).

## Decision
ALG 4 is 4 → 3, with operator 3 and operator 2 both modulating operator 1;
operator 2 is unmodulated. This is the TX81Z / DX21 / DX27 / DX100 algorithm
4. Every other algorithm keeps p81z's routing, which already matches the TX81Z.

## Alternatives considered
- **Keep p81z's routing:** p81z is otherwise a faithful reference, but here it
  plays a different algorithm from the one the TX81Z's panel diagram shows.
- **Follow the old design spec:** it matches neither the TX81Z nor p81z.

## Consequences
The algorithm page diagram (`ALG_EDGES[3]`) and the `engine_fm_alg` screen
golden were updated. No audio golden uses ALG 4, so none changed.
`fm_test::alg4_op2_is_not_modulated_by_op3` pins the routing: with operator 4
off, swapping operators 2 and 3 leaves the output unchanged.

## Sources
- TX81Z owner's manual, algorithm chart.
- Carcosa v3.0 for the Ambika (the owner's other firmware), whose TX81Z FM path
  is checked sample for sample against a reference: ALG 4 = ((4→3) + 2) → 1.
- `chimera-core/src/dsp/engine_fm.rs`, `chimera-core/src/ui/viz.rs`; #18.
