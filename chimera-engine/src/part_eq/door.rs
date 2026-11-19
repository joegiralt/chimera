//! The Parts' EQ behind bank 2's door (ADR 0284): `dsp::run` links into an
//! S4 group (`bank2.x`, `*7part_eq3dsp*`), reached through `EqDoor` at its
//! own address after VA's solos' table (`PART_EQ_DOOR_ADDR`): the door's
//! own table is full. Without `bank2` it is bank 1's, called directly.
//! A closed door plays every Part flat.

use super::PartEqs;
use super::dsp::PartsBlock;

/// What bank 1 asks of the EQ: one entry, as the table's stride holds
/// (`DOOR_TABLE_STRIDE`).
pub enum Ask<'a, 'b> {
    /// The Parts' block: LO CUT, TILT, then SMART EQ's listening and cuts.
    Parts(&'a mut PartEqs, PartsBlock<'b>),
    /// SMART EQ on the reverb's return (ADR 0296), later in the block.
    Wash {
        eqs: &'a mut PartEqs,
        wet: &'a mut crate::dsp::Stereo,
        /// Whether the reverb returned anything this block.
        sounding: bool,
        sample_rate: u32,
    },
}

/// The EQ's entry as the table carries it.
pub type EqRun = for<'a, 'b> fn(Ask<'a, 'b>);

/// `EqDoor`'s entry: called, never handed out.
#[repr(transparent)]
pub struct EqEntry(EqRun);

impl EqEntry {
    #[inline(always)]
    pub fn run(&self, ask: Ask<'_, '_>) {
        (self.0)(ask)
    }
}

/// `EqDoor`'s mark: a table at the slot that isn't this one fails the
/// boot's door check (`door_ok`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Magic(u32);

const MAGIC: Magic = Magic(0xB2E0_0296);

/// The EQ's table, at its own slot (`PART_EQ_DOOR_ADDR`, `bank2.x`).
#[repr(C)]
pub struct EqDoor {
    magic: Magic,
    eq: EqEntry,
}

impl EqDoor {
    const OWN: EqDoor = EqDoor {
        magic: MAGIC,
        eq: EqEntry(super::dsp::answer),
    };

    #[inline(always)]
    pub fn eq(&self) -> &EqEntry {
        &self.eq
    }
}

#[cfg(feature = "bank2")]
// SAFETY: `bank2.x` links `EQ_DOOR`, an `EqDoor`, at `PART_EQ_DOOR_ADDR`
// in every firmware build with `bank2` (its assert, `__bank2_part_eq_door`).
const SLOT: crate::bank2::DoorSlot<EqDoor> =
    unsafe { crate::bank2::DoorSlot::new(crate::bank2::PART_EQ_DOOR_ADDR) };

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.door.part_eq")]
#[used]
static EQ_DOOR: EqDoor = EqDoor::OWN;

/// The EQ's table in judged door `j` answers (the boot's door check, run
/// before the door opens).
#[cfg(feature = "bank2")]
pub fn door_ok(j: &crate::bank2::Judged) -> bool {
    j.with_slot(SLOT, |d| d.magic == MAGIC)
}

/// `f` on the EQ's table: bank 2's through the door on the unit, `None`
/// closed; this build's own elsewhere. A host test under `bank2` answers
/// as the unit does: only while the door cell is open.
#[inline(always)]
pub fn through<R>(f: impl FnOnce(&EqDoor) -> R) -> Option<R> {
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
        Some(f(&EqDoor::OWN))
    }
}
