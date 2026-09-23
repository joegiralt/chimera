# 004 — Mod matrix amounts are indexed by column, not by destination

Found by the engine-refactor final review (2026-09-23). Predates the refactor
(same logic at 78c7c89); belongs with sub-project 2 (patch format).

## Problem
`MatrixState.amounts` (`chimera-core/src/ui/mod_grid.rs`) is indexed by matrix
column and never re-keyed to the registry's destinations.

1. **Removing a destination** shifts the registry entries left but not the
   amount columns, so the remaining routes' depths jump onto different
   parameters.
2. **Switching tracks** keeps the previous track's amounts; the next prime on
   the new track picks them up through `sync_mod_state`.

This undermines ADR 0009 ("addresses never remap routes") from the user's
point of view, even though the addresses themselves are stable.

## Suggested fix
Store amounts with the route (per `ParamAddr` in the patch's registry), not
per UI column; rebuild the matrix view from the patch on track switch.
