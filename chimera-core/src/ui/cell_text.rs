//! A cell's text, as the box draws it: the renderer's and `ed get`'s one
//! source. What a page's cell reads beyond its value is `CellCx`.

use crate::addr::BlockRef;
use crate::preset::Part;
use crate::preset::Sound;
use crate::ui::block_def::BlockDef;
use crate::ui::filter_panel::{RouteView, cell_route};
use crate::ui::fmt::{self, FmtBuf};
use crate::ui::view::{self, SlotCtx, View};

/// What `value_text` reads: the page, what its slots resolve against, and
/// the live state a few cells (a held choice, SCALA's marquee, the reverb's
/// clock, a closed door) put in their text.
#[derive(Clone, Copy)]
pub struct CellCx<'a> {
    pub def: &'static BlockDef,
    pub ctx: &'a SlotCtx,
    pub held: &'a Option<crate::ui::held::Shown>,
    /// The Part being edited.
    pub part: &'a Part,
    pub focus: usize,
    pub clock: &'a crate::ui::animation::UiClock,
    pub bank2: &'a Option<crate::bank2::Bank2Status>,
    pub fx: &'a crate::dsp::fx_bus::FxParams,
    pub rev_clock: &'a (
        crate::dsp::rev::sync::Tempo,
        crate::dsp::rev::sync::FoldState,
    ),
}

/// What a `CellCx` borrows that isn't already in a `Frame` or the project:
/// `ed get` holds these, as a frame holds its own.
pub struct CellOwn {
    pub ctx: SlotCtx,
    pub clock: crate::ui::animation::UiClock,
    pub bank2: Option<crate::bank2::Bank2Status>,
    pub rev_clock: (
        crate::dsp::rev::sync::Tempo,
        crate::dsp::rev::sync::FoldState,
    ),
}

/// The route's cell has no route on `sound`: it reads `--`, and dims.
#[inline(never)]
pub(crate) fn route_absent(sound: &Sound, route: RouteView) -> bool {
    let (row, c) = route.cell(sound);
    cell_route(sound, row, c).is_none()
}

/// Slot `i`'s value as a cell, the focus band and a viz readout show it:
/// `--` for an absent route, a fixed readout's text.
#[inline(always)]
pub(crate) fn value_text(cx: &CellCx, i: usize, v: f32) -> FmtBuf {
    let mut buf = FmtBuf::new();
    match view::view(cx.def, i, cx.ctx) {
        View::Text { text, .. } => {
            let _ = core::fmt::Write::write_str(&mut buf, text);
        }
        View::Source(s) => s.text(&mut buf),
        View::Route { route, .. } if route_absent(&cx.part.sound, route) => {
            let _ = core::fmt::Write::write_str(&mut buf, "--");
        }
        view => buf = param_text(cx, i, &view, v),
    }
    buf
}

/// The refused choice `view` shows in place of its value, if it is the cell.
pub(crate) fn held_on(
    held: Option<crate::ui::held::Shown>,
    view: &View,
) -> Option<crate::ui::held::Held> {
    held.map(|s| s.held).filter(|h| view.addr() == Some(h.addr))
}

/// A param's text at display value `v`: a reverb panel cell's in its
/// units (`reverb_panel::value_text`), any other by its format.
pub(crate) fn param_text(cx: &CellCx, i: usize, view: &View, v: f32) -> FmtBuf {
    if let Some(h) = held_on(*cx.held, view) {
        let mut buf = FmtBuf::new();
        let fmt = view.fmt();
        fmt::fmt_val(
            &mut buf,
            f32::from(h.code()) / f32::from(fmt.max_int().max(1)),
            fmt,
        );
        return buf;
    }
    if let (Some(a), view::SourceState::Held { rate, frames, .. }) = (view.addr(), cx.ctx.src)
        && a.block == BlockRef::TapeEngine
        && matches!(
            a.param,
            crate::dsp::tape_engine::TapeEngineParams::START
                | crate::dsp::tape_engine::TapeEngineParams::END
        )
    {
        let mut buf = FmtBuf::new();
        crate::ui::onset::time_text(&mut buf, v, rate.get(), frames);
        return buf;
    }
    // SCALA reads the Part's file by the name it shows (ADR 0259): its
    // first 7 characters (8 wide ones run into the next cell), a longer
    // one scrolling while the cell has focus.
    let part = cx.part;
    if view.addr() == Some(view::TUNING)
        && part.mix.tuning == crate::part::TUNING_SCALA
        && let Some(n) = part.scala()
    {
        use crate::ui::tuning_walk::{SHOWN, marquee};
        const CELL: usize = 7;
        let mut name = [0; crate::scala_name::LONG_MAX];
        let len = SHOWN.name(n, &mut name);
        let shown: &[u8] = if len > 0 { &name[..len] } else { n.as_bytes() };
        let at = match i == cx.focus {
            true => marquee(shown.len(), CELL, cx.clock.frame()),
            false => 0,
        };
        let w = &shown[at..shown.len().min(at + CELL)];
        let mut buf = FmtBuf::new();
        let _ = core::fmt::Write::write_str(&mut buf, core::str::from_utf8(w).unwrap_or_default());
        return buf;
    }
    if let Some(t) = view
        .addr()
        .and_then(|a| crate::bank2::closed_text(a, v, *cx.bank2))
    {
        let mut buf = FmtBuf::new();
        let _ = core::fmt::Write::write_str(&mut buf, t);
        return buf;
    }
    match view.addr().filter(|_| view::is_reverb_panel(cx.def, i)) {
        Some(a) => {
            let mut p = cx.fx.reverb;
            crate::block::Block::set(&mut p, a.param, v);
            let (tempo, fold) = *cx.rev_clock;
            crate::ui::reverb_panel::value_text(a.param, &p, tempo, fold)
        }
        None => fmt_text(view.addr(), view.fmt(), v),
    }
}

/// `v`, a 0..1 display value, in `fmt`: the box's way for any cell, and
/// `ed get`'s for a param no cell shows now.
#[inline(always)]
pub(crate) fn fmt_text(
    addr: Option<crate::addr::ParamAddr>,
    fmt: crate::ui::page::ValFmt,
    v: f32,
) -> FmtBuf {
    let mut buf = FmtBuf::new();
    // These two read the stored value (`ValFmt::LogHz`).
    let v = match (fmt, addr.and_then(|a| a.spec())) {
        (crate::ui::page::ValFmt::LogHz { .. } | crate::ui::page::ValFmt::Vowel, Some(s)) => {
            s.denormalize(v)
        }
        _ => v,
    };
    fmt::fmt_val(&mut buf, v, fmt);
    buf
}
