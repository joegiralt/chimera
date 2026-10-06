//! The filter's jacks (filter-types spec § 2, § 6): FM3 IN and QM IN pick a
//! matrix source, or on ALGO an audio-rate tap of OP 5 or OP 6.

use crate::block::DiskCode;
use crate::modulation::ModSource;
use crate::params::EngineType;

/// An ALGO operator that also feeds the filter at audio rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TapOp {
    Op5 = 0,
    Op6 = 1,
}

impl TapOp {
    /// The operator's index in the ALGO voice.
    pub const fn op_index(self) -> usize {
        match self {
            TapOp::Op5 => 4,
            TapOp::Op6 => 5,
        }
    }

    pub const fn tag(self) -> &'static str {
        match self {
            TapOp::Op5 => "OP5",
            TapOp::Op6 => "OP6",
        }
    }
}

/// What a jack is patched to. `ModSource` stays the 8-row matrix type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jack {
    Mod(ModSource),
    Tap(TapOp),
}

const fn m(s: ModSource) -> Jack {
    Jack::Mod(s)
}

const OP5: Jack = Jack::Tap(TapOp::Op5);
const OP6: Jack = Jack::Tap(TapOp::Op6);

impl Jack {
    /// Every jack, in disk-code order.
    pub const ALL: [Jack; 10] = [
        m(ModSource::Env1),
        m(ModSource::Lfo1),
        m(ModSource::Env2),
        m(ModSource::Env3),
        m(ModSource::Lfo2),
        m(ModSource::Lfo3),
        m(ModSource::Vel),
        m(ModSource::Note),
        OP5,
        OP6,
    ];

    /// FM3 IN's choices: ENV 1 and NOTE are FM1's and FM2's.
    pub const FM3_CHOICES: [Jack; 8] = [
        m(ModSource::Lfo1),
        m(ModSource::Lfo2),
        m(ModSource::Lfo3),
        m(ModSource::Env2),
        m(ModSource::Env3),
        m(ModSource::Vel),
        OP5,
        OP6,
    ];

    /// QM IN's choices: every matrix source, then the taps.
    pub const QM_CHOICES: [Jack; 10] = Jack::ALL;

    /// What the jack acts as on `engine`: a tap off ALGO is `default`, and
    /// the stored tap is kept for a return to ALGO (L-new-2).
    pub fn effective(self, engine: EngineType, default: Jack) -> Jack {
        match self {
            Jack::Tap(_) if engine != EngineType::Algo => default,
            _ => self,
        }
    }

    pub const fn tag(self) -> &'static str {
        match self {
            Jack::Mod(s) => s.tag(),
            Jack::Tap(t) => t.tag(),
        }
    }
}

impl DiskCode for Jack {
    fn disk_code(self) -> u8 {
        match self {
            Jack::Mod(s) => s.disk_code(),
            Jack::Tap(t) => 8 + t as u8,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Jack::Mod(s) => s.disk_ident(),
            Jack::Tap(t) => t.tag(),
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Jack::ALL.into_iter().find(|j| j.disk_code() == c)
    }
}

const fn tags<const N: usize>(j: [Jack; N]) -> [&'static str; N] {
    let mut t = [""; N];
    let mut i = 0;
    while i < N {
        t[i] = j[i].tag();
        i += 1;
    }
    t
}

/// FM3 IN's and QM IN's values on screen.
pub static FM3_IN_NAMES: [&str; 8] = tags(Jack::FM3_CHOICES);
pub static QM_IN_NAMES: [&str; 10] = tags(Jack::QM_CHOICES);

/// `j`'s index in `choices`, if it is one.
fn index_of(choices: &[Jack], j: Jack) -> Option<u8> {
    choices.iter().position(|&c| c == j).map(|i| i as u8)
}

/// FM3 IN and QM IN in one byte: their indexes into `FM3_CHOICES` (high
/// nibble) and `QM_CHOICES` (low). Only a choice is representable, and the
/// voice chain keeps its 2,112 B (ADR 0101): two `Jack` fields would pad
/// `FilterParams` by 4 B in every voice's snapshot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct JackPair(u8);

const _: () = assert!(Jack::FM3_CHOICES.len() <= 16 && Jack::QM_CHOICES.len() <= 16);

impl JackPair {
    /// FM3 IN on LFO 1, QM IN on ENV 3 (owner answer 6).
    pub const DEFAULT: JackPair = JackPair(3);

    pub const fn fm3(self) -> Jack {
        Jack::FM3_CHOICES[(self.0 >> 4) as usize]
    }

    pub const fn qm(self) -> Jack {
        Jack::QM_CHOICES[(self.0 & 0xF) as usize]
    }

    /// FM3 IN's index into `FM3_CHOICES`, and QM IN's into `QM_CHOICES`.
    pub const fn fm3_index(self) -> u8 {
        self.0 >> 4
    }

    pub const fn qm_index(self) -> u8 {
        self.0 & 0xF
    }

    /// With FM3 IN on `j`; `None` if `j` isn't one of its choices.
    pub fn with_fm3(self, j: Jack) -> Option<Self> {
        index_of(&Jack::FM3_CHOICES, j).map(|i| JackPair(i << 4 | self.0 & 0xF))
    }

    /// With QM IN on `j`; `None` if `j` isn't one of its choices.
    pub fn with_qm(self, j: Jack) -> Option<Self> {
        index_of(&Jack::QM_CHOICES, j).map(|i| JackPair(self.0 & 0xF0 | i))
    }
}

impl core::fmt::Debug for JackPair {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JackPair")
            .field("fm3", &self.fm3())
            .field("qm", &self.qm())
            .finish()
    }
}
