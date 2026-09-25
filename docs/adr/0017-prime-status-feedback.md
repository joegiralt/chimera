# 0017. MIX+PLUS reports its outcome in place, until the next input

- **Status:** Accepted (2026-09-25)
- **Deciders:** project owner (issue sweep, #21)

## Context
Priming a modulation destination (MIX+PLUS) used to discard the registry's
result, so a refused prime (a parameter that isn't modulatable per ADR 0010,
or a full matrix) looked the same as a successful one. On hardware this read
as "MIX+PLUS doesn't work" (#21). The matrix page's hint also suggested
priming there, where MIX+PLUS does nothing.

## Decision
- MIX+PLUS on a parameter page yields one of four outcomes, a `PrimeStatus`
  enum: `ADDED`, `ALREADY ROUTED`, `NOT MODULATABLE`, `MATRIX FULL`.
- The message is shown where the eye already is: in the focus band, in place
  of the value readout. On BigViz pages, which have no focus band, it is one
  line at the top of the viz band.
- It stays until the next encoder turn, button press or page change. There is
  no timer, which matches the focus band's rule.
- The destination registry's capacity equals what the matrix and `ModState`
  hold, so `MATRIX FULL` fires at the real limit.
- The matrix page hint reads `PRIME: MIX+PLUS ON A PARAM`.

## Alternatives considered
- **Timed toast (e.g. 1 s):** adds a timer and a redraw that the user didn't
  cause; the focus band deliberately has no timers.
- **A dedicated status line:** there is no free row on the 240×320 layout
  without shrinking the viz band.
- **Marking modulatable parameters up front** (a dot by the label): useful,
  but a separate visual-language decision; left open on #21.

## Consequences
Every refusal is visible, and the message costs one region redraw. MIX+PLUS on
an unbound slot is still silent: there is nothing to prime.

## Sources
#21; `chimera-core/src/ui/mod.rs` (`PrimeStatus`), `chimera-core/src/mod_path.rs`;
ADR 0010 (modulation targets), ADR 0016 (visual direction).
