//! Encoders and display for Part-chain pages, driven by the `BlockDef`'s
//! slot bindings (spec §5): label, format, step and range all come from the
//! bound param's spec.

use crate::addr::{BlockRead, Blocks, Op};
use crate::ui::block_def::{BlockDef, SlotBinding, slot_addr};
use crate::ui::view::{SlotCtx, View, view};

/// Normalized (0..1) display values of the six slots (a route view reads 0:
/// the UI overlays its amount).
pub fn read_values(def: &BlockDef, params: &impl BlockRead, sel_op: Op) -> [f32; 6] {
    let ctx = SlotCtx::read(params, sel_op);
    core::array::from_fn(|i| match view(def, i, &ctx) {
        View::SelectOp => sel_op.index() as f32 / (Op::ALL.len() - 1) as f32,
        v => v
            .addr()
            .and_then(|a| Some(params.block(a.block)?.normalized(a.param)))
            .unwrap_or(0.0),
    })
}

/// One encoder turn on `slot`: steps the bound param, or the operator
/// selection for the `SelectOp` slot.
pub fn apply_encoder(
    def: &BlockDef,
    slot: usize,
    delta: i8,
    params: &mut impl Blocks,
    sel_op: &mut Op,
) {
    if def
        .params
        .get(slot)
        .is_some_and(|s| s.binding == SlotBinding::SelectOp)
    {
        *sel_op = sel_op.nudged(delta);
    } else if let Some(a) = slot_addr(def, slot, &SlotCtx::read(&*params, *sel_op))
        && let Some(b) = params.block_mut(a.block)
    {
        b.nudge(a.param, delta);
    }
}

/// Shift+encoder on `slot`: snap the bound param (the selector does not snap).
pub fn snap_encoder(def: &BlockDef, slot: usize, delta: i8, params: &mut impl Blocks, sel_op: Op) {
    if let Some(a) = slot_addr(def, slot, &SlotCtx::read(&*params, sel_op))
        && let Some(b) = params.block_mut(a.block)
    {
        b.snap(a.param, delta);
    }
}
