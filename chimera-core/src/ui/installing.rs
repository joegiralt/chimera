//! Bank 2's install (ADR 0137): DON'T POWER OFF over the whole screen,
//! what it is doing under it. The UI loop is held until it ends; the
//! audio plays on.

use chimera_hal::ChimeraDisplay;

use super::fmt::FmtBuf;
use super::{draw, theme};

/// Baselines: the warning's two lines, then `line`.
const WARN_Y: [i32; 2] = [130, 180];
const LINE_Y: i32 = 220;
/// `line`'s widest, a margin each side.
pub const MAX_W: i32 = theme::SCREEN_W - 2 * 8;

/// The screen, drawn and flushed. Full words: no bank jargon.
pub fn draw<D: ChimeraDisplay>(d: &mut D, version: &str) {
    let cx = theme::SCREEN_W / 2;
    let _ = d.clear(theme::BG);
    for (s, y) in ["DON'T", "POWER OFF"].into_iter().zip(WARN_Y) {
        draw::text_center(d, &theme::FONT_FOCUS, s, cx, y, theme::WARN, 0);
    }
    draw::text_center(
        d,
        &theme::FONT_VALUE,
        line(version).as_str(),
        cx,
        LINE_Y,
        theme::INK,
        theme::LABEL_TRACKING,
    );
    d.flush();
}

/// `INSTALLING <version>` within `MAX_W`: the build hash dropped first,
/// then chars off the end.
pub fn line(version: &str) -> FmtBuf {
    let of = |v: &str| {
        let mut l = FmtBuf::new();
        let _ = write!(l, "INSTALLING");
        if !v.is_empty() {
            let _ = write!(l, " {v}");
        }
        l
    };
    let fits = |l: &FmtBuf| {
        draw::text_width(&theme::FONT_VALUE, l.as_str(), theme::LABEL_TRACKING) <= MAX_W
    };
    let mut v = version;
    let mut l = of(v);
    if !fits(&l) {
        v = version.split(' ').next().unwrap_or("");
        l = of(v);
    }
    while !fits(&l) && !v.is_empty() {
        let cut = v.char_indices().next_back().map_or(0, |(i, _)| i);
        v = &v[..cut];
        l = of(v);
    }
    l
}
