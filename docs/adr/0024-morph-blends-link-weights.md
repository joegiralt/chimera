# 0024. MORPH blends link weights; one plan orders both algorithms

- **Status:** Accepted (2026-09-26)
- **Deciders:** project owner

## Context
MORPH moves a voice from ALG A to ALG B. The operators must be evaluated in
an order where a modulator runs before its target, and each end of the morph
must sound exactly like its algorithm alone.

## Decision
- Each link's weight is `a + m (b − a)` (1 or 0 in each algorithm), and so is
  each operator's carrier gain; MORPH ramps per sample across each block.
- The output is `Σ gain · out / sqrt(max(1, Σ gain))`: equal loudness for
  uncorrelated carriers, smooth through the morph.
- The plan is the union of both algorithms' links, in a topological order
  (Kahn's, highest operator first). A link that runs backwards in that order
  reads its source's previous sample, and the plan's `delayed` mask records
  it. A plan with no delayed link renders operator-major, a block per
  operator; one with any renders sample-major, where each operator's latest
  output is kept in place. Both give the same output.
- Every table algorithm has its modulators numbered above their targets, so
  the union of any two is ordered 6 → 1 with no link running backwards. One
  plan therefore serves the whole morph range, and at MORPH 0 and 1 the
  output is bit-identical to the algorithm alone (tested for A14 → A22).
- A change of ALG A or B while a note sounds ducks the output over one block,
  swaps the plan, and ramps back over the next.

## Alternatives considered
- **Switch between A's and B's own orders at the ends:** needless when one
  order suits both, and a switch mid-morph would delay a live link.
- **Crossfade two whole renders:** twice the cost.

## Consequences
A future user-defined algorithm that links upward will make some unions
cyclic; its backward links read one sample late, and its plans render
sample-major, the slower path.

## Sources
`chimera-core/src/dsp/algo/{plan,kernel,morph}.rs`;
`chimera-core/tests/algo_engine_test.rs`.
