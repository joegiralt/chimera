//! NEW: the project every load fails back to, and its CRC. Only this
//! module makes a `TemplateCrc`, so one always names a NEW as built.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::factory::{FACTORY_LEN, factory_sound};
use crate::in_place::uninit_at;
use crate::params::EngineType;
use crate::preset::Sound;

use super::{Performance, Pool, Project, ProjectMeta, SlotId, project_crc};

/// The CRC of NEW, as `init_in_place` or `reset_new` built it: a project
/// equal to it is `Pristine`. Nothing else makes one:
///
/// ```compile_fail,E0423
/// let t = chimera_core::project::TemplateCrc(0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateCrc(u32);

impl TemplateCrc {
    #[doc(hidden)]
    pub fn get(self) -> u32 {
        self.0
    }
}

impl Project {
    /// NEW, built in place; `slot` is initialised on return. Its CRC is the
    /// template: a project equal to it is `Pristine`.
    pub fn init_in_place(slot: &mut MaybeUninit<Project>) -> TemplateCrc {
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
        TemplateCrc(project_crc(project))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn boxed() -> (alloc::boxed::Box<Project>, TemplateCrc) {
        let mut raw = alloc::boxed::Box::<Project>::new_uninit();
        let t = Self::init_in_place(&mut raw);
        // SAFETY: `init_in_place` built a valid project in the box.
        (unsafe { raw.assume_init() }, t)
    }

    /// Back to NEW, in place: `+ NEW`, and a failed load's fallback.
    pub(crate) fn reset_new(&mut self) -> TemplateCrc {
        self.meta = ProjectMeta::new_project();
        self.perf.reset();
        self.fill_new_pool();
        TemplateCrc(project_crc(self))
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
    use crate::name::ProjectName;
    use crate::project::{Origin, PartFrom, PartId, PartSource};

    #[test]
    fn reset_new_is_new_and_moves_every_generation() {
        let (mut p, _) = Project::boxed();
        let (fresh, t) = Project::boxed();
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
        assert_eq!(p.reset_new(), t);
        assert_eq!(project_crc(&p), t.get());
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
