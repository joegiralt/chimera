//! PIT's tuner worked out (ADR 0201): bank 2's with `bank2` (`bank2.x`
//! globs this module), reached through the UI's door (`UiDoor::pit`).
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

use super::{Ins, Line, Shown, Tuner};
use crate::dsp::tape_engine::params::{TapeMachine, at_root};
use crate::params::EngineType;
use crate::preset::Sound;
use crate::ui::fmt::FmtBuf;
use crate::ui::saver::{cents, nearest, note_name};
use crate::ui::theme::{self, Face, TextSize};

/// The note the corner reads before the Part has played one: middle C.
pub const REF_NOTE: u8 = 60;
/// The corner's width: LARGE falls back to NORMAL past it.
pub const CORNER_W: i32 = 112;
/// Below this |MOTION| SCRUB's head stands.
const STILL_MOTION: f32 = 1e-3;

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
        let _ = b.write_str("REF ");
        note_name(&mut b, i32::from(REF_NOTE));
        let _ = b.write_str(" ");
    }
    match p {
        Pitch::At(c) => {
            let (n, off) = nearest(libm::roundf(c) as i32);
            note_name(&mut a, n);
            let _ = a.write_str(" ");
            cents(&mut a, off);
            let tenths = libm::roundf(hz(c) * 10.0) as u32;
            let _ = write!(b, "{}.{} Hz", tenths / 10, tenths % 10);
        }
        Pitch::Shift(c) => {
            let _ = a.write_str("ROOT ?");
            let (st, off) = nearest(libm::roundf(c) as i32);
            let _ = write!(b, "{st:+}st ");
            cents(&mut b, off);
        }
        Pitch::Still => {
            let _ = a.write_str("STILL");
        }
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
    let left = lines.iter().flatten().map(|l| l.x).min().unwrap_or(right);
    let top = theme::VIZ_BAND_TOP;
    let back = (left - 4, top, right - (left - 4), yb + 4 - top);
    let key = crate::ui::region::settings_key(&[
        lines[0].as_ref().map_or(&[][..], |l| l.text.as_str().as_bytes()),
        lines[1].as_ref().map_or(&[][..], |l| l.text.as_str().as_bytes()),
        &[u8::from(lit), size as u8],
    ]);
    Shown { back, lines, key }
}

/// The door's entry: `played`'s last note taken for its Part, then, on
/// PIT, the edited Part's corner.
pub fn pass_in(t: &mut Tuner, i: &Ins<'_>) {
    let heard = t.heard();
    if heard.is_none()
        && let (Some(n), Some(o)) = (crate::played::last_in(), crate::played::last_out())
        && let Some(last) = t.last.get_mut(usize::from(n.part))
    {
        *last = Some(o.note);
    }
    if !i.on {
        t.shown = Shown::NONE;
        return;
    }
    let at = i.active.index();
    let (note, reference) = t.last[at].map_or((REF_NOTE, true), |n| (n, false));
    t.shown = match pitch(note, &i.parts[at].sound, i.tune) {
        Some(p) => {
            let (a, b) = lines(p, reference);
            let sounds = heard.unwrap_or_else(|| i.live.contains(i.active));
            shown(a, b, i.text, !reference && sounds)
        }
        None => Shown::NONE,
    };
}
