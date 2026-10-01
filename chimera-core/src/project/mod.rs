//! A project: its name, its pool of Sounds and the Parts that play them
//! (projects spec § Model). The fields are private, so only this module
//! sets an `Origin` or moves a slot's generation.

mod ids;
mod parts;
mod pool;

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

pub use ids::{PartId, PartSet, SlotId};
pub use parts::{Origin, Part, PartEdit, Performance, part_block, part_block_mut};
pub use pool::Pool;

use crate::dsp::fx_bus::FxParams;
use crate::factory::{FACTORY_LEN, factory_sound};
use crate::in_place::uninit_at;
use crate::name::ProjectName;
use crate::params::EngineType;
use crate::preset::Sound;
use crate::storage::{ProjectId, sound_crc};

pub const NEW_NAME: &str = "NEW PROJECT";

/// What a project is besides its Sounds and Parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectMeta {
    id: Option<ProjectId>,
    name: ProjectName,
    /// The canonical CRC at the last save or load.
    saved_crc: Option<u32>,
}

impl ProjectMeta {
    fn new_project() -> Self {
        ProjectMeta {
            id: None,
            name: ProjectName::new(NEW_NAME).expect("a valid name"),
            saved_crc: None,
        }
    }

    pub fn id(&self) -> Option<ProjectId> {
        self.id
    }

    pub fn name(&self) -> ProjectName {
        self.name
    }

    pub fn saved_crc(&self) -> Option<u32> {
        self.saved_crc
    }
}

/// What a Part is loaded from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartFrom {
    Slot(SlotId),
    Init(EngineType),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartSource {
    pub part: PartId,
    pub from: PartFrom,
}

/// The slot's users: a slot a Part plays isn't cleared.
#[derive(Debug, PartialEq)]
pub struct InUse(pub PartSet);

#[derive(Debug, PartialEq)]
pub enum ReplaceError {
    SlotEmpty,
    /// The slot moved since the replace was asked for.
    Changed,
}

pub struct Project {
    meta: ProjectMeta,
    pool: Pool,
    perf: Performance,
}

crate::in_place::field_list!(Project => Project { meta, pool, perf });

impl Project {
    /// NEW, built in place.
    pub fn init_in_place(slot: &mut MaybeUninit<Project>) -> &mut Project {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; `meta`, `pool` (in place) and
        // `perf` are each written once before `assume_init_mut`.
        let project = unsafe {
            addr_of_mut!((*p).meta).write(ProjectMeta::new_project());
            Pool::init_in_place(uninit_at(addr_of_mut!((*p).pool)));
            addr_of_mut!((*p).perf).write(Performance::new());
            slot.assume_init_mut()
        };
        project.fill_new_pool();
        project
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn boxed() -> alloc::boxed::Box<Project> {
        let mut raw = alloc::boxed::Box::<Project>::new_uninit();
        Self::init_in_place(&mut raw);
        // SAFETY: `init_in_place` built a valid project in the box.
        unsafe { raw.assume_init() }
    }

    pub fn meta(&self) -> &ProjectMeta {
        &self.meta
    }

    pub fn perf(&self) -> &Performance {
        &self.perf
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    pub fn part(&self, p: PartId) -> &Part {
        self.perf.part(p)
    }

    pub fn edit_part(&mut self, p: PartId) -> PartEdit<'_> {
        self.perf.edit(p)
    }

    pub fn edit_fx(&mut self) -> &mut FxParams {
        &mut self.perf.fx
    }

    pub fn set_name(&mut self, n: ProjectName) {
        self.meta.name = n;
    }

    /// The Parts whose Origin is slot `s`.
    pub fn users(&self, s: SlotId) -> PartSet {
        PartId::ALL
            .into_iter()
            .filter(|&p| matches!(self.part(p).origin, Origin::Slot { slot, .. } if slot == s))
            .fold(PartSet::EMPTY, PartSet::with)
    }

    pub fn pool_store(&mut self, s: SlotId, sound: Sound) {
        self.pool.store(s, sound);
    }

    pub fn pool_clear(&mut self, s: SlotId) -> Result<(), InUse> {
        let users = self.users(s);
        if !users.is_empty() {
            return Err(InUse(users));
        }
        self.pool.clear(s);
        Ok(())
    }

    /// Stores a copy of p's sound in s and sets p's Origin fresh. Returns the
    /// other users of s that now derive Stale(s).
    pub fn save_part_to(&mut self, p: PartId, s: SlotId) -> PartSet {
        let others = self.users(s);
        let sound = self.part(p).sound.clone();
        let crc = sound_crc(&sound);
        self.pool.store(s, sound);
        self.perf.parts[p.index()].origin = Origin::Slot {
            slot: s,
            generation: self.pool.generation(s),
            crc,
        };
        others
            .iter()
            .filter(|&o| o != p && self.stale(o))
            .fold(PartSet::EMPTY, PartSet::with)
    }

    /// `part_status`'s Stale: the Part differs from its slot, the slot moved,
    /// and the Part is as it was loaded.
    fn stale(&self, p: PartId) -> bool {
        let part = self.part(p);
        let Origin::Slot {
            slot,
            generation,
            crc,
        } = part.origin
        else {
            return false;
        };
        !self.pool.get(slot).is_some_and(|s| part.sound.bits_eq(s))
            && self.pool.generation(slot) != generation
            && sound_crc(&part.sound) == crc
    }

    /// Replaces the Part's Sound; its mix stays.
    pub fn load_part(&mut self, src: PartSource) -> Result<(), ReplaceError> {
        let part = &mut self.perf.parts[src.part.index()];
        match src.from {
            PartFrom::Slot(s) => {
                let sound = self.pool.get(s).ok_or(ReplaceError::SlotEmpty)?;
                part.sound.clone_from(sound);
                part.origin = Origin::Slot {
                    slot: s,
                    generation: self.pool.generation(s),
                    crc: sound_crc(sound),
                };
            }
            PartFrom::Init(e) => {
                part.sound = Sound::init(e);
                part.origin = Origin::Init(e);
            }
        }
        Ok(())
    }

    /// Back to NEW, in place.
    // A failed load's fallback, which the load protocol calls.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn reset_new(&mut self) {
        self.meta = ProjectMeta::new_project();
        self.perf.reset();
        self.fill_new_pool();
    }

    /// The factory Sounds, then an INIT Sound per engine, then empty slots;
    /// a slot at a time, and every generation moves.
    fn fill_new_pool(&mut self) {
        for s in SlotId::ALL {
            let i = s.index();
            let sound = match i.checked_sub(FACTORY_LEN) {
                None => factory_sound(i),
                Some(j) => EngineType::ALL.get(j).map(|&e| Sound::init(e)),
            };
            match sound {
                Some(sound) => self.pool.store(s, sound),
                None => self.pool.clear(s),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_new_is_new_and_moves_every_generation() {
        let mut p = Project::boxed();
        let fresh = Project::boxed();
        let s = SlotId::ALL[0];
        p.set_name(ProjectName::new("OLD").unwrap());
        p.load_part(PartSource {
            part: PartId::ALL[2],
            from: PartFrom::Slot(s),
        })
        .unwrap();
        p.edit_fx().delay.mix = 0.5;
        p.pool_store(SlotId::ALL[25], Sound::init(EngineType::Modal));
        let before = SlotId::ALL.map(|s| p.pool().generation(s));
        p.reset_new();
        assert_eq!(p.meta(), fresh.meta());
        assert_eq!(p.pool().used(), fresh.pool().used());
        for s in SlotId::ALL {
            assert_eq!(p.pool().get(s).is_some(), fresh.pool().get(s).is_some());
            if let (Some(a), Some(b)) = (p.pool().get(s), fresh.pool().get(s)) {
                assert!(a.bits_eq(b));
            }
            assert_ne!(p.pool().generation(s), before[s.index()]);
        }
        for id in PartId::ALL {
            assert_eq!(p.part(id).origin(), Origin::Init(EngineType::Algo));
            assert!(p.part(id).sound.bits_eq(&fresh.part(id).sound));
            assert_eq!(p.part(id).mix, fresh.part(id).mix);
        }
        assert_eq!(p.perf().fx.delay.mix, fresh.perf().fx.delay.mix);
    }
}
