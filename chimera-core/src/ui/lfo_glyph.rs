// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The LFO page's pictures (#649, ADR 0177): each control's glyph in the
//! focus band's gauge box, and the scope's one cycle of the LFO with the
//! newest voice's playhead on it. The engine draws them
//! (`chimera_engine::lfo_glyph`, bank 2's with `bank2`); this paints them
//! in the theme's tokens.

use crate::addr::{BlockRef, ParamAddr};
use crate::block::ParamId;
use crate::dsp::lfo::{LfoParams as L, LfoShape};
use crate::dsp::modulator::{LfoForm, LfoSlot};
use crate::ui::draw;
use crate::ui::theme;
use crate::ui::view::LfoKind;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

pub use chimera_engine::lfo_glyph::{
    COLS, Canvas, Glyph, H, Ink, LfoGauge, Q, SCOPE_AMP, W, Wave, curve, paint,
};

const _: () = assert!(W == theme::WAVE_W && H == theme::WAVE_H);
const _: () = assert!(COLS as i32 == theme::VIZ_RIGHT - theme::VIZ_LEFT + 1);
const _: () = assert!(SCOPE_AMP == theme::VIZ_BAND_AMP);

/// The colour of each `Ink`: the theme's tokens.
pub const fn ink(i: Ink) -> Rgb565 {
    match i {
        Ink::Bg => theme::BG,
        Ink::Ink => theme::INK,
        Ink::Mid => theme::MID,
        Ink::Faint => theme::FAINT,
        Ink::Accent => theme::ACCENT,
        Ink::Soft => theme::ACCENT_SOFT,
    }
}

/// What a `Wave` reads of slot `s`, in `wave`'s order: CLASSIC's RATE,
/// SHAPE, SYNC, PHASE and DEPTH, then FUNC's RISE, FALL and SHAPE.
pub const fn params(s: LfoSlot) -> [ParamAddr; 8] {
    let ids = [
        L::RATE,
        L::SHAPE,
        L::SYNC,
        L::PHASE,
        L::DEPTH,
        L::RISE,
        L::FALL,
        L::SHAPE_B,
    ];
    let mut out = [ParamAddr::new(BlockRef::Lfo(s), L::RATE); 8];
    let mut i = 0;
    while i < ids.len() {
        out[i] = ParamAddr::new(BlockRef::Lfo(s), ids[i]);
        i += 1;
    }
    out
}

/// The slot's wave under `kind`, from `params`' set values (normalized).
pub fn wave(kind: LfoKind, set: [f32; 8]) -> Wave {
    match kind {
        LfoKind::Classic => Wave::Classic {
            rate: Q::of(set[0]),
            shape: LfoShape::from_u8(libm::roundf(crate::dsp::clampf(set[1], 0.0, 1.0) * 4.0) as u8),
            retrig: set[2] >= 0.5,
            phase: Q::of(set[3]),
            depth: Q::of(set[4]),
        },
        LfoKind::Func(form) => Wave::Func {
            rate: Q::of(set[5]),
            form,
            fall: Q::of(set[6]),
            tilt: Q::of(set[7]),
        },
    }
}

/// The glyph `param`'s cell shows under `kind`; none for TYPE (and OFST,
/// off the page).
pub fn glyph_for(kind: LfoKind, param: ParamId) -> Option<Glyph> {
    let lfv = kind == LfoKind::Func(LfoForm::Lfv);
    Some(match (kind, param) {
        (LfoKind::Classic, L::RATE) | (LfoKind::Func(_), L::RISE) => Glyph::Rate,
        (LfoKind::Classic, L::SHAPE) => Glyph::Shape,
        (LfoKind::Classic, L::SYNC) | (LfoKind::Func(_), L::FORM) => Glyph::Lane,
        (LfoKind::Classic, L::PHASE) => Glyph::Phase,
        (LfoKind::Classic, L::DEPTH) => Glyph::Depth,
        (LfoKind::Func(_), L::FALL) if lfv => Glyph::Delta,
        (LfoKind::Func(_), L::FALL) => Glyph::Phase,
        (LfoKind::Func(_), L::SHAPE_B) if lfv => Glyph::Slew,
        (LfoKind::Func(_), L::SHAPE_B) => Glyph::Tilt,
        _ => return None,
    })
}

/// `g` in the focus band's gauge box: every pixel of it, so a redraw
/// needs no clear; RATE's `1 S` over its ruler. Nothing with bank 2 closed.
pub fn draw<D>(d: &mut D, g: &LfoGauge)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (x, y) = (theme::WAVE_X, theme::WAVE_Y);
    let mut c = Canvas::new();
    if !paint(g, &mut c) {
        return;
    }
    let area = Rectangle::new(Point::new(x, y), Size::new(W as u32, H as u32));
    let _ = d.fill_contiguous(&area, c.0.iter().map(|&i| ink(i)));
    if g.ruler() {
        draw::text_center(
            d,
            &theme::FONT_LABEL,
            "1 S",
            x + 5 + 26,
            y + 47,
            theme::MID,
            0,
        );
    }
}

// ── The scope ────────────────────────────────────────────────────────

/// The newest voice's place in its LFO: its phase and how many cycles it
/// has wrapped (counted by the UI, frame to frame).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Head {
    pub phase: f32,
    pub wraps: u8,
}

impl Head {
    /// The next frame's: a phase that fell by more than half a cycle
    /// wrapped.
    pub fn next(prev: Option<Head>, phase: f32) -> Head {
        let wraps = prev.map_or(0, |h| h.wraps.wrapping_add((phase < h.phase - 0.5) as u8));
        Head { phase, wraps }
    }
}

/// The playhead on the scope: parked at the start with no voice, else on
/// a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dot {
    Parked,
    At(u8),
}

impl Dot {
    /// Where `head` sits on `w`'s picture.
    pub fn of(w: &Wave, head: Option<Head>) -> Dot {
        match head {
            None => Dot::Parked,
            Some(h) => {
                let x = libm::roundf(w.head(h.phase, h.wraps) * (COLS - 1) as f32);
                Dot::At((x as usize).min(COLS - 1) as u8)
            }
        }
    }

    /// For the viz's key: 0 parked, else the column + 1.
    pub fn key(self) -> u16 {
        match self {
            Dot::Parked => 0,
            Dot::At(x) => x as u16 + 1,
        }
    }

    /// `key`'s dot.
    pub fn from_key(k: u16) -> Dot {
        match k {
            0 => Dot::Parked,
            k => Dot::At((k - 1).min(COLS as u16 - 1) as u8),
        }
    }

    const fn col(self) -> usize {
        match self {
            Dot::Parked => 0,
            Dot::At(x) => x as usize,
        }
    }

    /// The box the dot draws in on `w`'s curve `ys`: 9 × 9 about it.
    pub fn rect(self, ys: &[i8; COLS]) -> (i32, i32, i32, i32) {
        let c = self.col();
        let y = theme::VIZ_BAND_MID - ys[c] as i32;
        (theme::VIZ_LEFT + c as i32 - R, y - R, 2 * R + 1, 2 * R + 1)
    }
}

/// The playhead's radius with its ground ring.
const R: i32 = 4;

/// `w`'s curve, flat with bank 2 closed (and `false`).
pub fn columns(w: &Wave) -> ([i8; COLS], bool) {
    let mut ys = [0i8; COLS];
    let drawn = curve(w, &mut ys);
    (ys, drawn)
}

/// The scope: `w`'s curve over the viz band, filled to the middle, and
/// `dot` on it. With bank 2 closed, a faint middle line alone.
pub fn draw_scope<D>(d: &mut D, w: &Wave, dot: Dot)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw_scope_cols(d, w, dot, 0..COLS);
}

/// `draw_scope` in columns `cols` alone (the line's segments into them
/// whole): over a cleared box of those columns it leaves what a full
/// draw would.
pub fn draw_scope_cols<D>(d: &mut D, w: &Wave, dot: Dot, cols: core::ops::Range<usize>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (ys, drawn) = columns(w);
    let mid = theme::VIZ_BAND_MID;
    if !drawn {
        draw::fill_rect(d, theme::VIZ_LEFT, mid, COLS as i32, 1, theme::FAINT);
        return;
    }
    trace(d, &ys, cols);
    let c = dot.col();
    let (x, y) = (theme::VIZ_LEFT + c as i32, mid - ys[c] as i32);
    match dot {
        Dot::At(_) => {
            disc(d, x, y, R, false, theme::BG);
            disc(d, x, y, R - 1, false, theme::INK);
        }
        Dot::Parked => {
            disc(d, x, y, R - 1, false, theme::BG);
            disc(d, x, y, R - 1, true, theme::MID);
        }
    }
}

/// A band trace of `ys` (offsets up from the middle) in columns `cols`:
/// the soft fill to the middle, then the accent line two rows thick
/// (`viz::live_output`'s), every segment that reaches the columns.
pub fn trace<D>(d: &mut D, ys: &[i8; COLS], cols: core::ops::Range<usize>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mid = theme::VIZ_BAND_MID;
    let y = |i: usize| mid - ys[i] as i32;
    let (c0, c1) = (cols.start.min(COLS), cols.end.min(COLS));
    for i in c0..c1 {
        let x = theme::VIZ_LEFT + i as i32;
        let (a, b) = if y(i) < mid {
            (y(i) + 1, mid)
        } else {
            (mid, y(i))
        };
        draw::fill_rect(d, x, a, 1, b - a, theme::ACCENT_SOFT);
    }
    for i in c0.max(1)..(c1 + 1).min(COLS) {
        let x = theme::VIZ_LEFT + i as i32;
        draw::thick_line(d, x - 1, y(i - 1), x, y(i), theme::ACCENT);
    }
}

/// The pixels within `r² + r` of the centre, a row at a time; `ring`
/// leaves out those within `(r−1)² + (r−1)`.
fn disc<D>(d: &mut D, cx: i32, cy: i32, r: i32, ring: bool, c: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    // The widest |dx| with dx² ≤ n, or −1 for none.
    let half = |n: i32| {
        let mut w = -1;
        while (w + 1) * (w + 1) <= n {
            w += 1;
        }
        w
    };
    for dy in -r..=r {
        let out = half(r * r + r - dy * dy);
        let inner = if ring {
            half((r - 1) * (r - 1) + r - 1 - dy * dy)
        } else {
            -1
        };
        if inner < 0 {
            draw::fill_rect(d, cx - out, cy + dy, 2 * out + 1, 1, c);
        } else {
            draw::fill_rect(d, cx - out, cy + dy, out - inner, 1, c);
            draw::fill_rect(d, cx + inner + 1, cy + dy, out - inner, 1, c);
        }
    }
}
