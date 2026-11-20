//! A DRUM Part's kit beside its Sound (ADR 0305): in the library's file,
//! and in the Part's status. Bank 1's side: the mark, what the status
//! reads, and the door to `kern`, which is bank 2's.

mod kern;

use chimera_hal::store::StoreError;

use crate::dsp::drum::types::Kit;
use crate::storage::{FileError, RecordTag, RecordWriter};

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
pub enum KitSeen {
    /// Not a DRUM Part's (its kit is out of play), or bank 2 closed.
    Unread,
    /// As marked, at this CRC.
    Kept(u32),
    /// Edited since its mark, to this CRC.
    Edited(u32),
}

impl KitSeen {
    #[optimize(size)]
    pub fn edited(self) -> bool {
        matches!(self, KitSeen::Edited(_))
    }

    /// For a witness: which, then the CRC.
    #[optimize(size)]
    pub(super) fn bytes(self) -> [u8; 5] {
        let (k, crc) = match self {
            KitSeen::Unread => (0, 0),
            KitSeen::Kept(c) => (1, c),
            KitSeen::Edited(c) => (2, c),
        };
        let [a, b, c, d] = crc.to_le_bytes();
        [k, a, b, c, d]
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
    put: fn(&Kit, &mut RecordWriter<'_>) -> Result<(), StoreError>,
    read: fn(RecordTag, &[u8], Option<&mut Kit>) -> Result<(), FileError>,
    land: fn(&mut Project, PartId),
    settle: fn(&mut Project, PartId),
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

    /// `kit`'s records after a library Sound's: `Kit`, then a `KitSound`
    /// a pad, as a project's Part writes them.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn put(kit: &Kit, w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
        Self::with(|d| (d.put)(kit, w)).unwrap_or(Ok(()))
    }

    /// A library file's `Kit` or `KitSound` record: checked, and onto
    /// `kit` if given. Any other record is none of its business.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn read(tag: RecordTag, p: &[u8], kit: Option<&mut Kit>) -> Result<(), FileError> {
        Self::with(|d| (d.read)(tag, p, kit)).unwrap_or(Ok(()))
    }

    /// The scratch kit onto Part `id`, marked as loaded, if the Sound it
    /// just loaded is DRUM's.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn land(p: &mut Project, id: PartId) {
        Self::with(|d| (d.land)(p, id));
    }

    /// Part `id`'s kit is the one loaded or saved: its mark is the kit
    /// as it is now.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn settle(p: &mut Project, id: PartId) {
        Self::with(|d| (d.settle)(p, id));
    }

    /// Part `id`'s kit against its mark.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn seen(p: &Project, id: PartId) -> KitSeen {
        Self::with(|d| (d.seen)(p, id)).unwrap_or(KitSeen::Unread)
    }
}
