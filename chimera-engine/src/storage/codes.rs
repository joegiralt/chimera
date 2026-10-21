//! The frozen code tables (ADR 0045): a block, a mod source or a stored enum
//! is named on the card by a code chosen here, never by a Rust discriminant,
//! a variant's position or a `ParamId`'s place in a page. Codes are forever:
//! `tests/fixtures/disk_codes_v1.txt` pins them, and only appending is allowed.

use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::{Block, DiskCode, ParamId, ParamKind, ParamSpec};
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::dsp::reverb::{RevSync, ReverbType};
use crate::dsp::tape_engine::params::{RETIRED_ONE, RetiredModel, TapeEngineParams};
use crate::modulation::ModSource;
use crate::orbit::chords::ScaleKind;
use crate::orbit::ring::{NoteMode, Range, SnapTo};
use crate::orbit::scene::TargetKind;
use crate::orbit::walk::Walk;
use chimera_theory::{Degree, PitchClass};

use super::block_codec::{ByteSet, MAX_BLOCK_PARAMS};

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
            BlockRef::Filter => 10,
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
            // A block code: the registry's retired page id 34 is another
            // namespace.
            BlockRef::TapeEngine => 34,
            BlockRef::Va => 35,
            BlockRef::VoiceFx => 36,
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
            BlockRef::Filter => "FILTER",
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
            BlockRef::TapeEngine => "TAPE_ENG",
            BlockRef::Va => "VA",
            BlockRef::VoiceFx => "VOICEFX",
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
/// (ADR 0009: the filter's DRIVE, FM, ENV and KEY). A reader skips them, unless a
/// `Translation` of the block reads them.
/// A retired code goes on one of these lists, and never gets another meaning:
/// the fixture check fails a gone line that isn't listed, and a listed one
/// that is produced again. A retired block lists each of its params.
pub const RETIRED: &[(u8, u8)] = &[
    (10, 3),
    (10, 4),
    (10, 5),
    (1, 2),
    (1, 5),
    (1, 7),
    (1, 8),
    (10, 2),
    // The reverb's GRIT (reverb TYPEs spec § GRIT is retired).
    (22, 5),
    // PART's sends moved to OUT (ADR 0123).
    (25, 5),
    (25, 6),
    (25, 7),
    // DRV and the folder, now the voice FX slot's (ADR 0155): their values
    // load through `voice_fx::legacy`, their routes through `READDRESS`.
    (9, 0),
    (9, 1),
    (9, 2),
    (11, 0),
    (11, 1),
    (11, 2),
];

/// Block codes that are gone for good (each of its params is in `RETIRED`).
pub const RETIRED_BLOCKS: &[u8] = &[9, 11];

/// `(block code, param id, value code)`: one enum value that is gone for good.
/// TAPE's MODEL REVERSE (#369): LOOP with END before START plays it, so a
/// file's code 1 goes to `Retired` and `tape_engine::translate_v1`. So do
/// PING-PONG (2) and ONESHOT (7), which PLAY folds (ADR 0170), TRON (6),
/// and MODE ONE (0), which PLAY's DIRECTION and TRON hold (ADR 0200).
pub const RETIRED_CODES: &[(u8, u8, u8)] = &retired_codes();

const fn retired_codes() -> [(u8, u8, u8); RetiredModel::ALL.len() + 1] {
    let Some(block) = BlockRef::TapeEngine.disk_code() else {
        panic!("TAPE_ENG has a code")
    };
    let mut out = [(0, 0, 0); RetiredModel::ALL.len() + 1];
    let mut i = 0;
    while i < RetiredModel::ALL.len() {
        out[i] = (block, TapeEngineParams::MODEL.0, RetiredModel::ALL[i] as u8);
        i += 1;
    }
    out[i] = (block, TapeEngineParams::MODE.0, RETIRED_ONE);
    out
}

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

/// A block's retired values from one file, by old `ParamId`, its retired
/// enum codes (`RETIRED_CODES`), by the live param that held them, and the
/// ids the file held.
pub struct Retired {
    vals: [Option<f32>; MAX_BLOCK_PARAMS],
    codes: [Option<u8>; MAX_BLOCK_PARAMS],
    held: ByteSet,
}

impl Retired {
    pub(super) const fn new(held: ByteSet) -> Self {
        Retired {
            vals: [None; MAX_BLOCK_PARAMS],
            codes: [None; MAX_BLOCK_PARAMS],
            held,
        }
    }

    pub(super) fn put(&mut self, id: ParamId, v: f32) {
        self.vals[usize::from(id.0)] = Some(v);
    }

    pub(super) fn put_code(&mut self, id: ParamId, c: u8) {
        if let Some(x) = self.codes.get_mut(usize::from(id.0)) {
            *x = Some(c);
        }
    }

    pub fn get(&self, id: ParamId) -> Option<f32> {
        self.vals.get(usize::from(id.0)).copied().flatten()
    }

    /// The retired code `id` held in the file.
    pub fn code(&self, id: ParamId) -> Option<u8> {
        self.codes.get(usize::from(id.0)).copied().flatten()
    }

    pub fn any(&self) -> bool {
        self.vals.iter().any(Option::is_some) || self.codes.iter().any(Option::is_some)
    }

    /// The file named `id`, live or retired, whatever its value: a stored
    /// param beats one translated into it.
    pub fn held(&self, id: ParamId) -> bool {
        self.held.contains(id.0)
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

pub const TRANSLATIONS: &[Translation] = &[
    Translation {
        block: 1,
        apply: crate::dsp::modal::translate_v1,
    },
    Translation {
        block: 10,
        apply: crate::dsp::filter::translate_drive,
    },
    Translation {
        block: 34,
        apply: crate::dsp::tape_engine::params::translate_v1,
    },
];

/// A param as a file names it, which this firmware may no longer have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskAddr {
    pub block: u8,
    pub id: ParamId,
}

impl DiskAddr {
    /// `id` of the stored block `b`: a block without a code fails the build.
    pub const fn of(b: BlockRef, id: ParamId) -> DiskAddr {
        match b.disk_code() {
            Some(block) => DiskAddr { block, id },
            None => panic!("a block that is never stored"),
        }
    }
}

/// The filter's DRIVE (id 2), retired for LEVEL (ADR 0096).
pub const FILTER_DRIVE: DiskAddr = DiskAddr::of(BlockRef::Filter, ParamId(2));

/// A retired address whose routes, ModDests and registry entries a file
/// loads at a live param, amount and label kept. DRIVE's land on LEVEL.
pub const READDRESS: &[(DiskAddr, ParamAddr)] = &[
    (
        FILTER_DRIVE,
        ParamAddr::new(BlockRef::Filter, crate::params::FilterParams::LEVEL),
    ),
    (DRIVE_DRIVE, FX_P1),
    (DRIVE_TONE, FX_P2),
    (DRIVE_MIX, FX_MIX),
    (FOLDER_FOLD, FX_P1),
    (FOLDER_SYM, FX_P2),
    (FOLDER_MIX, FX_MIX),
];

/// DRV (block 9) and the folder (block 11), retired into the FX slot.
const fn old(block: u8, id: u8) -> DiskAddr {
    DiskAddr {
        block,
        id: ParamId(id),
    }
}
const DRIVE_DRIVE: DiskAddr = old(9, 0);
const DRIVE_TONE: DiskAddr = old(9, 1);
const DRIVE_MIX: DiskAddr = old(9, 2);
const FOLDER_FOLD: DiskAddr = old(11, 0);
const FOLDER_SYM: DiskAddr = old(11, 1);
const FOLDER_MIX: DiskAddr = old(11, 2);

use crate::dsp::voice_fx::params::VoiceFxParams as Fx;
const FX_P1: ParamAddr = ParamAddr::new(BlockRef::VoiceFx, Fx::P1);
const FX_P2: ParamAddr = ParamAddr::new(BlockRef::VoiceFx, Fx::P2);
const FX_MIX: ParamAddr = ParamAddr::new(BlockRef::VoiceFx, Fx::MIX);

// Only a retired address is re-addressed, so a live one always loads as
// itself; and only onto a modulatable param, so its routes play.
const _: () = {
    let mut i = 0;
    while i < READDRESS.len() {
        let from = READDRESS[i].0;
        let mut j = 0;
        while j < RETIRED.len() && (RETIRED[j].0 != from.block || RETIRED[j].1 != from.id.0) {
            j += 1;
        }
        assert!(j < RETIRED.len(), "re-addressed but not retired");
        assert!(
            READDRESS[i].1.modulatable(),
            "re-addressed onto a param routes can't play"
        );
        i += 1;
    }
};

/// The live param a retired address's routes load at, if any.
pub fn readdress(a: DiskAddr) -> Option<ParamAddr> {
    READDRESS
        .iter()
        .find(|(from, _)| *from == a)
        .map(|&(_, to)| to)
}

/// A param that exists in a live spec table. Only built from one, so a saver
/// can't write, and a loader can't apply, an address the firmware lacks.
///
/// ```compile_fail,E0451
/// use chimera_engine::{addr::BlockRef, params::FILTER_SPECS, storage::ValidAddr};
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

/// The one of `all` stored as `c`.
fn find_code<T: DiskCode>(all: &[T], c: u8) -> Option<T> {
    all.iter().copied().find(|x| x.disk_code() == c)
}

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
        find_code(&ScaleKind::ALL, c)
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
        find_code(&PitchClass::ALL, c)
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
        find_code(&Range::ALL, c)
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
        find_code(&SnapTo::ALL, c)
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
        find_code(&NoteMode::ALL, c)
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
        find_code(&Walk::ALL, c)
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

// The reverb's (reverb TYPEs spec § The TYPE list). Code 6 was never
// assigned: ECHO never shipped.

impl DiskCode for ReverbType {
    fn disk_code(self) -> u8 {
        match self {
            ReverbType::Quad => 0,
            ReverbType::Room => 1,
            ReverbType::Hall => 2,
            ReverbType::Gate => 3,
            ReverbType::Reverse => 4,
            ReverbType::Bloom => 5,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            ReverbType::Quad => "QUAD",
            ReverbType::Room => "ROOM",
            ReverbType::Hall => "HALL",
            ReverbType::Gate => "GATE",
            ReverbType::Reverse => "REVERSE",
            ReverbType::Bloom => "BLOOM",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        find_code(&ReverbType::ALL, c)
    }
}

impl DiskCode for RevSync {
    fn disk_code(self) -> u8 {
        match self {
            RevSync::Off => 0,
            RevSync::Time => 1,
            RevSync::All => 2,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            RevSync::Off => "OFF",
            RevSync::Time => "TIME",
            RevSync::All => "ALL",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        find_code(&RevSync::ALL, c)
    }
}
