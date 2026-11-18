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
use crate::MidiNote;
use crate::dsp::tape_engine::params::{TapeMachine, plays_at_root};
use crate::params::EngineType;
use crate::preset::Sound;
use crate::ui::UiState;
use crate::ui::block_def::VizType;
use crate::ui::draw::text_width;
use crate::ui::fmt::FmtBuf;
use crate::ui::saver::{nearest, note_name};
use crate::ui::tape_strip::Paint;
use crate::ui::theme::{self, Face, TextSize};

/// The note the corner reads before the Part has played one: middle C.
pub const REF_NOTE: MidiNote = MidiNote::clamped(60);
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
    // Checked, not `from_utf8_unchecked`: `WORDS` is ASCII, so it never
    // fails, and a `&str` table would put its bytes in bank 1's unnamed
    // rodata.
    // A match, not `Option::map`: the map's instance would be core's, in
    // bank 1.
    if let Some(w) = WORDS.get(at..at + len)
        && let Ok(s) = core::str::from_utf8(w)
    {
        let _ = b.write_str(s);
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

/// A pitch as cents above MIDI note 0: A4 (6 900) is 440 Hz. What the
/// voice's sum comes to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone(f32);

impl Tone {
    pub const fn of_cents(c: f32) -> Tone {
        Tone(c)
    }

    /// The key's own pitch.
    pub fn of_note(n: MidiNote) -> Tone {
        Tone(f32::from(n.get()) * 100.0)
    }

    /// `by` cents up.
    pub fn up(self, by: f32) -> Tone {
        Tone(self.0 + by)
    }

    pub fn cents(self) -> f32 {
        self.0
    }

    /// At A4 = 440 Hz.
    pub fn hz(self) -> Hz {
        Hz(440.0 * libm::exp2f((self.0 - 6_900.0) / 1_200.0))
    }

    /// The nearest note, to 1 cent.
    pub fn named(self) -> Named {
        let (note, off) = nearest(libm::roundf(self.0) as i32);
        Named { note, off }
    }
}

/// A frequency.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hz(f32);

impl Hz {
    pub fn get(self) -> f32 {
        self.0
    }

    /// To 0.1 Hz.
    fn tenths(self) -> u32 {
        libm::roundf(self.0 * 10.0) as u32
    }
}

/// A tone's nearest note (a note number, C4 60; past MIDI's range it
/// names on) and the cents off it, −50..=49. Only `Tone::named` makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Named {
    note: i32,
    off: i32,
}

/// Cents off a source's own speed: how a source with no known root plays.
/// It names no note, so nothing can show it as one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shift(f32);

impl Shift {
    pub const fn of_cents(c: f32) -> Shift {
        Shift(c)
    }

    /// Whole semitones and the cents past them, −50..=49, to 1 cent.
    fn steps(self) -> (i32, i32) {
        nearest(libm::roundf(self.0) as i32)
    }
}

/// A pitch that is known: a tone, or SCRUB's head standing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Known {
    Note(Tone),
    Still,
}

/// Where a Part's pitch rests: known, or a source's shift with no root.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reading {
    Known(Known),
    RootUnknown(Shift),
}

/// How the corner draws: lit while the Part sounds, dim between notes,
/// and dim with `REF C4` before the Part's first note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Light {
    Lit,
    Dim,
    Reference,
}

/// What the corner shows. A root that isn't known carries a `Shift`, never
/// a `Tone`, so it can't read as a note.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Readout {
    None,
    Lit(Known),
    Dim(Known),
    Reference(Known),
    RootUnknown(Shift, Light),
}

impl Readout {
    /// `r` drawn `light`; no reading (TAPE with no source), nothing.
    pub fn of(r: Option<Reading>, light: Light) -> Readout {
        match (r, light) {
            (None, _) => Readout::None,
            (Some(Reading::RootUnknown(s)), l) => Readout::RootUnknown(s, l),
            (Some(Reading::Known(k)), Light::Lit) => Readout::Lit(k),
            (Some(Reading::Known(k)), Light::Dim) => Readout::Dim(k),
            (Some(Reading::Known(k)), Light::Reference) => Readout::Reference(k),
        }
    }

    fn light(self) -> Option<Light> {
        match self {
            Readout::None => None,
            Readout::Lit(_) => Some(Light::Lit),
            Readout::Dim(_) => Some(Light::Dim),
            Readout::Reference(_) => Some(Light::Reference),
            Readout::RootUnknown(_, l) => Some(l),
        }
    }
}

/// `s` playing `note` under `tune` cents: SETTINGS' TUNE and the Part's
/// TUNING at the key; `None` for TAPE with no source, which plays nothing,
/// and for DRUM, whose key names a pad, not a pitch.
pub fn pitch(note: MidiNote, s: &Sound, tune: f32) -> Option<Reading> {
    let p = &s.params;
    let set = p.pitch.semitones() * 100.0 + tune;
    let key = Tone::of_note(note).up(set);
    let at = |extra: f32| Some(Reading::Known(Known::Note(key.up(extra))));
    match p.engine() {
        EngineType::Algo => at(f32::from(p.algo.transpose) * 100.0),
        EngineType::Modal => at(0.0),
        EngineType::Drum => None,
        EngineType::Va => at(f32::from(p.va.coarse[0].clamp(-24, 24)) * 100.0 + p.va.fine[0]),
        EngineType::Tape => {
            s.source?;
            let t = &p.tape;
            // SLICE NOTE and SCRUB's KEY POSITION play every key at ROOT.
            let key = if plays_at_root(t) {
                Tone(t.root).up(set)
            } else {
                key
            };
            let c = match t.machine {
                // TRANSWAVE's cycle plays the note itself.
                TapeMachine::Wave => return at(0.0),
                // SCRUB runs at 2 · |MOTION| of the source's speed.
                TapeMachine::Scrub => {
                    let m = libm::fabsf(t.motion);
                    if m < STILL_MOTION {
                        return Some(Reading::Known(Known::Still));
                    }
                    key.up(1_200.0 * libm::log2f(2.0 * m))
                }
                // WALK and STRETCH (each TYPE) play the key's pitch.
                TapeMachine::Walk | TapeMachine::Stretch => key,
            };
            Some(if t.period > 0.0 {
                Reading::Known(Known::Note(c))
            } else {
                Reading::RootUnknown(Shift(c.cents() - t.root))
            })
        }
    }
}

/// The corner's two lines: the note and cents, then Hz, to 1 cent and
/// 0.1 Hz; a source with no root, `ROOT ?` and its shift. A reference
/// leads its second line with `REF C4`.
pub fn lines(r: Readout) -> (FmtBuf, FmtBuf) {
    let (mut a, mut b) = (FmtBuf::new(), FmtBuf::new());
    if r.light() == Some(Light::Reference) {
        word(&mut b, Word::Ref);
        note_name(&mut b, i32::from(REF_NOTE.get()));
        word(&mut b, Word::Space);
    }
    let known = match r {
        Readout::None => None,
        Readout::Lit(k) | Readout::Dim(k) | Readout::Reference(k) => Some(k),
        Readout::RootUnknown(s, _) => {
            word(&mut a, Word::Root);
            let (st, off) = s.steps();
            int(&mut b, st, true);
            word(&mut b, Word::St);
            word(&mut b, Word::Space);
            cents(&mut b, off);
            None
        }
    };
    match known {
        Some(Known::Note(t)) => {
            let n = t.named();
            note_name(&mut a, n.note);
            word(&mut a, Word::Space);
            cents(&mut a, n.off);
            let tenths = t.hz().tenths();
            int(&mut b, (tenths / 10) as i32, false);
            let _ = b.write_char('.');
            int(&mut b, (tenths % 10) as i32, false);
            word(&mut b, Word::Space);
            word(&mut b, Word::Hz);
        }
        Some(Known::Still) => word(&mut a, Word::Still),
        None => {}
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

/// `r` set right on the band's edge at `size`, or NORMAL where LARGE
/// overruns the corner.
pub fn shown(r: Readout, size: TextSize) -> Shown {
    let Some(light) = r.light() else {
        return Shown::NONE;
    };
    let lit = light == Light::Lit;
    let (a, b) = lines(r);
    let fonts = |s| (theme::font(Face::Value, s), theme::font(Face::Label, s));
    let width = |s| {
        let (fa, fb) = fonts(s);
        text_width(fa, a.as_str(), 0).max(text_width(fb, b.as_str(), 0))
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
        let w = text_width(font, text.as_str(), 0);
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
    let key = crate::ui::region::settings_key(&[text(0), text(1), &[u8::from(lit), size as u8]]);
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
        && let Some(last) = t.last.get_mut(n.part.index())
    {
        *last = Some(crate::MidiNote::clamped(o.note));
    }
    let at = ui.active_part;
    let part = ui.project.perf().parts().get(at.index());
    let (Some(part), true) = (part, on) else {
        t.shown = Shown::NONE;
        return;
    };
    let last = t.last.get(at.index()).copied().flatten();
    let sounds = heard.unwrap_or_else(|| ui.live.contains(at));
    let (note, light) = match last {
        None => (REF_NOTE, Light::Reference),
        Some(n) if sounds => (n, Light::Lit),
        Some(n) => (n, Light::Dim),
    };
    // The Part's TUNING at the key, as the voice reads it at note-on.
    let tuning = crate::tuning::offset(at.index(), note.get()) * 100.0;
    let r = Readout::of(
        pitch(note, &part.sound, f32::from(ui.midi.tune) + tuning),
        light,
    );
    t.shown = shown(r, ui.theme.text);
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
