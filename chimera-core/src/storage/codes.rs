//! The frozen code tables (ADR 0045): a block, a mod source or a stored enum
//! is named on the card by a code chosen here, never by a Rust discriminant,
//! a variant's position or a `ParamId`'s place in a page. Codes are forever:
//! `tests/fixtures/disk_codes_v1.txt` pins them, and only appending is allowed.

use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::{Block, DiskCode, ParamId, ParamKind, ParamSpec};
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::modulation::ModSource;
use crate::orbit::chords::ScaleKind;
use crate::orbit::ring::{NoteMode, Range, SnapTo};
use crate::orbit::scene::TargetKind;
use crate::orbit::walk::Walk;
use chimera_theory::{Degree, PitchClass};

use super::block_codec::MAX_BLOCK_PARAMS;

impl BlockRef {
    /// The block's code. `Option`, so a block that is never stored can be
    /// added without a code.
    pub const fn disk_code(self) -> Option<u8> {
        Some(match self {
            BlockRef::Modal => 1,
            BlockRef::Algo => 2,
            BlockRef::AlgoOp(Op::A) => 3,
            BlockRef::AlgoOp(Op::B) => 4,
            BlockRef::AlgoOp(Op::C) => 5,
            BlockRef::AlgoOp(Op::D) => 6,
            BlockRef::AlgoOp(Op::E) => 7,
            BlockRef::AlgoOp(Op::F) => 8,
            BlockRef::Drive => 9,
            BlockRef::Filter => 10,
            BlockRef::Folder => 11,
            BlockRef::Env(EnvSlot::Env1) => 12,
            BlockRef::Env(EnvSlot::Env2) => 13,
            BlockRef::Env(EnvSlot::Env3) => 14,
            BlockRef::Lfo(LfoSlot::Lfo1) => 15,
            BlockRef::Lfo(LfoSlot::Lfo2) => 16,
            BlockRef::Lfo(LfoSlot::Lfo3) => 17,
            BlockRef::Out => 18,
            BlockRef::Pitch => 19,
            BlockRef::Chorus => 20,
            BlockRef::Delay => 21,
            BlockRef::Reverb => 22,
            BlockRef::Tape => 23,
            BlockRef::Comp => 24,
            BlockRef::Part => 25,
            BlockRef::Theme => 26,
            BlockRef::OrbitChords => 27,
            BlockRef::OrbitRing(r) => 28 + r.get(),
            BlockRef::Midi => 33,
            BlockRef::PartMix(_) | BlockRef::Tempo => return None,
        })
    }

    /// The block's frozen ident, an explicit literal; the fixture pins it
    /// beside the code, so swapping two blocks' codes (ENV 1 and 2) is caught.
    pub const fn disk_ident(self) -> Option<&'static str> {
        Some(match self {
            BlockRef::Modal => "MODAL",
            BlockRef::Algo => "ALGO",
            BlockRef::AlgoOp(Op::A) => "OP_A",
            BlockRef::AlgoOp(Op::B) => "OP_B",
            BlockRef::AlgoOp(Op::C) => "OP_C",
            BlockRef::AlgoOp(Op::D) => "OP_D",
            BlockRef::AlgoOp(Op::E) => "OP_E",
            BlockRef::AlgoOp(Op::F) => "OP_F",
            BlockRef::Drive => "DRIVE",
            BlockRef::Filter => "FILTER",
            BlockRef::Folder => "FOLDER",
            BlockRef::Env(EnvSlot::Env1) => "ENV1",
            BlockRef::Env(EnvSlot::Env2) => "ENV2",
            BlockRef::Env(EnvSlot::Env3) => "ENV3",
            BlockRef::Lfo(LfoSlot::Lfo1) => "LFO1",
            BlockRef::Lfo(LfoSlot::Lfo2) => "LFO2",
            BlockRef::Lfo(LfoSlot::Lfo3) => "LFO3",
            BlockRef::Out => "OUT",
            BlockRef::Pitch => "PITCH",
            BlockRef::Chorus => "CHORUS",
            BlockRef::Delay => "DELAY",
            BlockRef::Reverb => "REVERB",
            BlockRef::Tape => "TAPE",
            BlockRef::Comp => "COMP",
            BlockRef::Part => "PART",
            BlockRef::Theme => "THEME",
            BlockRef::OrbitChords => "ORBIT",
            BlockRef::OrbitRing(r) => {
                ["RING1", "RING2", "RING3", "RING4", "RING5"][r.get() as usize]
            }
            BlockRef::Midi => "MIDI",
            BlockRef::PartMix(_) | BlockRef::Tempo => return None,
        })
    }

    /// The block with code `c`; `None`: retired or from a newer firmware.
    pub fn from_disk_code(c: u8) -> Option<BlockRef> {
        BlockRef::ALL.into_iter().find(|b| b.disk_code() == Some(c))
    }
}

impl ModSource {
    pub const fn disk_code(self) -> u8 {
        match self {
            ModSource::Env1 => 0,
            ModSource::Lfo1 => 1,
            ModSource::Env2 => 2,
            ModSource::Env3 => 3,
            ModSource::Lfo2 => 4,
            ModSource::Lfo3 => 5,
            ModSource::Vel => 6,
            ModSource::Note => 7,
        }
    }

    /// The source's frozen ident, an explicit literal (not `name` or `tag`,
    /// which are display text).
    pub const fn disk_ident(self) -> &'static str {
        match self {
            ModSource::Env1 => "ENV1",
            ModSource::Lfo1 => "LFO1",
            ModSource::Env2 => "ENV2",
            ModSource::Env3 => "ENV3",
            ModSource::Lfo2 => "LFO2",
            ModSource::Lfo3 => "LFO3",
            ModSource::Vel => "VEL",
            ModSource::Note => "NOTE",
        }
    }

    pub fn from_disk_code(c: u8) -> Option<ModSource> {
        ModSource::ALL.into_iter().find(|s| s.disk_code() == c)
    }
}

/// `(block code, param id)` pairs that once existed and never come back
/// (ADR 0009: the filter's FM, ENV and KEY). A reader skips them, unless a
/// `Translation` of the block reads them.
/// A retired code goes on one of these lists, and never gets another meaning:
/// the fixture check fails a gone line that isn't listed, and a listed one
/// that is produced again. A retired block lists each of its params.
pub const RETIRED: &[(u8, u8)] = &[(10, 3), (10, 4), (10, 5), (1, 2), (1, 5), (1, 7), (1, 8)];

/// Block codes that are gone for good (each of its params is in `RETIRED`).
pub const RETIRED_BLOCKS: &[u8] = &[];

/// `(block code, param id, value code)`: one enum value that is gone for good.
pub const RETIRED_CODES: &[(u8, u8, u8)] = &[];

/// `ModSource` codes that are gone for good.
pub const RETIRED_SOURCES: &[u8] = &[];

/// A param that moved or changed unit: a file's `old` value, run through
/// `map`, is `new`'s. None yet.
pub struct Migration {
    pub block: u8,
    pub old: ParamId,
    pub new: ParamId,
    pub map: fn(f32) -> f32,
}

pub const MIGRATIONS: &[Migration] = &[];

/// A block's retired values from one file, by old `ParamId`.
pub struct Retired([Option<f32>; MAX_BLOCK_PARAMS]);

impl Retired {
    pub(super) const fn new() -> Self {
        Retired([None; MAX_BLOCK_PARAMS])
    }

    pub(super) fn put(&mut self, id: ParamId, v: f32) {
        self.0[usize::from(id.0)] = Some(v);
    }

    pub fn get(&self, id: ParamId) -> Option<f32> {
        self.0.get(usize::from(id.0)).copied().flatten()
    }

    pub fn any(&self) -> bool {
        self.0.iter().any(Option::is_some)
    }
}

// Every retired id has a slot in `Retired`.
const _: () = {
    let mut i = 0;
    while i < RETIRED.len() {
        assert!((RETIRED[i].1 as usize) < MAX_BLOCK_PARAMS);
        i += 1;
    }
};

/// Live params several retired ones derive from together (spec § 3): run once
/// the file's live values are written, only when the file held a retired id of `block`.
pub struct Translation {
    pub block: u8,
    pub apply: fn(&Retired, &mut dyn Block),
}

pub const TRANSLATIONS: &[Translation] = &[Translation {
    block: 1,
    apply: crate::dsp::modal::translate_v1,
}];

/// A param that exists in a live spec table. Only built from one, so a saver
/// can't write, and a loader can't apply, an address the firmware lacks.
///
/// ```compile_fail,E0451
/// use chimera_core::{addr::BlockRef, params::FILTER_SPECS, storage::ValidAddr};
/// let _ = ValidAddr { block: BlockRef::Filter, spec: &FILTER_SPECS[0] };
/// ```
#[derive(Clone, Copy, Debug)]
pub struct ValidAddr {
    block: BlockRef,
    spec: &'static ParamSpec,
}

impl ValidAddr {
    /// `b`'s stored params in spec-table order: the canonical save and load
    /// order, so a param whose decoding needs another comes after it.
    pub fn of_block(b: BlockRef) -> impl Iterator<Item = ValidAddr> {
        b.specs()
            .iter()
            .filter(|s| s.stored)
            .map(move |spec| ValidAddr { block: b, spec })
    }

    pub fn find(b: BlockRef, id: ParamId) -> Option<ValidAddr> {
        Self::of_block(b).find(|a| a.spec.id == id)
    }

    pub fn addr(self) -> ParamAddr {
        ParamAddr::new(self.block, self.spec.id)
    }

    pub fn spec(self) -> &'static ParamSpec {
        self.spec
    }

    /// Stored as a disk code, not a float.
    pub fn coded(self) -> bool {
        self.spec.kind == ParamKind::Enum
    }
}

/// What a param is on the card: a float, or an enum's frozen code.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DiskValue {
    Real(f32),
    Code(u8),
}

/// `a`'s value in `b`, which must be `a`'s block.
pub fn read_value(b: &dyn Block, a: ValidAddr) -> DiskValue {
    debug_assert!(
        core::ptr::eq(b.specs(), a.block.specs()),
        "block does not match the address"
    );
    let id = a.spec.id;
    let code = b.enum_code(id);
    debug_assert!(!a.coded() || code.is_some(), "an Enum without a code");
    match code {
        Some(c) if a.coded() => DiskValue::Code(c),
        _ => DiskValue::Real(b.get(id)),
    }
}

// ORBIT's stored enums (spec § 8). `from_disk_code` searches `ALL` by the
// explicit code, as `BlockRef` does, so a variant's place never matters.

impl DiskCode for ScaleKind {
    fn disk_code(self) -> u8 {
        match self {
            ScaleKind::Major => 0,
            ScaleKind::Minor => 1,
            ScaleKind::Dorian => 2,
            ScaleKind::Phrygian => 3,
            ScaleKind::Lydian => 4,
            ScaleKind::Mixolydian => 5,
            ScaleKind::Locrian => 6,
            ScaleKind::HarmonicMinor => 7,
            ScaleKind::PentatonicMajor => 8,
            ScaleKind::PentatonicMinor => 9,
            ScaleKind::Custom => 10,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            ScaleKind::Major => "MAJOR",
            ScaleKind::Minor => "MINOR",
            ScaleKind::Dorian => "DORIAN",
            ScaleKind::Phrygian => "PHRYGIAN",
            ScaleKind::Lydian => "LYDIAN",
            ScaleKind::Mixolydian => "MIXOLYDIAN",
            ScaleKind::Locrian => "LOCRIAN",
            ScaleKind::HarmonicMinor => "HARMONIC_MINOR",
            ScaleKind::PentatonicMajor => "PENT_MAJOR",
            ScaleKind::PentatonicMinor => "PENT_MINOR",
            ScaleKind::Custom => "CUSTOM",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        ScaleKind::ALL.into_iter().find(|k| k.disk_code() == c)
    }
}

impl DiskCode for PitchClass {
    fn disk_code(self) -> u8 {
        match self {
            PitchClass::C => 0,
            PitchClass::Cs => 1,
            PitchClass::D => 2,
            PitchClass::Ds => 3,
            PitchClass::E => 4,
            PitchClass::F => 5,
            PitchClass::Fs => 6,
            PitchClass::G => 7,
            PitchClass::Gs => 8,
            PitchClass::A => 9,
            PitchClass::As => 10,
            PitchClass::B => 11,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            PitchClass::C => "C",
            PitchClass::Cs => "C_SHARP",
            PitchClass::D => "D",
            PitchClass::Ds => "D_SHARP",
            PitchClass::E => "E",
            PitchClass::F => "F",
            PitchClass::Fs => "F_SHARP",
            PitchClass::G => "G",
            PitchClass::Gs => "G_SHARP",
            PitchClass::A => "A",
            PitchClass::As => "A_SHARP",
            PitchClass::B => "B",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        PitchClass::ALL.into_iter().find(|p| p.disk_code() == c)
    }
}

/// A degree's code is its index from the root, I = 0 .. XII = 11.
const DEGREE_IDENTS: [&str; 12] = [
    "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII",
];

impl DiskCode for Degree {
    fn disk_code(self) -> u8 {
        self.index() as u8
    }

    fn disk_ident(self) -> &'static str {
        DEGREE_IDENTS[self.index()]
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Degree::ALL.get(c as usize).copied()
    }
}

impl DiskCode for Range {
    fn disk_code(self) -> u8 {
        match self {
            Range::Zero => 0,
            Range::One => 1,
            Range::Two => 2,
            Range::Three => 3,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Range::Zero => "0",
            Range::One => "1",
            Range::Two => "2",
            Range::Three => "3",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Range::ALL.into_iter().find(|r| r.disk_code() == c)
    }
}

impl DiskCode for SnapTo {
    fn disk_code(self) -> u8 {
        match self {
            SnapTo::Scale => 0,
            SnapTo::Chord => 1,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            SnapTo::Scale => "SCALE",
            SnapTo::Chord => "CHORD",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        SnapTo::ALL.into_iter().find(|s| s.disk_code() == c)
    }
}

impl DiskCode for NoteMode {
    fn disk_code(self) -> u8 {
        match self {
            NoteMode::Step => 0,
            NoteMode::Pool => 1,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            NoteMode::Step => "STEP",
            NoteMode::Pool => "POOL",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        NoteMode::ALL.into_iter().find(|m| m.disk_code() == c)
    }
}

impl DiskCode for Walk {
    fn disk_code(self) -> u8 {
        match self {
            Walk::Up => 0,
            Walk::Down => 1,
            Walk::Random => 2,
            Walk::PingPong => 3,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Walk::Up => "UP",
            Walk::Down => "DOWN",
            Walk::Random => "RANDOM",
            Walk::PingPong => "PING_PONG",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Walk::ALL.into_iter().find(|w| w.disk_code() == c)
    }
}

impl DiskCode for TargetKind {
    fn disk_code(self) -> u8 {
        match self {
            TargetKind::Part => 0,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            TargetKind::Part => "PART",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        (c == 0).then_some(TargetKind::Part)
    }
}
