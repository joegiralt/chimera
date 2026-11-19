//! The encoders' feel (ADR 0279): the curve a turn takes, whether SEQ +
//! turn steps a cell fine, the band saying so, and CLASSIC RATE's
//! semitone tick. Bank 2's (`bank2.x`'s
//! `ui::feel` group, through `UiDoor`); bank 1 keeps the board's count,
//! SEQ's state, the gate's mute and the door's stubs.

use chimera_hal::{Accel, NUM_ENCODERS, Ticks, Turn};

use crate::block::{ParamKind, ParamSpec, ValFmt};
use crate::ui::glyph::FocusGlyph;
use crate::ui::hint::{Gesture, Hint};
use crate::ui::UiState;
use crate::ui::page::PageLayout;
use crate::ui::view::{self, View, Words};

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

/// Of the encoders `turned` with SEQ down, those on a cell that steps
/// fine: a Part's or its mixer's value (`takes`); never in SETTINGS or
/// ORBIT, nor on the matrix.
pub fn fine_cells(ui: &UiState, turned: [bool; NUM_ENCODERS]) -> [bool; NUM_ENCODERS] {
    let mut out = [false; NUM_ENCODERS];
    if ui.loc.settings().is_some() || ui.loc.orbit().is_some() || ui.loc.page(&ui.cx()).is_none()
    {
        return out;
    }
    let (def, ctx) = (ui.page_def(), ui.ctx());
    if def.layout == PageLayout::Matrix {
        return out;
    }
    for (i, (o, t)) in out.iter_mut().zip(turned).enumerate() {
        if t {
            let v = view::view(def, i, &ctx);
            *o = takes(&v, view::cell_words(def, &v, &ctx));
        }
    }
    out
}

/// A turn's step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The cell's curve.
    Plain,
    /// SEQ held: a tick a detent, no acceleration.
    Fine,
}

/// `t`'s ticks on `v`.
pub fn ticks(v: &View, t: Turn, step: Step) -> Ticks {
    match step {
        Step::Fine => t.detents().ticks(),
        Step::Plain => accel(v).ticks(t),
    }
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

/// The band's hint on `v` with words `w`: the fine turn where `v` takes
/// it, beside learn's or the pad's words; on a mixer cell at rest too (a
/// pause on SEQ would open ORBIT), MIX held its PART jump.
pub fn hint(v: &View, w: Words, band: Band) -> Hint {
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
static WORDS: [u8; 12] = *b"SEQ+TURNFINE";

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

/// CLASSIC RATE `hz` turned `d` semitones, in its range.
pub fn lfo_rate(hz: f32, d: i8) -> f32 {
    RATE_TICK.nudged(hz, d)
}
