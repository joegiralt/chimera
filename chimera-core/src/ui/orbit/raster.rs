//! Bank 2's side of ORBIT's pages (`door`): the table's entries, each
//! `draw`'s, on an `Ink`.

use super::door::Ink;
use super::draw::{self, Focus, OrbitFrame};
use crate::orbit::RingIdx;
use crate::ui::block_def::BlockDef;
use crate::ui::view::SlotCtx;

#[inline(never)]
pub(super) fn focus(
    def: &BlockDef,
    slot: usize,
    ctx: &SlotCtx,
    f: &OrbitFrame,
    hints: bool,
    values: &dyn Fn(usize) -> f32,
) -> Option<Focus> {
    draw::focus(def, slot, ctx, f, hints, values)
}

pub(super) fn rhythm(d: &mut Ink<'_>, f: &OrbitFrame, r: RingIdx) {
    draw::rhythm(d, f, r)
}

pub(super) fn voice(d: &mut Ink<'_>, f: &OrbitFrame, r: RingIdx) {
    draw::voice(d, f, r)
}

pub(super) fn notes(d: &mut Ink<'_>, f: &OrbitFrame, r: RingIdx) {
    draw::notes(d, f, r)
}

pub(super) fn orb(d: &mut Ink<'_>, f: &OrbitFrame, sel: RingIdx) {
    draw::orb(d, f, sel)
}

pub(super) fn circle(d: &mut Ink<'_>, f: &OrbitFrame, cursor: f32) {
    draw::circle(d, f, cursor)
}

pub(super) fn chords(d: &mut Ink<'_>, f: &OrbitFrame) {
    draw::chords(d, f)
}

pub(super) fn scenes(d: &mut Ink<'_>, f: &OrbitFrame) {
    draw::scenes(d, f)
}
