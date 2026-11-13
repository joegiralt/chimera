//! A Sound's MIDI map (spec §§ 2-3): up to 16 CC -> param entries, a fixed
//! array with no heap. `Mappable` is the whitelist type, so an address a CC
//! may not drive (a switch, a setting) can never be stored.

pub mod builtin;
pub mod codec;
pub mod ctl;

use core::sync::atomic::{AtomicU8, Ordering};

/// Controller map files a console `put` or `rm` has changed, wrapping.
static MAPS_PUT: AtomicU8 = AtomicU8::new(0);

/// How many map files the console has changed: a move says every layer
/// reads the card again (ADR 0226).
pub fn maps_put() -> u8 {
    MAPS_PUT.load(Ordering::Relaxed)
}

/// The console changed a map file.
pub fn map_put() {
    MAPS_PUT.fetch_add(1, Ordering::Relaxed);
}

/// The file stem for `n`: up to four of its letters and digits, upper
/// case, then a 16-bit FNV-1a of the whole name as four hex digits. Names
/// that share one are told apart by the header: a load whose header names
/// another map is `Missing`. Bank 1's, not `ctl`'s: the console checks a
/// map `put` by it.
pub fn stem(n: &crate::name::SoundName) -> ([u8; 8], usize) {
    let mut out = [b'M'; 8];
    let mut len = 0;
    let mut h: u32 = 0x811C_9DC5;
    for c in n.as_str().bytes() {
        h = (h ^ u32::from(c)).wrapping_mul(0x0100_0193);
        if len < 4 && c.is_ascii_alphanumeric() {
            out[len] = c.to_ascii_uppercase();
            len += 1;
        }
    }
    let len = len.max(1);
    let h = (h ^ h >> 16) as u16;
    for k in 0..4 {
        let d = (h >> (12 - 4 * k) & 0xF) as u8;
        out[len + k] = if d < 10 { b'0' + d } else { b'A' + d - 10 };
    }
    (out, len + 4)
}

use codec::{Bounds, MapRecord};

pub use crate::midi_in::Cc;
pub use chimera_midi::out::Cc14Msb;

use chimera_midi::ctl::{Ctl, FreeCc, Sel};
use chimera_midi::rel::ByteEncoding;
use chimera_midi::sxmap::Shape;
use chimera_midi::take::Takeover;
use chimera_midi::value::U14;

use crate::addr::{BlockRef, ParamAddr};
use crate::block::{Motion, ParamId};
use crate::dsp::algo::params::AlgoParams;
use crate::dsp::modal::ModalParams;
use crate::dsp::va::VaParams;
use crate::dsp::voice_fx::params::VoiceFxParams;
use crate::orbit::scene::SceneRing;
use crate::params::{EngineType, FilterParams, OutParams};
use crate::part::PartParams;

/// A `ParamAddr` a CC may drive: a Sound's own, stored, non-patching
/// params, the Part's LEVEL and PAN (CC 7 and 10), the master FX's knobs
/// and ORBIT's ring amounts (#512). A switch that reshapes a bus or a ring
/// (a TYPE, STEPS, MUTE, a ring's choices) stays the hand's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mappable(ParamAddr);

impl Mappable {
    pub const fn new(a: ParamAddr) -> Option<Self> {
        let (p, Some(s)) = (a.param.0, a.spec()) else {
            return None;
        };
        let knob = s.stored && !s.patching;
        let ok = match a.block {
            BlockRef::Part => p == PartParams::LEVEL.0 || p == PartParams::PAN.0,
            // TYPE and PLACE crossfade a stage in and out: a switch, not a knob.
            BlockRef::VoiceFx => knob && p >= VoiceFxParams::P1.0,
            BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Tape
            | BlockRef::Comp => knob && matches!(s.motion(), Motion::Continuous(_)),
            // HITS to OCTAVE but MUTE.
            BlockRef::OrbitRing(_) => {
                p >= SceneRing::HITS.0 && p <= SceneRing::OCTAVE.0 && p != SceneRing::MUTE.0
            }
            b => knob && b.in_sound(),
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

    /// The engine whose own block it is; `None`: every engine reads it.
    pub fn engine(self) -> Option<EngineType> {
        match self.0.block {
            BlockRef::Modal => Some(EngineType::Modal),
            BlockRef::Va => Some(EngineType::Va),
            BlockRef::Algo | BlockRef::AlgoOp(_) => Some(EngineType::Algo),
            BlockRef::TapeEngine => Some(EngineType::Tape),
            _ => None,
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
    /// A SysEx parameter message an old box sends (#846, ADR 0241):
    /// absolute, `Jump`, never echoed.
    SysEx(SxKey),
}

/// A learned SysEx message: where its value sits and its fixed bytes'
/// key (`chimera_midi::sxmap`), as bytes, so `Input` stays 6 B. Without
/// `sysex-map` (a build that hears no map: the bench's) it holds nothing
/// and none is made, so `Binding` keeps its 10 B and D2 its room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SxKey(#[cfg(feature = "sysex-map")] [u8; 5]);

impl SxKey {
    /// `None` without `sysex-map`: the binding is not kept.
    pub const fn new(shape: Shape, key: u32) -> Option<Self> {
        #[cfg(feature = "sysex-map")]
        {
            let k = key.to_le_bytes();
            Some(Self([shape.bits(), k[0], k[1], k[2], k[3]]))
        }
        #[cfg(not(feature = "sysex-map"))]
        {
            let _ = (shape, key);
            None
        }
    }

    pub const fn shape(self) -> Option<Shape> {
        #[cfg(feature = "sysex-map")]
        return Shape::from_bits(self.0[0]);
        #[cfg(not(feature = "sysex-map"))]
        None
    }

    pub const fn key(self) -> u32 {
        #[cfg(feature = "sysex-map")]
        return u32::from_le_bytes([self.0[1], self.0[2], self.0[3], self.0[4]]);
        #[cfg(not(feature = "sysex-map"))]
        0
    }
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
            Self::SysEx(_) => put(self),
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

    /// The same control, whatever its kind: a 14-bit pair is its MSB CC
    /// and its LSB CC, MSB + 32. Inlined: its callers but one are bank
    /// 2's, so no copy is left in bank 1.
    #[inline(always)]
    pub fn same_control(&self, o: &Input) -> bool {
        match (*self, *o) {
            (Self::Cc(a, _), Self::Cc(b, _)) => a == b,
            (Self::Cc(c, _), Self::Cc14(m, _)) | (Self::Cc14(m, _), Self::Cc(c, _)) => {
                c.get() == m.get() || c.get() == m.get() + 32
            }
            (Self::Cc14(a, _), Self::Cc14(b, _)) => a == b,
            (Self::Nrpn(a, _), Self::Nrpn(b, _)) => a == b,
            (Self::Pressure(_), Self::Pressure(_)) => true,
            (Self::SysEx(a), Self::SysEx(b)) => a == b,
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

/// A Part's map record a closed door left unread: a project save writes it
/// back verbatim (#815). One per Part, on the `Project`: a Sound copy never
/// carries it.
pub struct KeptMap {
    which: Option<MapRecord>,
    len: u8,
    raw: [u8; KEPT_LEN],
}

/// A full `Bindings` payload: a version byte and 8 B an entry, 12 with
/// SysEx keys (`sysex-map`, #846).
pub const KEPT_LEN: usize = 1 + MAP_LEN * if cfg!(feature = "sysex-map") { 12 } else { 8 };
const _: () = assert!(KEPT_LEN <= u8::MAX as usize, "`KeptMap::len` is a byte");

impl KeptMap {
    pub const EMPTY: Self = Self {
        which: None,
        len: 0,
        raw: [0; KEPT_LEN],
    };

    pub fn get(&self) -> Option<(MapRecord, &[u8])> {
        Some((self.which?, &self.raw[..usize::from(self.len)]))
    }

    pub fn clear(&mut self) {
        self.which = None;
    }

    /// Keeps `r`'s payload `p`; `Bindings` wins over `Legacy`, as when
    /// read. Over a full record is `Bounds`, as open.
    pub fn keep(&mut self, r: MapRecord, p: &[u8]) -> Result<(), Bounds> {
        if p.len() > KEPT_LEN {
            return Err(Bounds);
        }
        if r == MapRecord::Legacy && self.which == Some(MapRecord::Bindings) {
            return Ok(());
        }
        self.raw[..p.len()].copy_from_slice(p);
        self.len = p.len() as u8;
        self.which = Some(r);
        Ok(())
    }
}

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

    /// `EMPTY` built at `p` by stores, no image of it copied: inlined into
    /// a bank-2 builder (ADR 0251).
    ///
    /// # Safety
    /// `p` is valid for a write of `MidiMap`.
    #[inline(always)]
    pub unsafe fn empty_at(p: *mut MidiMap) {
        let filler = Binding {
            input: Input::Pressure(Takeover::Jump),
            target: Mappable(ParamAddr::new(BlockRef::Filter, crate::block::ParamId(0))),
            span: Span::FULL,
            feedback: Feedback::Off,
        };
        // SAFETY: each field of the caller's `*p`, written whole.
        unsafe {
            let e = core::ptr::addr_of_mut!((*p).entries).cast::<Binding>();
            for i in 0..MAP_LEN {
                e.add(i).write(filler);
            }
            core::ptr::addr_of_mut!((*p).len).write(0);
        }
    }

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
    /// target's entry (a param appears once) or appends. Inlined: its
    /// callers (learn, the codec) are bank 2's.
    #[inline(always)]
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

    /// Replaces entry `i`. Out of range does nothing. Inlined: MIDI MAP's
    /// keys, its caller, are bank 2's.
    #[inline(always)]
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

/// Spec § 9: the six shared entries in learn order, then CCs 16-19 for
/// ALGO, MODAL and VA (TAPE has none, ADR 0132). CC 1 is the wheel, a
/// matrix source, so not here (#876).
static DEFAULTS: [Binding; 18] = {
    use BlockRef::{Algo, Filter, Modal, Out, Part, Va, VoiceFx};
    [
        ent(74, Filter, FilterParams::CUTOFF),
        ent(71, Filter, FilterParams::RESONANCE),
        ent(7, Part, PartParams::LEVEL),
        ent(10, Part, PartParams::PAN),
        ent(91, Out, OutParams::SEND3),
        ent(93, Out, OutParams::SEND1),
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

/// The default map with CC 1 → LFO 1 DEPTH where it sat before WHEEL was
/// a source: a factory Sound's whose LFO 1 is routed (ADR 0250).
pub fn wheel_map_for(e: EngineType) -> MidiMap {
    use crate::dsp::lfo::LfoParams;
    use crate::dsp::modulator::LfoSlot;
    let mut m = default_for(e);
    let n = usize::from(m.len);
    m.entries.copy_within(6..n, 7);
    m.entries[6] = ent(1, BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::DEPTH);
    m.len += 1;
    m
}

/// The map INIT and the factory Sounds ship with (spec § 9).
#[inline(never)]
pub fn default_for(e: EngineType) -> MidiMap {
    let own = match e {
        EngineType::Algo => 6..10,
        EngineType::Modal => 10..14,
        EngineType::Va => 14..18,
        // No engine section of their own (DRUM's knobs are its pads').
        EngineType::Tape | EngineType::Drum => 6..6,
    };
    let mut m = MidiMap::EMPTY;
    m.entries[..6].copy_from_slice(&DEFAULTS[..6]);
    m.entries[6..6 + own.len()].copy_from_slice(&DEFAULTS[own.clone()]);
    m.len = 6 + own.len() as u8;
    m
}
