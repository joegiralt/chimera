//! KEY's table (ADR 0303): `kern::run` links into an S4 group, reached
//! through `KeyDoor` at its own address a stride after the stereo's
//! (`COMP_KEY_DOOR_ADDR`). Without `bank2` it is bank 1's, called
//! directly. A closed door plays FULL.

use super::{Ask, Door, Pairs};

/// KEY's block as the table carries it (`kern::run`).
pub(crate) type KeyRun = for<'a, 'b> fn(Ask<'a, 'b>) -> Pairs;

/// `KeyDoor`'s entry: called, never handed out.
#[repr(transparent)]
pub struct KeyEntry(KeyRun);

impl KeyEntry {
    #[inline(always)]
    pub(crate) fn run(&self, ask: Ask<'_, '_>) -> Pairs {
        (self.0)(ask)
    }
}

/// `KeyDoor`'s mark: a table at the slot that isn't this one fails the
/// boot's door check (`door_ok`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Magic(u32);

const MAGIC: Magic = Magic(0xB2E0_0303);

/// KEY's table, at its own slot (`COMP_KEY_DOOR_ADDR`, `bank2.x`).
#[repr(C)]
pub struct KeyDoor {
    magic: Magic,
    key: KeyEntry,
}

impl KeyDoor {
    const OWN: KeyDoor = KeyDoor {
        magic: MAGIC,
        key: KeyEntry(super::kern::run),
    };

    #[inline(always)]
    pub(crate) fn key(&self) -> &KeyEntry {
        &self.key
    }
}

#[cfg(feature = "bank2")]
// SAFETY: `bank2.x` links `KEY_DOOR`, a `KeyDoor`, at `COMP_KEY_DOOR_ADDR`
// in every firmware build with `bank2` (its assert,
// `__bank2_comp_key_door`).
const SLOT: crate::bank2::DoorSlot<KeyDoor> =
    unsafe { crate::bank2::DoorSlot::new(crate::bank2::COMP_KEY_DOOR_ADDR) };

#[cfg(feature = "bank2")]
// SAFETY: the section is `bank2.x`'s own slot for this table, which it
// keeps and places at `COMP_KEY_DOOR_ADDR`; nothing else is linked there.
#[unsafe(link_section = ".bank2.door.comp_key")]
#[used]
static KEY_DOOR: KeyDoor = KeyDoor::OWN;

/// KEY's table in judged door `j` answers (the boot's door check, run
/// before the door opens).
#[cfg(feature = "bank2")]
pub fn door_ok(j: &crate::bank2::Judged) -> bool {
    j.with_slot(SLOT, |d| d.magic == MAGIC)
}

/// The door as KEY finds it now: closed, every key plays FULL
/// (`CompKey::in_effect`).
#[inline(always)]
pub fn state() -> Door {
    match through(|_| ()) {
        Some(()) => Door::Open,
        None => Door::Closed,
    }
}

/// `f` on KEY's table: bank 2's through the door on the unit, `None`
/// closed; this build's own elsewhere. A host test under `bank2` answers
/// as the unit does: only while the door cell is open.
#[inline(always)]
pub(crate) fn through<R>(f: impl FnOnce(&KeyDoor) -> R) -> Option<R> {
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
        Some(f(&KeyDoor::OWN))
    }
}
