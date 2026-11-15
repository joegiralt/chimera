//! `ed get <addr>`: `val <addr> <normalized> <stored> "<text>"`. The text
//! is the box's own (`cell_text::value_text`) from the cell that shows the
//! param under the Sound's panels now; a param no cell shows now reads by
//! its spec's format.

use core::fmt::{self, Write};

use super::EdAddr;
use super::describe::{def_at, num, pages};
use crate::addr::{BlockRead, ParamAddr};
use crate::ui::UiState;
use crate::ui::block_def::slot_addr;
use crate::ui::cell_text::{self, CellCx};
use crate::ui::fmt::FmtBuf;
use chimera_engine::params::EngineType;

/// The slot of the page `cx` that shows `a`.
fn shown_on(cx: &CellCx, a: ParamAddr) -> Option<usize> {
    let mut i = 0;
    while i < cx.def.params.len() {
        if slot_addr(cx.def, i, cx.ctx) == Some(a) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// The answer but for the closing `OK`. The value is the Sound's stored
/// one, as the unit holds it: a refused choice the box shows dimmed is not
/// reported; `None` for an address the Part's
/// Sound doesn't hold (not ALGO's, or a block it lacks).
pub(super) fn get(addr: EdAddr, ui: &UiState, w: &mut dyn Write) -> Option<fmt::Result> {
    let sound = &ui.project().part(ui.active_part).sound;
    if sound.engine() != EngineType::Algo {
        return None;
    }
    let a = addr.0;
    let blk = ui.params().block(a.block)?;
    let spec = a.spec()?;
    let (stored, norm) = (blk.get(a.param), blk.normalized(a.param));
    let own = ui.cell_own();
    let mut text = FmtBuf::new();
    let mut n = 0;
    let mut found = false;
    while n < pages(EngineType::Algo) && !found {
        let cx = ui.cell_cx(def_at(usize::from(n))?, &own);
        if let Some(i) = shown_on(&cx, a) {
            text = cell_text::value_text(&cx, i, norm);
            found = true;
        }
        n += 1;
    }
    if !found {
        text = cell_text::fmt_text(Some(a), spec.fmt, norm);
    }
    Some(line(addr, norm, stored, text.as_str(), w))
}

fn line(a: EdAddr, norm: f32, stored: f32, text: &str, w: &mut dyn Write) -> fmt::Result {
    write!(w, "val {a} ")?;
    num(w, norm)?;
    w.write_char(' ')?;
    num(w, stored)?;
    writeln!(w, " \"{text}\"")
}
