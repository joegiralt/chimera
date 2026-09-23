# 0005. Engine selection is patch-based

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
Options for choosing a part's engine: load an init patch for that engine
(Edit+B patch browser, as implemented), MIX + encoder A on the ENGINE node,
or the main encoder. MIX + any encoder already means "coarse snap" on every
page. The photo of the unit shows six encoders; whether a seventh (main)
encoder exists is unconfirmed although `chimera-hal` declares one.

## Decision
Keep engine choice patch-based: loading a patch (or an engine's init patch)
via Edit+B picks the engine. No new control.

## Alternatives considered
- **MIX + A on the ENGINE node** — breaks the "MIX + encoder = coarse snap
  everywhere" rule with a one-page exception.
- **Main encoder** — may not exist on the hardware.

## Consequences
Fits the Digitone-style sound pool; switching engines never leaves a patch
half-configured. The on-device chain editor (sub-project 3) may revisit how
blocks are chosen.

## Sources
Session of 2026-09-23; `chimera-core/src/ui/mod.rs` (Edit+B browser).
