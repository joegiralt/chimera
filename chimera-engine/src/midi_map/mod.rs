//! A Sound's MIDI map (spec §§ 2-3): up to 16 CC -> param entries, a fixed
//! array with no heap. `Mappable` is the whitelist type, so an address a CC
//! may not drive (FX, ORBIT, settings) can never be stored.

pub mod codec;
pub mod ramp;

pub use crate::midi_in::Cc;
pub use chimera_midi::out::Cc14Msb;

use chimera_midi::ctl::{Ctl, FreeCc, Sel};
use chimera_midi::rel::ByteEncoding;
use chimera_midi::take::Takeover;
use chimera_midi::value::U14;

use crate::addr::{BlockRef, ParamAddr};
use crate::block::ParamId;
use crate::dsp::algo::params::AlgoParams;
use crate::dsp::lfo::LfoParams;
use crate::dsp::modal::ModalParams;
use crate::dsp::modulator::LfoSlot;
use crate::dsp::va::VaParams;
use crate::dsp::voice_fx::params::VoiceFxParams;
use crate::params::{EngineType, FilterParams, OutParams};
use crate::part::PartParams;

/// A `ParamAddr` a CC may drive: a Sound's own, stored, non-patching
/// params, and the Part's LEVEL and PAN (CC 7 and 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mappable(ParamAddr);

impl Mappable {
    pub const fn new(a: ParamAddr) -> Option<Self> {
        let ok = if matches!(a.block, BlockRef::Part) {
            a.param.0 == PartParams::LEVEL.0 || a.param.0 == PartParams::PAN.0
        } else if matches!(a.block, BlockRef::VoiceFx) && a.param.0 < VoiceFxParams::P1.0 {
            // TYPE and PLACE crossfade a stage in and out: a switch, not a knob.
            false
        } else if a.block.in_sound() {
            match a.spec() {
                Some(s) => !s.patching && s.stored,
                None => false,
            }
        } else {
            false
        };
        if ok { Some(Self(a)) } else { None }
    }

    pub const fn addr(self) -> ParamAddr {
        self.0
    }

    /// Whether a Part on `e` reads it: an engine's own block only on that
    /// engine. A dead entry is kept, and a CC to it does nothing (spec § 3).
    pub fn live(self, e: EngineType) -> bool {
        match self.0.block {
            BlockRef::Modal => e == EngineType::Modal,
            BlockRef::Va => e == EngineType::Va,
            BlockRef::Algo | BlockRef::AlgoOp(_) => e == EngineType::Algo,
            BlockRef::TapeEngine => e == EngineType::Tape,
            _ => true,
        }
    }
}

/// A CC a binding may be learned on: `FreeCc` already leaves out bank
/// select, data entry, sustain, RPN/NRPN and channel mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LearnCc(FreeCc);

impl LearnCc {
    pub const fn new(n: u8) -> Option<Self> {
        match FreeCc::new(n) {
            Some(c) => Some(Self(c)),
            None => None,
        }
    }

    pub const fn get(self) -> u8 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonMode {
    Momentary,
    Toggle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CcKind {
    Absolute(Takeover),
    Relative(ByteEncoding),
    Button(ButtonMode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NrpnKind {
    Absolute(Takeover),
    Relative(ByteEncoding),
    IncDec,
    Button(ButtonMode),
}

const TAKEOVERS: [Takeover; 3] = [Takeover::Jump, Takeover::Pickup, Takeover::Scale];

/// The control and how to read it, as one type: IncDec on a CC, or
/// relative on pressure, can't be written down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Cc(LearnCc, CcKind),
    Cc14(Cc14Msb, Takeover),
    Nrpn(U14, NrpnKind),
    Pressure(Takeover),
}

impl Input {
    /// CC `n` read as absolute with `Jump`: the default kind. `None` for a
    /// number a binding can't take.
    pub const fn cc(n: u8) -> Option<Self> {
        match LearnCc::new(n) {
            Some(c) => Some(Self::Cc(c, CcKind::Absolute(Takeover::Jump))),
            None => None,
        }
    }

    /// The legal kinds of this control, this control number kept: E's cycle
    /// (spec § 8.2). At most 10: three takeovers, four relatives, two
    /// buttons, IncDec.
    pub fn kinds(self) -> impl Iterator<Item = Input> {
        let mut out = [self; 10];
        let mut n = 0;
        let mut put = |i| {
            out[n] = i;
            n += 1;
        };
        match self {
            Self::Cc(c, _) => {
                for t in TAKEOVERS {
                    put(Self::Cc(c, CcKind::Absolute(t)));
                }
                for e in ByteEncoding::ALL {
                    put(Self::Cc(c, CcKind::Relative(e)));
                }
                put(Self::Cc(c, CcKind::Button(ButtonMode::Momentary)));
                put(Self::Cc(c, CcKind::Button(ButtonMode::Toggle)));
            }
            Self::Nrpn(a, _) => {
                for t in TAKEOVERS {
                    put(Self::Nrpn(a, NrpnKind::Absolute(t)));
                }
                for e in ByteEncoding::ALL {
                    put(Self::Nrpn(a, NrpnKind::Relative(e)));
                }
                put(Self::Nrpn(a, NrpnKind::IncDec));
                put(Self::Nrpn(a, NrpnKind::Button(ButtonMode::Momentary)));
                put(Self::Nrpn(a, NrpnKind::Button(ButtonMode::Toggle)));
            }
            Self::Cc14(m, _) => {
                for t in TAKEOVERS {
                    put(Self::Cc14(m, t));
                }
            }
            Self::Pressure(_) => {
                for t in TAKEOVERS {
                    put(Self::Pressure(t));
                }
            }
        }
        out.into_iter().take(n)
    }

    /// Whether `c` belongs to this control, whatever its kind. A 14-bit
    /// pair hears its MSB CC and the LSB CC, MSB + 32.
    pub fn heard(&self, c: &Ctl) -> bool {
        match (*self, c) {
            (Self::Cc(l, _), Ctl::Cc { cc, .. }) => cc.get() == l.get(),
            (Self::Cc14(m, _), Ctl::Cc { cc, .. }) => {
                cc.get() == m.get() || cc.get() == m.get() + 32
            }
            (Self::Nrpn(n, _), Ctl::Data { sel, .. } | Ctl::Step { sel, .. }) => {
                *sel == Sel::Nrpn(n)
            }
            (Self::Pressure(_), Ctl::Pressure(_)) => true,
            _ => false,
        }
    }

    /// The same control, whatever its kind.
    fn same_control(&self, o: &Input) -> bool {
        match (*self, *o) {
            (Self::Cc(a, _), Self::Cc(b, _)) => a == b,
            (Self::Cc14(a, _), Self::Cc14(b, _)) => a == b,
            (Self::Nrpn(a, _), Self::Nrpn(b, _)) => a == b,
            (Self::Pressure(_), Self::Pressure(_)) => true,
            _ => false,
        }
    }
}

/// What a binding sends back to the controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feedback {
    Off,
    Value,
}

/// `min` and `max` are places 0..=255 of the param's normalized span;
/// `min > max` inverts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub min: u8,
    pub max: u8,
}

impl Span {
    pub const FULL: Self = Self { min: 0, max: 255 };

    /// A CC value 0..=127 as a place 0..1 of the span.
    pub fn place(&self, v: u8) -> f32 {
        let (lo, hi) = (self.min as f32, self.max as f32);
        (lo + (hi - lo) * (v.min(127) as f32 / 127.0)) / 255.0
    }
}

/// One mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub input: Input,
    pub target: Mappable,
    pub span: Span,
    pub feedback: Feedback,
}

const _: () = assert!(core::mem::size_of::<Binding>() <= 12);

pub const MAP_LEN: usize = 16;

/// Sixteen mappings are held and one more was learned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapFull;

#[derive(Clone, Copy)]
pub struct MidiMap {
    entries: [Binding; MAP_LEN],
    len: u8,
}

impl core::fmt::Debug for MidiMap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.entries()).finish()
    }
}

/// Equal by the live entries: slots past `len` are scratch.
impl PartialEq for MidiMap {
    fn eq(&self, o: &Self) -> bool {
        self.entries() == o.entries()
    }
}

impl Eq for MidiMap {}

impl MidiMap {
    pub const EMPTY: Self = Self {
        entries: [Binding {
            input: Input::Pressure(Takeover::Jump),
            target: Mappable(ParamAddr::new(BlockRef::Filter, crate::block::ParamId(0))),
            span: Span::FULL,
            feedback: Feedback::Off,
        }; MAP_LEN],
        len: 0,
    };

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn entries(&self) -> &[Binding] {
        &self.entries[..self.len()]
    }

    /// Maps `input` to `t` over the full range, echo off: replaces the
    /// target's entry (a param appears once) or appends.
    pub fn learn(&mut self, input: Input, t: Mappable) -> Result<(), MapFull> {
        let e = Binding {
            input,
            target: t,
            span: Span::FULL,
            feedback: Feedback::Off,
        };
        let n = self.len();
        if let Some(i) = self.entries().iter().position(|x| x.target == t) {
            self.entries[i] = e;
        } else if n < MAP_LEN {
            self.entries[n] = e;
            self.len += 1;
        } else {
            return Err(MapFull);
        }
        Ok(())
    }

    /// Deletes entry `i`, keeping learn order. Out of range does nothing.
    pub fn remove(&mut self, i: usize) {
        let n = self.len();
        if i < n {
            self.entries.copy_within(i + 1..n, i);
            self.len -= 1;
        }
    }

    /// Replaces entry `i`. Out of range does nothing.
    pub fn set(&mut self, i: usize, e: Binding) {
        if i < self.len() {
            self.entries[i] = e;
        }
    }

    /// The entries on the same control as `input`, whatever their kind.
    pub fn targets_of<'a>(
        &'a self,
        input: &'a Input,
    ) -> impl Iterator<Item = (usize, &'a Binding)> + 'a {
        self.entries()
            .iter()
            .enumerate()
            .filter(move |(_, e)| e.input.same_control(input))
    }
}

const fn ent(cc: u8, b: BlockRef, p: ParamId) -> Binding {
    match (Input::cc(cc), Mappable::new(ParamAddr::new(b, p))) {
        (Some(input), Some(target)) => Binding {
            input,
            target,
            span: Span::FULL,
            feedback: Feedback::Off,
        },
        _ => panic!("a default map entry must be mappable"),
    }
}

/// Spec § 9: the seven shared entries in learn order, then CCs 16-19 for
/// ALGO, MODAL and VA (TAPE has none, ADR 0132).
static DEFAULTS: [Binding; 19] = {
    use BlockRef::{Algo, Filter, Lfo, Modal, Out, Part, Va, VoiceFx};
    [
        ent(74, Filter, FilterParams::CUTOFF),
        ent(71, Filter, FilterParams::RESONANCE),
        ent(7, Part, PartParams::LEVEL),
        ent(10, Part, PartParams::PAN),
        ent(91, Out, OutParams::SEND3),
        ent(93, Out, OutParams::SEND1),
        ent(1, Lfo(LfoSlot::Lfo1), LfoParams::DEPTH),
        ent(16, Algo, AlgoParams::MORPH),
        ent(17, Algo, AlgoParams::VOWEL),
        ent(18, VoiceFx, VoiceFxParams::P1),
        ent(19, VoiceFx, VoiceFxParams::P2),
        ent(16, Modal, ModalParams::STRUCTURE),
        ent(17, Modal, ModalParams::BRIGHT),
        ent(18, Modal, ModalParams::DAMP),
        ent(19, Modal, ModalParams::POS),
        ent(16, Va, VaParams::AMOUNT),
        ent(17, Va, VaParams::SHAPE1),
        ent(18, Va, VaParams::SHAPE2),
        ent(19, Va, VaParams::SUB),
    ]
};

/// The map INIT and the factory Sounds ship with (spec § 9).
#[inline(never)]
pub fn default_for(e: EngineType) -> MidiMap {
    let own = match e {
        EngineType::Algo => 7..11,
        EngineType::Modal => 11..15,
        EngineType::Va => 15..19,
        EngineType::Tape => 7..7,
    };
    let mut m = MidiMap::EMPTY;
    m.entries[..7].copy_from_slice(&DEFAULTS[..7]);
    m.entries[7..7 + own.len()].copy_from_slice(&DEFAULTS[own.clone()]);
    m.len = 7 + own.len() as u8;
    m
}
