//! FB's kernels behind bank 2's door (ADR 0190, as FUZZ's, ADR 0157).
//!
//! Each TYPE's FB loop is its `moving` loop with every sample's `g` through
//! the FB signal: one more copy of every loop, ~6 KB, too much for bank 1.
//! Each lives in its type's `fed` module (`clean::fed`, `dirty::fed`,
//! `ladder::fed`, `k2040::fed`), which with `bank2` links into `.bank2`
//! whole (`bank2.x`, `*6filter*3fed*`); bank 1 builds the block's arrays
//! and reaches the loop through `FedDoor`. Closed, a block runs its plain
//! moving path: FB is off until the door opens.

use super::Fed;
use super::clean::Clean;
use super::dirty::Dirty;
use super::k2040::{K2040, voice};
use super::ladder::Ladder;
use crate::hw::BLOCK_SIZE;

type Buf = [f32; BLOCK_SIZE];
/// A `(from, step)`, a range or a ramp.
type Pair = (f32, f32);
/// Each kernel's signature: the state, the block, its arrays, FB, `g`'s
/// range.
pub type CleanFed = fn(&mut Clean, u8, &mut Buf, &Buf, &Buf, Pair, Fed, Pair);
pub type DirtyFed = fn(&mut Dirty, u8, &mut Buf, &Buf, &Buf, Pair, Fed, Pair) -> f32;
pub type LadderFed = fn(&mut Ladder, &mut Buf, &Buf, &Buf, super::ladder::FedMix, Fed, Pair);
pub type K2040Fed = fn(&mut K2040, &mut Buf, &Buf, &Buf, Pair, Fed, Pair) -> Pair;

/// The kernels' entries in the door's table.
#[repr(C)]
pub struct FedDoor {
    pub clean: CleanFed,
    pub dirty: DirtyFed,
    pub ladder: LadderFed,
    pub rev2: K2040Fed,
    pub exp: K2040Fed,
}

impl FedDoor {
    pub const DOOR: FedDoor = FedDoor {
        clean: Clean::fed,
        dirty: Dirty::fed,
        ladder: Ladder::fed,
        rev2: K2040::fed::<voice::Rev2>,
        exp: K2040::fed::<voice::Exp>,
    };
}

/// `f` on the kernels: through the door with `bank2` (`None` while it is
/// closed), else bank 1's own.
#[inline(always)]
pub(super) fn through<R>(f: impl FnOnce(&FedDoor) -> R) -> Option<R> {
    #[cfg(feature = "bank2")]
    {
        crate::bank2::DOOR_CELL.with(|d| f(&d.fed))
    }
    #[cfg(not(feature = "bank2"))]
    {
        Some(f(&FedDoor::DOOR))
    }
}
