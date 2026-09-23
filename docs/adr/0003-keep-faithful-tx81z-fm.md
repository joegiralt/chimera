# 0003. Keep the faithful TX81Z 4-op FM engine

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
A faithful TX81Z port already exists (`dsp/engine_fm.rs`): 8 algorithms,
5-stage envelopes, 64-entry coarse ratio table, p81z waveforms, KVS
polynomial. The first FM engine was deleted for using `libm::sinf` in the
render path (~2,000 cycles/sample). The engine-pivot spec initially proposed
replacing TX81Z with a Monomachine-style 2–3-op FM+.

## Decision
Keep the TX81Z engine. Drop the planned FM+ engine as redundant. Rules from
the FM work stand: no libm in the render path (1024-point sine LUT,
`fast_sin`), DC blocking, waveform names from p81z ("do not invent names").

## Alternatives considered
- **Replace with FM+** — throws away a working, faithful engine.

## Consequences
FM remains the most complex engine UI (operator focus page, ratio page,
per-op envelopes as MOD sub-pages).

## Sources
- `docs/superpowers/specs/2026-04-24-4op-fm-engine-design.md`
- `docs/issues/002-fm-tx81z-rewrite.md`
- p81z, ymfm (feedback averaging), cesaref; deicsonze warned against for waveforms
