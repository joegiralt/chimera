//! Learn senses the kind (spec §§ 6.3, 8.1). The first control heard on
//! the armed Part's channel writes the focused cell's entry as `Absolute`
//! (`Jump`), and learn stays open: each message of that control is a sample
//! for `classify`, and the entry's input and echo follow the guess. Nothing
//! sensed is applied. Another control starts over on it, but for CC n+32
//! after CC n, a 14-bit pair's LSB half. Bank 1 ends it (MIX+SEQ, a nav key,
//! a page change, a turn) and draws the words this module writes.

use core::fmt::Write;

use chimera_midi::ctl::{Ctl, DataValue, Sel};
use chimera_midi::learn::{Guess, Seen, SeenCtl, Sure, classify};
use chimera_midi::rel::ByteEncoding;
use chimera_midi::take::Takeover;
use chimera_midi::value::{U7, U14};

#[cfg(feature = "midi-map")]
use super::super::PrimeStatus;
use super::super::UiState;
use super::super::fmt::FmtBuf;
use super::super::learn::{Learn, LearnNote, Line, Sensed};
use super::super::{draw, theme};
use super::NO_SEEN;
use crate::MidiChannel;
use crate::midi_in::{Cc, CcEvent};
use crate::midi_map::{Binding, ButtonMode, Cc14Msb, CcKind, Feedback, Input, LearnCc, NrpnKind};
use crate::project::PartId;

/// A guess is sure once this many samples agree on it, and a relative one
/// only once both ways were turned.
pub const SURE_AFTER: usize = 3;

/// Which control a message is from: learn starts over on another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Cc(u8),
    Nrpn(U14),
    Pressure,
}

impl Control {
    /// The control `i` reads, whatever its kind.
    #[inline(always)]
    fn of(i: Input) -> Self {
        match i {
            Input::Cc(c, _) => Self::Cc(c.get()),
            Input::Cc14(m, _) => Self::Cc(m.get()),
            Input::Nrpn(n, _) => Self::Nrpn(n),
            Input::Pressure(_) => Self::Pressure,
        }
    }

    /// `k` is this control: itself, or CC n+32 after CC n (n in 1..=31),
    /// the LSB half of a 14-bit pair.
    #[inline(always)]
    fn hears(self, k: Control) -> bool {
        match (self, k) {
            (Self::Cc(n), Self::Cc(l)) => n == l || (Cc14Msb::new(n).is_some() && l == n + 32),
            _ => self == k,
        }
    }
}

/// A message learn hears: its control and its sample. RPN data and steps,
/// program changes and the rest are no control learn takes.
fn sample(c: &Ctl) -> Option<(Control, SeenCtl, U7)> {
    Some(match *c {
        Ctl::Cc { cc, value } => (Control::Cc(cc.get()), SeenCtl::Cc(cc), value),
        Ctl::Data {
            sel: Sel::Nrpn(n),
            value,
        } => match value {
            DataValue::Coarse(v) => (Control::Nrpn(n), SeenCtl::Data, v),
            DataValue::Fine(f) => (Control::Nrpn(n), SeenCtl::DataFine, f.msb()),
        },
        Ctl::Step {
            sel: Sel::Nrpn(n),
            dir,
        } => (Control::Nrpn(n), SeenCtl::Step(dir), U7::ZERO),
        Ctl::Pressure(v) => (Control::Pressure, SeenCtl::Pressure, v),
        _ => return None,
    })
}

/// The entry a control's first message writes: `Absolute(Jump)`.
fn first_input(k: Control) -> Option<Input> {
    const JUMP: Takeover = Takeover::Jump;
    match k {
        Control::Cc(n) => Input::cc(n),
        Control::Nrpn(n) => Some(Input::Nrpn(n, NrpnKind::Absolute(JUMP))),
        Control::Pressure => Some(Input::Pressure(JUMP)),
    }
}

/// The input and echo `seen` says the control `was` reads is (pure). Echo
/// is on for a relative or IncDec guess, a 14-bit pair, and an NRPN that
/// sent a fine value; off for the rest (owner, spec § 6.2).
pub fn sensed_input(was: Input, seen: &Seen) -> (Input, Feedback) {
    use Feedback::{Off, Value};
    const JUMP: Takeover = Takeover::Jump;
    const PRESS: ButtonMode = ButtonMode::Momentary;
    let g = classify(seen);
    match Control::of(was) {
        Control::Cc(n) => {
            let Some(c) = LearnCc::new(n) else {
                return (was, Off);
            };
            match g {
                Guess::Paired14 => match Cc14Msb::new(n) {
                    Some(m) => (Input::Cc14(m, JUMP), Value),
                    None => (Input::Cc(c, CcKind::Absolute(JUMP)), Off),
                },
                Guess::Relative(e, _) => (Input::Cc(c, CcKind::Relative(e)), Value),
                Guess::Button => (Input::Cc(c, CcKind::Button(PRESS)), Off),
                Guess::Absolute => (Input::Cc(c, CcKind::Absolute(JUMP)), Off),
            }
        }
        Control::Nrpn(n) if seen.stepped() => (Input::Nrpn(n, NrpnKind::IncDec), Value),
        Control::Nrpn(n) => match g {
            Guess::Relative(e, _) => (Input::Nrpn(n, NrpnKind::Relative(e)), Value),
            Guess::Button => (Input::Nrpn(n, NrpnKind::Button(PRESS)), Off),
            Guess::Absolute | Guess::Paired14 => {
                let echo = if seen.fine_seen() { Value } else { Off };
                (Input::Nrpn(n, NrpnKind::Absolute(JUMP)), echo)
            }
        },
        Control::Pressure => (Input::Pressure(JUMP), Off),
    }
}

/// The guess for `seen` is sure (pure): `SURE_AFTER` samples, and for a
/// relative guess both directions. Pressure and steps have one kind each.
pub fn sure(input: Input, seen: &Seen) -> bool {
    let one_kind = matches!(input, Input::Pressure(_) | Input::Nrpn(_, NrpnKind::IncDec));
    let unsure = !one_kind && matches!(classify(seen), Guess::Relative(_, Sure::No));
    seen.len() >= SURE_AFTER && !unsure
}

const TAKEOVERS: [Takeover; 3] = [Takeover::Jump, Takeover::Pickup, Takeover::Scale];
/// `Sensed`'s kind codes past the takeovers' and the relatives'.
const MOMENTARY: u8 = 7;
const TOGGLE: u8 = 8;
const INC_DEC: u8 = 9;

/// A kind's code: 0-2 the takeovers, 3-6 the relatives, then a button's
/// two modes and IncDec.
fn kind_code(i: Input) -> u8 {
    let t = |t: Takeover| TAKEOVERS.iter().position(|&x| x == t).unwrap_or(0) as u8;
    let e = |e: ByteEncoding| 3 + ByteEncoding::ALL.iter().position(|&x| x == e).unwrap_or(0) as u8;
    let b = |b: ButtonMode| {
        if b == ButtonMode::Momentary {
            MOMENTARY
        } else {
            TOGGLE
        }
    };
    match i {
        Input::Cc(_, CcKind::Absolute(x)) | Input::Nrpn(_, NrpnKind::Absolute(x)) => t(x),
        Input::Cc(_, CcKind::Relative(x)) | Input::Nrpn(_, NrpnKind::Relative(x)) => e(x),
        Input::Cc(_, CcKind::Button(x)) | Input::Nrpn(_, NrpnKind::Button(x)) => b(x),
        Input::Nrpn(_, NrpnKind::IncDec) => INC_DEC,
        Input::Cc14(_, x) | Input::Pressure(x) => t(x),
    }
}

impl Sensed {
    /// `input` and whether its guess is `sure`, packed: byte 0 the sure
    /// bit, the control's tag (bits 1-2) and the kind's code (bits 3-6);
    /// bytes 1-2 the control's number (an NRPN's MSB and LSB).
    pub fn new(input: Input, sure: bool) -> Self {
        let (tag, a, b) = match input {
            Input::Cc(c, _) => (0, c.get(), 0),
            Input::Cc14(m, _) => (1, m.get(), 0),
            Input::Nrpn(n, _) => (2, n.msb().get(), n.lsb().get()),
            Input::Pressure(_) => (3, 0, 0),
        };
        Self([u8::from(sure) | tag << 1 | kind_code(input) << 3, a, b])
    }

    /// The input packed (`new`).
    pub fn input(self) -> Option<Input> {
        let [h, a, b] = self.0;
        let k = h >> 3;
        let take = TAKEOVERS.get(usize::from(k)).copied();
        let rel = k
            .checked_sub(3)
            .and_then(|e| ByteEncoding::ALL.get(usize::from(e)).copied());
        let button = match k {
            MOMENTARY => Some(ButtonMode::Momentary),
            TOGGLE => Some(ButtonMode::Toggle),
            _ => None,
        };
        let cc = take
            .map(CcKind::Absolute)
            .or(rel.map(CcKind::Relative))
            .or(button.map(CcKind::Button));
        Some(match h >> 1 & 3 {
            0 => Input::Cc(LearnCc::new(a)?, cc?),
            1 => Input::Cc14(Cc14Msb::new(a)?, take?),
            2 => {
                let n = U14::from_parts(U7::new(a)?, U7::new(b)?);
                let kind = match cc {
                    _ if k == INC_DEC => NrpnKind::IncDec,
                    Some(CcKind::Absolute(t)) => NrpnKind::Absolute(t),
                    Some(CcKind::Relative(e)) => NrpnKind::Relative(e),
                    Some(CcKind::Button(m)) => NrpnKind::Button(m),
                    None => return None,
                };
                Input::Nrpn(n, kind)
            }
            _ => Input::Pressure(take?),
        })
    }
}

/// The words this module writes, named so they link into bank 2 with it
/// (`bank2.x`'s `.rodata.*ui4midi*`), not into bank 1's unnamed rodata;
/// cut by `Word`. No `write!` either: its pieces would be bank 1's too.
static WORDS: [u8; 126] = *b"14-BIT CC NRPN :PRESSURE \xe2\x86\x92 TURN IT RIGHT, THEN LEFTRELATIVE \xc2\xb7 TWOS COMPLEMENTBINARY OFFSETSIGNED BIT 2BUTTONINC DECABSOLUTE";

#[derive(Clone, Copy)]
enum Word {
    Bits14,
    Nrpn,
    Colon,
    Pressure,
    Arrow,
    Ask,
    Relative,
    Twos,
    Offset,
    Signed2,
    Button,
    IncDec,
    Absolute,
    Cc,
    Bits14Guess,
    Signed,
}

/// `w`'s text in `WORDS`; checked, never failing (`WORDS` is UTF-8).
/// Matches, not `unwrap_or_default`: a core instance would be bank 1's.
#[inline(always)]
#[allow(clippy::manual_unwrap_or_default)]
fn word(w: Word) -> &'static str {
    let (at, len) = match w {
        Word::Bits14 => (0, 10),
        Word::Nrpn => (10, 5),
        Word::Colon => (15, 1),
        Word::Pressure => (16, 8),
        Word::Arrow => (24, 5),
        Word::Ask => (29, 24),
        Word::Relative => (53, 12),
        Word::Twos => (65, 15),
        Word::Offset => (80, 13),
        Word::Signed2 => (93, 12),
        Word::Button => (105, 6),
        Word::IncDec => (111, 7),
        Word::Absolute => (118, 8),
        Word::Cc => (7, 3),
        Word::Bits14Guess => (0, 9),
        Word::Signed => (93, 10),
    };
    match WORDS.get(at..at + len) {
        Some(b) => match core::str::from_utf8(b) {
            Ok(s) => s,
            Err(_) => "",
        },
        None => "",
    }
}

/// What learn says but for sensing (spec § 8.1's refusals, and armed).
static NOTES: [u8; 74] =
    *b" NOT MAPPABLE|CC | ON CH |, PART ON CH | RESERVED|MAP FULL|MOVE A KNOB \xc2\xb7 ";

/// `NOTES`' words, in order.
#[derive(Clone, Copy)]
#[repr(u8)]
enum Note {
    NotMappable,
    Cc,
    OnCh,
    PartOnCh,
    Reserved,
    Full,
    Armed,
}

/// `w`'s text in `NOTES`.
#[inline(never)]
fn note(w: Note) -> &'static str {
    super::pick(&NOTES, w as u8)
}

/// Learn's words for `n` on the cell labelled `label`, into `out`: `None`
/// armed, `MOVE A KNOB · CUTOFF`; a refusal; sensing's guess. The door's
/// `note`.
pub fn note_words(n: Option<LearnNote>, label: &str, out: &mut FmtBuf) {
    let put = |out: &mut FmtBuf, w: Note| {
        let _ = out.write_str(note(w));
    };
    let cc = |out: &mut FmtBuf, c: Cc| {
        put(out, Note::Cc);
        put_num(out, c.get());
    };
    match n {
        None => {
            put(out, Note::Armed);
            let _ = out.write_str(label);
        }
        Some(LearnNote::NotMappable) => {
            let _ = out.write_str(label);
            put(out, Note::NotMappable);
        }
        Some(LearnNote::WrongChannel { cc: c, got, part }) => {
            cc(out, c);
            put(out, Note::OnCh);
            put_num(out, got.get() + 1);
            put(out, Note::PartOnCh);
            put_num(out, part.get() + 1);
        }
        Some(LearnNote::Reserved(c)) => {
            cc(out, c);
            put(out, Note::Reserved);
        }
        Some(LearnNote::Sensing(s)) => words(s, label, Line::Words, out),
        Some(LearnNote::Full) => put(out, Note::Full),
    }
}

/// `n` in decimal.
pub(super) fn put_num(out: &mut FmtBuf, n: u8) {
    let (h, t, o) = (n / 100, n / 10 % 10, n % 10);
    for (d, shown) in [(h, h > 0), (t, n >= 10), (o, true)] {
        if shown {
            let _ = out.write_char(char::from(b'0' + d));
        }
    }
}

/// Sensing's line `line` for `s` on the cell labelled `label`, into `out`
/// (spec § 8.1). `Label`: `CC 21 → CUTOFF`. `Words`: `TURN IT RIGHT, THEN
/// LEFT` till sure, then the guess in words; the band reads them in the
/// value's place, where they fit, and bank 1 draws `MIX + SEQ  DONE` on
/// the hint line after a sure guess. The door's `words`.
pub fn words(s: Sensed, label: &str, line: Line, out: &mut FmtBuf) {
    let Some(input) = s.input() else {
        return;
    };
    match line {
        Line::Label => label_line(input, label, out),
        Line::Words if !s.sure() => {
            let _ = out.write_str(word(Word::Ask));
        }
        Line::Words => guess_words(input, out),
    }
}

fn label_line(input: Input, label: &str, out: &mut FmtBuf) {
    let control = |out: &mut FmtBuf| match input {
        Input::Cc(c, _) => {
            let _ = out.write_str(word(Word::Cc));
            put_num(out, c.get());
        }
        Input::Cc14(m, _) => {
            let _ = out.write_str(word(Word::Bits14));
            put_num(out, m.get());
        }
        Input::Nrpn(n, _) => {
            let _ = out.write_str(word(Word::Nrpn));
            put_num(out, n.msb().get());
            let _ = out.write_str(word(Word::Colon));
            put_num(out, n.lsb().get());
        }
        Input::Pressure(_) => {
            let _ = out.write_str(word(Word::Pressure));
        }
    };
    control(out);
    let _ = out.write_str(word(Word::Arrow));
    let _ = out.write_str(label);
    // Too wide (`NRPN 127:127 → MECHANICS`): the control alone, its target
    // the focused cell's.
    let (face, t) = (&theme::FONT_VALUE, theme::LABEL_TRACKING);
    if draw::text_width(face, out.as_str(), t) > theme::CONTENT_W {
        *out = FmtBuf::new();
        control(out);
    }
}

fn guess_words(input: Input, out: &mut FmtBuf) {
    let w = match input {
        Input::Cc(_, CcKind::Relative(e)) | Input::Nrpn(_, NrpnKind::Relative(e)) => {
            let _ = out.write_str(word(Word::Relative));
            match e {
                ByteEncoding::TwosComplement => Word::Twos,
                ByteEncoding::BinaryOffset => Word::Offset,
                ByteEncoding::SignBitDown => Word::Signed,
                ByteEncoding::SignBitUp => Word::Signed2,
            }
        }
        Input::Nrpn(_, NrpnKind::IncDec) => {
            let _ = out.write_str(word(Word::Relative));
            Word::IncDec
        }
        Input::Cc(_, CcKind::Button(_)) | Input::Nrpn(_, NrpnKind::Button(_)) => Word::Button,
        Input::Cc14(..) => Word::Bits14Guess,
        _ => Word::Absolute,
    };
    let _ = out.write_str(word(w));
}

impl UiState {
    /// A control message on channel `ch`, learn's first (spec § 6.3):
    /// `true` when learn took it, so no binding hears it. Armed, the first
    /// control on the Part's channel writes the entry; sensing, the same
    /// control's message is a sample, another's starts over. A CC on
    /// another channel is said and heard on.
    pub(super) fn learn_in(&mut self, ch: MidiChannel, c: &Ctl) -> bool {
        let (part, entry) = match self.learn {
            Learn::Off => return false,
            Learn::Armed { part, .. } => (part, None),
            Learn::Sensing { part, entry } => (part, Some(usize::from(entry))),
        };
        let Some((k, s, v)) = sample(c) else {
            return false;
        };
        let at = self.project.part(part).mix.channel;
        if ch != at {
            if let (None, Ctl::Cc { cc, .. }) = (entry, c)
                && let Some(cc) = Cc::new(cc.get())
            {
                let note = LearnNote::WrongChannel {
                    cc,
                    got: ch,
                    part: at,
                };
                self.learn_note(part, note);
            }
            return false;
        }
        if let Some(j) = entry {
            let ours = self.learn_target();
            match self.project.part(part).sound.map.entries().get(j) {
                Some(e) if Some(e.target) == ours && Control::of(e.input).hears(k) => {
                    self.learn_sense(part, j, s, v);
                    return true;
                }
                Some(e) if Some(e.target) == ours => {}
                // The map changed under it (not by learn): learn is over.
                _ => {
                    self.learn = Learn::Off;
                    return false;
                }
            }
        }
        self.learn_start(part, k, s, v)
    }

    /// A CC no binding may take, heard while armed: said (`CC 120
    /// RESERVED`, or its channel as any CC's), and learn stays armed.
    pub(super) fn learn_reserved(&mut self, ev: CcEvent) {
        if let Learn::Armed { part, .. } = self.learn {
            let at = self.project.part(part).mix.channel;
            let note = if ev.channel == at {
                LearnNote::Reserved(ev.cc)
            } else {
                LearnNote::WrongChannel {
                    cc: ev.cc,
                    got: ev.channel,
                    part: at,
                }
            };
            self.learn_note(part, note);
        }
    }

    #[inline(always)]
    fn learn_note(&mut self, part: PartId, note: LearnNote) {
        self.learn = Learn::Armed {
            part,
            note: Some(note),
        };
    }

    /// Control `k` maps the focused cell, its first sample `s`, `v`: the
    /// entry written as `Absolute(Jump)`, then sensed. A full map is said
    /// and ends learn; a cell no control may drive leaves it armed.
    fn learn_start(&mut self, part: PartId, k: Control, s: SeenCtl, v: U7) -> bool {
        let (Some(input), Some(t)) = (first_input(k), self.learn_target()) else {
            // Nothing learnable: learn stands, the message falls through.
            return false;
        };
        let mut map = self.project.part(part).sound.map;
        let entry = match map.learn(input, t) {
            Ok(()) => map.entries().iter().position(|e| e.target == t),
            Err(_) => None,
        };
        let Some(entry) = entry else {
            self.learn = Learn::Off;
            #[cfg(feature = "midi-map")]
            {
                self.prime_status = Some(PrimeStatus::Learn(Some(LearnNote::Full)));
            }
            return true;
        };
        // A Sound edit, so `*` shows; entries may have moved.
        self.project.edit_part(part).sound.map = map;
        self.cc.reset_part(part);
        self.cc.ui().seen = NO_SEEN;
        self.learn = Learn::Sensing {
            part,
            entry: entry as u8,
        };
        self.learn_sense(part, entry, s, v);
        true
    }

    /// Sample `s`, `v` of entry `j`'s control on Part `part`: the entry's
    /// input and echo rewritten when the guess changes, never applied. Past
    /// `Seen::CAP` samples the guess stands.
    fn learn_sense(&mut self, part: PartId, j: usize, s: SeenCtl, v: U7) {
        let seen = &mut self.cc.ui().seen;
        if !seen.push(s, v) {
            return;
        }
        let Some(&e) = self.project.part(part).sound.map.entries().get(j) else {
            return;
        };
        let (input, feedback) = sensed_input(e.input, &self.cc.ui().seen);
        if (input, feedback) != (e.input, e.feedback) {
            let b = Binding {
                input,
                feedback,
                ..e
            };
            self.project.edit_part(part).sound.map.set(j, b);
            self.cc.reset_part(part);
        }
    }

    /// What sensing has made of the control: the entry's input and whether
    /// its guess is sure. The door's `sensed`.
    pub(in crate::ui) fn sensed_in(&self) -> Option<Sensed> {
        let Learn::Sensing { part, entry } = self.learn else {
            return None;
        };
        let e = self
            .project
            .part(part)
            .sound
            .map
            .entries()
            .get(usize::from(entry))?;
        let seen = &self.cc.built()?.seen;
        Some(Sensed::new(e.input, sure(e.input, seen)))
    }
}
