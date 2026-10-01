//! The card overlay: a centred box over whatever the screen shows. BUSY
//! is drawn and flushed before a card operation blocks the UI loop (boot);
//! a toast is drawn after one, over the frames its time lasts (leaving
//! System), and blocks nothing.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::types::VerticalPosition;

use crate::project::Line;
use crate::storage::{Exit, SyncError};
use crate::ui::{draw, theme};

const BOX_W: i32 = 120;
const BOX_H: i32 = 40;
/// Ground around the panel, so it reads over any page.
const EDGE: i32 = 4;
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

/// Space between the label and the panel's side, when the label is wider
/// than the box.
const PAD: i32 = 8;
/// The widest line the screen-wide box holds.
const MAX_INK: i32 = theme::SCREEN_W - 2 * (EDGE + PAD);
/// Baseline to baseline, when a message takes two lines.
const LINE_PITCH: i32 = 14;

/// Draws BUSY (reading the card at boot) and returns the band to flush,
/// rows `y0..y1`.
pub fn draw_busy<D: DrawTarget<Color = Rgb565>>(d: &mut D) -> (u16, u16) {
    draw_band(d, "BUSY")
}

/// Draws a toast's text in the overlay's box, widened to fit it, on two
/// lines if one won't fit the screen, and returns the band to flush.
pub fn draw_toast<D: DrawTarget<Color = Rgb565>>(d: &mut D, text: &str) -> (u16, u16) {
    draw_band(d, text)
}

fn ink(text: &str) -> i32 {
    draw::text_width(&theme::FONT_VALUE, text, theme::LABEL_TRACKING)
}

/// `text` as it fits: one line, or split at the space that makes the
/// wider of the two lines narrowest.
fn lines(text: &str) -> (&str, Option<&str>) {
    if ink(text) <= MAX_INK {
        return (text, None);
    }
    text.match_indices(' ')
        .map(|(i, _)| (&text[..i], &text[i + 1..]))
        .min_by_key(|(a, b)| ink(a).max(ink(b)))
        .map_or((text, None), |(a, b)| (a, Some(b)))
}

fn draw_band<D: DrawTarget<Color = Rgb565>>(d: &mut D, text: &str) -> (u16, u16) {
    let (first, second) = lines(text);
    let widest = ink(first).max(second.map_or(0, ink));
    let w = (widest + 2 * (EDGE + PAD)).clamp(BOX_W, theme::SCREEN_W);
    let x = (theme::SCREEN_W - w) / 2;
    draw::fill_rect(d, x, Y, w, BOX_H, theme::BG);
    draw::fill_rect(
        d,
        x + EDGE,
        Y + EDGE,
        w - 2 * EDGE,
        BOX_H - 2 * EDGE,
        theme::FAINT,
    );
    let half = if second.is_some() { LINE_PITCH / 2 } else { 0 };
    for (line, dy) in [(Some(first), -half), (second, half)] {
        if let Some(line) = line {
            draw::text_center(
                d,
                &theme::FONT_VALUE,
                line,
                theme::SCREEN_W / 2,
                baseline(line) + dy,
                theme::ACCENT,
                theme::LABEL_TRACKING,
            );
        }
    }
    (Y as u16, (Y + BOX_H) as u16)
}

/// A line shown for a while after a card operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Toast {
    pub text: Line,
    pub ms: u32,
}

impl Toast {
    pub const SAVED_MS: u32 = 600;
    pub const ERROR_MS: u32 = 1_200;
}

/// What leaving System shows: SAVED for a write that landed, the error's
/// message for a failure, nothing when it loaded (the theme changing is the
/// feedback) or had nothing to do.
pub fn toast_for(r: &Result<Exit, SyncError>) -> Option<Toast> {
    let (text, ms) = match *r {
        Ok(Exit::Wrote) => ("SAVED", Toast::SAVED_MS),
        Ok(Exit::Loaded | Exit::Unchanged) => return None,
        Err(SyncError::Store(e)) => (e.message(), Toast::ERROR_MS),
        Err(SyncError::File(e)) => (e.message(), Toast::ERROR_MS),
    };
    Some(Toast {
        text: Line::new(text),
        ms,
    })
}

/// What the shell does with the toast this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastStep {
    Idle,
    /// Draw it over the rendered frame and flush its band.
    Show(Line),
    /// It went this frame: repaint what it covered.
    Ended,
}

/// The toast on screen and its time left. Counted in milliseconds the shell
/// measures, not in frames: the UI loop has no fixed rate. Its time starts
/// when it is shown, so the step spanning the card work that made it, which
/// can take seconds, doesn't count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToastTimer {
    on: Option<Toast>,
    /// Shown since the last step: that step's time predates it.
    fresh: bool,
    ended: bool,
}

impl ToastTimer {
    pub const fn new() -> Self {
        Self {
            on: None,
            fresh: false,
            ended: false,
        }
    }

    pub fn show(&mut self, t: Toast) {
        self.on = Some(t);
        self.fresh = true;
        self.ended = false;
    }

    /// Takes it down early (new input).
    pub fn dismiss(&mut self) {
        self.ended |= self.on.take().is_some();
    }

    /// Once a frame: `elapsed_ms` since the last.
    pub fn step(&mut self, elapsed_ms: u32) -> ToastStep {
        let elapsed_ms = if core::mem::take(&mut self.fresh) {
            0
        } else {
            elapsed_ms
        };
        match &mut self.on {
            Some(t) if t.ms > elapsed_ms => {
                t.ms -= elapsed_ms;
                ToastStep::Show(t.text)
            }
            Some(_) => {
                self.on = None;
                ToastStep::Ended
            }
            None if core::mem::take(&mut self.ended) => ToastStep::Ended,
            None => ToastStep::Idle,
        }
    }
}
