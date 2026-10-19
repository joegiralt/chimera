//! TAPE's strip (spec § 3): how SRC and DECK show what the machine does to
//! the source. A 216 × 40 px view of it, min/max a column in `MID`, START
//! and END marked; over it the machine's part in `ACCENT_DIM`, and its
//! heads in `ACCENT`, as the audio stores them once a block
//! (`instrument::HEADS`). The head's arrow points from START to END.
//!
//! The raster (`raster`: the overview's columns, the key and the strip's
//! rectangles) is bank 2's with `bank2`, through the UI's table
//! (`bank2::UiDoor`, ADR 0175), onto `Paint`, a display behind a `dyn`;
//! closed, the strip and its overview stay empty. Bank 1 keeps the
//! overview's cache and these wrappers.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::dsp::tape_engine::Heads;
use crate::dsp::tape_engine::params::{TapeEngineParams, TapeModel};
use crate::dsp::tape_engine::read::SourceView;
use crate::ui::{draw, theme};

pub(crate) mod raster;

/// The strip's columns: one a pixel.
pub const COLS: usize = 216;
/// Its height, px.
pub const H: i32 = 40;
/// Its top on SRC and DECK: the viz band's middle.
pub const Y: i32 = theme::VIZ_BAND_MID - H / 2;

/// A source's min and max a column, its top 8 bits: the UI's one cache,
/// for the page shown, worked out when the page opens or SOURCE changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overview {
    cols: [[i8; 2]; COLS],
    frames: u32,
    rate: u16,
    /// A transwave's frames; 0 for any other shape.
    count: u8,
}

/// The most frames one column reads.
pub const COL_READS: u32 = 32;

impl Overview {
    /// No source: a flat line.
    pub const EMPTY: Overview = Overview {
        cols: [[0; 2]; COLS],
        frames: 0,
        rate: 0,
        count: 0,
    };

    /// `[START, END]` of `v` in `COLS` spans (`raster`'s `of_in`); closed,
    /// `EMPTY`.
    pub fn of(v: &SourceView<'_>, start: f32, end: f32) -> Overview {
        crate::bank2::ui_door(|u| (u.overview)(v, start, end)).unwrap_or(Overview::EMPTY)
    }

    pub fn col(&self, c: usize) -> [i8; 2] {
        self.cols[c]
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn rate(&self) -> u16 {
        self.rate
    }

    /// Frame `f`'s column, from 0.
    fn column(&self, f: u32) -> i32 {
        (u64::from(f.min(self.frames - 1)) * COLS as u64 / u64::from(self.frames)) as i32
    }
}

/// What the viz region redraws on: the overview, the machine's params and
/// the heads to the column (`raster`'s `key_in`); closed, 0.
pub fn key(o: &Overview, model: TapeModel, p: &TapeEngineParams, heads: Heads) -> u32 {
    crate::bank2::ui_door(|u| (u.strip_key)(o, model, p, heads)).unwrap_or(0)
}

/// What the strip draws with: a display behind a `dyn`, so its raster is
/// one function the door can name.
pub trait Paint {
    /// `draw::fill_rect`.
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb565);
    /// `draw::line`, 1 px.
    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb565);
}

/// A display as `Paint`.
struct On<'a, D>(&'a mut D);

impl<D: DrawTarget<Color = Rgb565>> Paint for On<'_, D> {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb565) {
        draw::fill_rect(self.0, x, y, w, h, color);
    }

    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb565) {
        draw::line(self.0, x0, y0, x1, y1, color, 1);
    }
}

/// The strip at `(x, y)`, 216 × 40 px (ticks hang 3 px below): `model`
/// over `o` at `p` (the page's values as they glide), `heads` the audio's.
/// Closed, nothing: the header says UPDATE.
pub fn draw_strip<D>(
    d: &mut D,
    o: &Overview,
    model: TapeModel,
    p: &TapeEngineParams,
    heads: Heads,
    x: i32,
    y: i32,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let mut on = On(d);
    crate::bank2::ui_door(|u| (u.strip)(&mut on, o, model, p, heads, (x, y)));
}
