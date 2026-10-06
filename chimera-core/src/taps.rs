//! The filter's jacks (filter-types spec § 2, § 6): FM3 IN and QM IN pick a
//! matrix source, or on ALGO an audio-rate tap of OP 5 or OP 6.

use crate::block::DiskCode;
use crate::modulation::{FixedCol, ModSource};
use crate::params::EngineType;

/// An ALGO operator that also feeds the filter at audio rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TapOp {
    Op5 = 0,
    Op6 = 1,
}

impl TapOp {
    pub const ALL: [TapOp; 2] = [TapOp::Op5, TapOp::Op6];

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

/// The taps' routes (spec § 6): an amount per (tap, fixed column), or
/// none. The columns are `FixedCol`, so a tap into an operator's pitch or
/// level can't be written. An absent route holds `ABSENT`, which no amount
/// is (±127): 8 B, so the voice chain keeps its 2,112 B (ADR 0101), where a
/// presence byte would cost 8 more in every voice's snapshot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TapRoutes {
    amount: [[i8; 4]; 2],
}

const ABSENT: i8 = i8::MIN;

impl Default for TapRoutes {
    fn default() -> Self {
        TapRoutes {
            amount: [[ABSENT; 4]; 2],
        }
    }
}

impl TapRoutes {
    pub fn get(&self, op: TapOp, c: FixedCol) -> Option<i8> {
        let a = self.amount[op as usize][c.index()];
        (a != ABSENT).then_some(a)
    }

    /// Sets (and so creates) the route; `a` is held to ±127.
    pub fn set(&mut self, op: TapOp, c: FixedCol, a: i8) {
        self.amount[op as usize][c.index()] = a.max(-127);
    }

    pub fn clear(&mut self, op: TapOp, c: FixedCol) {
        self.amount[op as usize][c.index()] = ABSENT;
    }

    /// A tap moves `c`: some route into it with a nonzero amount.
    pub fn moving(&self, c: FixedCol) -> bool {
        TapOp::ALL
            .into_iter()
            .any(|op| self.get(op, c).is_some_and(|a| a != 0))
    }

    /// The ops some moving route reads: the ones the engine must capture.
    pub fn wanted(&self) -> [bool; 2] {
        TapOp::ALL.map(|op| {
            FixedCol::ALL
                .into_iter()
                .any(|c| self.get(op, c).is_some_and(|a| a != 0))
        })
    }

    pub fn is_empty(&self) -> bool {
        self.entries().next().is_none()
    }

    /// Every present route, op-major.
    pub fn entries(&self) -> impl Iterator<Item = (TapOp, FixedCol, i8)> + '_ {
        TapOp::ALL.into_iter().flat_map(move |op| {
            FixedCol::ALL
                .into_iter()
                .filter_map(move |c| self.get(op, c).map(|a| (op, c, a)))
        })
    }
}

/// A CUTOFF route's amount as a FREQ tap's, keeping its depth in octaves
/// (M-new-2): 100 % is 10 octaves on a route, 4 on a tap.
pub fn to_tap_amount(a: i8) -> i8 {
    div_round(a as i16 * 10, 4).clamp(-127, 127) as i8
}

/// A FREQ tap's amount as a CUTOFF route's.
pub fn from_tap_amount(a: i8) -> i8 {
    div_round(a as i16 * 4, 10) as i8
}

/// `n / d` rounded half away from zero (`d > 0`).
const fn div_round(n: i16, d: i16) -> i16 {
    (n + n.signum() * (d / 2)) / d
}

impl core::fmt::Debug for TapRoutes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.entries().map(|(op, c, a)| (op.tag(), c.tag(), a)))
            .finish()
    }
}
