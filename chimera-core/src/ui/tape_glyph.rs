//! TAPE's machine glyphs in the focus band (#641, ADR 0173): the engine's
//! raster (`chimera_engine::tape_glyph`, bank 2's with `bank2`) painted
//! in the theme's tokens into the gauge box.

use crate::ui::theme;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

pub use chimera_engine::tape_glyph::{
    Bellows, Blind, Bounce, Canvas, Film, GlyphInputs, H, Ink, Kind, Loop, Machine, Pose, Q, Q3,
    Q4, Q5, Q6, Run, Sponge, TapeGauge, W, Wear,
};

const _: () = assert!(W == theme::WAVE_W && H == theme::WAVE_H);

/// The colour of each `Ink`: the theme's tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette(pub [Rgb565; Ink::ALL.len()]);

impl Palette {
    /// By `Ink`: BG, INK, INK2, MID, FAINT, ACCENT, ACCENT_SOFT.
    pub const THEME: Palette = Palette([
        theme::BG,
        theme::INK,
        theme::INK2,
        theme::MID,
        theme::FAINT,
        theme::ACCENT,
        theme::ACCENT_SOFT,
    ]);

    pub const fn of(&self, ink: Ink) -> Rgb565 {
        self.0[ink as usize]
    }
}

/// `g` in the focus band's gauge box.
pub fn draw<D>(d: &mut D, g: &TapeGauge, pal: &Palette)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw_at(d, g, pal, theme::WAVE_X, theme::WAVE_Y);
}

/// `g` with its box's top left at (`x`, `y`): every pixel of the box, so
/// a redraw needs no clear. Nothing with bank 2 closed.
pub fn draw_at<D>(d: &mut D, g: &TapeGauge, pal: &Palette, x: i32, y: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut c = Canvas::new();
    if chimera_engine::tape_glyph::paint(g, &mut c) {
        let area = Rectangle::new(Point::new(x, y), Size::new(W as u32, H as u32));
        let _ = d.fill_contiguous(&area, c.0.iter().map(|&i| pal.of(i)));
    }
}
