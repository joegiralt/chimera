//! PIT's tuner worked out and painted (ADR 0201): bank 2's with `bank2`
//! (`bank2.x` globs this module, and the font raster's instances over
//! `Runs`, whose names carry it), reached through the UI's door
//! (`UiDoor::pit`, `pit_paint`). Its words are `WORDS`, a named static,
//! and its numbers its own digits, so it adds no unnamed rodata to bank 1.
//!
//! The pitch is the voice's own sum, as `Slot::set_pitch` and each engine
//! take it: the note after the scale's snap, PITCH and FINE, TUNING, and
//! the engine's own transposition (ALGO's TRNSP, VA's OSC 1 CRSE and
//! FINE). TAPE sounds a source with a root at the note (the root key plays
//! it at its own speed), TRANSWAVE always; a source with no period has no
//! known root, so the corner reads `ROOT ?` and the shift off the source's
//! own speed. SCRUB runs at `2 · |MOTION|` of it, still at 0. What moves
//! the pitch for a moment (glide, bend, routes, wow, a SCRUB finger) is
//! left out: the corner reads where the pitch rests.

use core::fmt::Write;

use embedded_graphics::Pixel;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{OriginDimensions, Point, Size};
use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::types::{FontColor, VerticalPosition};

use super::{Line, Shown};
use crate::dsp::tape_engine::params::{TapeMachine, at_root};
use crate::params::EngineType;
use crate::preset::Sound;
use crate::ui::UiState;
use crate::ui::block_def::VizType;
use crate::ui::fmt::FmtBuf;
use crate::ui::saver::{nearest, note_name};
use crate::ui::tape_strip::Paint;
use crate::ui::theme::{self, Face, TextSize};

/// The note the corner reads before the Part has played one: middle C.
pub const REF_NOTE: u8 = 60;
/// The corner's width: LARGE falls back to NORMAL past it.
pub const CORNER_W: i32 = 112;
/// Below this |MOTION| SCRUB's head stands.
const STILL_MOTION: f32 = 1e-3;

/// Every word the corner writes, cut by `Word`.
static WORDS: [u8; 22] = *b"REF  HzROOT ?STILLstct";

#[derive(Clone, Copy)]
enum Word {
    Ref,
    Space,
    Hz,
    Root,
    Still,
    St,
    Ct,
}

fn word(b: &mut FmtBuf, w: Word) {
    let (at, len) = match w {
        Word::Ref => (0, 4),
        Word::Space => (3, 1),
        Word::Hz => (5, 2),
        Word::Root => (7, 6),
        Word::Still => (13, 5),
        Word::St => (18, 2),
        Word::Ct => (20, 2),
    };
    if let Some(s) = WORDS.get(at..at + len) {
        // SAFETY: `WORDS` is ASCII, so every slice of it is UTF-8.
        let _ = b.write_str(unsafe { core::str::from_utf8_unchecked(s) });
    }
}

/// `v` in decimal, signed (`+0` for 0) where `plus`.
fn int(b: &mut FmtBuf, v: i32, plus: bool) {
    let mut d = [0u8; 11];
    let mut n = v.unsigned_abs();
    let mut i = d.len();
    loop {
        i -= 1;
        d[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 || i == 1 {
            break;
        }
    }
    if v < 0 || plus {
        i -= 1;
        d[i] = if v < 0 { b'-' } else { b'+' };
    }
    if let Some(s) = d.get(i..) {
        // SAFETY: digits and a sign, ASCII.
        let _ = b.write_str(unsafe { core::str::from_utf8_unchecked(s) });
    }
}

/// "+12ct", "-3ct", "+0ct".
fn cents(b: &mut FmtBuf, c: i32) {
    int(b, c, true);
    word(b, Word::Ct);
}

/// Where a Part's pitch rests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pitch {
    /// Cents above MIDI note 0: A4 (6 900) is 440 Hz.
    At(f32),
    /// A source with no known root: cents off its own speed.
    Shift(f32),
    /// SCRUB at MOTION 0.
    Still,
}

/// `s` playing `note` under TUNING `tune` cents; `None` for TAPE with no
/// source, which plays nothing.
pub fn pitch(note: u8, s: &Sound, tune: i8) -> Option<Pitch> {
    let p = &s.params;
    let key = f32::from(note) * 100.0;
    let set = p.pitch.semitones() * 100.0 + f32::from(tune);
    let at = |extra: f32| Some(Pitch::At(key + set + extra));
    match p.engine() {
        EngineType::Algo => at(f32::from(p.algo.transpose) * 100.0),
        EngineType::Modal => at(0.0),
        EngineType::Va => at(f32::from(p.va.coarse[0].clamp(-24, 24)) * 100.0 + p.va.fine[0]),
        EngineType::Tape => {
            s.source?;
            let t = &p.tape;
            if t.machine == TapeMachine::Wave {
                return at(0.0);
            }
            let speed = if t.machine == TapeMachine::Scrub {
                let m = libm::fabsf(t.motion);
                if m < STILL_MOTION {
                    return Some(Pitch::Still);
                }
                1_200.0 * libm::log2f(2.0 * m)
            } else {
                0.0
            };
            // SLICE NOTE plays every key at ROOT.
            let key = if at_root(t) { t.root } else { key };
            let c = key + set + speed;
            Some(if t.period > 0.0 {
                Pitch::At(c)
            } else {
                Pitch::Shift(c - t.root)
            })
        }
    }
}

/// `cents` above MIDI note 0 in Hz, A4 = 440.
pub fn hz(cents: f32) -> f32 {
    440.0 * libm::exp2f((cents - 6_900.0) / 1_200.0)
}

/// The corner's two lines: the note and cents, then Hz, to 1 cent and
/// 0.1 Hz. `reference`: no note played yet, so the pitch is `REF_NOTE`'s.
pub fn lines(p: Pitch, reference: bool) -> (FmtBuf, FmtBuf) {
    let (mut a, mut b) = (FmtBuf::new(), FmtBuf::new());
    if reference {
        word(&mut b, Word::Ref);
        note_name(&mut b, i32::from(REF_NOTE));
        word(&mut b, Word::Space);
    }
    match p {
        Pitch::At(c) => {
            let (n, off) = nearest(libm::roundf(c) as i32);
            note_name(&mut a, n);
            word(&mut a, Word::Space);
            cents(&mut a, off);
            let tenths = libm::roundf(hz(c) * 10.0) as i32;
            int(&mut b, tenths / 10, false);
            let _ = b.write_char('.');
            int(&mut b, tenths % 10, false);
            word(&mut b, Word::Space);
            word(&mut b, Word::Hz);
        }
        Pitch::Shift(c) => {
            word(&mut a, Word::Root);
            let (st, off) = nearest(libm::roundf(c) as i32);
            int(&mut b, st, true);
            word(&mut b, Word::St);
            word(&mut b, Word::Space);
            cents(&mut b, off);
        }
        Pitch::Still => word(&mut a, Word::Still),
    }
    (a, b)
}

/// Each line's baseline at `size`.
fn baselines(size: TextSize) -> (i32, i32) {
    let top = theme::VIZ_BAND_TOP;
    match size {
        TextSize::Normal => (top + 14, top + 27),
        TextSize::Large => (top + 16, top + 29),
    }
}

/// `a` and `b` set right on the band's edge at `size`, or NORMAL where
/// LARGE overruns the corner; lit or dim.
pub fn shown(a: FmtBuf, b: FmtBuf, size: TextSize, lit: bool) -> Shown {
    let fonts = |s| (theme::font(Face::Value, s), theme::font(Face::Label, s));
    let width = |s| {
        let (fa, fb) = fonts(s);
        crate::ui::draw::text_width(fa, a.as_str(), 0)
            .max(crate::ui::draw::text_width(fb, b.as_str(), 0))
    };
    let size = theme::fit(size, CORNER_W, width);
    let (fa, fb) = fonts(size);
    let (ya, yb) = baselines(size);
    let right = theme::VIZ_RIGHT + 1;
    let (ca, cb) = if lit {
        (theme::INK, theme::INK2)
    } else {
        (theme::MID, theme::MID)
    };
    let line = |text: FmtBuf, font, y, color| {
        let w = crate::ui::draw::text_width(font, text.as_str(), 0);
        (w > 0).then_some(Line {
            text,
            font,
            x: right - w,
            y,
            color,
        })
    };
    let lines = [line(a, fa, ya, ca), line(b, fb, yb, cb)];
    let left = match &lines {
        [Some(a), Some(b)] => a.x.min(b.x),
        [Some(l), None] | [None, Some(l)] => l.x,
        [None, None] => right,
    };
    let top = theme::VIZ_BAND_TOP;
    let back = (left - 4, top, right - (left - 4), yb + 4 - top);
    let text = |i: usize| {
        lines
            .get(i)
            .and_then(Option::as_ref)
            .map_or(&[][..], |l| l.text.as_str().as_bytes())
    };
    let key =
        crate::ui::region::settings_key(&[text(0), text(1), &[u8::from(lit), size as u8]]);
    Shown { back, lines, key }
}

/// The door's entry, once a frame: `played`'s last note taken for its
/// Part, then, on PIT, the edited Part's corner.
pub fn pass_in(ui: &mut UiState) {
    let on = matches!(ui.page_def().viz, VizType::Tuner);
    let t = &mut ui.tuner;
    let heard = t.heard();
    if heard.is_none()
        && let (Some(n), Some(o)) = (crate::played::last_in(), crate::played::last_out())
        && let Some(last) = t.last.get_mut(usize::from(n.part))
    {
        *last = Some(o.note);
    }
    let at = ui.active_part;
    let part = ui.project.perf().parts().get(at.index());
    let (Some(part), true) = (part, on) else {
        t.shown = Shown::NONE;
        return;
    };
    let last = t.last.get(at.index()).copied().flatten();
    let (note, reference) = last.map_or((REF_NOTE, true), |n| (n, false));
    t.shown = match pitch(note, &part.sound, ui.midi.tune) {
        Some(p) => {
            let (a, b) = lines(p, reference);
            let sounds = heard.unwrap_or_else(|| ui.live.contains(at));
            shown(a, b, ui.theme.text, !reference && sounds)
        }
        None => Shown::NONE,
    };
}

/// A display behind `Paint` as the fonts' target: each row's run of one
/// colour one rect.
struct Runs<'a>(&'a mut dyn Paint);

impl OriginDimensions for Runs<'_> {
    fn size(&self) -> Size {
        Size::new(theme::SCREEN_W as u32, theme::SCREEN_H as u32)
    }
}

impl DrawTarget for Runs<'_> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        let mut run: Option<(Point, i32, Rgb565)> = None;
        for Pixel(p, c) in pixels {
            match &mut run {
                Some((at, w, rc)) if at.y == p.y && at.x + *w == p.x && *rc == c => *w += 1,
                _ => {
                    if let Some((at, w, rc)) = run {
                        self.0.rect(at.x, at.y, w, 1, rc);
                    }
                    run = Some((p, 1, c));
                }
            }
        }
        if let Some((at, w, rc)) = run {
            self.0.rect(at.x, at.y, w, 1, rc);
        }
        Ok(())
    }
}

/// The door's paint: the backing box, then the lines.
pub fn paint_in(on: &mut dyn Paint, s: &Shown) {
    let (x, y, w, h) = s.back;
    on.rect(x, y, w, h, theme::BG);
    let mut runs = Runs(on);
    for l in &s.lines {
        let Some(l) = l else { continue };
        let _ = l.font.render(
            l.text.as_str(),
            Point::new(l.x, l.y),
            VerticalPosition::Baseline,
            FontColor::Transparent(l.color),
            &mut runs,
        );
    }
}
