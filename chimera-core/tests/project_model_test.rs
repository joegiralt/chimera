//! The project model (projects spec § Model): typed ids, the pool's
//! generations, NEW's contents and the Part origins.

use chimera_core::MidiChannel;
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{
    InUse, Origin, PartFrom, PartId, PartSet, PartSource, Project, ReplaceError, ReplaceGuard,
    SlotId, TemplateCrc,
};
use chimera_core::storage::sound_crc;

/// A load into a Clean or Stale Part: the guard lets it through.
fn load(p: &mut Project, t: TemplateCrc, src: PartSource) -> Result<(), ReplaceError> {
    let c = ReplaceGuard::check(p, t, src).expect("a Clean or Stale Part");
    p.replace_part(c)
}

#[test]
fn ids_are_bounded() {
    assert_eq!(PartId::new(5).map(PartId::index), Some(5));
    assert_eq!(PartId::new(6), None);
    assert_eq!(SlotId::new(31).map(SlotId::index), Some(31));
    assert_eq!(SlotId::new(32), None);
    assert_eq!((PartId::ALL.len(), SlotId::ALL.len()), (6, 32));
}

#[test]
fn part_set_holds_what_it_was_given() {
    let (a, b) = (PartId::ALL[1], PartId::ALL[4]);
    let s = PartSet::EMPTY.with(a).with(b).with(a);
    assert_eq!(s.len(), 2);
    assert!(s.contains(a) && s.contains(b) && !s.contains(PartId::ALL[0]));
    assert!(s.iter().eq([a, b]));
    assert_eq!(PartSet::EMPTY.len(), 0);
}

#[test]
fn new_project_contents() {
    let p = Project::boxed().0;
    assert_eq!(p.meta().name().as_str(), "NEW PROJECT");
    assert_eq!((p.meta().id(), p.meta().saved_crc()), (None, None));
    for i in 0..FACTORY_LEN {
        assert!(
            p.pool()
                .get(SlotId::ALL[i])
                .unwrap()
                .bits_eq(&factory_sound(i).unwrap())
        );
    }
    assert!(
        p.pool()
            .get(SlotId::ALL[8])
            .unwrap()
            .bits_eq(&Sound::init(EngineType::Algo))
    );
    assert!(
        p.pool()
            .get(SlotId::ALL[9])
            .unwrap()
            .bits_eq(&Sound::init(EngineType::Modal))
    );
    assert_eq!(p.pool().used(), 10);
    assert_eq!(p.pool().first_free(), SlotId::new(10));
    for id in PartId::ALL {
        assert_eq!(p.part(id).origin(), Origin::Init(EngineType::Algo));
        assert!(p.part(id).sound.bits_eq(&Sound::init(EngineType::Algo)));
    }
}

#[test]
fn store_and_clear_bump_the_generation() {
    let mut p = Project::boxed().0;
    let s = SlotId::ALL[20];
    let g = p.pool().generation(s);
    p.pool_store(s, Sound::init(EngineType::Modal));
    assert_eq!(p.pool().generation(s), g.wrapping_add(1));
    assert_eq!(p.pool_clear(s), Ok(()));
    assert_eq!(p.pool().generation(s), g.wrapping_add(2));
    assert!(p.pool().get(s).is_none());
    // Clearing an empty slot still moves it.
    assert_eq!(p.pool_clear(s), Ok(()));
    assert_eq!(p.pool().generation(s), g.wrapping_add(3));
}

#[test]
fn clear_refuses_a_used_slot() {
    let (mut p, t) = Project::boxed();
    let (a, b, s) = (PartId::ALL[1], PartId::ALL[3], SlotId::ALL[0]);
    for part in [a, b] {
        load(
            &mut p,
            t,
            PartSource {
                part,
                from: PartFrom::Slot(s),
            },
        )
        .unwrap();
    }
    assert_eq!(p.users(s), PartSet::EMPTY.with(a).with(b));
    let g = p.pool().generation(s);
    assert_eq!(p.pool_clear(s), Err(InUse(PartSet::EMPTY.with(a).with(b))));
    assert!(p.pool().get(s).is_some());
    assert_eq!(p.pool().generation(s), g);
}

#[test]
fn load_sets_origin_from_the_slot() {
    let (mut p, t) = Project::boxed();
    let (part, s) = (PartId::ALL[0], SlotId::ALL[2]);
    load(
        &mut p,
        t,
        PartSource {
            part,
            from: PartFrom::Slot(s),
        },
    )
    .unwrap();
    let crc = sound_crc(p.pool().get(s).unwrap());
    assert_eq!(
        p.part(part).origin(),
        Origin::Slot {
            slot: s,
            generation: p.pool().generation(s),
            crc
        }
    );
    assert!(p.part(part).sound.bits_eq(p.pool().get(s).unwrap()));
    let empty = PartSource {
        part,
        from: PartFrom::Slot(SlotId::ALL[30]),
    };
    assert_eq!(load(&mut p, t, empty), Err(ReplaceError::SlotEmpty));
    assert_eq!(
        p.part(part).origin(),
        Origin::Slot {
            slot: s,
            generation: p.pool().generation(s),
            crc
        }
    );
}

#[test]
fn load_init_keeps_the_mix() {
    let (mut p, t) = Project::boxed();
    let part = PartId::ALL[3];
    p.edit_part(part).mix.level = 0.25;
    p.edit_part(part).mix.channel = MidiChannel::new(9).unwrap();
    load(
        &mut p,
        t,
        PartSource {
            part,
            from: PartFrom::Init(EngineType::Modal),
        },
    )
    .unwrap();
    assert_eq!(p.part(part).origin(), Origin::Init(EngineType::Modal));
    assert!(p.part(part).sound.bits_eq(&Sound::init(EngineType::Modal)));
    assert_eq!(p.part(part).mix.level, 0.25);
    assert_eq!(p.part(part).mix.channel.get(), 9);
}

#[test]
fn save_part_to_returns_only_the_parts_now_stale() {
    let (mut p, t) = Project::boxed();
    let s = SlotId::ALL[0];
    let [a, b, c] = [PartId::ALL[0], PartId::ALL[2], PartId::ALL[4]];
    for part in [a, b, c] {
        load(
            &mut p,
            t,
            PartSource {
                part,
                from: PartFrom::Slot(s),
            },
        )
        .unwrap();
    }
    p.edit_part(c).sound.params.filter.cutoff *= 0.5; // c is Edited
    p.edit_part(a).sound.params.filter.cutoff *= 0.25;
    assert_eq!(p.save_part_to(a, s), PartSet::EMPTY.with(b)); // c, edited, isn't Stale
    let crc = sound_crc(p.pool().get(s).unwrap());
    assert_eq!(
        p.part(a).origin(),
        Origin::Slot {
            slot: s,
            generation: p.pool().generation(s),
            crc
        }
    );
    assert!(p.part(a).sound.bits_eq(p.pool().get(s).unwrap()));
}

#[test]
fn save_part_to_an_empty_slot_has_no_other_users() {
    let mut p = Project::boxed().0;
    let (a, s) = (PartId::ALL[5], SlotId::ALL[17]);
    assert_eq!(p.save_part_to(a, s), PartSet::EMPTY);
    assert_eq!(p.users(s), PartSet::EMPTY.with(a));
    assert!(p.pool().get(s).unwrap().bits_eq(&p.part(a).sound));
}

#[test]
fn edit_fx_and_set_name_reach_the_project() {
    let mut p = Project::boxed().0;
    p.edit_fx().reverb.mix = 0.5;
    assert_eq!(p.perf().fx.reverb.mix, 0.5);
    let n = chimera_core::name::ProjectName::new("SET ONE").unwrap();
    p.set_name(n);
    assert_eq!(p.meta().name(), n);
}

#[test]
fn sound_crc_sees_the_name() {
    let mut s = Sound::init(EngineType::Algo);
    let before = sound_crc(&s);
    s.name = SoundName::new("RENAMED").unwrap();
    assert_ne!(sound_crc(&s), before);
}

#[test]
fn sound_crc_sees_a_param() {
    let mut s = Sound::init(EngineType::Algo);
    let before = sound_crc(&s);
    assert_eq!(sound_crc(&s.clone()), before);
    s.params.filter.cutoff *= 0.5;
    assert_ne!(sound_crc(&s), before);
}
