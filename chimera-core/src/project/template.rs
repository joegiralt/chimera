//! NEW: the project every load fails back to, and its CRC. Only this
//! module makes a `TemplateCrc`, so one always names a NEW as built.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use super::arena::ArenaWriter;
use crate::factory::{factory_entries, new_sound};
use crate::in_place::uninit_at;
use crate::orbit::scene::OrbitSong;
use crate::params::EngineType;
use crate::preset::Sound;
use crate::storage::sound_crc;

use super::parts::new_part;
use super::{
    Origin, Part, PartFrom, PartId, PartSource, Performance, Pool, Project, ProjectMeta, SlotId,
    project_crc,
};

/// The CRC of NEW, as `init_in_place` built it (`reset_new` builds the
/// same project): a project equal to it is `Pristine`. Nothing else makes
/// one:
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
    /// NEW, built in place, its pool's Sounds in `arena`; `slot` is
    /// initialised on return. Its CRC is the template: a project equal to
    /// it is `Pristine`.
    pub fn init_in_place(slot: &mut MaybeUninit<Project>, arena: ArenaWriter) -> TemplateCrc {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; `meta`, `pool`, `perf` and
        // `orbit` (the last three in place) and `rev` are each written once
        // before `assume_init_mut`.
        let project = unsafe {
            addr_of_mut!((*p).meta).write(ProjectMeta::new_project());
            Pool::init_in_place(uninit_at(addr_of_mut!((*p).pool)), arena);
            Performance::init_in_place(uninit_at(addr_of_mut!((*p).perf)));
            OrbitSong::init_in_place(uninit_at(addr_of_mut!((*p).orbit)));
            addr_of_mut!((*p).rev).write(0);
            slot.assume_init_mut()
        };
        project.fill_new();
        TemplateCrc(project_crc(project))
    }

    /// NEW on the heap, on an arena of its own, leaked (host tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn boxed() -> (alloc::boxed::Box<Project>, TemplateCrc) {
        let mut raw = alloc::boxed::Box::<Project>::new_uninit();
        let t = Self::init_in_place(&mut raw, ArenaWriter::leaked());
        // SAFETY: `init_in_place` built a valid project in the box.
        (unsafe { raw.assume_init() }, t)
    }

    /// Back to NEW, in place: `+ NEW`, and a failed load's fallback. No
    /// CRC: the template was taken once, at `init_in_place`, and a hash
    /// here is a whole project's (about 0.5–0.9 ms on the chip).
    pub(crate) fn reset_new(&mut self) {
        self.bump();
        self.meta = ProjectMeta::new_project();
        self.perf.reset();
        self.orbit.reset();
        self.fill_new();
    }

    /// The pool: every source out (retired if it sounds, compacted
    /// through the gate if not), emptied, every generation moved and the
    /// arena's Sounds dropped; then the factory Sounds, then an INIT Sound
    /// per engine. Then Parts 1–5 load theirs from it, clean (`NEW_PARTS`).
    /// Never panics: a factory Sound a sounding source still leaves no
    /// room for is skipped (`take_new_skipped` counts them), and a Part
    /// whose slot is missing or won't load loads INIT.
    fn fill_new(&mut self) {
        self.pool.sources_clear();
        self.pool.clear_all();
        let mut skipped = 0;
        for (s, (_, bytes)) in SlotId::ALL.into_iter().zip(factory_entries()) {
            if self.pool.store_bytes(s, bytes).is_err() {
                skipped += 1;
            }
        }
        self.pool.new_skipped = skipped;
        for (part, slot) in PartId::ALL.into_iter().zip(NEW_PARTS) {
            let init = PartFrom::Init(EngineType::Algo);
            let from = match self.pool.get(slot) {
                Some(_) => PartFrom::Slot(slot),
                None => init,
            };
            if self.load_part(PartSource { part, from }).is_err() {
                // INIT never fails.
                let _ = self.load_part(PartSource { part, from: init });
            }
        }
    }

    /// Factory Sounds the last NEW had no room for, once: a sounding
    /// source held their bytes. `None` when NEW came up whole.
    pub fn take_new_skipped(&mut self) -> Option<u8> {
        let n = core::mem::take(&mut self.pool.new_skipped);
        (n > 0).then_some(n)
    }
}

/// The factory slots NEW's Parts 1–5 load, for the init scene's rings
/// (the owner's pick at UAT): TX BASS, TX EPIANO, MORPH PAD, TX BELL and
/// SAW LEAD. Part 6 stays INIT Algo.
pub const NEW_PARTS: [SlotId; 5] = {
    let s = SlotId::ALL;
    [s[0], s[1], s[6], s[3], s[4]]
};

/// NEW's Part `i`, as `fill_new` leaves it but for the generation, which
/// no record keeps.
pub(super) fn new_project_part(i: usize) -> Part {
    let mut part = new_part(i);
    if let Some(&slot) = NEW_PARTS.get(i)
        && let Some(sound) = new_slot(slot)
    {
        part.origin = Origin::Slot {
            slot,
            generation: 0,
            crc: sound_crc(&sound),
        };
        part.sound = sound;
    }
    part
}

/// NEW's Sound in slot `s`.
pub(super) fn new_slot(s: SlotId) -> Option<Sound> {
    new_sound(s.index())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::ProjectName;
    use crate::params::EngineType;
    use crate::project::{
        Origin, PartFrom, PartId, PartSource, PartStatus, ProjectStatus, part_status,
        project_status,
    };

    /// The owner's UAT pick: the init scene's Sounds, loaded clean, and NEW
    /// still reads as NEW.
    #[test]
    fn new_parts_hold_the_factory_sounds_clean() {
        let (p, t) = Project::boxed();
        let names = ["TX BASS", "TX EPIANO", "MORPH PAD", "TX BELL", "SAW LEAD"];
        for (id, name) in PartId::ALL.into_iter().zip(names) {
            let part = p.part(id);
            assert_eq!(part.sound.name.as_str(), name, "{id:?}");
            assert!(matches!(part.origin(), Origin::Slot { .. }), "{id:?}");
            assert_eq!(part_status(part, p.pool()), PartStatus::Clean, "{id:?}");
        }
        let six = p.part(PartId::ALL[5]);
        assert_eq!(six.origin(), Origin::Init(EngineType::Algo));
        assert!(six.sound.bits_eq(&Sound::init(EngineType::Algo)));
        assert_eq!(project_status(&p, t), ProjectStatus::Pristine);
    }

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
        p.pool_store(SlotId::ALL[25], &Sound::init(EngineType::Modal))
            .unwrap();
        let before = SlotId::ALL.map(|s| p.pool().generation(s));
        p.reset_new();
        assert_eq!(project_crc(&p), t.get());
        assert_eq!(p.meta(), fresh.meta());
        assert_eq!(p.pool().used(), fresh.pool().used());
        for s in SlotId::ALL {
            assert_eq!(p.pool().get(s).is_some(), fresh.pool().get(s).is_some());
            assert_eq!(p.pool().get(s), fresh.pool().get(s));
            assert_ne!(p.pool().generation(s), before[s.index()]);
        }
        // By slot and CRC, never generation (ADR 0043).
        let from = |o: Origin| match o {
            Origin::Slot { slot, crc, .. } => Ok((slot, crc)),
            Origin::Init(e) => Err(e),
        };
        for id in PartId::ALL {
            assert_eq!(from(p.part(id).origin()), from(fresh.part(id).origin()));
            assert!(p.part(id).sound.bits_eq(&fresh.part(id).sound));
            assert_eq!(p.part(id).mix, fresh.part(id).mix);
        }
        assert_eq!(p.perf().fx.delay.mix, fresh.perf().fx.delay.mix);
    }
}
