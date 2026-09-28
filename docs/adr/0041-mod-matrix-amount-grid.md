# 0041. The mod matrix is an amount grid of outlined cells (supersedes in part 0016)

- **Status:** Proposed
- **Deciders:** owner (mockup review, 2026-09-28), firmware (#161)

## Context
ADR 0016 rules out outline boxes. The MTX page drew a dot per route under a
large focus band, showing three sources at a time; the owner's approved
mockup (env-ui, screen e6) instead shows every source at once as a grid of
outlined cells that print their amounts.

## Decision
MOD › MTX is a grid: all eight sources as rows (no vertical scroll), primed
destinations as 36 px columns, five visible, scrolling sideways once the
cursor passes the last one. Every cell is a FAINT outline; a route fills it
with ACCENT_SOFT and prints its amount in ACCENT (`0` unfilled, in the rest
grey); the cursor's outline is ACCENT. The big readout and arc go; a
one-line `SRC → TAG DEST` readout sits under the grid. The exception to
0016's "no outline boxes" is this grid only.

## Alternatives considered
- Keep the dot grid: three sources on screen, amounts only as dot size.
- The mockup's 42 px columns: four destinations visible instead of five.

## Consequences
Every route and its exact amount read at a glance. Column names clip to
33 px (`CUTOFF` reads `CUTO`); the readout carries the full name.

## Sources
Issue #161; `chimera-core/src/ui/mod_grid.rs`; env-ui mockup screen e6.
