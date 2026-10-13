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
    } else if let Some(a) = slot_addr(def, slot, ctx) {
        edit(params, a, Edit::Nudge(delta));
    }
}

/// How an edit moves a param: a turn, a MIX+turn, or a mapped CC's value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit {
    Nudge(i8),
    Snap(i8),
    To(f32),
}

/// The one edit path (spec § 6): `a` in `params` moved by `how`, then a
/// TAPE MODEL change's machine. `true` when the value moved.
pub fn edit(params: &mut impl Blocks, a: ParamAddr, how: Edit) -> bool {
    let Some(mut b) = params.block_mut(a.block) else {
        return false;
    };
    let before = b.get(a.param);
    match how {
        // The last kind the budget allows, as a turn steps to it.
        Edit::Snap(d)
            if matches!(a.block, BlockRef::AlgoOp(_)) && a.param == AlgoOpParams::WAVE =>
        {
            b.nudge(a.param, d.signum() * i8::MAX)
        }
        Edit::Snap(d) => b.snap(a.param, d),
        Edit::Nudge(d) => b.nudge(a.param, d),
        Edit::To(v) => b.set(a.param, v),
    }
    machine_edit(a, before, &mut *b);
    b.get(a.param) != before
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

/// Shift+encoder on `slot`: snap the bound param (the selector does not snap).
pub fn snap_encoder(def: &BlockDef, slot: usize, delta: i8, params: &mut impl Blocks, sel_op: Op) {
    let ctx = SlotCtx::read(&*params, sel_op);
    snap_under(def, slot, delta, params, &ctx);
}

/// `snap_encoder` under `ctx`.
pub fn snap_under(def: &BlockDef, slot: usize, delta: i8, params: &mut impl Blocks, ctx: &SlotCtx) {
    if let Some(a) = slot_addr(def, slot, ctx) {
        edit(params, a, Edit::Snap(delta));
    }
}
