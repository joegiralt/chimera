//! The project model (projects spec § Model): typed ids, the pool's
//! generations, NEW's contents and the Part origins.

use chimera_core::MidiChannel;
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::arena::{ARENA_BYTES, Cost};
use chimera_core::project::test_support::{dense, save_part_to};
use chimera_core::project::{
    InUse, NEW_PARTS, Origin, PartFrom, PartId, PartSet, PartSource, PoolFull, Project,
    ReplaceError, ReplaceGuard, SlotId, TemplateCrc,
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
    assert_eq!(SlotId::new(127).map(SlotId::index), Some(127));
    assert_eq!(SlotId::new(128), None);
    assert_eq!((PartId::ALL.len(), SlotId::ALL.len()), (6, 128));
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
                .to_sound()
                .bits_eq(&factory_sound(i).unwrap())
        );
    }
    // Then an INIT per playable engine (ADR 0187).
    for (i, e) in EngineType::PLAYABLE.into_iter().enumerate() {
        let got = p
            .pool()
            .get(SlotId::ALL[FACTORY_LEN + i])
            .unwrap()
            .to_sound();
        assert!(got.bits_eq(&Sound::init(e)), "{e:?}");
    }
    assert_eq!(
        p.pool().sound_count(),
        FACTORY_LEN + EngineType::PLAYABLE.len()
    );
    assert_eq!(
        p.pool().first_free(),
        SlotId::new(FACTORY_LEN as u8 + EngineType::PLAYABLE.len() as u8)
    );
    // Parts 1–5 on the init scene's factory Sounds, from their slots;
    // Part 6 on INIT Algo.
    for (id, slot) in PartId::ALL.into_iter().zip(NEW_PARTS) {
        let Origin::Slot { slot: from, .. } = p.part(id).origin() else {
            panic!("{id:?} from INIT");
        };
        assert_eq!(from, slot);
        assert!(
            p.part(id)
                .sound
                .bits_eq(&p.pool().get(slot).unwrap().to_sound())
        );
    }
    let six = p.part(PartId::ALL[5]);
    assert_eq!(six.origin(), Origin::Init(EngineType::Algo));
    assert!(six.sound.bits_eq(&Sound::init(EngineType::Algo)));
}

#[test]
fn store_and_clear_bump_the_generation() {
    let mut p = Project::boxed().0;
    let s = SlotId::ALL[20];
    let g = p.pool().generation(s);
    p.pool_store(s, &Sound::init(EngineType::Modal)).unwrap();
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
    // TX BRASS: no Part of NEW's plays it.
    let (a, b, s) = (PartId::ALL[1], PartId::ALL[3], SlotId::ALL[2]);
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
    let crc = p.pool().get(s).unwrap().crc();
    assert_eq!(
        p.part(part).origin(),
        Origin::Slot {
            slot: s,
            generation: p.pool().generation(s),
            crc
        }
    );
    assert!(
        p.part(part)
            .sound
            .bits_eq(&p.pool().get(s).unwrap().to_sound())
    );
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
    assert_eq!(save_part_to(&mut p, a, s), PartSet::EMPTY.with(b)); // c, edited, isn't Stale
    let crc = p.pool().get(s).unwrap().crc();
    assert_eq!(
        p.part(a).origin(),
        Origin::Slot {
            slot: s,
            generation: p.pool().generation(s),
            crc
        }
    );
    assert!(
        p.part(a)
            .sound
            .bits_eq(&p.pool().get(s).unwrap().to_sound())
    );
}

#[test]
fn save_part_to_an_empty_slot_has_no_other_users() {
    let mut p = Project::boxed().0;
    let (a, s) = (PartId::ALL[5], SlotId::ALL[17]);
    assert_eq!(save_part_to(&mut p, a, s), PartSet::EMPTY);
    assert_eq!(p.users(s), PartSet::EMPTY.with(a));
    assert!(
        p.pool()
            .get(s)
            .unwrap()
            .to_sound()
            .bits_eq(&p.part(a).sound)
    );
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

/// A Sound's records as `encode_sound` writes them: a file's body.
fn records_len(s: &Sound) -> usize {
    use chimera_core::storage::{
        FileKind, Generation, HEADER_LEN, Header, TRAILER_LEN, encode_sound, write_file,
    };
    struct Count(usize);
    impl chimera_hal::store::ByteSink for Count {
        fn put(&mut self, b: &[u8]) -> Result<(), chimera_hal::store::StoreError> {
            self.0 += b.len();
            Ok(())
        }
    }
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(s.name),
    };
    let mut c = Count(0);
    write_file(&mut c, &h, &mut |w| encode_sound(s, w)).unwrap();
    c.0 - HEADER_LEN - TRAILER_LEN
}

/// NEW's 12 factory Sounds and 4 INITs, each its 16 B name and its sparse
/// records, granule-rounded.
#[test]
fn new_pool_reads_its_cost() {
    let p = Project::boxed().0;
    assert_eq!(
        p.pool().sound_count(),
        FACTORY_LEN + EngineType::PLAYABLE.len()
    );
    let want: u32 = SlotId::ALL
        .into_iter()
        .filter_map(|s| p.pool().get(s))
        .map(|e| Cost::of(16 + records_len(&e.to_sound())).bytes())
        .sum();
    assert_eq!(p.pool().used().bytes(), want);
    assert_eq!(
        p.pool().free().bytes() as usize,
        ARENA_BYTES - want as usize
    );
    for s in SlotId::ALL.into_iter().take(10) {
        let e = p.pool().get(s).unwrap();
        assert_eq!(e.cost(), Cost::of(16 + records_len(&e.to_sound())));
        assert_eq!(e.crc(), sound_crc(&e.to_sound()));
        assert_eq!(e.name(), e.to_sound().name);
        assert_eq!(e.engine(), e.to_sound().engine());
    }
}

/// Worst-case Sounds until the arena refuses one: `PoolFull` says what it
/// needed and what was free, and the pool is as it was.
#[test]
fn store_refuses_past_budget() {
    let mut p = Project::boxed().0;
    let mut refused = None;
    for s in SlotId::ALL.into_iter().skip(10) {
        let mut x = Sound::init(EngineType::Algo);
        dense(&mut x, s.index() as u64);
        let before: Vec<_> = SlotId::ALL
            .iter()
            .map(|&s| p.pool().get(s).map(|e| e.to_sound()))
            .collect();
        let (used, g) = (p.pool().used(), p.pool().generation(s));
        assert_eq!(p.pool().fits(x_cost(&x)), x_cost(&x) <= p.pool().free());
        match p.pool_store(s, &x) {
            Ok(()) => assert!(p.pool().get(s).unwrap().to_sound().bits_eq(&x)),
            Err(PoolFull { needs, free }) => {
                assert!(free < needs, "{free:?} < {needs:?}");
                assert_eq!(needs, x_cost(&x));
                assert_eq!(free, p.pool().free());
                assert!(!p.pool().fits(needs));
                assert_eq!(p.pool().used(), used);
                assert_eq!(p.pool().generation(s), g);
                for (slot, was) in SlotId::ALL.iter().zip(&before) {
                    let now = p.pool().get(*slot).map(|e| e.to_sound());
                    match (now, was) {
                        (None, None) => {}
                        (Some(a), Some(b)) => assert!(a.bits_eq(b)),
                        _ => panic!("slot {} changed", slot.index()),
                    }
                }
                refused = Some(s);
                break;
            }
        }
    }
    let s = refused.expect("the arena fills before 128 dense Sounds");
    assert!(s.index() > 60, "refused at slot {}", s.index());
}

fn x_cost(x: &Sound) -> Cost {
    Cost::of(16 + records_len(x))
}

/// A clear gives its cost back at once, moves the generation, and every
/// other slot still decodes as it did.
#[test]
fn clear_compacts_and_moves_gen() {
    let mut p = Project::boxed().0;
    for (i, s) in SlotId::ALL.into_iter().enumerate().skip(10).take(30) {
        let mut x = factory_sound(i % FACTORY_LEN).unwrap();
        dense(&mut x, i as u64);
        p.pool_store(s, &x).unwrap();
    }
    for s in [SlotId::ALL[25], SlotId::ALL[2], SlotId::ALL[39]] {
        let before: Vec<_> = SlotId::ALL
            .iter()
            .map(|&s| p.pool().get(s).map(|e| e.to_sound()))
            .collect();
        let (free, g) = (p.pool().free(), p.pool().generation(s));
        let cost = p.pool().get(s).unwrap().cost();
        assert_eq!(p.pool_clear(s), Ok(()));
        assert_eq!(p.pool().free().bytes(), free.bytes() + cost.bytes());
        assert_eq!(p.pool().generation(s), g.wrapping_add(1));
        assert!(p.pool().get(s).is_none());
        for (slot, was) in SlotId::ALL.iter().zip(&before) {
            if *slot == s {
                continue;
            }
            let now = p.pool().get(*slot).map(|e| e.to_sound());
            match (now, was) {
                (None, None) => {}
                (Some(a), Some(b)) => assert!(a.bits_eq(b), "slot {}", slot.index()),
                _ => panic!("slot {} changed", slot.index()),
            }
        }
    }
}

/// A slot a Part plays is never cleared, at any of the 128.
#[test]
fn in_use_clear_still_refused() {
    let (mut p, t) = Project::boxed();
    assert_eq!(SlotId::ALL.len(), 128);
    let (part, s) = (PartId::ALL[4], SlotId::ALL[100]);
    p.pool_store(s, &Sound::init(EngineType::Modal)).unwrap();
    let from = PartFrom::Slot(s);
    load(&mut p, t, PartSource { part, from }).unwrap();
    let (g, free) = (p.pool().generation(s), p.pool().free());
    assert_eq!(p.pool_clear(s), Err(InUse(PartSet::EMPTY.with(part))));
    assert!(p.pool().get(s).is_some());
    assert_eq!((p.pool().generation(s), p.pool().free()), (g, free));
}

/// A filled slot overwritten with a larger Sound while the arena is full:
/// refused, the old Sound's bytes and generation as they were.
#[test]
fn overwrite_with_growth_refused_when_full() {
    let mut p = Project::boxed().0;
    let target = SlotId::ALL[10];
    let small = factory_sound(1).unwrap();
    p.pool_store(target, &small).unwrap();
    // Fill the rest with worst-case Sounds until one is refused, then the
    // gap left with INITs, so what's free is under any one Sound's growth.
    let mut rest = SlotId::ALL.into_iter().skip(11);
    for s in rest.by_ref() {
        let mut x = Sound::init(EngineType::Algo);
        dense(&mut x, s.index() as u64);
        if p.pool_store(s, &x).is_err() {
            break;
        }
    }
    for s in rest {
        if p.pool_store(s, &Sound::init(EngineType::Algo)).is_err() {
            break;
        }
    }
    let mut big = small.clone();
    dense(&mut big, 7);
    let grows = x_cost(&big).bytes() - p.pool().get(target).unwrap().cost().bytes();
    assert!(grows > p.pool().free().bytes(), "the arena is full enough");
    let (before, g, used) = (
        p.pool().get(target).unwrap().bytes().to_vec(),
        p.pool().generation(target),
        p.pool().used(),
    );
    let refused = p.pool_store(target, &big).unwrap_err();
    assert_eq!(refused.needs.bytes(), grows);
    assert_eq!(p.pool().get(target).unwrap().bytes(), &before[..]);
    assert_eq!(p.pool().generation(target), g);
    assert_eq!(p.pool().used(), used);
}
