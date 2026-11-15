//! `ed set <addr> <value>`: the value resolved here, bank 2's, queued for
//! the shell's UI pass, which applies it as a knob turn (`UiState::edit_to`,
//! ADR 0112's glide): never a snap, never a zipper.

use core::fmt::{self, Write};

use super::describe::{def_at, pages};
use super::{EdAddr, EdValue};
use crate::addr::{BlockRead, BlockRef, ParamAddr};
use crate::block::ParamId;
use crate::project::PartId;
use crate::ui::UiState;
use crate::ui::block_def::slot_addr;
use crate::ui::page::ValFmt;
use crate::ui::view;
use chimera_engine::params::EngineType;

/// Sets waiting for the next UI pass. A lap of a browser's sliders is a
/// handful; more than this is the host outrunning the pass.
const CAP: usize = 16;

#[derive(Clone, Copy)]
struct Pending {
    part: PartId,
    addr: ParamAddr,
    to: f32,
}

impl Pending {
    /// A slot nothing is queued in (`len` says so): all zeros, so the
    /// queue sits in `.bss` with no init code.
    const SPARE: Pending = Pending {
        part: PartId::ALL[0],
        addr: ParamAddr::new(BlockRef::Algo, ParamId(0)),
        to: 0.0,
    };
}

/// What `EdQueue::push` made of a set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    Queued,
    Full,
}

/// The sets the console has taken and the UI pass has not yet applied: as
/// `Hands` queues `Gesture`s, a fixed array the shell owns.
pub struct EdQueue {
    items: [Pending; CAP],
    len: u8,
}

impl EdQueue {
    pub const fn new() -> Self {
        EdQueue {
            items: [Pending::SPARE; CAP],
            len: 0,
        }
    }

    pub fn push(&mut self, part: PartId, addr: ParamAddr, to: f32) -> Pushed {
        match self.items.get_mut(usize::from(self.len)) {
            None => Pushed::Full,
            Some(slot) => {
                *slot = Pending { part, addr, to };
                self.len += 1;
                Pushed::Queued
            }
        }
    }

    /// Every queued set, oldest first, as a knob turn on the Part it was
    /// taken for. A Part that is no longer ALGO drops its sets.
    pub fn apply(&mut self, ui: &mut UiState) {
        let mut i = 0;
        while i < usize::from(self.len) {
            if let Some(&p) = self.items.get(i)
                && ui.project().part(p.part).sound.engine() == EngineType::Algo
            {
                ui.edit_to(p.part, p.addr, p.to);
            }
            i += 1;
        }
        self.len = 0;
    }
}

impl Default for EdQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// `a`'s format as the Sound shows it now: its cell's where a page shows
/// it, else its spec's (as `ed get` reads it).
fn fmt_of(ui: &UiState, a: ParamAddr) -> Option<ValFmt> {
    let own = ui.cell_own();
    let mut n = 0;
    while n < pages(EngineType::Algo) {
        let cx = ui.cell_cx(def_at(usize::from(n))?, &own);
        let mut i = 0;
        while i < cx.def.params.len() {
            if slot_addr(cx.def, i, cx.ctx) == Some(a) {
                return Some(view::view(cx.def, i, cx.ctx).fmt());
            }
            i += 1;
        }
        n += 1;
    }
    Some(a.spec()?.fmt)
}

/// The stored value `val` means for `a`, or why not: a choice's name (its
/// own, matched exactly, the typed word kept) or a number.
fn resolve(ui: &UiState, a: ParamAddr, val: EdValue) -> Result<f32, &'static str> {
    let (word, num) = match val {
        EdValue::Num(n, w) => (w, Some(n)),
        EdValue::Name(w) => (w, None),
    };
    match fmt_of(ui, a) {
        Some(ValFmt::Names(names)) => {
            let mut i = 0;
            while i < names.len() {
                if names[i].as_bytes() == word.as_bytes() {
                    return Ok(i as f32);
                }
                i += 1;
            }
            Err("takes one of its names")
        }
        _ => num.ok_or("takes a number"),
    }
}

/// The answer, `OK queued` or `ERR`: whole, for `answer_in`.
pub(super) fn set(
    addr: EdAddr,
    val: EdValue,
    ui: &UiState,
    q: Option<&mut EdQueue>,
    w: &mut dyn Write,
) -> fmt::Result {
    let part = ui.active_part;
    let a = addr.0;
    if ui.project().part(part).sound.engine() != EngineType::Algo
        || ui.params().block(a.block).is_none()
        || a.spec().is_none()
    {
        return writeln!(w, "ERR ed set has no {addr} here");
    }
    let to = match resolve(ui, a, val) {
        Ok(to) => to,
        Err(why) => return writeln!(w, "ERR ed set {addr} {why}"),
    };
    match q.map(|q| q.push(part, a, to)) {
        None => w.write_str("ERR ed set is not in this build\n"),
        Some(Pushed::Full) => w.write_str("ERR busy\n"),
        Some(Pushed::Queued) => w.write_str("OK queued\n"),
    }
}
