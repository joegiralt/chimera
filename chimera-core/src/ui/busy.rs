//! The BUSY / SAVING overlay, drawn and flushed before a card operation
//! blocks the UI loop: a centred box over whatever the screen shows.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::types::VerticalPosition;

use crate::ui::{draw, theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusyLabel {
    /// Reading the card (boot).
    Busy,
    Saving,
}

impl BusyLabel {
    fn text(self) -> &'static str {
        match self {
            BusyLabel::Busy => "BUSY",
            BusyLabel::Saving => "SAVING",
        }
    }
}

const BOX_W: i32 = 120;
const BOX_H: i32 = 40;
/// Ground around the panel, so it reads over any page.
const EDGE: i32 = 4;
const X: i32 = (theme::SCREEN_W - BOX_W) / 2;
const Y: i32 = (theme::SCREEN_H - BOX_H) / 2;
/// The baseline that centres `text`'s ink in the box, from the font's own
/// glyph bounds.
fn baseline(text: &str) -> i32 {
    let ink = theme::FONT_VALUE
        .get_rendered_dimensions(text, Point::zero(), VerticalPosition::Baseline)
        .ok()
        .and_then(|d| d.bounding_box)
        .map_or(0, |b| b.top_left.y + b.size.height as i32 / 2);
    Y + BOX_H / 2 - ink
}

/// Draws the overlay and returns the band to flush, rows `y0..y1`.
pub fn draw_busy<D: DrawTarget<Color = Rgb565>>(d: &mut D, label: BusyLabel) -> (u16, u16) {
    draw::fill_rect(d, X, Y, BOX_W, BOX_H, theme::BG);
    draw::fill_rect(
        d,
        X + EDGE,
        Y + EDGE,
        BOX_W - 2 * EDGE,
        BOX_H - 2 * EDGE,
        theme::FAINT,
    );
    draw::text_center(
        d,
        &theme::FONT_VALUE,
        label.text(),
        theme::SCREEN_W / 2,
        baseline(label.text()),
        theme::ACCENT,
        theme::LABEL_TRACKING,
    );
    (Y as u16, (Y + BOX_H) as u16)
}
