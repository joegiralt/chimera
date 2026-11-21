//! A DRUM Part's kit beside its Sound (ADR 0305): in the library's file,
//! and in the Part's status. Bank 1's side: the mark, what the status
//! reads, and the door to `kern`, which is bank 2's.

mod kern;

use chimera_hal::store::StoreError;

use crate::dsp::drum::types::Kit;
use crate::preset::Sound;
use crate::storage::{Event, FileError, RecordWriter};

use super::{PartId, Project};

/// What a Part's kit is weighed against: the kit it last loaded (NEW's,
/// a project's, a library Sound's) or saved to the library. Never
/// stored: a load sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KitMark {
    /// `Kit::DEFAULT`: NEW, and a file from before kits.
    Init,
    /// The kit's CRC then (`kern::kit_crc`).
    At(u32),
}

/// A Part's kit as its status and its replace's witness read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KitSeen {
    edited: bool,
    witness: u32,
}

impl KitSeen {
    /// Not a DRUM Part's (its kit is out of play), or bank 2 closed.
    pub const UNREAD: KitSeen = KitSeen {
        edited: false,
        witness: 0,
    };

    /// A DRUM Part's kit at `crc`, `edited` since its mark or not: a
    /// witness that moves with either.
    #[inline(always)]
    pub(super) fn new(crc: u32, edited: bool) -> KitSeen {
        KitSeen {
            edited,
            witness: if edited { !crc } else { crc },
        }
    }

    #[inline(always)]
    pub fn edited(self) -> bool {
        self.edited
    }

    #[inline(always)]
    pub(super) fn witness(self) -> u32 {
        self.witness
    }
}

/// `KitDoor`'s mark: a table at the slot that isn't this one fails the
/// boot's door check (`door_ok`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Magic(u32);

const MAGIC: Magic = Magic(0xB2E0_0305);

/// What bank 1 may call of a kit's library records and marks (ADR 0305),
/// all bank 2's with `bank2` (`kern`), at its own slot in the S4 groups
/// (`KIT_DOOR_ADDR`, `bank2.x`): the door's own table is full. Closed, a
/// save writes no kit, a load reads none and no kit edit is seen: as
/// before the ADR.
#[repr(C)]
pub struct KitDoor {
    magic: Magic,
    put: fn(&Sound, &Kit, &mut RecordWriter<'_>) -> Result<(), StoreError>,
    read: fn(&Event<'_>, &mut Kit) -> Result<(), FileError>,
    land: fn(&mut Project, PartId),
    settle: fn(&mut Project, PartId),
    saved: fn(&mut Project, PartId),
    seen: fn(&Project, PartId) -> KitSeen,
}

#[cfg(feature = "bank2")]
// SAFETY: `bank2.x` links `KIT_DOOR`, a `KitDoor`, at `KIT_DOOR_ADDR` in
// every firmware build with `bank2` (its assert, `__bank2_kit_door`).
const SLOT: crate::bank2::DoorSlot<KitDoor> =
    unsafe { crate::bank2::DoorSlot::new(crate::bank2::KIT_DOOR_ADDR) };

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.door.kit")]
#[used]
static KIT_DOOR: KitDoor = KitDoor::OWN;

/// The kit's table in judged door `j` answers (the boot's door check,
/// run before the door opens).
#[cfg(feature = "bank2")]
pub fn kit_door_ok(j: &crate::bank2::Judged) -> bool {
    j.with_slot(SLOT, |d| d.magic == MAGIC)
}

impl KitDoor {
    const OWN: KitDoor = KitDoor {
        magic: MAGIC,
        put: kern::put,
        read: kern::read,
        land: kern::land,
        settle: kern::settle,
        saved: kern::saved,
        seen: kern::seen,
    };

    /// `f` on the kit's table: bank 2's through the door on the unit,
    /// `None` closed; this build's own elsewhere. A host test under
    /// `bank2` answers as the unit does: only while the door cell is
    /// open.
    #[inline(always)]
    #[optimize(size)]
    fn with<R>(f: impl FnOnce(&KitDoor) -> R) -> Option<R> {
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
            Some(f(&KitDoor::OWN))
        }
    }

    /// After a library Sound `s`'s records, if it is DRUM's: `kit`'s, a
    /// `Kit`, then a `KitSound` a pad, as a project's Part writes them.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn put(s: &Sound, kit: &Kit, w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
        Self::with(|d| (d.put)(s, kit, w)).unwrap_or(Ok(()))
    }

    /// One event of a library file's pass 2, onto the scratch `kit`: its
    /// header resets it, a `Kit` or `KitSound` record lands on it or
    /// refuses the file. Anything else is none of its business.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn read(e: &Event<'_>, kit: &mut Kit) -> Result<(), FileError> {
        Self::with(|d| (d.read)(e, kit)).unwrap_or(Ok(()))
    }

    /// The scratch kit onto Part `id`, marked as loaded, if the Sound it
    /// just loaded is DRUM's.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn land(p: &mut Project, id: PartId) {
        Self::with(|d| (d.land)(p, id));
    }

    /// Part `id`'s kit is the one a project load read: its mark.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn settle(p: &mut Project, id: PartId) {
        Self::with(|d| (d.settle)(p, id));
    }

    /// Part `id`'s library save held its kit, if it plays DRUM: its mark.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn saved(p: &mut Project, id: PartId) {
        Self::with(|d| (d.saved)(p, id));
    }

    /// Part `id`'s kit against its mark. Out of line: the status and the
    /// witness share the one call.
    #[inline(never)]
    #[optimize(size)]
    pub(super) fn seen(p: &Project, id: PartId) -> KitSeen {
        Self::with(|d| (d.seen)(p, id)).unwrap_or(KitSeen::UNREAD)
    }
}
