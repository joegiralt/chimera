//! Encoders and display for Part-chain pages, driven by the `BlockDef`'s
//! slot bindings (spec §5): label, format, step and range all come from the
//! bound param's spec.

use crate::addr::{BlockRead, BlockRef, Blocks, Op, ParamAddr};
use crate::block::Block;
use crate::dsp::algo::params::AlgoOpParams;
use crate::dsp::tape_engine::machine::defaults;
use crate::dsp::tape_engine::{TapeEngineParams, TapeModel};
use crate::ui::block_def::{BlockDef, SlotBinding, slot_addr};
use crate::ui::view::{SlotCtx, View, view};

/// Normalized (0..1) display values of the six slots (a route view reads 0:
/// the UI overlays its amount).
pub fn read_values(def: &BlockDef, params: &impl BlockRead, sel_op: Op) -> [f32; 6] {
    values_under(def, params, &SlotCtx::read(params, sel_op))
}

/// `read_values` under `ctx`: an ORBIT page's ring is the location's.
pub fn values_under(def: &BlockDef, params: &impl BlockRead, ctx: &SlotCtx) -> [f32; 6] {
    core::array::from_fn(|i| match view(def, i, ctx) {
        View::SelectOp => ctx.sel_op.index() as f32 / (Op::ALL.len() - 1) as f32,
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
    let ctx = SlotCtx::read(&*params, *sel_op);
    apply_under(def, slot, delta, params, &ctx, sel_op);
}

/// `apply_encoder` under `ctx`: an ORBIT page's ring is the location's.
pub fn apply_under(
    def: &BlockDef,
    slot: usize,
    delta: i8,
    params: &mut impl Blocks,
    ctx: &SlotCtx,
    sel_op: &mut Op,
) {
    if def
        .params
        .get(slot)
        .is_some_and(|s| s.binding == SlotBinding::SelectOp)
    {
        *sel_op = sel_op.nudged(delta);
    } else if let Some(a) = slot_addr(def, slot, ctx)
        && let Some(mut b) = params.block_mut(a.block)
    {
        let before = b.get(a.param);
        b.nudge(a.param, delta);
        machine_edit(a, before, &mut *b);
    }
}

/// A MODEL edit on TAPE writes the new machine's defaults (spec § 2
/// Machines); a load never does. The engine eases them in.
fn machine_edit(a: ParamAddr, before: f32, b: &mut dyn Block) {
    let model = TapeEngineParams::MODEL;
    if a == ParamAddr::new(BlockRef::TapeEngine, model) && b.get(model) != before {
        for (id, v) in defaults(TapeModel::from_index(b.get(model) as u8)) {
            b.set(id, v);
        }
    }
}

/// A pushed encoder's turn on `slot` (ADR 0140): the bound param fine, or
/// the selector a plain tick.
pub fn fine_encoder(
    def: &BlockDef,
    slot: usize,
    delta: i8,
    params: &mut impl Blocks,
    sel_op: &mut Op,
) {
    let ctx = SlotCtx::read(&*params, *sel_op);
    if def
        .params
        .get(slot)
        .is_some_and(|s| s.binding == SlotBinding::SelectOp)
    {
        *sel_op = sel_op.nudged(delta.signum());
    } else if let Some(a) = slot_addr(def, slot, &ctx)
        && let Some(mut b) = params.block_mut(a.block)
    {
        let before = b.get(a.param);
        b.nudge_fine(a.param, delta);
        machine_edit(a, before, &mut *b);
    }
}

/// A push tap on `slot` (ADR 0140): the bound param to its default, set
/// as shift+encoder sets a snap point. The selector has none.
pub fn default_encoder(def: &BlockDef, slot: usize, params: &mut impl Blocks, sel_op: Op) {
    let ctx = SlotCtx::read(&*params, sel_op);
    if let Some(a) = slot_addr(def, slot, &ctx)
        && let Some(mut b) = params.block_mut(a.block)
        && let Some(s) = b.spec(a.param)
    {
        let before = b.get(a.param);
        b.set(a.param, s.default);
        machine_edit(a, before, &mut *b);
    }
}

/// Shift+encoder on `slot`: snap the bound param (the selector does not snap).
pub fn snap_encoder(def: &BlockDef, slot: usize, delta: i8, params: &mut impl Blocks, sel_op: Op) {
    let ctx = SlotCtx::read(&*params, sel_op);
    snap_under(def, slot, delta, params, &ctx);
}

/// `snap_encoder` under `ctx`.
pub fn snap_under(def: &BlockDef, slot: usize, delta: i8, params: &mut impl Blocks, ctx: &SlotCtx) {
    if let Some(a) = slot_addr(def, slot, ctx)
        && let Some(mut b) = params.block_mut(a.block)
    {
        let before = b.get(a.param);
        if matches!(a.block, BlockRef::AlgoOp(_)) && a.param == AlgoOpParams::WAVE {
            // The last kind the budget allows, as a turn steps to it.
            b.nudge(a.param, delta.signum() * i8::MAX);
        } else {
            b.snap(a.param, delta);
        }
        machine_edit(a, before, &mut *b);
    }
}
