//! `ed get <addr>`: `val <addr> <normalized> <stored> "<text>"`. The text
//! is the box's own (`cell_text::value_text`) from the cell that shows the
//! param under the Sound's panels now, a pad's under its own; a param no
//! cell shows now reads by its spec's format.

use core::fmt::{self, Write};

use super::addr::{EdBlock, PadBlock};
use super::describe::num;
use super::pages::Pages;
use super::{EdAddr, since};
use crate::addr::{BlockRead, BlockRef};
use crate::dsp::drum::sound::RouteId;
use crate::dsp::drum::types::PadId;
use crate::ui::UiState;
use crate::ui::block_def::{BlockDef, SlotBinding};
use crate::ui::cell_text::{self, CellCx, CellOwn};
use crate::ui::fmt::FmtBuf;
use crate::ui::view::{self, SlotCtx};
use chimera_engine::addr::BlockView;
use chimera_engine::params::EngineType;

/// The followed Part's engine.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.value.engine_of"))]
pub(super) fn engine_of(ui: &UiState) -> EngineType {
    ui.project().part(ui.active_part).sound.engine()
}

/// `at`'s values on the followed Part: its Sound's block, its kit's, or
/// pad `n`'s, never the box's selected pad's.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.value.block"))]
pub(super) fn block(ui: &UiState, at: EdBlock) -> Option<BlockView<'_>> {
    let kit = ui.project().kit(ui.active_part);
    match at {
        EdBlock::Sound(s) => match s.get() {
            BlockRef::Kit => Some(BlockView::Plain(kit)),
            b => ui.params().block(b),
        },
        EdBlock::Pad(n, part) => {
            let pad = kit.pad(n);
            let s = pad.sound();
            Some(BlockView::Plain(match part {
                PadBlock::Pad => pad,
                PadBlock::Filter => s.filter(),
                PadBlock::Amp => s.amp(),
                PadBlock::Env => s.env(),
                PadBlock::Lfo => s.lfo(),
                PadBlock::Route(r) => s.routes().route(r),
            }))
        }
    }
}

/// What the followed Part's texts read beside the bits, as the editor
/// sees it: on DRUM, pad 1 and route 1, never the box's selected pad, so
/// choosing a pad on the box moves no text.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.value.text_ctx"))]
pub(super) fn text_ctx(ui: &UiState, own: &CellOwn) -> SlotCtx {
    pad_ctx(ui, own, PadId::ALL[0], RouteId::ALL[0])
}

/// The context pad `n`'s cells read under, its route `r` selected. Off
/// DRUM, the live context.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.value.pad_ctx"))]
fn pad_ctx(ui: &UiState, own: &CellOwn, n: PadId, r: RouteId) -> SlotCtx {
    match engine_of(ui) {
        EngineType::Drum => own
            .ctx
            .on_kit(ui.project().kit(ui.active_part), n)
            .on_route(r)
            .on_drum(),
        EngineType::Algo | EngineType::Modal | EngineType::Tape | EngineType::Va => own.ctx,
    }
}

/// The context `a`'s cell reads under.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.value.ctx_for"))]
fn ctx_for(ui: &UiState, own: &CellOwn, a: EdAddr) -> SlotCtx {
    match a.at() {
        EdBlock::Pad(n, PadBlock::Route(r)) => pad_ctx(ui, own, n, r),
        EdBlock::Pad(n, _) => pad_ctx(ui, own, n, RouteId::ALL[0]),
        EdBlock::Sound(_) => text_ctx(ui, own),
    }
}

/// Where a page of the followed engine shows `a` now: the page, its slot
/// and the context it reads under. A pad's on that pad's page; the
/// Sound's on the Part's pages, or pad 1's.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.value.find"))]
pub(super) fn find(
    ui: &UiState,
    own: &CellOwn,
    a: EdAddr,
) -> Option<(&'static BlockDef, usize, SlotCtx)> {
    let ctx = ctx_for(ui, own, a);
    let engine = engine_of(ui);
    // A pad's address is on its own pad's pages alone; the Sound's on the
    // Part's, else pad 1's (a Sound cell on a pad page reads the same on
    // every pad).
    let (first, then) = match a.pad() {
        Some(p) => (Pages::pad(engine, p), None),
        None => (Pages::part(engine), Some(Pages::pad(engine, PadId::ALL[0]))),
    };
    if let Some((def, i)) = slot_in(first, a, &ctx) {
        return Some((def, i, ctx));
    }
    let (def, i) = slot_in(then?, a, &ctx)?;
    Some((def, i, ctx))
}

/// The first page of `pages` with a slot that shows `a` under `ctx`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.value.slot_in"))]
fn slot_in(pages: Pages, a: EdAddr, ctx: &SlotCtx) -> Option<(&'static BlockDef, usize)> {
    for (def, _) in pages {
        let mut i = 0;
        while i < def.params.len() {
            if view::view(def, i, ctx).addr() == Some(a.param()) {
                return Some((def, i));
            }
            i += 1;
        }
    }
    None
}

/// What `ed get` and `ed since`'s `val` line say of an address: its place, the
/// stored value and the box's own text. One path, so a since line and a get
/// never disagree. `None` for an address the Part's Sound doesn't hold (not
/// its engine's, or a block it lacks). The value is the Sound's stored one, as
/// the unit holds it: a refused choice the box shows dimmed is not reported.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.value.reading"))]
pub(super) fn reading(addr: EdAddr, ui: &UiState, own: &CellOwn) -> Option<(f32, f32, FmtBuf)> {
    if !since::in_engine(engine_of(ui), addr.at()) {
        return None;
    }
    let blk = block(ui, addr.at())?;
    let spec = addr.param().spec()?;
    let (stored, norm) = (blk.get(addr.id()), blk.normalized(addr.id()));
    let text = match find(ui, own, addr) {
        Some((def, i, ctx)) => {
            let cx = CellCx {
                ctx: &ctx,
                ..ui.cell_cx(def, own)
            };
            cell_text::value_text(&cx, i, norm)
        }
        None => cell_text::fmt_text(Some(addr.param()), spec.fmt, norm),
    };
    Some((norm, stored, text))
}

/// TAPE's SOURCE as the box reads it, if the followed engine shows one.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.value.source_text")
)]
pub(super) fn source_text(ui: &UiState, own: &CellOwn) -> Option<FmtBuf> {
    let ctx = text_ctx(ui, own);
    for (def, _) in Pages::part(engine_of(ui)) {
        let mut i = 0;
        while let Some(s) = def.params.get(i) {
            if let SlotBinding::Source = s.binding {
                let cx = CellCx {
                    ctx: &ctx,
                    ..ui.cell_cx(def, own)
                };
                return Some(cell_text::value_text(&cx, i, 0.0));
            }
            i += 1;
        }
    }
    None
}

/// The answer but for the closing `OK`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.value.get"))]
pub(super) fn get(addr: EdAddr, ui: &UiState, w: &mut dyn Write) -> Option<fmt::Result> {
    let (norm, stored, text) = reading(addr, ui, &ui.cell_own())?;
    Some(line(addr, norm, stored, text.as_str(), w))
}

/// `val <addr> <normalized> <stored> "<text>"`, newline-ended.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.value.line"))]
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
