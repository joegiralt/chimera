//! A gesture hint (ADR 0086): what a MIX or hold gesture does here, shown
//! where the touched cell, row or route is read, and only while it is.
//! One look everywhere: the keys in bold INK, then what they do in MID.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::ui::{draw, theme};

/// Keys and what they do: `MIX + NOTE`, `◂▸ STEP`. A browser hint's
/// words carry bytes, so they may be built at draw time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gesture<'a>(pub &'a str, pub &'a str);

/// The gestures one cell answers to, in a line.
pub type Hint = &'static [Gesture<'static>];

pub const NONE: Hint = &[];

/// The focus band's hint: under the value, over the viz band.
pub const FOCUS_Y: i32 = theme::FOCUS_BOTTOM - 3;
const KEY_GAP: i32 = 3;
const GAP: i32 = 8;

const T: i32 = theme::LABEL_TRACKING;

fn keys_w(g: &Gesture<'_>) -> i32 {
    draw::text_width(&theme::FONT_LABEL_BOLD, g.0, T) + KEY_GAP
}

/// `hint`'s width as `draw` sets it: the tests' fit check.
#[cfg(any(test, feature = "test-support"))]
pub fn width(hint: &[Gesture<'_>]) -> i32 {
    let w: i32 = hint
        .iter()
        .map(|g| keys_w(g) + draw::text_width(&theme::FONT_LABEL, g.1, T) + GAP)
        .sum();
    (w - GAP).max(0)
}

/// `hint` from `x`, its baseline at `y`. Out of line: three callers.
#[inline(never)]
pub fn draw<D>(d: &mut D, hint: &[Gesture<'_>], x: i32, y: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut x = x;
    for g in hint {
        x = gesture(d, g, x, y, theme::MID);
    }
}

/// One gesture from `x`, what it does in `does`: `MID`, or `ALERT` for
/// one that can't apply. Where the next one starts.
pub fn gesture<D>(d: &mut D, g: &Gesture<'_>, x: i32, y: i32, does: Rgb565) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::text_tracked(d, &theme::FONT_LABEL_BOLD, g.0, x, y, theme::INK, T);
    let x = x + keys_w(g);
    x + draw::text_tracked(d, &theme::FONT_LABEL, g.1, x, y, does, T) + GAP
}

/// `hint` in the focus band, at the margin.
pub fn focus<D>(d: &mut D, hint: Hint)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw(d, hint, theme::MARGIN_X, FOCUS_Y);
}
