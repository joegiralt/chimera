//! A DRUM Part's kit beside its Sound (ADR 0305): in the library's file,
//! and what a library load would lose of it. Bank 1's side: the types
//! and the door to `kern`, which is bank 2's.

mod kern;

use chimera_hal::store::StoreError;

use crate::dsp::drum::types::{Kit, PadSet};
use crate::storage::{Event, FileError, RecordWriter};

use super::codec::KitRead;
use super::{PartId, Project};

/// A kit's CRC: its records', so two kits a card round trip can't tell
/// apart are one. Its own type: never a Sound's CRC. Only `kern` makes
/// one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KitCrc(u32);

/// The kit the library holds for a Part: the one its last library load
/// brought or its last library save wrote, if any. Never stored, and no
/// project load sets it: a project's file is no second copy, as the next
/// save writes over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KitMark(Option<KitCrc>);

impl KitMark {
    /// No library file known to hold the Part's kit.
    pub const NONE: KitMark = KitMark(None);

    #[inline(always)]
    pub(super) fn held(crc: KitCrc) -> KitMark {
        KitMark(Some(crc))
    }
}

/// A Part's kit as a library load's guard reads it, whatever engine the
/// Part plays: where it is kept, and its CRC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KitSeen {
    /// Bank 2 closed: unread, and no load lands a kit.
    Closed,
    /// `Kit::DEFAULT`: nothing to lose.
    Default(KitCrc),
    /// The kit its mark says the library holds.
    InLibrary(KitCrc),
    /// Neither: a library load of a DRUM Sound would lose it.
    Unsaved(KitCrc),
}

impl KitSeen {
    #[inline(always)]
    pub fn unsaved(self) -> bool {
        matches!(self, KitSeen::Unsaved(_))
    }

    /// For a witness: which, then the CRC. Never all zero.
    #[optimize(size)]
    pub(super) fn bytes(self) -> [u8; 5] {
        let (tag, crc) = match self {
            KitSeen::Closed => (1, 0),
            KitSeen::Default(c) => (2, c.0),
            KitSeen::InLibrary(c) => (3, c.0),
            KitSeen::Unsaved(c) => (4, c.0),
        };
        let [a, b, c, d] = crc.to_le_bytes();
        [tag, a, b, c, d]
    }
}

/// A DRUM Part's kit against the library, for the Part's status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KitState {
    /// The default kit, one the library holds, or a kit out of play (the
    /// Part plays another engine, or bank 2 is closed).
    Kept,
    /// In play and in no library file.
    Edited,
}

/// What a library file's records say of a kit, as they come (both
/// passes): the file's engine, the shared `KitRead`, and whether a save
/// would write other bytes. Only `kern` reads or writes it.
#[derive(Clone, Copy, Debug)]
pub struct KitScan {
    drum: bool,
    read: KitRead,
    lossy: bool,
}

impl KitScan {
    pub(super) const NEW: KitScan = KitScan {
        drum: false,
        read: KitRead::NONE,
        lossy: false,
    };
}

/// The kit a library file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKit {
    /// No kit record: a Sound's own save, another engine's, or a file
    /// from before ADR 0305.
    Absent,
    /// A `Kit`, and these pads' own `KitSound`s; the others take the
    /// Sound's (ADR 0269).
    Whole(PadSet),
}

/// A library file's kit, read: what `adopt` lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KitFound {
    kit: FileKit,
    /// A kit record held a value this firmware reads as another.
    lossy: bool,
}

impl KitFound {
    #[inline(always)]
    pub fn kit(self) -> FileKit {
        self.kit
    }

    #[inline(always)]
    pub fn lossy(self) -> bool {
        self.lossy
    }
}

/// A library read's kit, with the door's state: a closed door is a value
/// its caller handles, not a silent nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadKit {
    /// Bank 2 closed: kit records unread, no kit lands.
    Closed,
    Found(KitFound),
}

/// What a library save did with the Part's kit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KitPut {
    /// A DRUM Part's kit, in the file at this CRC.
    Written(KitCrc),
    /// Not a DRUM Part's Sound: no kit belongs in its file.
    NotDrum,
    /// A DRUM Part's, and bank 2 is closed: nothing written.
    Closed,
}

/// `KitDoor`'s mark: a table at the slot that isn't this one fails the
/// boot's door check (`kit_door_ok`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Magic(u32);

const MAGIC: Magic = Magic(0xB2E0_0305);

/// What bank 1 may call of a kit's library records and marks (ADR 0305),
/// all bank 2's with `bank2` (`kern`), at its own slot in the S4 groups
/// (`KIT_DOOR_ADDR`, `bank2.x`): the door's own table is full.
#[repr(C)]
pub struct KitDoor {
    magic: Magic,
    put: fn(&Kit, &mut RecordWriter<'_>) -> Result<KitCrc, StoreError>,
    scan: fn(&mut KitScan, &Event<'_>, Option<&mut Kit>) -> Result<(), FileError>,
    found: fn(&KitScan) -> Result<KitFound, FileError>,
    land: fn(&mut Project, PartId, KitFound),
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
        scan: kern::scan,
        found: kern::found,
        land: kern::land,
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

    /// `kit`'s records after a DRUM library Sound's: a `Kit`, then a
    /// `KitSound` a pad, as a project's Part writes them. `None` closed.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn put(kit: &Kit, w: &mut RecordWriter<'_>) -> Result<Option<KitCrc>, StoreError> {
        Self::with(|d| (d.put)(kit, w)).transpose()
    }

    /// One event of a library file, either pass: the header starts
    /// `scan` anew (and resets `kit`, pass 2's scratch); a `Kit` or
    /// `KitSound` record is checked, and lands on `kit` if given. Closed,
    /// nothing. Out of line: every reader shares the one call.
    #[inline(never)]
    #[optimize(size)]
    pub(super) fn scan(
        scan: &mut KitScan,
        e: &Event<'_>,
        kit: Option<&mut Kit>,
    ) -> Result<(), FileError> {
        Self::with(|d| (d.scan)(scan, e, kit)).unwrap_or(Ok(()))
    }

    /// The file's kit at a pass's end; a kit in part is `Corrupt`.
    #[inline(never)]
    #[optimize(size)]
    pub(super) fn found(scan: &KitScan) -> Result<ReadKit, FileError> {
        match Self::with(|d| (d.found)(scan)) {
            Some(r) => r.map(ReadKit::Found),
            None => Ok(ReadKit::Closed),
        }
    }

    /// The kit a library read found, onto Part `id` after its Sound's
    /// load.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn land(p: &mut Project, id: PartId, found: KitFound) {
        Self::with(|d| (d.land)(p, id, found));
    }

    /// Part `id`'s kit against the library.
    #[inline(always)]
    #[optimize(size)]
    pub(super) fn seen(p: &Project, id: PartId) -> KitSeen {
        Self::with(|d| (d.seen)(p, id)).unwrap_or(KitSeen::Closed)
    }
}
