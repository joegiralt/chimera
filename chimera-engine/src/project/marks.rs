//! The marks, derived and never stored (projects spec § Derived marks): a
//! Part's `*` and `◦` from its Sound, Origin and slot; the project's `*`
//! from its CRC. And the Part actions, offered only where they apply.

use crate::preset::Sound;
use crate::storage::sound_crc;

use super::arena::Cost;
use super::pool::entry_len;
use super::{Origin, Part, PartFrom, PartId, PartSet, PartSource, Pool, PoolFull, Project, SlotId};
use super::{TemplateCrc, project_crc};

/// A Part against its slot: `Edited` is `*`, `Stale` is `◦`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartStatus {
    Clean,
    Edited,
    /// As loaded, but the slot moved on under it.
    Stale(SlotId),
}

#[optimize(size)]
pub fn part_status(part: &Part, pool: &Pool) -> PartStatus {
    let s = slot_status(part, pool);
    // KEEP MAP's rows over a load are no edit (ADR 0253); a slot moved on
    // since the load still reads Stale.
    #[cfg(feature = "controller-maps")]
    if s == PartStatus::Edited && part.map_kept == Some(sound_crc(&part.sound)) {
        return match part.origin {
            Origin::Slot {
                slot, generation, ..
            } if pool.generation(slot) != generation => PartStatus::Stale(slot),
            _ => PartStatus::Clean,
        };
    }
    s
}

/// `part_status` but for KEEP MAP.
#[optimize(size)]
fn slot_status(part: &Part, pool: &Pool) -> PartStatus {
    let (slot, generation, crc) = match part.origin {
        Origin::Init(e) if is_init(&part.sound, e) => return PartStatus::Clean,
        Origin::Init(_) => return PartStatus::Edited,
        Origin::Slot {
            slot,
            generation,
            crc,
        } => (slot, generation, crc),
    };
    if pool.get(slot).is_some_and(|s| s.matches(&part.sound)) {
        PartStatus::Clean
    } else if pool.generation(slot) == generation || sound_crc(&part.sound) != crc {
        PartStatus::Edited
    } else {
        PartStatus::Stale(slot)
    }
}

/// Whether `s` is engine `e`'s INIT. Out of line: the INIT Sound's frame
/// is not under `part_status`'s CRC (ADR 0025's stack check).
#[inline(never)]
#[optimize(size)]
fn is_init(s: &Sound, e: crate::params::EngineType) -> bool {
    s.bits_eq(&Sound::init(e))
}

/// The project against NEW and its last save or load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectStatus {
    Pristine,
    Saved,
    Modified,
}

#[optimize(size)]
pub fn project_status(p: &Project, t: TemplateCrc) -> ProjectStatus {
    status_at(p, t, project_crc(p))
}

/// `project_status` given the project's CRC, for a caller that needs both.
#[optimize(size)]
pub(super) fn status_at(p: &Project, t: TemplateCrc, crc: u32) -> ProjectStatus {
    if crc == t.get() {
        ProjectStatus::Pristine
    } else if p.meta.saved_crc == Some(crc) {
        ProjectStatus::Saved
    } else {
        ProjectStatus::Modified
    }
}

/// `project_status`, hashed once per `Project::rev`: the CRC costs about
/// 1.7 ms on the chip, so no frame may run it. It serves one `Project`
/// instance: a whole project assigned over it brings its own `rev`, so
/// whoever hands out that `&mut Project` calls `invalidate` first.
#[derive(Clone, Copy, Debug)]
pub struct StatusCache {
    rev: Option<u32>,
    status: ProjectStatus,
    /// Hashes run, for tests that pin "once per revision".
    #[cfg(any(test, feature = "test-support"))]
    hashes: u32,
}

impl Default for StatusCache {
    #[optimize(size)]
    fn default() -> Self {
        Self::new()
    }
}

impl StatusCache {
    #[optimize(size)]
    pub const fn new() -> Self {
        StatusCache {
            rev: None,
            status: ProjectStatus::Pristine,
            #[cfg(any(test, feature = "test-support"))]
            hashes: 0,
        }
    }

    /// The next `get` hashes, whatever the revision.
    #[optimize(size)]
    pub fn invalidate(&mut self) {
        self.rev = None;
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[optimize(size)]
    pub fn hashes_for_test(&self) -> u32 {
        self.hashes
    }

    /// Hashes with `crc` only when `p.rev()` moved since the last call.
    #[optimize(size)]
    pub fn get_with(
        &mut self,
        p: &Project,
        t: TemplateCrc,
        crc: impl FnOnce(&Project) -> u32,
    ) -> ProjectStatus {
        if self.rev != Some(p.rev()) {
            self.status = status_at(p, t, crc(p));
            self.rev = Some(p.rev());
            #[cfg(any(test, feature = "test-support"))]
            {
                self.hashes += 1;
            }
        } else {
            // A `&mut` path that skipped `bump` shows up here.
            debug_assert_eq!(
                status_at(p, t, project_crc(p)),
                self.status,
                "the project changed without its revision moving"
            );
        }
        self.status
    }

    #[optimize(size)]
    pub fn get(&mut self, p: &Project, t: TemplateCrc) -> ProjectStatus {
        self.get_with(p, t, project_crc)
    }

    /// The last result while it is current; otherwise a hash, not kept.
    #[optimize(size)]
    pub fn peek(&self, p: &Project, t: TemplateCrc) -> ProjectStatus {
        if self.rev == Some(p.rev()) {
            self.status
        } else {
            project_status(p, t)
        }
    }

    /// The last result; `Pristine` before any.
    #[optimize(size)]
    pub fn cached(&self) -> ProjectStatus {
        self.status
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
    #[optimize(size)]
    pub fn kind(self) -> PartActionKind {
        self.kind
    }
}

/// At most three, in menu order, with what they were weighed by.
pub struct PartActions {
    items: [Option<PartAction>; 3],
    status: PartStatus,
    save_needs: Cost,
}

impl PartActions {
    #[optimize(size)]
    pub fn iter(&self) -> impl Iterator<Item = PartAction> + '_ {
        self.items.iter().flatten().copied()
    }

    /// The Part's status as they were offered against it.
    #[optimize(size)]
    pub fn status(&self) -> PartStatus {
        self.status
    }

    /// What the Part's save takes from the gap, offered or not: over its
    /// slot when it is Edited from one, else a whole new slot.
    #[optimize(size)]
    pub fn save_needs(&self) -> Cost {
        self.save_needs
    }
}

/// What applies to `part` now (§ Copy rules). A Stale Part never offers
/// `OverSlot`: that would undo the other Part's save. An offer that
/// can't apply isn't made (ADR 0043): a save that won't fit is no save.
#[optimize(size)]
pub fn part_actions(p: &Project, part: PartId) -> PartActions {
    let x = p.part(part);
    let status = part_status(x, &p.pool);
    let crc = sound_crc(&x.sound);
    // Counted once: every save below weighs the same bytes.
    let len = entry_len(&x.sound, &p.kept[part.index()]);
    let new_needs = Cost::of(len);
    let free = p
        .pool
        .first_free()
        .filter(|_| p.pool.fits(new_needs))
        .map(PartActionKind::NewSlot);
    let filled = |s: SlotId| p.pool.get(s).is_some().then_some(PartActionKind::Revert(s));
    let (kinds, save_needs) = match (status, x.origin) {
        (PartStatus::Edited, Origin::Slot { slot, .. }) => {
            let over = p.pool.needs_len(slot, len);
            let kinds = [
                p.pool.fits(over).then_some(PartActionKind::OverSlot(slot)),
                free,
                filled(slot),
            ];
            (kinds, over)
        }
        (PartStatus::Stale(s), _) => ([filled(s), free, None], new_needs),
        (PartStatus::Clean | PartStatus::Edited, _) => ([free, None, None], new_needs),
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
        status,
        save_needs,
    }
}

/// Why a Part action didn't run.
#[derive(Debug, PartialEq)]
pub enum ActionRefused {
    /// It no longer applies: its Part's status, Origin or Sound changed,
    /// its new slot filled, or its revert slot emptied.
    Gone,
    /// The Sound won't fit the pool's free bytes.
    Full(PoolFull),
}

impl Project {
    /// Ok: the other users of the slot that now derive Stale (as `save_part_to`).
    #[optimize(size)]
    pub fn apply_part_action(&mut self, a: PartAction) -> Result<PartSet, ActionRefused> {
        self.bump();
        let x = self.part(a.part);
        if x.origin != a.origin
            || part_status(x, &self.pool) != a.status
            || sound_crc(&x.sound) != a.crc
        {
            return Err(ActionRefused::Gone);
        }
        match a.kind {
            PartActionKind::OverSlot(s) => {
                self.save_part_to(a.part, s).map_err(ActionRefused::Full)
            }
            PartActionKind::NewSlot(s) if self.pool.get(s).is_none() => {
                self.save_part_to(a.part, s).map_err(ActionRefused::Full)
            }
            PartActionKind::Revert(s) => {
                let src = PartSource {
                    part: a.part,
                    from: PartFrom::Slot(s),
                };
                self.load_part(src).map_err(|_| ActionRefused::Gone)?;
                Ok(PartSet::EMPTY)
            }
            PartActionKind::NewSlot(_) => Err(ActionRefused::Gone),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[optimize(size)]
    pub fn mark_saved_for_test(&mut self) {
        self.bump();
        self.meta.saved_crc = Some(project_crc(self));
    }
}
