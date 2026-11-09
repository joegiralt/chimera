//! SETTINGS › PART › MIDI MAP's rows, keys and words (spec § 8.2, ADR
//! 0226), and the pickup hint's reading (§ 8.3). Row 0 is the Part's
//! controller map: B picks it, MIX+EDIT renames it, MIX+MINUS deletes it.
//! The layer's rows follow, edited in the map on the card, then the
//! Sound's own. On either B steps the control number (a CC past
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
use super::super::settings::midi_map::{MapDelete, MapRow, MapText, name};
use super::super::settings::prompt::Choice;
use super::super::settings::view::RowLook;
use super::layer::{W as Lw, shadowed, word as lword};
use super::learn::put_num;
use crate::addr::BlockRef;
use crate::dsp::voice_fx::params::FxType;
use crate::midi_map::{
    Binding, ButtonMode, Cc14Msb, CcKind, Feedback, Input, LearnCc, MidiMap, NrpnKind,
};

/// The page's words, `|` between: a named static, so its bytes are bank
/// 2's (`bank2.x`), not anonymous string rodata left in bank 1.
static WORDS: [u8; 518] = *b"JUMP|PICKUP|SCALE|TWOS COMP|OFFSET|SIGNED|SIGNED 2|INC DEC|MOMENTARY|TOGGLE|\
ABSOLUTE \xc2\xb7 JUMPS TO THE KNOB|ABSOLUTE \xc2\xb7 WAITS TO PICK UP|ABSOLUTE \xc2\xb7 SCALES TO MEET|\
RELATIVE \xc2\xb7 TWOS COMPLEMENT|RELATIVE \xc2\xb7 BINARY OFFSET|RELATIVE \xc2\xb7 SIGNED BIT|\
RELATIVE \xc2\xb7 SIGNED BIT 2|RELATIVE \xc2\xb7 NRPN|BUTTON \xc2\xb7 HELD IS MAX|BUTTON \xc2\xb7 EACH PRESS FLIPS|\
+1 = 1  -1 = 127|+1 = 65  -1 = 63|+1 = 1  -1 = 65|+1 = 65  -1 = 1|INCREMENT AND DECREMENT|\
CC |NRPN |PRESSURE|OFF|ON|- |CHORUS |DELAY |REVERB |REEL |MASTER |RING |MIX + PLUS KEEP MAP|SYSEX ";

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
    Keep,
    SysEx,
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
        Input::SysEx(_) => W::Jump,
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
        // Its value's byte, F0 the first: `SYSEX 8`.
        Input::SysEx(k) => {
            let _ = out.write_str(word(W::SysEx as u8));
            put_num(out, k.shape().map_or(0, |s| s.at() as u8 + 2));
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
    out.fixed = matches!(
        e.input,
        Input::Nrpn(..) | Input::Pressure(_) | Input::SysEx(_)
    );
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
        Input::SysEx(k) => (4, (k.key() ^ k.key() >> 16) & 0xFFFF),
    };
    tag << 24 | n << 8 | kind(i) as u32
}

/// `a` and `b` the same binding but for their targets.
#[inline(always)]
pub(super) fn same(a: &Binding, b: &Binding) -> bool {
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
        Input::SysEx(_) => Some(Takeover::Jump),
        _ => None,
    }
}

/// `e` as B to F's detents `d` leave it: B the control, C and D the
/// range, E the input, F the echo.
fn edited(e: &Binding, d: [i8; 6]) -> Binding {
    let by = |v: u8, d: i8| (i16::from(v) + i16::from(d)).clamp(0, 255) as u8;
    Binding {
        input: cycle(step(e.input, d[1]), d[4]),
        span: crate::midi_map::Span {
            min: by(e.span.min, d[2]),
            max: by(e.span.max, d[3]),
        },
        // Nothing can send a learned SysEx message back: no echo.
        feedback: if matches!(e.input, Input::SysEx(_)) {
            Feedback::Off
        } else {
            echo(e.feedback, d[5])
        },
        ..*e
    }
}

/// A MIDI MAP row (ADR 0226): a layer's or a Sound's, at its index.
#[derive(Clone, Copy)]
enum Row {
    Pick,
    Layer(usize, Binding),
    Own(usize, Binding),
}

/// Row `i` of a Part whose Sound's map is `own` and layer `layer`.
fn row_of(own: &MidiMap, layer: &MidiMap, i: usize) -> Option<Row> {
    let Some(k) = i.checked_sub(1) else {
        return Some(Row::Pick);
    };
    match layer.entries().get(k) {
        Some(&b) => Some(Row::Layer(k, b)),
        None => {
            let j = k - layer.len();
            own.entries().get(j).map(|&b| Row::Own(j, b))
        }
    }
}

impl UiState {
    /// The active Part's layer, empty before the door first ran.
    fn page_layer(&self) -> MidiMap {
        let i = self.active_part.index();
        self.cc.built().map_or(MidiMap::EMPTY, |m| *m.ctl.layer(i))
    }

    /// The page's rows: row 0, the layer's, the Sound's. The door's
    /// `map_rows`.
    pub(in crate::ui) fn map_rows_in(&self) -> usize {
        1 + self.page_layer().len() + self.project.part(self.active_part).sound.map.len()
    }

    /// Row `i`'s words and look into `out`. The door's `map_row`.
    pub(in crate::ui) fn map_row_in(&self, i: usize, out: &mut MapText) {
        let s = &self.project.part(self.active_part).sound;
        let (fx, engine) = (s.params.fx.ty(), s.params.engine());
        let layer = self.page_layer();
        match row_of(&s.map, &layer, i) {
            None => {}
            Some(Row::Pick) => {
                out.row = MapRow::Pick;
                let _ = out.label.write_str(lword(Lw::Controller));
                let n = self.project.controller(self.active_part);
                let head = n.as_ref().map_or(lword(Lw::NoMap), |n| n.as_str());
                let n = n.as_ref().map_or(lword(Lw::None), |n| n.as_str());
                let _ = out.word.write_str(n);
                let _ = out.head.write_str(head);
                out.key = settings_key(&[out.label.as_str().as_bytes(), n.as_bytes()]);
                // A map picked: its rename here, its delete on the hint
                // line (`fixed`, ADR 0255); else whether the card holds
                // more maps than listed.
                if self.project.controller(self.active_part).is_some() {
                    let _ = out.tail.write_str(lword(Lw::Whole));
                    out.fixed = true;
                    out.key ^= 4;
                } else if self.cc.built().is_some_and(|m| m.ctl.more) {
                    let _ = out.tail.write_str(lword(Lw::More));
                    out.key ^= 1;
                }
                // KEEP MAP's state beside MIX + PLUS (ADR 0253).
                let keep = self.keeps_map(self.active_part);
                let w = if keep { W::On } else { W::Off };
                let _ = out.control.write_str(word(W::Keep as u8));
                let _ = out.echo.write_str(word(w as u8));
                out.key ^= u32::from(keep) << 1;
            }
            Some(Row::Layer(_, e)) => {
                let live = e.target.live(engine) && !shadowed(&s.map, &e);
                text(&e, fx, live, out);
                out.row = MapRow::Layer;
                out.look = if live {
                    RowLook::Normal
                } else {
                    RowLook::Dimmed
                };
                if out.tail.as_str().is_empty()
                    && let Some(n) = self.project.controller(self.active_part)
                {
                    let _ = out.tail.write_str(lword(Lw::In));
                    let _ = out.tail.write_str(n.as_str());
                }
                out.key ^= 0x9E37_79B9;
            }
            Some(Row::Own(_, e)) => {
                let live = e.target.live(engine);
                text(&e, fx, live, out);
                if !live {
                    out.look = RowLook::Dimmed;
                }
            }
        }
    }

    /// A pass's keys on MIDI MAP: the encoders' detents A to F, and MIX
    /// down (bit 0) with MINUS (bit 1, delete) or PLUS (bit 2, KEEP MAP)
    /// pressed. The door's `map_in`.
    pub(in crate::ui) fn map_in(&mut self, d: [i8; 6], keys: u8) {
        let delete = keys & 0b011 == 0b011;
        let keep = keys & 0b101 == 0b101;
        let p = self.active_part;
        let (map, Some(at)) = (self.project.part(p).sound.map, self.loc.settings()) else {
            return;
        };
        let layer = *self.cc.ui().ctl.layer(p.index());
        let fx = self.project.part(p).sound.params.fx.ty();
        let (layer_at, own_at, e) = match row_of(&map, &layer, at.row() as usize) {
            None => return,
            Some(Row::Pick) => {
                if d[1] != 0 {
                    self.ctl_pick(p, d[1]);
                }
                if keep {
                    self.keep_map(p);
                }
                // The whole map deleted from the card (ADR 0255).
                if let (true, Some(n)) = (delete, self.project.controller(p)) {
                    let ask = MapDelete {
                        part: p,
                        entry: None,
                        label: "",
                        layer: true,
                        map: Some(n),
                    };
                    self.ask(Ask::MapDelete(ask, Choice::new()));
                }
                return;
            }
            Some(Row::Layer(k, e)) => (Some(k), 0, e),
            Some(Row::Own(i, e)) => (None, i, e),
        };
        if delete {
            let ask = MapDelete {
                part: p,
                entry: Some(e),
                label: name(&e, fx),
                layer: layer_at.is_some(),
                map: None,
            };
            return self.ask(Ask::MapDelete(ask, Choice::new()));
        }
        let n = edited(&e, d);
        if same(&n, &e) {
            return;
        }
        match layer_at {
            // A layer row: in the map on the card, by the next card work.
            Some(k) => {
                if self.cc.ui().ctl.edit(p, k, e, n) {
                    self.cc.reset_part(p);
                }
            }
            None => {
                let mut m = map;
                m.set(own_at, n);
                self.set_map(p, m);
            }
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
        let m = self.cc.built()?;
        let t = self.learn_target()?;
        let map = &self.project.part(self.active_part).sound.map;
        for j in 0..super::BINDS {
            if let Some(e) = super::bind_at(map, m.ctl.layer(p), j)
                && e.target == t
                && takeover(e.input) == Some(Takeover::Pickup)
                && let Some(d) = m.parts[p].rt[j].waiting()
            {
                return Some(d);
            }
        }
        None
    }
}
