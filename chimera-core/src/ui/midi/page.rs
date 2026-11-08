//! SETTINGS › PART › MIDI MAP's keys and words (spec § 8.2), and the
//! pickup hint's reading (§ 8.3). B steps the control number (a CC past
//! the reserved ones, a 14-bit pair along its MSBs; an NRPN or pressure is
//! fixed), C and D the range, E the input among the control's legal kinds
//! (never the echo: only learn sets that), F the echo. Bank 2's, behind
//! `MidiUiDoor`; bank 1 draws the rows and the bar's lines from these
//! words.

use core::fmt::Write;

use chimera_midi::ctl::Dir;
use chimera_midi::rel::ByteEncoding;
use chimera_midi::take::Takeover;

use super::super::UiState;
use super::super::fmt::FmtBuf;
use super::super::region::settings_key;
use super::super::settings::Ask;
use super::super::settings::midi_map::{MapDelete, MapText, name};
use super::super::settings::prompt::Choice;
use super::learn::put_num;
use crate::addr::BlockRef;
use crate::dsp::voice_fx::params::FxType;
use crate::midi_map::{Binding, ButtonMode, Cc14Msb, CcKind, Feedback, Input, LearnCc, NrpnKind};

/// The page's words, `|` between: a named static, so its bytes are bank
/// 2's (`bank2.x`), not anonymous string rodata left in bank 1.
static WORDS: [u8; 491] = *b"JUMP|PICKUP|SCALE|TWOS COMP|OFFSET|SIGNED|SIGNED 2|INC DEC|MOMENTARY|TOGGLE|\
ABSOLUTE \xc2\xb7 JUMPS TO THE KNOB|ABSOLUTE \xc2\xb7 WAITS TO PICK UP|ABSOLUTE \xc2\xb7 SCALES TO MEET|\
RELATIVE \xc2\xb7 TWOS COMPLEMENT|RELATIVE \xc2\xb7 BINARY OFFSET|RELATIVE \xc2\xb7 SIGNED BIT|\
RELATIVE \xc2\xb7 SIGNED BIT 2|RELATIVE \xc2\xb7 NRPN|BUTTON \xc2\xb7 HELD IS MAX|BUTTON \xc2\xb7 EACH PRESS FLIPS|\
+1 = 1  -1 = 127|+1 = 65  -1 = 63|+1 = 1  -1 = 65|+1 = 65  -1 = 1|INCREMENT AND DECREMENT|\
CC |NRPN |PRESSURE|OFF|ON|- |CHORUS |DELAY |REVERB |REEL |MASTER |RING ";

/// A word's place in `WORDS`.
// The heads and tails are reached by offset from their kind's word.
#[allow(dead_code)]
#[derive(Clone, Copy)]
#[repr(u8)]
enum W {
    Jump,
    Pickup,
    Scale,
    Twos,
    Offset,
    Signed,
    Signed2,
    IncDec,
    Momentary,
    Toggle,
    JumpsHead,
    PickupHead,
    ScaleHead,
    TwosHead,
    OffsetHead,
    SignedHead,
    Signed2Head,
    IncDecHead,
    MomentaryHead,
    ToggleHead,
    TwosTail,
    OffsetTail,
    SignedTail,
    Signed2Tail,
    IncDecTail,
    Cc,
    Nrpn,
    Pressure,
    Off,
    On,
    Dead,
    Chorus,
    Delay,
    Reverb,
    Reel,
    Master,
    Ring,
}

/// Word number `want`'s text in `WORDS`.
#[inline(never)]
fn word(want: u8) -> &'static str {
    super::pick(&WORDS, want)
}

/// The kind's short word, `Jump` to `Toggle`: its focus head is as far on
/// past them, a relative's or IncDec's tail as far on past the heads.
fn kind(i: Input) -> W {
    let rel = |e| match e {
        ByteEncoding::TwosComplement => W::Twos,
        ByteEncoding::BinaryOffset => W::Offset,
        ByteEncoding::SignBitDown => W::Signed,
        ByteEncoding::SignBitUp => W::Signed2,
    };
    let take = |t| match t {
        Takeover::Jump => W::Jump,
        Takeover::Pickup => W::Pickup,
        Takeover::Scale => W::Scale,
    };
    let button = |m| match m {
        ButtonMode::Momentary => W::Momentary,
        ButtonMode::Toggle => W::Toggle,
    };
    match i {
        Input::Cc(_, CcKind::Absolute(t))
        | Input::Cc14(_, t)
        | Input::Nrpn(_, NrpnKind::Absolute(t))
        | Input::Pressure(t) => take(t),
        Input::Cc(_, CcKind::Relative(e)) | Input::Nrpn(_, NrpnKind::Relative(e)) => rel(e),
        Input::Cc(_, CcKind::Button(m)) | Input::Nrpn(_, NrpnKind::Button(m)) => button(m),
        Input::Nrpn(_, NrpnKind::IncDec) => W::IncDec,
    }
}

/// The control as the row and DELETE name it: `CC 74`, `CC 2+34`,
/// `NRPN 10:3`, `PRESSURE`.
fn control(i: Input, out: &mut FmtBuf) {
    match i {
        Input::Cc(c, _) => {
            let _ = out.write_str(word(W::Cc as u8));
            put_num(out, c.get());
        }
        Input::Cc14(m, _) => {
            let _ = out.write_str(word(W::Cc as u8));
            put_num(out, m.get());
            let _ = out.write_char('+');
            put_num(out, m.get() + 32);
        }
        Input::Nrpn(n, _) => {
            let _ = out.write_str(word(W::Nrpn as u8));
            put_num(out, n.msb().get());
            let _ = out.write_char(':');
            put_num(out, n.lsb().get());
        }
        Input::Pressure(_) => {
            let _ = out.write_str(word(W::Pressure as u8));
        }
    }
}

/// `e`'s words into `out` (spec § 8.2's tables), its target `live` on the
/// Part's engine, FX type `fx` naming an FX pot. The door's `map_text`.
pub fn text(e: &Binding, fx: FxType, live: bool, out: &mut MapText) {
    control(e.input, &mut out.control);
    let _ = out.label.write_str(out.control.as_str());
    // A CC's number padded to three, as the list always had it.
    let pad = match e.input {
        Input::Cc(c, _) => 1 + usize::from(c.get() < 100) + usize::from(c.get() < 10),
        _ => 2,
    };
    for _ in 0..pad {
        let _ = out.label.write_char(' ');
    }
    if !live {
        let _ = out.label.write_str(word(W::Dead as u8));
    }
    // A global target says whose: `REVERB MIX`, `RING 1 CHANCE`.
    let whose = match e.target.addr().block {
        BlockRef::Chorus => Some(W::Chorus),
        BlockRef::Delay => Some(W::Delay),
        BlockRef::Reverb => Some(W::Reverb),
        BlockRef::Tape => Some(W::Reel),
        BlockRef::Comp => Some(W::Master),
        BlockRef::OrbitRing(_) => Some(W::Ring),
        _ => None,
    };
    if let Some(w) = whose {
        let _ = out.label.write_str(word(w as u8));
    }
    if let BlockRef::OrbitRing(r) = e.target.addr().block {
        put_num(&mut out.label, r.get() + 1);
        let _ = out.label.write_char(' ');
    }
    let _ = out.label.write_str(name(e, fx));
    let k = kind(e.input) as u8;
    let _ = out.word.write_str(word(k));
    let _ = out.head.write_str(word(k + W::JumpsHead as u8));
    if (W::Twos as u8..=W::IncDec as u8).contains(&k) {
        let _ = out
            .tail
            .write_str(word(k - W::Twos as u8 + W::TwosTail as u8));
    }
    put_num(&mut out.min, e.span.min);
    put_num(&mut out.max, e.span.max);
    let echo = match e.feedback {
        Feedback::Off => W::Off,
        Feedback::Value => W::On,
    };
    let _ = out.echo.write_str(word(echo as u8));
    out.fixed = matches!(e.input, Input::Nrpn(..) | Input::Pressure(_));
    out.key = settings_key(&[
        out.label.as_str().as_bytes(),
        out.word.as_str().as_bytes(),
        out.head.as_str().as_bytes(),
        out.tail.as_str().as_bytes(),
        out.min.as_str().as_bytes(),
        out.max.as_str().as_bytes(),
        out.echo.as_str().as_bytes(),
        &[out.fixed as u8],
    ]);
}

/// B's detents: a CC along the free numbers, a 14-bit pair along its
/// MSBs, stopping at the ends; an NRPN or pressure stays.
fn step(i: Input, d: i8) -> Input {
    let n = match i {
        Input::Cc(c, _) => c.get(),
        Input::Cc14(m, _) => m.get(),
        _ => return i,
    };
    let ok = |m: u8| match i {
        Input::Cc14(..) => Cc14Msb::new(m).is_some(),
        _ => LearnCc::new(m).is_some(),
    };
    let s = i16::from(d.signum());
    let mut n = i16::from(n);
    for _ in 0..d.unsigned_abs() {
        let mut m = n + s;
        while (0..=127).contains(&m) && !ok(m as u8) {
            m += s;
        }
        if !(0..=127).contains(&m) {
            break;
        }
        n = m;
    }
    match i {
        Input::Cc(c, k) => Input::Cc(LearnCc::new(n as u8).unwrap_or(c), k),
        Input::Cc14(m, t) => Input::Cc14(Cc14Msb::new(n as u8).unwrap_or(m), t),
        _ => i,
    }
}

/// E's detents: along the control's legal kinds (`Input::kinds`), round.
fn cycle(i: Input, d: i8) -> Input {
    if d == 0 {
        return i;
    }
    let mut legal = [i; 10];
    let mut n = 0;
    let mut at = 0;
    for k in i.kinds() {
        if code(k) == code(i) {
            at = n;
        }
        legal[n] = k;
        n += 1;
    }
    legal[(at as i16 + i16::from(d)).rem_euclid(n as i16) as usize]
}

/// F's detents: up is ON, down OFF.
fn echo(f: Feedback, d: i8) -> Feedback {
    match d {
        1.. => Feedback::Value,
        ..=-1 => Feedback::Off,
        0 => f,
    }
}

/// `i` as one number: its control, its number and its kind. Compared
/// here, not by `Input`'s `eq`, which a bank-2 caller leaves out of line
/// in bank 1.
#[inline(always)]
fn code(i: Input) -> u32 {
    let (tag, n) = match i {
        Input::Cc(c, _) => (0, u32::from(c.get())),
        Input::Cc14(m, _) => (1, u32::from(m.get())),
        Input::Nrpn(n, _) => (2, u32::from(n.get())),
        Input::Pressure(_) => (3, 0),
    };
    tag << 24 | n << 8 | kind(i) as u32
}

/// `a` and `b` the same binding but for their targets.
#[inline(always)]
fn same(a: &Binding, b: &Binding) -> bool {
    code(a.input) == code(b.input)
        && a.span.min == b.span.min
        && a.span.max == b.span.max
        && (a.feedback as u8) == (b.feedback as u8)
}

/// The input's takeover, if it reads absolute.
#[inline(always)]
fn takeover(i: Input) -> Option<Takeover> {
    match i {
        Input::Cc(_, CcKind::Absolute(t))
        | Input::Cc14(_, t)
        | Input::Nrpn(_, NrpnKind::Absolute(t))
        | Input::Pressure(t) => Some(t),
        _ => None,
    }
}

impl UiState {
    /// A pass's keys on MIDI MAP: the encoders' detents A to F, and
    /// MIX+MINUS. The door's `map_in`.
    pub(in crate::ui) fn map_in(&mut self, d: [i8; 6], delete: bool) {
        let p = self.active_part;
        let (map, Some(at)) = (self.project.part(p).sound.map, self.loc.settings()) else {
            return;
        };
        let i = at.row() as usize;
        let Some(&e) = map.entries().get(i) else {
            return;
        };
        if delete {
            let fx = self.project.part(p).sound.params.fx.ty();
            let ask = MapDelete {
                part: p,
                entry: e,
                label: name(&e, fx),
            };
            return self.ask(Ask::MapDelete(ask, Choice::new()));
        }
        let by = |v: u8, d: i8| (i16::from(v) + i16::from(d)).clamp(0, 255) as u8;
        let n = Binding {
            input: cycle(step(e.input, d[1]), d[4]),
            span: crate::midi_map::Span {
                min: by(e.span.min, d[2]),
                max: by(e.span.max, d[3]),
            },
            feedback: echo(e.feedback, d[5]),
            ..e
        };
        if !same(&n, &e) {
            let mut m = map;
            m.set(i, n);
            self.set_map(p, m);
        }
    }

    /// The focused cell's pickup, if a binding on the active Part waits
    /// for the knob: which way it must go (spec § 8.3). The door's
    /// `focus_note`.
    pub(in crate::ui) fn focus_note(&self) -> Option<Dir> {
        let p = self.active_part.index();
        if self.cc.stale >> p & 1 != 0 {
            return None;
        }
        let rt = &self.cc.built()?.parts[p].rt;
        let t = self.learn_target()?;
        let map = self.project.part(self.active_part).sound.map;
        let e = map.entries();
        for j in 0..e.len() {
            if e[j].target == t
                && takeover(e[j].input) == Some(Takeover::Pickup)
                && let Some(d) = rt[j].waiting()
            {
                return Some(d);
            }
        }
        None
    }
}
