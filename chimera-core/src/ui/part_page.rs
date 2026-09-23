//! Encoders and display for Part-chain pages, driven by the `BlockDef`'s
//! slot bindings (spec §5): label, format, step and range all come from the
//! bound param's spec.

use crate::addr::Op;
use crate::params::ParamSnapshot;
use crate::ui::block_def::{slot_addr, BlockDef, SlotBinding};

/// Normalized (0..1) display values of the six slots.
pub fn read_values(def: &BlockDef, params: &ParamSnapshot, sel_op: Op) -> [f32; 6] {
    core::array::from_fn(|i| match def.params[i].binding {
        SlotBinding::SelectOp => sel_op.index() as f32 / 3.0,
        _ => slot_addr(def, i, sel_op).map_or(0.0, |a| params.block(a.block).normalized(a.param)),
    })
}

/// One encoder turn on `slot`: steps the bound param, or the operator
/// selection for the `SelectOp` slot.
pub fn apply_encoder(def: &BlockDef, slot: usize, delta: i8, params: &mut ParamSnapshot, sel_op: &mut Op) {
    if def.params.get(slot).is_some_and(|s| s.binding == SlotBinding::SelectOp) {
        *sel_op = sel_op.nudged(delta);
    } else if let Some(a) = slot_addr(def, slot, *sel_op) {
        params.block_mut(a.block).nudge(a.param, delta);
    }
}

/// Shift+encoder on `slot`: snap the bound param (the selector does not snap).
pub fn snap_encoder(def: &BlockDef, slot: usize, delta: i8, params: &mut ParamSnapshot, sel_op: Op) {
    if let Some(a) = slot_addr(def, slot, sel_op) {
        params.block_mut(a.block).snap(a.param, delta);
    }
}
