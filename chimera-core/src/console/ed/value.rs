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
use crate::ui::cell_text::{self, CellCx, CellOwn};
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

/// What `ed get` and `ed since`'s `val` line say of an address: its place, the
/// stored value and the box's own text. One path, so a since line and a get
/// never disagree. `None` for an address the Part's Sound doesn't hold (not
/// ALGO's, or a block it lacks). The value is the Sound's stored one, as the
/// unit holds it: a refused choice the box shows dimmed is not reported.
pub(super) fn reading(addr: EdAddr, ui: &UiState, own: &CellOwn) -> Option<(f32, f32, FmtBuf)> {
    let sound = &ui.project().part(ui.active_part).sound;
    if sound.engine() != EngineType::Algo {
        return None;
    }
    let a = addr.0;
    let blk = ui.params().block(a.block)?;
    let spec = a.spec()?;
    let (stored, norm) = (blk.get(a.param), blk.normalized(a.param));
    let mut n = 0;
    while n < pages(EngineType::Algo) {
        let cx = ui.cell_cx(def_at(usize::from(n))?, own);
        if let Some(i) = shown_on(&cx, a) {
            return Some((norm, stored, cell_text::value_text(&cx, i, norm)));
        }
        n += 1;
    }
    Some((norm, stored, cell_text::fmt_text(Some(a), spec.fmt, norm)))
}

/// The answer but for the closing `OK`.
pub(super) fn get(addr: EdAddr, ui: &UiState, w: &mut dyn Write) -> Option<fmt::Result> {
    let (norm, stored, text) = reading(addr, ui, &ui.cell_own())?;
    Some(line(addr, norm, stored, text.as_str(), w))
}

/// `val <addr> <normalized> <stored> "<text>"`, newline-ended.
pub(super) fn line(
    a: EdAddr,
    norm: f32,
    stored: f32,
    text: &str,
    w: &mut dyn Write,
) -> fmt::Result {
    write!(w, "val {a} ")?;
    num(w, norm)?;
    w.write_char(' ')?;
    num(w, stored)?;
    writeln!(w, " \"{text}\"")
}
