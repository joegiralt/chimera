//! The encoders' feel (ADR 0279): the curve a turn takes, whether SEQ +
//! turn steps a cell fine, the band saying so, and CLASSIC RATE's
//! semitone tick. Bank 2's (`bank2.x`'s
//! `ui::feel` group, through `UiDoor`); bank 1 keeps the board's count,
//! SEQ's state, the gate's mute and the door's stubs.

use chimera_hal::{Accel, NUM_ENCODERS, Ticks, Turn};

use crate::addr::ParamAddr;
use crate::block::{Block, ParamKind, ParamSpec, ValFmt};
use crate::ui::UiState;
use crate::ui::fmt::FmtBuf;
use crate::ui::glyph::FocusGlyph;
use crate::ui::hint::{Gesture, Hint};
use crate::ui::hold::{SeqKey, SeqTurn};
use crate::ui::page::PageLayout;
use crate::ui::turns::{EncTurn, FrameTurns};
use crate::ui::view::{self, View, Words};

/// What bank 1 asks of the feel through `UiDoor`'s one entry, each with
/// its own answer's slot.
pub enum Ask<'a> {
    /// `frame`.
    Frame(
        &'a UiState,
        [Turn; NUM_ENCODERS],
        SeqKey,
        &'a mut Option<FrameTurns>,
    ),
    /// `list_ticks`.
    List(Turn, &'a mut Option<Ticks>),
    /// `hint`.
    Hint(&'a View, Words, Band, Pushes, &'a mut Option<Hint>),
    /// `nudge`: `true` where the feel turned it.
    Nudge(&'a mut dyn Block, ParamAddr, Ticks, &'a mut Option<bool>),
}

/// `UiDoor`'s entry.
pub(crate) fn ask_in(a: Ask<'_>) {
    match a {
        Ask::Frame(ui, turns, key, out) => *out = Some(frame(ui, turns, key)),
        Ask::List(t, out) => *out = Some(list_ticks(t)),
        Ask::Hint(v, w, band, push, out) => *out = Some(hint(v, w, band, push)),
        Ask::Nudge(b, a, d, out) => *out = Some(nudge(b, a, d)),
    }
}

/// How a turn on `v` accelerates: a pick from names (a choice, SOURCE,
/// OP, PAD, ROUTE) as a list, the rest as a parameter.
pub fn accel(v: &View) -> Accel {
    let names = matches!(v.fmt(), ValFmt::Names(_))
        || v.addr()
            .and_then(|a| a.spec())
            .is_some_and(|s| s.kind == ParamKind::Enum);
    if names || matches!(v, View::SelectOp | View::SelectPad | View::SelectRoute) {
        Accel::List
    } else {
        Accel::Param
    }
}

/// `v`, with words `w`, takes SEQ + turn as a fine step: a value, not a
/// choice (a list turn is a row a detent already), nor a cell drawn on
/// the staff (TRANSPOSE: its notes fill the band); and one whose words
/// leave the fine turn's hint room (no words of its own).
pub fn takes(v: &View, w: Words) -> bool {
    matches!(v, View::Param { .. })
        && accel(v) == Accel::Param
        && v.glyph() != FocusGlyph::Staff
        && !matches!(w, Words::Own(_))
}

/// The frame's `turns` with SEQ's and MIX's `key`: each encoder's ticks
/// by its cell's curve, or a tick a detent (`Fine`) where SEQ is down on a
/// cell that `takes` it; and a turn with SEQ down on a Part's, mixer's or
/// FX page cancels SEQ's tap and ORBIT (the owner's ruling), except while
/// a card row loads, the library works, a prompt is up or learn is armed
/// (they take the turn), and never in SETTINGS, the browser or ORBIT.
pub fn frame(ui: &UiState, turns: [Turn; NUM_ENCODERS], key: SeqKey) -> FrameTurns {
    let mut out = FrameTurns::closed(turns);
    if ui.loc.orbit().is_some() || ui.loc.page(&ui.cx()).is_none() {
        return out;
    }
    let (def, ctx) = (ui.page_def(), ui.ctx());
    let part = ui.loc.settings().is_none();
    let free = !ui.src.loading()
        && !ui.lib.busy()
        && ui.modal.is_none()
        && matches!(ui.learn, crate::ui::learn::Learn::Off);
    let held = key == SeqKey::Down && part && free;
    for (i, (e, t)) in out.enc.iter_mut().zip(turns).enumerate() {
        if *e == EncTurn::Still {
            continue;
        }
        let v = view::view(def, i, &ctx);
        *e = if held
            && def.layout != PageLayout::Matrix
            && takes(&v, view::cell_words(def, &v, &ctx))
        {
            EncTurn::Fine(t.detents().ticks())
        } else {
            EncTurn::Plain(accel(&v).ticks(t))
        };
        if held {
            out.seq = SeqTurn::Turned;
        }
    }
    out
}

/// A list's rows: `Accel::List`'s ticks.
pub fn list_ticks(t: Turn) -> Ticks {
    Accel::List.ticks(t)
}

/// Where the band is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// A Part's page.
    Sound,
    /// A mixer page; `mix`: MIX is down, so its PART jump.
    Mixer { mix: bool },
    /// Anywhere else (SETTINGS, ORBIT): the cell's own words.
    Other,
}

/// SYSTEM's PUSH ENCODERS (ADR 0140): on, a value cell with no words of
/// its own hints the pushes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushes {
    Off,
    On,
}

/// The band's hint on `v` with words `w`: the fine turn where `v` takes
/// it, beside learn's or the pad's words; on a mixer cell at rest too (a
/// pause on SEQ would open ORBIT), MIX held its PART jump. Pushes on, a
/// plain or mapped value cell hints them, and the SEQ fine turn in the
/// push's own gesture.
pub fn hint(v: &View, w: Words, band: Band, push: Pushes) -> Hint {
    let generic = matches!(w, Words::Plain | Words::Learn);
    if cfg!(feature = "push-encoders") && push == Pushes::On && generic && v.pushes() {
        return if takes(v, w) && band != Band::Other {
            &PUSH_FINE
        } else {
            crate::ui::push::HINT
        };
    }
    match band {
        Band::Sound | Band::Mixer { mix: false } if takes(v, w) => match w {
            Words::Learn => &FINE_LEARN,
            Words::Pad => &PAD_FINE,
            _ => &FINE,
        },
        Band::Mixer { .. } => view::mixer_hint(w),
        _ => w.hint(),
    }
}

/// The words this module adds, named so they link into bank 2 with it
/// (`bank2.x`'s `.rodata.*feel*`), not into bank 1's unnamed rodata.
static WORDS: [u8; 23] = *b"SEQ+TURNFINEPUSH/SEQ Hz";

/// `WORDS[at..at + len]`; checked at compile time (`WORDS` is UTF-8).
const fn word(at: usize, len: usize) -> &'static str {
    let (_, b) = WORDS.split_at(at);
    let (b, _) = b.split_at(len);
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => panic!("WORDS is UTF-8"),
    }
}

/// SEQ held, a turn steps fine; unspaced, so learn's words fit beside.
const FINE_TURN: Gesture<'static> = Gesture(word(0, 8), word(8, 4));

/// A value cell with no words of its own.
pub static FINE: [Gesture<'static>; 1] = [FINE_TURN];
/// A mapped value cell.
pub static FINE_LEARN: [Gesture<'static>; 2] = [FINE_TURN, view::LEARN_SHORT];
/// A pad's value cell.
pub static PAD_FINE: [Gesture<'static>; 2] = [view::PAD_HINT[0], FINE_TURN];
/// A value cell with PUSH ENCODERS on: a push's default, then either
/// key's fine turn.
pub static PUSH_FINE: [Gesture<'static>; 2] =
    [crate::ui::push::HINT[0], Gesture(word(12, 8), word(8, 4))];

/// The steps an octave is cut into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerOctave(u8);

impl PerOctave {
    pub const SEMITONES: PerOctave = PerOctave(12);

    pub const fn steps(self) -> f32 {
        self.0 as f32
    }
}

/// CLASSIC RATE's spec with a semitone's tick: 0.006 Hz at 0.1 Hz, 0.06
/// at 1, 0.6 at 10; 132 ticks end to end, as the linear 0.15 Hz steps
/// were. The stored Hz, the range, CCs and the readout stay RATE's.
static RATE_TICK: ParamSpec =
    crate::dsp::lfo::LFO_SPECS[0].ticks_per_octave(PerOctave::SEMITONES.steps());

/// `a` on `b` turned `d`: CLASSIC RATE a semitone a tick; `false` for any
/// other, which turns by its spec.
pub fn nudge(b: &mut dyn Block, a: ParamAddr, d: Ticks) -> bool {
    let rate = matches!(a.block, crate::addr::BlockRef::Lfo(_))
        && a.param == crate::dsp::lfo::LfoParams::RATE;
    if rate {
        let hz = b.get(a.param);
        b.write(a.param, lfo_rate(hz, d.get()));
    }
    rate
}

/// CLASSIC RATE `hz` turned `d` semitones, in its range.
pub fn lfo_rate(hz: f32, d: i8) -> f32 {
    RATE_TICK.nudged(hz, d)
}

/// CLASSIC RATE's readout of `v` (its 0-1 knob): 4 places under 0.1 Hz,
/// 3 under 1 Hz, then 2 and 1, so each semitone reads new.
pub(crate) fn rate_text(buf: &mut FmtBuf, v: f32) {
    let s = &crate::dsp::lfo::LFO_SPECS[0];
    let hz = s.min + v * (s.max - s.min);
    let m = libm::roundf(hz * 1000.0);
    let places = if m < 100.0 {
        4
    } else if m < 1000.0 {
        3
    } else if libm::roundf(hz * 100.0) < 1000.0 {
        2
    } else {
        1
    };
    crate::ui::fmt::fixed(buf, hz, places, word(20, 3));
}
