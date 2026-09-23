# 0009. Address parameters by what they are, not where they sit

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
`ParamPath::Block { block, param }` used the chain node index and encoder
slot. The same index meant different blocks in different chains (a route to
the Modal chain's filter cutoff modulated drive), it couldn't tell sub-pages
apart, and reordering cells or blocks would silently remap saved routes.

## Decision
`ParamAddr { block: BlockRef, param: ParamId }`. `BlockRef` names the block
kind (`Pizza, Modal, Fm, FmOp(Op), Drive, Filter, …`); `ParamId` is a stable
per-block id, never reused. `Op { A, B, C, D }` makes a bad operator index
unrepresentable. UI slots carry a `SlotBinding` (`Param`, `SelectedOp`,
`SelectOp`, `Empty`, `Legacy`); a `SelectedOp` slot resolves to a concrete
operator when a route is created.

## Alternatives considered
- **One `BlockTarget` per page** — can't express FM operator/ratio pages or
  mixed Demo pages.
- **Slot-index addresses** — break when cells are rearranged.

## Consequences
Mod routes survive block reordering (needed by sub-project 2). Multiple
instances of one block kind need an instance id, added with the chain model.

## Sources
Spec §2, §5 of `docs/superpowers/specs/2026-09-23-engine-refactor-design.md`.
