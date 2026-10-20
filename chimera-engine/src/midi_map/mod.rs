//! A Sound's MIDI map (spec §§ 2-3): up to 16 CC -> param entries, a fixed
//! array with no heap. `Mappable` is the whitelist type, so an address a CC
//! may not drive (FX, ORBIT, settings) can never be stored.

pub mod ramp;

pub use crate::midi_in::Cc;

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

/// One mapping. `min` and `max` are places 0..=255 of the param's
/// normalized span; `min > max` inverts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapEntry {
    pub cc: Cc,
    pub target: Mappable,
    pub min: u8,
    pub max: u8,
}

impl MapEntry {
    /// A CC value 0..=127 as a place 0..1 of the span.
    pub fn place(&self, v: u8) -> f32 {
        let (lo, hi) = (self.min as f32, self.max as f32);
        (lo + (hi - lo) * (v.min(127) as f32 / 127.0)) / 255.0
    }
}

pub const MAP_LEN: usize = 16;

const CC0: Cc = match Cc::new(0) {
    Some(c) => c,
    None => panic!(),
};

/// Sixteen mappings are held and one more was learned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapFull;

#[derive(Clone, Copy)]
pub struct MidiMap {
    entries: [MapEntry; MAP_LEN],
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
        entries: [MapEntry {
            cc: CC0,
            target: Mappable(ParamAddr::new(BlockRef::Filter, crate::block::ParamId(0))),
            min: 0,
            max: 255,
        }; MAP_LEN],
        len: 0,
    };

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn entries(&self) -> &[MapEntry] {
        &self.entries[..self.len()]
    }

    /// Maps `cc` to `t` over the full range: replaces the target's entry
    /// (a param appears once) or appends.
    pub fn learn(&mut self, cc: Cc, t: Mappable) -> Result<(), MapFull> {
        let e = MapEntry {
            cc,
            target: t,
            min: 0,
            max: 255,
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
    pub fn set(&mut self, i: usize, e: MapEntry) {
        if i < self.len() {
            self.entries[i] = e;
        }
    }

    pub fn targets_of(&self, cc: Cc) -> impl Iterator<Item = (usize, &MapEntry)> {
        self.entries()
            .iter()
            .enumerate()
            .filter(move |(_, e)| e.cc == cc)
    }
}

const fn ent(cc: u8, b: BlockRef, p: ParamId) -> MapEntry {
    match (Cc::new(cc), Mappable::new(ParamAddr::new(b, p))) {
        (Some(cc), Some(target)) => MapEntry {
            cc,
            target,
            min: 0,
            max: 255,
        },
        _ => panic!("a default map entry must be mappable"),
    }
}

/// Spec § 9: the seven shared entries in learn order, then CCs 16-19 for
/// ALGO, MODAL and VA (TAPE has none, ADR 0132).
static DEFAULTS: [MapEntry; 19] = {
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
