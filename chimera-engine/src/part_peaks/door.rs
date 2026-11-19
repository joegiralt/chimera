//! The meters' table (ADR 0293): `kern::run` links into an S4 group, reached
//! through `PeakDoor` at its own address a stride after the Parts' EQ's
//! (`PART_PEAKS_DOOR_ADDR`). Without `bank2` it is bank 1's, called
//! directly. A closed door measures nothing: the meters rest.

use crate::hw::{BLOCK_SIZE, MAX_PARTS};
use crate::instrument::Sides;

/// The meters' block as the table carries it.
pub type PeaksRun = fn(&[[f32; BLOCK_SIZE]; MAX_PARTS], &Sides, &[bool; MAX_PARTS]);

/// `PeakDoor`'s entry: called, never handed out.
#[repr(transparent)]
pub struct PeaksEntry(PeaksRun);

impl PeaksEntry {
    #[inline(always)]
    pub fn run(
        &self,
        buses: &[[f32; BLOCK_SIZE]; MAX_PARTS],
        sides: &Sides,
        written: &[bool; MAX_PARTS],
    ) {
        (self.0)(buses, sides, written)
    }
}

/// `PeakDoor`'s mark: a table at the slot that isn't this one fails the
/// boot's door check (`door_ok`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Magic(u32);

const MAGIC: Magic = Magic(0xB2E0_0293);

/// The meters' table, at its own slot (`PART_PEAKS_DOOR_ADDR`, `bank2.x`).
#[repr(C)]
pub struct PeakDoor {
    magic: Magic,
    peaks: PeaksEntry,
}

impl PeakDoor {
    const OWN: PeakDoor = PeakDoor {
        magic: MAGIC,
        peaks: PeaksEntry(super::kern::run),
    };

    #[inline(always)]
    pub fn peaks(&self) -> &PeaksEntry {
        &self.peaks
    }
}

#[cfg(feature = "bank2")]
// SAFETY: `bank2.x` links `PEAK_DOOR`, a `PeakDoor`, at
// `PART_PEAKS_DOOR_ADDR` in every firmware build with `bank2` (its
// assert, `__bank2_part_peaks_door`).
const SLOT: crate::bank2::DoorSlot<PeakDoor> =
    unsafe { crate::bank2::DoorSlot::new(crate::bank2::PART_PEAKS_DOOR_ADDR) };

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.door.part_peaks")]
#[used]
static PEAK_DOOR: PeakDoor = PeakDoor::OWN;

/// The meters' table in judged door `j` answers (the boot's door check,
/// run before the door opens).
#[cfg(feature = "bank2")]
pub fn door_ok(j: &crate::bank2::Judged) -> bool {
    j.with_slot(SLOT, |d| d.magic == MAGIC)
}

/// `f` on the meters' table: bank 2's through the door on the unit, `None`
/// closed; this build's own elsewhere. A host test under `bank2` answers
/// as the unit does: only while the door cell is open.
#[inline(always)]
pub fn through<R>(f: impl FnOnce(&PeakDoor) -> R) -> Option<R> {
    #[cfg(all(feature = "bank2", target_os = "none"))]
    {
        crate::bank2::DOOR_CELL.with_slot(SLOT, f)
    }
    #[cfg(not(all(feature = "bank2", target_os = "none")))]
    {
        #[cfg(feature = "bank2")]
        if !crate::bank2::DOOR_CELL.is_open() {
            return None;
        }
        Some(f(&PeakDoor::OWN))
    }
}
