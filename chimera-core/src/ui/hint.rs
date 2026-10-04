//! A gesture hint (ADR 0086): what a MIX or hold gesture does here, shown
//! where the touched cell, row or route is read, and only while it is.
//! One look everywhere: the keys in bold INK, then what they do in MID.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::ui::{draw, theme};

/// Keys and what they do: `MIX + NOTE`, `◂▸ STEP`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gesture(pub &'static str, pub &'static str);

/// The gestures one cell answers to, in a line.
pub type Hint = &'static [Gesture];

pub const NONE: Hint = &[];

/// The focus band's hint: under the value, over the viz band.
pub const FOCUS_Y: i32 = theme::FOCUS_BOTTOM - 3;
/// Widest a hint may be: the screen less both margins.
pub const MAX_W: i32 = theme::SCREEN_W - 2 * theme::MARGIN_X;
const KEY_GAP: i32 = 3;
const GAP: i32 = 8;

const T: i32 = theme::LABEL_TRACKING;

fn keys_w(g: &Gesture) -> i32 {
    draw::text_width(&theme::FONT_LABEL_BOLD, g.0, T) + KEY_GAP
}

/// `hint`'s width as `draw` sets it.
pub fn width(hint: Hint) -> i32 {
    let w: i32 = hint
        .iter()
        .map(|g| keys_w(g) + draw::text_width(&theme::FONT_LABEL, g.1, T) + GAP)
        .sum();
    (w - GAP).max(0)
}

/// `hint` from `x`, its baseline at `y`. Out of line: three callers.
#[inline(never)]
pub fn draw<D>(d: &mut D, hint: Hint, x: i32, y: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut x = x;
    for g in hint {
        draw::text_tracked(d, &theme::FONT_LABEL_BOLD, g.0, x, y, theme::INK, T);
        x += keys_w(g);
        x += draw::text_tracked(d, &theme::FONT_LABEL, g.1, x, y, theme::MID, T) + GAP;
    }
}

/// `hint` in the focus band, at the margin.
pub fn focus<D>(d: &mut D, hint: Hint)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw(d, hint, theme::MARGIN_X, FOCUS_Y);
}
