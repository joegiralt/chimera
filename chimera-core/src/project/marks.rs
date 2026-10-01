//! The marks, derived and never stored (projects spec § Derived marks): a
//! Part's `*` and `◦` from its Sound, Origin and slot; the project's `*`
//! from its CRC. And the Part actions, offered only where they apply.

use crate::preset::Sound;
use crate::storage::sound_crc;

use super::{Origin, Part, PartFrom, PartId, PartSet, PartSource, Pool, Project, SlotId};
use super::{TemplateCrc, project_crc};

/// A Part against its slot: `Edited` is `*`, `Stale` is `◦`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartStatus {
    Clean,
    Edited,
    /// As loaded, but the slot moved on under it.
    Stale(SlotId),
}

pub fn part_status(part: &Part, pool: &Pool) -> PartStatus {
    let (slot, generation, crc) = match part.origin {
        Origin::Init(e) if part.sound.bits_eq(&Sound::init(e)) => return PartStatus::Clean,
        Origin::Init(_) => return PartStatus::Edited,
        Origin::Slot {
            slot,
            generation,
            crc,
        } => (slot, generation, crc),
    };
    if pool.get(slot).is_some_and(|s| part.sound.bits_eq(s)) {
        PartStatus::Clean
    } else if pool.generation(slot) == generation || sound_crc(&part.sound) != crc {
        PartStatus::Edited
    } else {
        PartStatus::Stale(slot)
    }
}

/// The project against NEW and its last save or load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectStatus {
    Pristine,
    Saved,
    Modified,
}

pub fn project_status(p: &Project, t: TemplateCrc) -> ProjectStatus {
    status_at(p, t, project_crc(p))
}

/// `project_status` given the project's CRC, for a caller that needs both.
pub(super) fn status_at(p: &Project, t: TemplateCrc, crc: u32) -> ProjectStatus {
    if crc == t.get() {
        ProjectStatus::Pristine
    } else if p.meta.saved_crc == Some(crc) {
        ProjectStatus::Saved
    } else {
        ProjectStatus::Modified
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartActionKind {
    /// Save the Part over its slot.
    OverSlot(SlotId),
    /// Save the Part to this free slot.
    NewSlot(SlotId),
    /// Copy the slot back; a Stale Part's UPDATE.
    Revert(SlotId),
}

/// An action `part_actions` offered, with what it was offered against: the
/// Part's Origin, status and `sound_crc`. Only `part_actions` makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartAction {
    part: PartId,
    kind: PartActionKind,
    origin: Origin,
    status: PartStatus,
    crc: u32,
}

impl PartAction {
    pub fn kind(self) -> PartActionKind {
        self.kind
    }
}

/// At most three, in menu order.
pub struct PartActions {
    items: [Option<PartAction>; 3],
}

impl PartActions {
    pub fn iter(&self) -> impl Iterator<Item = PartAction> + '_ {
        self.items.iter().flatten().copied()
    }
}

/// What applies to `part` now (§ Copy rules). A Stale Part never offers
/// `OverSlot`: that would undo the other Part's save.
pub fn part_actions(p: &Project, part: PartId) -> PartActions {
    let x = p.part(part);
    let status = part_status(x, &p.pool);
    let crc = sound_crc(&x.sound);
    let free = p.pool.first_free().map(PartActionKind::NewSlot);
    let filled = |s: SlotId| p.pool.get(s).is_some().then_some(PartActionKind::Revert(s));
    let kinds = match (status, x.origin) {
        (PartStatus::Edited, Origin::Slot { slot, .. }) => {
            [Some(PartActionKind::OverSlot(slot)), free, filled(slot)]
        }
        (PartStatus::Stale(s), _) => [filled(s), free, None],
        (PartStatus::Clean | PartStatus::Edited, _) => [free, None, None],
    };
    PartActions {
        items: kinds.map(|k| {
            k.map(|kind| PartAction {
                part,
                kind,
                origin: x.origin,
                status,
                crc,
            })
        }),
    }
}

/// The action no longer applies: its Part's status, Origin or Sound changed,
/// its new slot filled, or its revert slot emptied.
#[derive(Debug, PartialEq)]
pub struct ActionGone;

impl Project {
    /// Ok: the other users of the slot that now derive Stale (as `save_part_to`).
    pub fn apply_part_action(&mut self, a: PartAction) -> Result<PartSet, ActionGone> {
        let x = self.part(a.part);
        if x.origin != a.origin
            || part_status(x, &self.pool) != a.status
            || sound_crc(&x.sound) != a.crc
        {
            return Err(ActionGone);
        }
        match a.kind {
            PartActionKind::OverSlot(s) => Ok(self.save_part_to(a.part, s)),
            PartActionKind::NewSlot(s) if self.pool.get(s).is_none() => {
                Ok(self.save_part_to(a.part, s))
            }
            PartActionKind::Revert(s) => {
                let src = PartSource {
                    part: a.part,
                    from: PartFrom::Slot(s),
                };
                self.load_part(src).map_err(|_| ActionGone)?;
                Ok(PartSet::EMPTY)
            }
            PartActionKind::NewSlot(_) => Err(ActionGone),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn mark_saved_for_test(&mut self) {
        self.meta.saved_crc = Some(project_crc(self));
    }
}
