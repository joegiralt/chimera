# 0010. Only parameters the voice reads per block are modulatable

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
"Every parameter is modulatable" was false: FX run outside `Voice`, filter/aux
envelopes are never rendered, FM ratios/envelopes and Modal settings are read
only at note-on. Modulating discrete params (FM algorithm, Modal mode) would
switch topology every block.

## Decision
`ParamSpec.modulatable` is true only for params `Voice` reads per block; the
mod registry refuses anything else, and a test proves every modulatable param
audibly changes output. `ParamKind::Enum` is never modulatable. Offsets use
exactly the previous formula, `(v + off * (max - min)).clamp(min, max)`, so
output stays bit-identical (a normalize/denormalize round-trip does not).

## Alternatives considered
- **Normalized-space offsets through curves** — nicer for future exponential
  curves but not bit-exact today; revisit with non-linear curves.

## Consequences
Some params a user might expect to modulate (FM ratios, Modal) aren't, until
their engines read them per block.

## Sources
Spec §4; adversarial review rounds 1–2.
