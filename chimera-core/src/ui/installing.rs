//! Bank 2's install (ADR 0137): DON'T POWER OFF over the whole screen,
//! what it is doing under it. The UI loop is held until it ends; the
//! audio plays on.

use chimera_hal::ChimeraDisplay;
use chimera_image::Phase;

use super::fmt::FmtBuf;
use super::{draw, theme};

/// Baselines: the warning's two lines, then `line`.
const WARN_Y: [i32; 2] = [130, 180];
const LINE_Y: i32 = 220;
/// The phase line's baseline, and the band it redraws.
const PHASE_Y: i32 = 250;
const PHASE_BAND: [u16; 2] = [236, 256];
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

/// The phase under the version line, redrawn and flushed alone.
pub fn phase<D: ChimeraDisplay>(d: &mut D, p: Phase) {
    show(d, phase_word(p), theme::INK);
}

/// A gate build's hold, `hold_line`'s.
#[cfg(feature = "bank2-gate")]
pub fn hold<D: ChimeraDisplay>(d: &mut D, p: Phase, secs: u8) {
    show(d, hold_line(p, secs).as_str(), theme::WARN);
}

/// `ERASING  PULL NOW 3`.
#[cfg(feature = "bank2-gate")]
pub fn hold_line(p: Phase, secs: u8) -> FmtBuf {
    let mut l = FmtBuf::new();
    let _ = write!(l, "{}  PULL NOW {secs}", phase_word(p));
    l
}

pub fn phase_word(p: Phase) -> &'static str {
    match p {
        Phase::Erasing => "ERASING",
        Phase::Writing => "WRITING",
        Phase::Sealing => "SEALING",
    }
}

fn show<D: ChimeraDisplay>(d: &mut D, s: &str, c: embedded_graphics::pixelcolor::Rgb565) {
    let [y0, y1] = PHASE_BAND;
    draw::fill_rect(
        d,
        0,
        y0.into(),
        theme::SCREEN_W,
        (y1 - y0).into(),
        theme::BG,
    );
    let cx = theme::SCREEN_W / 2;
    draw::text_center(
        d,
        &theme::FONT_VALUE,
        s,
        cx,
        PHASE_Y,
        c,
        theme::LABEL_TRACKING,
    );
    d.flush_region(y0, y1);
}
