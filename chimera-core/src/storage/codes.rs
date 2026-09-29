//! The frozen code tables (ADR 0045): a block, a mod source or a stored enum
//! is named on the card by a code chosen here, never by a Rust discriminant,
//! a variant's position or a `ParamId`'s place in a page. Codes are forever:
//! `tests/fixtures/disk_codes_v1.txt` pins them, and only appending is allowed.

use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::{Block, ParamId, ParamKind, ParamSpec};
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::modulation::ModSource;

impl BlockRef {
    /// The block's code; `None` for `Channels`, which is a view of the Parts'
    /// own `CH` params and is never stored.
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
            BlockRef::Channels => return None,
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

    pub fn from_disk_code(c: u8) -> Option<ModSource> {
        ModSource::ALL.into_iter().find(|s| s.disk_code() == c)
    }
}

/// `(block code, param id)` pairs that once existed and never come back
/// (ADR 0009: the filter's FM, ENV and KEY). A reader skips them silently.
/// A retired code goes on one of these lists, and never gets another meaning:
/// the fixture check fails a gone line that isn't listed, and a listed one
/// that is produced again. A retired block lists each of its params.
pub const RETIRED: &[(u8, u8)] = &[(10, 3), (10, 4), (10, 5)];

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
