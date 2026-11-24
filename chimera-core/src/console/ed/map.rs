// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI MAP (ADR 0289, slice 4): `ed describe MAP` from the binding
//! model's tables, `since`'s MAP section, `ed map …`, `ed maps` and
//! `ed learn`. A row is ADR 0217's `Binding`; the Part's own rows are its
//! Sound's (`S<i>`), its controller map's are `M<i>` (ADR 0226). Every
//! word is the box's own (`ui::midi::page::text`, spaces as `-`). Commands
//! are typed here, checked against the box now (`check`), queued, and
//! applied in the UI pass through MIDI MAP's own writes (`apply`): learn's
//! `MidiMap::learn`, the page's B to F, DELETE's drop, row 0's pick, KEEP
//! MAP, save and rename, MIX + SEQ's arming.

use core::fmt::{self, Write};

use super::EdAddr;
use super::addr::{EdBlock, PartBlock};
use super::lit::{QUOTE, QUOTE_SHUT, UNQUOTE, lit, quoted, text, uint};
use super::project::EdName;
use super::schema::Round;
use crate::addr::{BlockRef, ParamAddr};
use crate::console::Words;
use crate::dsp::voice_fx::params::FxType;
use crate::midi_map::ctl::MAP_ROWS;
use crate::midi_map::{
    Binding, ButtonMode, Cc14Msb, CcKind, Feedback, Input, LearnCc, MAP_LEN, Mappable, MidiMap,
    NrpnKind, Span, SxKey,
};
use crate::name::SoundName;
use crate::params::FilterParams;
use crate::project::PartId;
use crate::ui::UiState;
use crate::ui::ed_cue::{self, CellSlot, LearnAt, LearnOn, Pass};
use crate::ui::learn::Learn;
use crate::ui::midi::layer::{CtlJob, MapMark, shadowed};
use crate::ui::settings::midi_map::MapText;
use chimera_midi::rel::ByteEncoding;
use chimera_midi::sxmap::Shape;
use chimera_midi::take::Takeover;
use chimera_midi::value::U14;

/// A kind of input, as MIDI MAP's E cycles them: the box's short words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapKind {
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
}

/// A target words are read under: any knob, the words name the input.
const SAMPLE: Mappable = match Mappable::new(ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF))
{
    Some(m) => m,
    None => panic!("the cutoff is mappable"),
};

/// An NRPN reads every kind: each kind's sample.
const NRPN0: U14 = match U14::new(0) {
    Some(n) => n,
    None => panic!(),
};

impl MapKind {
    pub const ALL: [MapKind; 10] = [
        MapKind::Jump,
        MapKind::Pickup,
        MapKind::Scale,
        MapKind::Twos,
        MapKind::Offset,
        MapKind::Signed,
        MapKind::Signed2,
        MapKind::IncDec,
        MapKind::Momentary,
        MapKind::Toggle,
    ];

    /// `i`'s kind.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.kind_of"))]
    fn of(i: Input) -> MapKind {
        let take = |t| match t {
            Takeover::Jump => MapKind::Jump,
            Takeover::Pickup => MapKind::Pickup,
            Takeover::Scale => MapKind::Scale,
        };
        let rel = |e| match e {
            ByteEncoding::TwosComplement => MapKind::Twos,
            ByteEncoding::BinaryOffset => MapKind::Offset,
            ByteEncoding::SignBitDown => MapKind::Signed,
            ByteEncoding::SignBitUp => MapKind::Signed2,
        };
        let button = |m| match m {
            ButtonMode::Momentary => MapKind::Momentary,
            ButtonMode::Toggle => MapKind::Toggle,
        };
        match i {
            Input::Cc(_, CcKind::Absolute(t))
            | Input::Cc14(_, t)
            | Input::Nrpn(_, NrpnKind::Absolute(t))
            | Input::Pressure(t) => take(t),
            // A learned SysEx message is absolute, `Jump` (ADR 0241).
            Input::SysEx(_) => MapKind::Jump,
            Input::Cc(_, CcKind::Relative(e)) | Input::Nrpn(_, NrpnKind::Relative(e)) => rel(e),
            Input::Cc(_, CcKind::Button(m)) | Input::Nrpn(_, NrpnKind::Button(m)) => button(m),
            Input::Nrpn(_, NrpnKind::IncDec) => MapKind::IncDec,
        }
    }

    /// This kind on an NRPN, which reads them all: its words' sample.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.sample"))]
    fn sample(self) -> Input {
        let k = match self {
            MapKind::Jump => NrpnKind::Absolute(Takeover::Jump),
            MapKind::Pickup => NrpnKind::Absolute(Takeover::Pickup),
            MapKind::Scale => NrpnKind::Absolute(Takeover::Scale),
            MapKind::Twos => NrpnKind::Relative(ByteEncoding::TwosComplement),
            MapKind::Offset => NrpnKind::Relative(ByteEncoding::BinaryOffset),
            MapKind::Signed => NrpnKind::Relative(ByteEncoding::SignBitDown),
            MapKind::Signed2 => NrpnKind::Relative(ByteEncoding::SignBitUp),
            MapKind::IncDec => NrpnKind::IncDec,
            MapKind::Momentary => NrpnKind::Button(ButtonMode::Momentary),
            MapKind::Toggle => NrpnKind::Button(ButtonMode::Toggle),
        };
        Input::Nrpn(NRPN0, k)
    }

    /// Its token: the box's word, each space `-` (`TWOS-COMP`).
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.kind_write"))]
    fn write(self, w: &mut dyn Write) -> fmt::Result {
        dashed(words(self.sample()).word.as_str(), w)
    }

    /// The kind token `w` names.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.kind_parse"))]
    fn parse(w: &[u8]) -> Option<MapKind> {
        MapKind::ALL.into_iter().find(|k| {
            let word = words(k.sample()).word;
            let b = word.as_str().as_bytes();
            b.len() == w.len()
                && b.iter()
                    .zip(w)
                    .all(|(&x, &y)| x == y || (x == b' ' && y == b'-'))
        })
    }
}

/// `s`, each space `-`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.dashed"))]
fn dashed(s: &str, w: &mut dyn Write) -> fmt::Result {
    for c in s.chars() {
        w.write_char(if c == ' ' { '-' } else { c })?;
    }
    Ok(())
}

/// `i`'s words as MIDI MAP shows them: its kind's word, head and tail.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.words"))]
fn words(i: Input) -> MapText {
    let b = Binding {
        input: i,
        target: SAMPLE,
        span: Span::FULL,
        feedback: Feedback::Off,
    };
    let mut t = MapText::default();
    crate::ui::midi::page::text(&b, FxType::Off, true, &mut t);
    t
}

/// A control `ed map add` binds: a CC, a 14-bit pair, an NRPN or channel
/// pressure. Never a SysEx message: only learn hears one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Cc(LearnCc),
    Cc14(Cc14Msb),
    Nrpn(U14),
    Press,
}

/// A row's input on the wire: a control, or a learned SysEx message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputTok {
    Control(Control),
    SysEx,
}

/// An input form, for the schema's `input` lines.
#[derive(Clone, Copy)]
enum Form {
    Cc,
    Cc14,
    Nrpn,
    Press,
    SysEx,
}

impl Form {
    const ALL: [Form; 5] = [Form::Cc, Form::Cc14, Form::Nrpn, Form::Press, Form::SysEx];

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.form_token"))]
    fn token(self) -> &'static str {
        match self {
            Form::Cc => CC.get(),
            Form::Cc14 => CC14.get(),
            Form::Nrpn => NRPN.get(),
            Form::Press => PRESS.get(),
            Form::SysEx => SYSEX.get(),
        }
    }

    /// An input of this form, read `Jump`: its kinds' sample.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.form_sample"))]
    fn sample(self) -> Option<Input> {
        let jump = Takeover::Jump;
        Some(match self {
            Form::Cc => Input::Cc(LearnCc::new(1)?, CcKind::Absolute(jump)),
            Form::Cc14 => Input::Cc14(Cc14Msb::new(1)?, jump),
            Form::Nrpn => Input::Nrpn(NRPN0, NrpnKind::Absolute(jump)),
            Form::Press => Input::Pressure(jump),
            Form::SysEx => Input::SysEx(SxKey::new(Shape::new(0, false, false)?, 0)?),
        })
    }
}

impl Control {
    /// `CC<n>`, `CC14:<msb>`, `NRPN:<n>` or `PRESS`.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.map.control_parse")
    )]
    fn parse(w: &[u8]) -> Option<Control> {
        if w == PRESS.bytes() {
            return Some(Control::Press);
        }
        if let Some(n) = after(w, CC14.bytes()) {
            return Cc14Msb::new(u8::try_from(n).ok()?).map(Control::Cc14);
        }
        if let Some(n) = after(w, NRPN.bytes()) {
            return U14::new(u16::try_from(n).ok()?).map(Control::Nrpn);
        }
        let n = num(behind(w, CC.bytes())?)?;
        LearnCc::new(u8::try_from(n).ok()?).map(Control::Cc)
    }

    /// The control `i` is on; `None` for a SysEx message.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.control_of"))]
    fn of(i: Input) -> Option<Control> {
        match i {
            Input::Cc(c, _) => Some(Control::Cc(c)),
            Input::Cc14(m, _) => Some(Control::Cc14(m)),
            Input::Nrpn(n, _) => Some(Control::Nrpn(n)),
            Input::Pressure(_) => Some(Control::Press),
            Input::SysEx(_) => None,
        }
    }

    /// This control read as `k`; `None` where it can't be (INC-DEC on a CC,
    /// relative on a 14-bit pair or pressure): `Input` can't hold those.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.map.control_input")
    )]
    fn input(self, k: MapKind) -> Option<Input> {
        let take = match k {
            MapKind::Jump => Some(Takeover::Jump),
            MapKind::Pickup => Some(Takeover::Pickup),
            MapKind::Scale => Some(Takeover::Scale),
            MapKind::Twos
            | MapKind::Offset
            | MapKind::Signed
            | MapKind::Signed2
            | MapKind::IncDec
            | MapKind::Momentary
            | MapKind::Toggle => None,
        };
        let nrpn = match k.sample() {
            Input::Nrpn(_, n) => n,
            Input::Cc(..) | Input::Cc14(..) | Input::Pressure(_) | Input::SysEx(_) => return None,
        };
        match self {
            Control::Nrpn(n) => Some(Input::Nrpn(n, nrpn)),
            Control::Cc14(m) => take.map(|t| Input::Cc14(m, t)),
            Control::Press => take.map(Input::Pressure),
            Control::Cc(c) => Some(Input::Cc(
                c,
                match nrpn {
                    NrpnKind::Absolute(t) => CcKind::Absolute(t),
                    NrpnKind::Relative(e) => CcKind::Relative(e),
                    NrpnKind::Button(m) => CcKind::Button(m),
                    NrpnKind::IncDec => return None,
                },
            )),
        }
    }
}

impl InputTok {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.tok_parse"))]
    fn parse(w: &[u8]) -> Option<InputTok> {
        if w == SYSEX.bytes() {
            return Some(InputTok::SysEx);
        }
        Control::parse(w).map(InputTok::Control)
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.tok_of"))]
    fn of(i: Input) -> InputTok {
        match Control::of(i) {
            Some(c) => InputTok::Control(c),
            None => InputTok::SysEx,
        }
    }

    /// `CC74`, `CC14:2`, `NRPN:1234`, `PRESS`, `SYSEX`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.tok_write"))]
    fn write(self, w: &mut dyn Write) -> fmt::Result {
        match self {
            InputTok::Control(Control::Cc(c)) => {
                w.write_str(CC.get())?;
                uint(w, u32::from(c.get()))
            }
            InputTok::Control(Control::Cc14(m)) => {
                w.write_str(CC14.get())?;
                w.write_char(':')?;
                uint(w, u32::from(m.get()))
            }
            InputTok::Control(Control::Nrpn(n)) => {
                w.write_str(NRPN.get())?;
                w.write_char(':')?;
                uint(w, u32::from(n.get()))
            }
            InputTok::Control(Control::Press) => w.write_str(PRESS.get()),
            InputTok::SysEx => w.write_str(SYSEX.get()),
        }
    }
}

/// `w` past `head`: a loop, as a slice's `strip_prefix` is a core
/// instance bank 1 would hold.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.behind"))]
fn behind<'a>(w: &'a [u8], head: &[u8]) -> Option<&'a [u8]> {
    let rest = w.get(head.len()..)?;
    let mut i = 0;
    while i < head.len() {
        if w.get(i) != head.get(i) {
            return None;
        }
        i += 1;
    }
    Some(rest)
}

/// `i` read as `k`, its control (or SysEx message) kept: `None` where
/// `Input` can't hold it.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.with_kind"))]
fn with_kind(i: Input, k: MapKind) -> Option<Input> {
    match Control::of(i) {
        Some(c) => c.input(k),
        None => (k == MapKind::Jump).then_some(i),
    }
}

/// `w` past `head` and a `:`, as a number.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.after"))]
fn after(w: &[u8], head: &[u8]) -> Option<u32> {
    num(behind(behind(w, head)?, b":")?)
}

/// A decimal: `chimera_theory`'s, called here (an `and_then` of it is
/// an instance the tuning group's glob would take, S0-S3's).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.num"))]
fn num(w: &[u8]) -> Option<u32> {
    chimera_theory::tuning::uint(w)
}

/// A row's place in its list: below `MAP_LEN` for a Sound's, `MAP_ROWS`
/// for a controller map's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowIx(u8);

impl RowIx {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.row_index"))]
    pub(crate) const fn index(self) -> usize {
        self.0 as usize
    }
}

const _: () = assert!(MAP_ROWS <= u8::MAX as usize);

/// A row of the followed Part's: its Sound's own (`S<i>`) or its
/// controller map's (`M<i>`), `i` from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdRow {
    Sound(RowIx),
    Map(RowIx),
}

impl EdRow {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.row_parse"))]
    fn parse(w: &[u8]) -> Option<EdRow> {
        let (&head, n) = w.split_first()?;
        let i = usize::try_from(num(n)?).ok()?.checked_sub(1)?;
        // A Sound holds 16 rows, a controller map 90 (ADR 0323).
        let (row, most): (fn(RowIx) -> EdRow, usize) = match head {
            b'S' => (EdRow::Sound, MAP_LEN),
            b'M' => (EdRow::Map, MAP_ROWS),
            _ => return None,
        };
        Some(row(RowIx(
            u8::try_from(i).ok().filter(|&i| usize::from(i) < most)?,
        )))
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.row_write"))]
    fn write(self, w: &mut dyn Write) -> fmt::Result {
        let (head, RowIx(i)) = match self {
            EdRow::Sound(i) => ('S', i),
            EdRow::Map(i) => ('M', i),
        };
        w.write_char(head)?;
        uint(w, u32::from(i) + 1)
    }
}

/// A control's number, `ed map set <row> control`: 0..127, the row's
/// control deciding which it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtlNo(u8);

/// A span's place, 0..255 (`Span`'s).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place(u8);

#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.place"))]
fn place(w: &[u8]) -> Option<Place> {
    u8::try_from(num(w)?).ok().map(Place)
}

/// What `ed map set` changes: MIDI MAP's B (the control), C and D (the
/// span), E (the input's kind), F (the echo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapField {
    Control(CtlNo),
    Min(Place),
    Max(Place),
    Kind(MapKind),
    Echo(Feedback),
}

impl MapField {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.field_parse"))]
    fn parse(f: &[u8], v: &[u8]) -> Option<MapField> {
        if f == CONTROL.bytes() {
            let n = u8::try_from(num(v)?).ok().filter(|&n| n <= 127)?;
            Some(MapField::Control(CtlNo(n)))
        } else if f == MIN.bytes() {
            place(v).map(MapField::Min)
        } else if f == MAX.bytes() {
            place(v).map(MapField::Max)
        } else if f == INPUT.bytes() {
            MapKind::parse(v).map(MapField::Kind)
        } else if f == ECHO.bytes() {
            feedback(v).map(MapField::Echo)
        } else {
            None
        }
    }
}

#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.feedback"))]
fn feedback(w: &[u8]) -> Option<Feedback> {
    if w == ON.bytes() {
        Some(Feedback::Value)
    } else if w == OFF.bytes() {
        Some(Feedback::Off)
    } else {
        None
    }
}

/// KEEP MAP (ADR 0253), as `ed map keep` sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeepMap {
    On,
    Off,
}

/// One `ed map` command, typed at the boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapCmd {
    /// A Sound row, as learn writes it, its span given.
    Add {
        input: Input,
        target: EdAddr,
        span: Span,
    },
    Set {
        row: EdRow,
        field: MapField,
    },
    /// Witnessed by the row's input and target as `since` said them.
    Rm {
        row: EdRow,
        input: InputTok,
        target: EdAddr,
    },
    /// A map on the card, or none.
    Pick(Option<EdName>),
    Keep(KeepMap),
    /// The Part's rows into the picked map, or into map `name`.
    Save(Option<EdName>),
    Rename(EdName),
}

/// Why a line is no `ed map` command: each its own `ERR`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapRefusal {
    Usage,
    /// `add` with no input, a kind it can't read, no target or a bad span.
    Add,
    /// A name NAMING couldn't type.
    Name,
}

impl MapRefusal {
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.map.refusal_write")
    )]
    pub fn write(self, w: &mut dyn Write) -> fmt::Result {
        w.write_str(match self {
            MapRefusal::Usage => USAGE.get(),
            MapRefusal::Add => ADD_USAGE.get(),
            MapRefusal::Name => NAME_USAGE.get(),
        })
    }
}

impl MapCmd {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.parse"))]
    pub fn parse(mut words: Words<'_>) -> Result<MapCmd, MapRefusal> {
        let sub = words.next().ok_or(MapRefusal::Usage)?;
        let is = |name: &[u8]| sub.eq_ignore_ascii_case(name);
        let name = |w: Option<&[u8]>| -> Result<EdName, MapRefusal> {
            EdName::parse(w.ok_or(MapRefusal::Usage)?).ok_or(MapRefusal::Name)
        };
        let cmd = if is(ADD.bytes()) {
            return MapCmd::parse_add(words).ok_or(MapRefusal::Add);
        } else if is(SET.bytes()) {
            let row = words.next().and_then(EdRow::parse);
            let f = words.next();
            let v = words.next();
            match (row, f, v) {
                (Some(row), Some(f), Some(v)) => MapCmd::Set {
                    row,
                    field: MapField::parse(f, v).ok_or(MapRefusal::Usage)?,
                },
                (Some(_) | None, Some(_) | None, Some(_) | None) => {
                    return Err(MapRefusal::Usage);
                }
            }
        } else if is(RM.bytes()) {
            let row = words.next().and_then(EdRow::parse);
            let input = words.next().and_then(InputTok::parse);
            let target = words.next().and_then(EdAddr::parse);
            match (row, input, target) {
                (Some(row), Some(input), Some(target)) => MapCmd::Rm { row, input, target },
                (Some(_) | None, Some(_) | None, Some(_) | None) => {
                    return Err(MapRefusal::Usage);
                }
            }
        } else if is(PICK.bytes()) {
            match words.next() {
                Some(w) if w == NONE.bytes() => MapCmd::Pick(None),
                w => MapCmd::Pick(Some(name(w)?)),
            }
        } else if is(KEEP.bytes()) {
            match feedback(words.next().ok_or(MapRefusal::Usage)?) {
                Some(Feedback::Value) => MapCmd::Keep(KeepMap::On),
                Some(Feedback::Off) => MapCmd::Keep(KeepMap::Off),
                None => return Err(MapRefusal::Usage),
            }
        } else if is(SAVE.bytes()) {
            match words.next() {
                None => MapCmd::Save(None),
                w => MapCmd::Save(Some(name(w)?)),
            }
        } else if is(RENAME.bytes()) {
            MapCmd::Rename(name(words.next())?)
        } else {
            return Err(MapRefusal::Usage);
        };
        match words.next() {
            None => Ok(cmd),
            Some(_) => Err(MapRefusal::Usage),
        }
    }

    /// `<input> <kind> <addr> [<min> <max>]`: the input and kind one typed
    /// `Input`, so an illegal pair is no command.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.parse_add"))]
    fn parse_add(mut words: Words<'_>) -> Option<MapCmd> {
        let c = Control::parse(words.next()?)?;
        let input = c.input(MapKind::parse(words.next()?)?)?;
        let target = EdAddr::parse(words.next()?)?;
        let span = match (words.next(), words.next(), words.next()) {
            (None, None, None) => Span::FULL,
            (Some(lo), Some(hi), None) => Span {
                min: place(lo)?.0,
                max: place(hi)?.0,
            },
            (Some(_) | None, Some(_) | None, Some(_) | None) => return None,
        };
        Some(MapCmd::Add {
            input,
            target,
            span,
        })
    }
}

/// `ed learn <addr>` or `ed learn off`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LearnCmd {
    At(EdAddr),
    Off,
}

impl LearnCmd {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.learn_parse"))]
    pub fn parse(mut words: Words<'_>) -> Option<LearnCmd> {
        let w = words.next()?;
        let c = match w == OFF.bytes() {
            true => LearnCmd::Off,
            false => LearnCmd::At(EdAddr::parse(w)?),
        };
        words.next().is_none().then_some(c)
    }
}

/// A row as `since` names it, its input's control and its target: what a
/// queued set or removal is taken against. A row changed under it (a
/// learn, the hand) is another row, and the act writes nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Witness(u32);

#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.witness"))]
fn witness(b: &Binding) -> Witness {
    let mut h = Fold::NEW;
    h.control(b.input);
    h.target(b.target);
    Witness(h.0)
}

/// FNV-1a 32 over a row's fields, bank 2's own: hashing the lines would
/// pass a hasher as a `dyn Write`, whose vtable is bank 1's.
struct Fold(u32);

impl Fold {
    const NEW: Fold = Fold(0x811C_9DC5);

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.fold_byte"))]
    fn byte(&mut self, b: u8) {
        self.0 = (self.0 ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.fold_bytes"))]
    fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.byte(x);
        }
    }

    /// The input's control (its form and number, or a SysEx message's
    /// key), never its kind.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.map.fold_control")
    )]
    fn control(&mut self, i: Input) {
        let (tag, n) = match i {
            Input::Cc(c, _) => (0, u32::from(c.get())),
            Input::Cc14(m, _) => (1, u32::from(m.get())),
            Input::Nrpn(n, _) => (2, u32::from(n.get())),
            Input::Pressure(_) => (3, 0),
            Input::SysEx(k) => (4, k.key()),
        };
        self.byte(tag);
        self.bytes(&n.to_le_bytes());
    }

    /// The target: its block's place in `BlockRef::ALL` and its param.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.fold_target"))]
    fn target(&mut self, t: Mappable) {
        let a = t.addr();
        let mut i = 0;
        while let Some(&b) = BlockRef::ALL.get(i) {
            if b == a.block {
                break;
            }
            i += 1;
        }
        self.byte(i as u8);
        self.byte(a.param.0);
    }

    /// The whole row: control, kind, target, span, echo.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.fold_row"))]
    fn row(&mut self, b: &Binding) {
        self.control(b.input);
        self.byte(MapKind::of(b.input) as u8);
        self.target(b.target);
        self.bytes(&[b.span.min, b.span.max, b.feedback as u8]);
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.fold_name"))]
    fn name(&mut self, n: Option<SoundName>) {
        match n {
            Some(n) => self.bytes(&n.padded()),
            None => self.byte(0xFF),
        }
    }
}

/// What MAP's `map`, `row` and `mapfile` lines read, hashed: the Part, its
/// map's name, KEEP MAP, the engine (which rows are live), its rows and
/// the controller map's, the card's maps. A move answers MAP whole.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.rows_hash"))]
pub(super) fn rows_hash(ui: &UiState) -> u32 {
    let p = ui.active_part;
    let mut h = Fold::NEW;
    h.byte(p.number());
    h.name(ui.project().controller(p));
    h.byte(ed_cue::keeps(ui, p) as u8);
    h.byte(ui.project().part(p).sound.engine() as u8);
    let mine = own(ui, p);
    h.byte(mine.len() as u8);
    for b in mine.entries() {
        h.row(b);
    }
    for b in ed_cue::layer(ui, p).rows() {
        h.row(&b);
    }
    match ed_cue::maps(ui) {
        Some(l) => {
            for &n in l {
                h.name(n);
            }
        }
        None => h.byte(0xFE),
    }
    h.0
}

/// What `learn`'s line reads, hashed: the state, its Part and row, the
/// cell armed on, the band's words.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.learn_hash"))]
pub(super) fn learn_hash(ui: &UiState) -> u32 {
    let mut h = Fold::NEW;
    match ui.learn() {
        Learn::Off => return h.0,
        Learn::Armed { part, .. } => {
            h.bytes(&[1, part.number()]);
            if let Some(t) = ed_cue::learn_target(ui) {
                h.target(t);
            }
        }
        Learn::Sensing { part, entry } => h.bytes(&[2, part.number(), entry]),
        Learn::Pointing(_) => h.byte(3),
    }
    if let Some(t) = ed_cue::learn_note(ui) {
        h.bytes(t.as_str().as_bytes());
    }
    h.0
}

/// What a queued `ed map`, `ed maps` or `ed learn` does in the UI pass.
#[derive(Clone, Copy)]
pub(super) enum MapAct {
    Add {
        part: PartId,
        row: Binding,
    },
    Set {
        part: PartId,
        row: EdRow,
        was: Witness,
        field: MapField,
    },
    Rm {
        part: PartId,
        row: EdRow,
        was: Witness,
    },
    Pick {
        part: PartId,
        to: Option<SoundName>,
    },
    Keep {
        part: PartId,
        to: KeepMap,
    },
    /// Into the map picked when the pass runs, or into this one.
    Save {
        part: PartId,
        name: Option<SoundName>,
    },
    /// The map the answer checked, marked `was`, renamed: the Part on
    /// another by the card job, nothing is renamed (`CtlJob::RenameOf`).
    Rename {
        part: PartId,
        was: MapMark,
        name: SoundName,
    },
    /// `ed maps`.
    List,
    /// `ed learn`: armed on this cell of this Part, or ended. The Part is
    /// the one the answer checked it for: a box on another by the pass
    /// arms nothing.
    Learn(Option<(PartId, EdAddr)>),
}

/// Why the box wouldn't take an `ed map` now: each its own `ERR`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NotNow {
    NoRow(EdRow),
    NotThatRow(EdRow),
    Unbindable(EdAddr),
    Full,
    Fixed(EdRow),
    NoControl(EdRow, CtlNo),
    NoKind(EdRow, MapKind),
    /// A SysEx row: nothing can send it back.
    NoEcho(EdRow),
    Busy,
    NotListed,
    NoPick,
}

impl NotNow {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.not_now"))]
    pub(super) fn write(self, w: &mut dyn Write) -> fmt::Result {
        match self {
            NotNow::NoRow(r) => {
                w.write_str(HAS_NO.get())?;
                r.write(w)?;
                w.write_str(super::lit::HERE.get())
            }
            NotNow::NotThatRow(r) => {
                w.write_str(RM_HEAD.get())?;
                r.write(w)?;
                w.write_str(NOT_THAT.get())
            }
            NotNow::Unbindable(a) => {
                w.write_str(CANT_BIND.get())?;
                a.write(w)?;
                w.write_char('\n')
            }
            NotNow::Full => w.write_str(FULL.get()),
            NotNow::Fixed(r) => {
                w.write_str(SET_HEAD.get())?;
                r.write(w)?;
                w.write_str(FIXED.get())
            }
            NotNow::NoControl(r, CtlNo(n)) => {
                w.write_str(SET_HEAD.get())?;
                r.write(w)?;
                w.write_str(NO_CONTROL.get())?;
                uint(w, u32::from(n))?;
                w.write_char('\n')
            }
            NotNow::NoKind(r, k) => {
                w.write_str(SET_HEAD.get())?;
                r.write(w)?;
                w.write_str(NO_KIND.get())?;
                k.write(w)?;
                w.write_char('\n')
            }
            NotNow::NoEcho(r) => {
                w.write_str(SET_HEAD.get())?;
                r.write(w)?;
                w.write_str(NO_ECHO.get())
            }
            NotNow::Busy => w.write_str(BUSY.get()),
            NotNow::NotListed => w.write_str(NOT_LISTED.get()),
            NotNow::NoPick => w.write_str(NO_PICK.get()),
        }
    }
}

/// The followed Part's row `r` now.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.row_at"))]
fn row_at(ui: &UiState, p: PartId, r: EdRow) -> Option<Binding> {
    match r {
        EdRow::Sound(RowIx(i)) => own(ui, p).entries().get(usize::from(i)).copied(),
        EdRow::Map(RowIx(i)) => ed_cue::layer(ui, p).get(usize::from(i)),
    }
}

#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.own"))]
fn own(ui: &UiState, p: PartId) -> MidiMap {
    ui.project().part(p).sound.map
}

/// `b` with `f` written, as B to F would leave it, or why not: B steps
/// only a CC or a 14-bit pair, E only along the control's legal kinds,
/// and nothing echoes to a SysEx row.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.edited"))]
fn edited(b: Binding, f: MapField, r: EdRow) -> Result<Binding, NotNow> {
    let input = match f {
        MapField::Control(n @ CtlNo(c)) => match b.input {
            Input::Cc(_, k) => Input::Cc(LearnCc::new(c).ok_or(NotNow::NoControl(r, n))?, k),
            Input::Cc14(_, t) => Input::Cc14(Cc14Msb::new(c).ok_or(NotNow::NoControl(r, n))?, t),
            Input::Nrpn(..) | Input::Pressure(_) | Input::SysEx(_) => {
                return Err(NotNow::Fixed(r));
            }
        },
        MapField::Kind(k) => with_kind(b.input, k).ok_or(NotNow::NoKind(r, k))?,
        MapField::Min(_) | MapField::Max(_) | MapField::Echo(_) => b.input,
    };
    let span = match f {
        MapField::Min(Place(m)) => Span { min: m, ..b.span },
        MapField::Max(Place(m)) => Span { max: m, ..b.span },
        MapField::Control(_) | MapField::Kind(_) | MapField::Echo(_) => b.span,
    };
    let feedback = match (f, b.input) {
        (MapField::Echo(Feedback::Value), Input::SysEx(_)) => return Err(NotNow::NoEcho(r)),
        (MapField::Echo(e), _) => e,
        (MapField::Control(_) | MapField::Kind(_) | MapField::Min(_) | MapField::Max(_), _) => {
            b.feedback
        }
    };
    Ok(Binding {
        input,
        span,
        feedback,
        target: b.target,
    })
}

/// `t` as the followed Part `p`'s Sound binds it: `PART<n>.LEVEL` for the
/// Part's own, the Sound's or a bus's or a ring's own address else.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.addr_of"))]
fn addr_of(t: Mappable, p: PartId) -> Option<EdAddr> {
    let a = t.addr();
    match a.block {
        BlockRef::Part => EdAddr::new(EdBlock::Part(p, PartBlock::Mix), a.param),
        BlockRef::Modal
        | BlockRef::TapeEngine
        | BlockRef::Va
        | BlockRef::Algo
        | BlockRef::AlgoOp(_)
        | BlockRef::Filter
        | BlockRef::VoiceFx
        | BlockRef::Env(_)
        | BlockRef::Lfo(_)
        | BlockRef::Out
        | BlockRef::Pitch
        | BlockRef::Chorus
        | BlockRef::Delay
        | BlockRef::Reverb
        | BlockRef::Tape
        | BlockRef::Comp
        | BlockRef::Theme
        | BlockRef::Midi
        | BlockRef::PartMix(_)
        | BlockRef::OrbitChords
        | BlockRef::OrbitRing(_)
        | BlockRef::Tempo
        | BlockRef::Random
        | BlockRef::Pad
        | BlockRef::Kit
        | BlockRef::PadFilter
        | BlockRef::PadAmp
        | BlockRef::PadEnv
        | BlockRef::PadLfo
        | BlockRef::PadRoute(_) => EdAddr::shown(a, Round::Shared),
    }
}

/// `addr_of`'s token, `-` for a target protocol 4 can't name.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.target"))]
fn target(t: Mappable, p: PartId, w: &mut dyn Write) -> fmt::Result {
    match addr_of(t, p) {
        Some(a) => a.write(w),
        None => w.write_char('-'),
    }
}

/// What a Sound row on Part `p` binds `a` to: a knob a control may drive
/// (`Mappable`), and a Part's own only for Part `p`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.bindable"))]
fn bindable(a: EdAddr, p: PartId) -> Option<Mappable> {
    match a.at() {
        EdBlock::Part(n, _) if n != p => None,
        EdBlock::Part(..)
        | EdBlock::Sound(_)
        | EdBlock::Pad(..)
        | EdBlock::Bus(_)
        | EdBlock::Global(_)
        | EdBlock::Orbit(_) => Mappable::new(a.param()),
    }
}

/// A map job (a save, a rename, a delete) waits for the card. A row's
/// edit or drop is never held up: it is the shared map's at once (ADR
/// 0323).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.busy"))]
fn busy(ui: &UiState) -> bool {
    ed_cue::ctl_job(ui).is_some()
}

/// What `c` would do on the followed Part now, or why the box wouldn't.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.check"))]
pub(super) fn check(c: MapCmd, ui: &UiState) -> Result<MapAct, NotNow> {
    let p = ui.active_part;
    match c {
        MapCmd::Add {
            input,
            target,
            span,
        } => {
            let t = bindable(target, p).ok_or(NotNow::Unbindable(target))?;
            let m = own(ui, p);
            if m.len() >= MAP_LEN && !m.entries().iter().any(|e| e.target == t) {
                return Err(NotNow::Full);
            }
            let row = Binding {
                input,
                target: t,
                span,
                feedback: Feedback::Off,
            };
            Ok(MapAct::Add { part: p, row })
        }
        MapCmd::Set { row, field } => {
            let b = row_at(ui, p, row).ok_or(NotNow::NoRow(row))?;
            edited(b, field, row)?;
            Ok(MapAct::Set {
                part: p,
                row,
                was: witness(&b),
                field,
            })
        }
        MapCmd::Rm { row, input, target } => {
            let b = row_at(ui, p, row).ok_or(NotNow::NoRow(row))?;
            if InputTok::of(b.input) != input || addr_of(b.target, p) != Some(target) {
                return Err(NotNow::NotThatRow(row));
            }
            Ok(MapAct::Rm {
                part: p,
                row,
                was: witness(&b),
            })
        }
        MapCmd::Pick(None) => Ok(MapAct::Pick { part: p, to: None }),
        MapCmd::Pick(Some(n)) => {
            let n = n.get();
            match listed(ui, n) {
                true => Ok(MapAct::Pick {
                    part: p,
                    to: Some(n),
                }),
                false => Err(NotNow::NotListed),
            }
        }
        MapCmd::Keep(to) => Ok(MapAct::Keep { part: p, to }),
        MapCmd::Save(name) => {
            if name.is_none() && ui.project().controller(p).is_none() {
                return Err(NotNow::NoPick);
            }
            if busy(ui) {
                return Err(NotNow::Busy);
            }
            Ok(MapAct::Save {
                part: p,
                name: name.map(EdName::get),
            })
        }
        MapCmd::Rename(n) => {
            let Some(was) = ui.project().controller(p) else {
                return Err(NotNow::NoPick);
            };
            if busy(ui) {
                return Err(NotNow::Busy);
            }
            Ok(MapAct::Rename {
                part: p,
                was: MapMark::of(was),
                name: n.get(),
            })
        }
    }
}

/// Map `n` is among the card's maps as last listed.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.listed"))]
fn listed(ui: &UiState, n: SoundName) -> bool {
    // A loop: a slice's `contains` is a core instance bank 1 would hold.
    let l = ed_cue::maps(ui).unwrap_or(&[]);
    let mut i = 0;
    while let Some(&x) = l.get(i) {
        if same_name(x, Some(n)) {
            return true;
        }
        i += 1;
    }
    false
}

/// `a` and `b` the same map, or both none: by their bytes (`Option`'s
/// `eq` would be an instance bank 1 holds).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.same_name"))]
fn same_name(a: Option<SoundName>, b: Option<SoundName>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.padded() == b.padded(),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

/// Why `ed learn` can't arm: each its own `ERR`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LearnNot {
    NotHere(EdAddr),
    /// ORBIT's or SETTINGS': learn arms on a Part's pages (ADR 0139).
    NoPage(EdAddr),
    NotMappable(EdAddr),
    /// MIDI MAP's pointing: MIX + SEQ does nothing then (ADR 0288).
    Pointing,
}

impl LearnNot {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.learn_not"))]
    pub(super) fn write(self, w: &mut dyn Write) -> fmt::Result {
        let (a, tail) = match self {
            LearnNot::Pointing => return w.write_str(LEARN_POINTING.get()),
            LearnNot::NotHere(a) => {
                w.write_str(LEARN_HAS_NO.get())?;
                a.write(w)?;
                return w.write_str(super::lit::HERE.get());
            }
            LearnNot::NoPage(a) => (a, NO_PAGE.get()),
            LearnNot::NotMappable(a) => (a, NOT_MAPPABLE.get()),
        };
        w.write_str(LEARN_HEAD.get())?;
        a.write(w)?;
        w.write_str(tail)
    }
}

/// `ed learn`'s act, or why not: `listed` says whether `describe` lists
/// the address for the followed Part.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.learn_check"))]
pub(super) fn learn_check(c: LearnCmd, ui: &UiState, listed: bool) -> Result<MapAct, LearnNot> {
    if let Learn::Pointing(_) = ui.learn() {
        return Err(LearnNot::Pointing);
    }
    let a = match c {
        LearnCmd::Off => return Ok(MapAct::Learn(None)),
        LearnCmd::At(a) => a,
    };
    match a.at() {
        EdBlock::Global(_) | EdBlock::Orbit(_) => return Err(LearnNot::NoPage(a)),
        EdBlock::Sound(_) | EdBlock::Pad(..) | EdBlock::Part(..) | EdBlock::Bus(_) => {}
    }
    if !listed {
        return Err(LearnNot::NotHere(a));
    }
    if Mappable::new(a.param()).is_none() {
        return Err(LearnNot::NotMappable(a));
    }
    match learn_cell(ui, a) {
        Some(c) => Ok(MapAct::Learn(Some((c.part, a)))),
        None => Err(LearnNot::NoPage(a)),
    }
}

/// Where learn arms for `a`: its Part, its page's chain (the Part's own,
/// or its mixer's) and the page's def and slot showing it; `None` for a
/// cell no Part page shows (SETTINGS', a pad's).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.learn_cell"))]
fn learn_cell(ui: &UiState, a: EdAddr) -> Option<LearnAt> {
    let (p, on) = match a.at() {
        EdBlock::Sound(_) => (ui.active_part, LearnOn::Own),
        EdBlock::Part(n, _) => (n, LearnOn::Mixer),
        EdBlock::Bus(_) => (ui.active_part, LearnOn::Mixer),
        EdBlock::Pad(..) | EdBlock::Global(_) | EdBlock::Orbit(_) => return None,
    };
    let (def, slot, _) = super::value::find(ui, &ui.cell_own(), a)?;
    Some(LearnAt {
        part: p,
        on,
        def,
        slot: CellSlot::new(slot)?,
    })
}

/// `a` in the UI pass, through MIDI MAP's own writes: it waits (a listing
/// behind a map job, learn behind a load), else is done. A row changed
/// since it was taken writes nothing.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.apply"))]
pub(super) fn apply(ui: &mut UiState, a: MapAct) -> Pass {
    match a {
        MapAct::Add { part, row } => {
            let mut m = own(ui, part);
            if m.learn(row.input, row.target).is_ok() {
                let mut i = 0;
                while i < m.len() {
                    if m.entries()[i].target == row.target {
                        m.set(i, row);
                    }
                    i += 1;
                }
                ed_cue::set_own(ui, part, m);
            }
        }
        MapAct::Set {
            part,
            row,
            was,
            field,
        } => {
            if let Some(b) = row_at(ui, part, row).filter(|b| witness(b) == was)
                && let Ok(to) = edited(b, field, row)
            {
                match row {
                    EdRow::Sound(RowIx(i)) => {
                        let mut m = own(ui, part);
                        m.set(usize::from(i), to);
                        ed_cue::set_own(ui, part, m);
                    }
                    EdRow::Map(i) => {
                        ed_cue::layer_edit(ui, part, i, b, to);
                    }
                }
            }
        }
        MapAct::Rm { part, row, was } => {
            if let Some(b) = row_at(ui, part, row).filter(|b| witness(b) == was) {
                match row {
                    EdRow::Sound(RowIx(i)) => {
                        ed_cue::set_own(ui, part, without(&own(ui, part), usize::from(i)));
                    }
                    EdRow::Map(_) => ed_cue::ctl_ask(ui, CtlJob::Drop(part, b)),
                }
            }
        }
        MapAct::Pick { part, to } => {
            let ok = match to {
                Some(n) => listed(ui, n),
                None => true,
            };
            if ok && !same_name(ui.project().controller(part), to) {
                ui.project_mut().set_controller(part, to);
            }
        }
        MapAct::Keep { part, to } => ed_cue::keep(ui, part, to),
        MapAct::Save { part, name } => {
            if let Some(n) = name.or(ui.project().controller(part)) {
                ed_cue::ctl_ask(ui, CtlJob::Save(part, n));
            }
        }
        MapAct::Rename { part, was, name } => {
            ed_cue::ctl_ask(ui, CtlJob::RenameOf(part, name, was));
        }
        MapAct::List => return ed_cue::ask_maps(ui),
        MapAct::Learn(None) => return ed_cue::learn(ui, None),
        MapAct::Learn(Some((part, a))) => {
            let cell = learn_cell(ui, a);
            return match cell {
                Some(c) if c.part == part => ed_cue::learn(ui, Some(c)),
                Some(_) | None => Pass::Done,
            };
        }
    }
    Pass::Done
}

/// `m` without row `i`, learn order kept: DELETE's drop (`MidiMap::remove`),
/// rebuilt here by learn's writes, as `remove`'s `copy_within` would be an
/// instance bank 1 holds.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.without"))]
fn without(m: &MidiMap, i: usize) -> MidiMap {
    let mut out = core::mem::MaybeUninit::<MidiMap>::uninit();
    // SAFETY: `empty_at` writes the whole map at `out`'s place.
    let mut out = unsafe {
        MidiMap::empty_at(out.as_mut_ptr());
        out.assume_init()
    };
    for (j, e) in m.entries().iter().enumerate() {
        if j != i && out.learn(e.input, e.target).is_ok() {
            out.set(out.len() - 1, *e);
        }
    }
    out
}

/// MAP's header and rows: `map <n> <"<name>"|none> keep <on|off>`, the
/// Sound's rows, then the controller map's.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.rows"))]
pub(super) fn rows(ui: &UiState, w: &mut dyn Write) -> fmt::Result {
    let p = ui.active_part;
    w.write_str(MAP_HEAD.get())?;
    uint(w, u32::from(p.number()))?;
    match ui.project().controller(p) {
        Some(n) => {
            w.write_str(QUOTE.get())?;
            text(w, n.as_str())?;
            w.write_char('"')?;
        }
        None => w.write_str(NONE_SP.get())?,
    }
    w.write_str(KEEP_HEAD.get())?;
    w.write_str(match ed_cue::keeps(ui, p) {
        true => ON.get(),
        false => OFF.get(),
    })?;
    w.write_char('\n')?;
    let engine = ui.project().part(p).sound.engine();
    let mine = own(ui, p);
    let layer = ed_cue::layer(ui, p);
    for (i, b) in mine.entries().iter().enumerate() {
        let st = match b.target.live(engine) {
            true => Status::Live,
            false => Status::Dead,
        };
        row(EdRow::Sound(RowIx(i as u8)), b, p, st, w)?;
    }
    for (i, b) in layer.rows().enumerate() {
        let b = &b;
        let st = match (b.target.live(engine), shadowed(&mine, b)) {
            (false, _) => Status::Dead,
            (true, true) => Status::Shadowed,
            (true, false) => Status::Live,
        };
        row(EdRow::Map(RowIx(i as u8)), b, p, st, w)?;
    }
    Ok(())
}

/// A row as the Part plays it: its engine reads it, a Sound row holds its
/// control (a map row's, ADR 0226), or its engine doesn't read it.
#[derive(Clone, Copy)]
enum Status {
    Live,
    Shadowed,
    Dead,
}

/// `row <S<i>|M<i>> <input> <kind> <addr> <min> <max> <on|off> <status>`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.row"))]
fn row(r: EdRow, b: &Binding, p: PartId, st: Status, w: &mut dyn Write) -> fmt::Result {
    w.write_str(ROW.get())?;
    r.write(w)?;
    w.write_char(' ')?;
    InputTok::of(b.input).write(w)?;
    w.write_char(' ')?;
    MapKind::of(b.input).write(w)?;
    w.write_char(' ')?;
    target(b.target, p, w)?;
    w.write_char(' ')?;
    uint(w, u32::from(b.span.min))?;
    w.write_char(' ')?;
    uint(w, u32::from(b.span.max))?;
    w.write_char(' ')?;
    w.write_str(match b.feedback {
        Feedback::Value => ON.get(),
        Feedback::Off => OFF.get(),
    })?;
    w.write_str(match st {
        Status::Live => LIVE.get(),
        Status::Shadowed => SHADOWED.get(),
        Status::Dead => DEAD.get(),
    })
}

/// `learn <off|armed <addr>|sensing S<i>|pointing> "<note>"`: the box's
/// learn, and its focus band's words.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.learn_line"))]
pub(super) fn learn_line(ui: &UiState, w: &mut dyn Write) -> fmt::Result {
    w.write_str(LEARN_LINE.get())?;
    match ui.learn() {
        Learn::Off => {
            w.write_str(OFF.get())?;
            return quoted(w, "");
        }
        Learn::Armed { part, .. } => {
            w.write_str(ARMED.get())?;
            match ed_cue::learn_target(ui) {
                Some(t) => target(t, part, w)?,
                None => w.write_char('-')?,
            }
        }
        Learn::Sensing { entry, .. } => {
            w.write_str(SENSING.get())?;
            EdRow::Sound(RowIx(entry)).write(w)?;
        }
        Learn::Pointing(_) => w.write_str(POINTING.get())?,
    }
    match ed_cue::learn_note(ui) {
        Some(t) => quoted(w, t.as_str()),
        None => quoted(w, ""),
    }
}

/// `mapfile "<name>"`, each of the card's maps as last listed.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.files"))]
pub(super) fn files(ui: &UiState, w: &mut dyn Write) -> fmt::Result {
    for n in ed_cue::maps(ui).unwrap_or(&[]).iter().flatten() {
        w.write_str(MAPFILE.get())?;
        quoted(w, n.as_str())?;
    }
    Ok(())
}

/// `ed describe MAP 0`: the binding grammar from the tables. Each input
/// form with its kinds in E's cycle (`Input::kinds`), each kind's words,
/// the CCs a binding may take (`LearnCc`), a 14-bit pair's MSBs, the
/// span's places and the rows each list holds.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.map.describe"))]
pub(super) fn describe(w: &mut dyn Write) -> fmt::Result {
    w.write_str(MAP_PAGE.get())?;
    for f in Form::ALL {
        let Some(i) = f.sample() else {
            continue;
        };
        w.write_str(INPUT_HEAD.get())?;
        w.write_str(f.token())?;
        // `Input::kinds`' cycle: the kinds `Input` can hold on this
        // control, in `MapKind::ALL`'s order, which is E's (a test holds
        // the two equal; `kinds` itself would be an instance bank 1 holds).
        for k in MapKind::ALL {
            if with_kind(i, k).is_some() {
                w.write_char(' ')?;
                k.write(w)?;
            }
        }
        w.write_char('\n')?;
    }
    for k in MapKind::ALL {
        let t = words(k.sample());
        w.write_str(KIND_HEAD.get())?;
        k.write(w)?;
        w.write_str(QUOTE.get())?;
        text(w, t.head.as_str())?;
        w.write_str(QUOTE_SHUT.get())?;
        w.write_char('"')?;
        text(w, t.tail.as_str())?;
        w.write_str(UNQUOTE.get())?;
    }
    w.write_str(CCS.get())?;
    let mut n = 0u8;
    while n <= 127 {
        if LearnCc::new(n).is_some() {
            w.write_char(' ')?;
            uint(w, u32::from(n))?;
        }
        n += 1;
    }
    w.write_char('\n')?;
    w.write_str(MSBS.get())?;
    let mut n = 0u8;
    while n <= 127 {
        if Cc14Msb::new(n).is_some() {
            w.write_char(' ')?;
            uint(w, u32::from(n))?;
        }
        n += 1;
    }
    w.write_char('\n')?;
    w.write_str(SPAN.get())?;
    uint(w, u32::from(Span::FULL.min))?;
    w.write_char(' ')?;
    uint(w, u32::from(Span::FULL.max))?;
    w.write_char('\n')?;
    w.write_str(ROWS.get())?;
    uint(w, MAP_LEN as u32)?;
    w.write_char('\n')?;
    // The controller map's rows: a flat list (ADR 0323).
    w.write_str(MAPROWS.get())?;
    uint(w, MAP_ROWS as u32)?;
    w.write_char('\n')
}

lit!(CC = "CC");
lit!(CC14 = "CC14");
lit!(NRPN = "NRPN");
lit!(PRESS = "PRESS");
lit!(SYSEX = "SYSEX");
lit!(ADD = "add");
lit!(SET = "set");
lit!(RM = "rm");
lit!(PICK = "pick");
lit!(KEEP = "keep");
lit!(SAVE = "save");
lit!(RENAME = "rename");
lit!(NONE = "none");
lit!(ON = "on");
lit!(OFF = "off");
lit!(CONTROL = "control");
lit!(MIN = "min");
lit!(MAX = "max");
lit!(INPUT = "input");
lit!(ECHO = "echo");
lit!(MAP_PAGE = "page 0 MAP of 1\n");
lit!(INPUT_HEAD = "input ");
lit!(KIND_HEAD = "kind ");
lit!(CCS = "ccs");
lit!(MSBS = "msbs");
lit!(SPAN = "span ");
lit!(ROWS = "rows ");
lit!(MAPROWS = "maprows ");
lit!(
    /// MAP's header's head, whole.
    MAP_HEAD = "map "
);
lit!(NONE_SP = " none");
lit!(KEEP_HEAD = " keep ");
lit!(ROW = "row ");
lit!(LIVE = " live\n");
lit!(SHADOWED = " shadowed\n");
lit!(DEAD = " dead\n");
lit!(LEARN_LINE = "learn ");
lit!(ARMED = "armed ");
lit!(SENSING = "sensing ");
lit!(POINTING = "pointing");
lit!(MAPFILE = "mapfile");
lit!(
    pub(super) USAGE = "ERR ed map takes add, set <row> <control|min|max|input|echo> <value>, rm <row> <input> <addr>, pick <name|none>, keep <on|off>, save [<name>] or rename <name>\n"
);
lit!(
    ADD_USAGE = "ERR ed map add takes CC<n>, CC14:<msb>, NRPN:<n> or PRESS, a kind it reads, an address and maybe <min> <max> 0-255\n"
);
lit!(NAME_USAGE = "ERR ed map takes a name of A-Z a-z 0-9 - and _ for a space, 1-16\n");
lit!(HAS_NO = "ERR ed map has no ");
lit!(RM_HEAD = "ERR ed map rm ");
lit!(NOT_THAT = " is not that row: ed since\n");
lit!(CANT_BIND = "ERR ed map can't bind ");
lit!(FULL = "ERR ed map add MAP FULL\n");
lit!(SET_HEAD = "ERR ed map set ");
lit!(FIXED = " has a fixed control\n");
lit!(NO_CONTROL = " can't take control ");
lit!(NO_KIND = " can't read ");
lit!(NO_ECHO = " echoes nothing: SYSEX\n");
lit!(BUSY = "ERR ed map CARD BUSY: a map job waits\n");
lit!(NOT_LISTED = "ERR ed map pick has no such map: ed maps\n");
lit!(NO_PICK = "ERR ed map has no map picked: ed map save <name>\n");
lit!(
    pub(super) LEARN_USAGE = "ERR ed learn takes <addr> or off\n"
);
lit!(LEARN_HAS_NO = "ERR ed learn has no ");
lit!(LEARN_HEAD = "ERR ed learn ");
lit!(NO_PAGE = " is no Part page's cell\n");
lit!(NOT_MAPPABLE = " is not mappable\n");
lit!(LEARN_POINTING = "ERR ed learn: the box is pointing (MIDI MAP)\n");
