//! Bank 2's install (ADR 0137): DON'T POWER OFF over the whole screen,
//! what it is doing under it. The UI loop is held until it ends; the
//! audio plays on.

use chimera_hal::ChimeraDisplay;

use super::{draw, theme};

/// Baselines: the warning's two lines, then `line`.
const WARN_Y: [i32; 2] = [130, 180];
const LINE_Y: i32 = 220;

/// The screen, drawn and flushed. Full words: no bank jargon.
pub fn draw<D: ChimeraDisplay>(d: &mut D, line: &str) {
    let cx = theme::SCREEN_W / 2;
    let _ = d.clear(theme::BG);
    for (s, y) in ["DON'T", "POWER OFF"].into_iter().zip(WARN_Y) {
        draw::text_center(d, &theme::FONT_FOCUS, s, cx, y, theme::WARN, 0);
    }
    draw::text_center(
        d,
        &theme::FONT_VALUE,
        line,
        cx,
        LINE_Y,
        theme::INK,
        theme::LABEL_TRACKING,
    );
    d.flush();
}
