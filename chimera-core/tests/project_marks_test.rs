//! Derived marks and the Part actions (projects spec § Derived marks,
//! § Copy rules): a Part's `*` and `◦`, the project's `*`, and the actions
//! an edited or stale Part offers.

mod common;

use chimera_core::factory::factory_sound;
use chimera_core::name::ProjectName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::test_support::save_part_to;
use chimera_core::project::{
    ActionGone, Origin, PartActionKind, PartActions, PartFrom, PartId, PartSet, PartSource,
    PartStatus, Project, ProjectStatus, SlotId, part_actions, part_status, project_crc,
    project_status,
};
use chimera_core::storage::sound_crc;
use common::project::{decode, encode, load, load_anyway};

fn src(part: PartId, slot: SlotId) -> PartSource {
    PartSource {
        part,
        from: PartFrom::Slot(slot),
    }
}

fn init(part: PartId, e: EngineType) -> PartSource {
    PartSource {
        part,
        from: PartFrom::Init(e),
    }
}

fn status(p: &Project, part: PartId) -> PartStatus {
    part_status(p.part(part), p.pool())
}

fn kinds(a: PartActions) -> Vec<PartActionKind> {
    a.iter().map(|x| x.kind()).collect()
}

fn edit(p: &mut Project, part: PartId, by: f32) {
    p.edit_part(part).sound.params.filter.cutoff *= by;
}

#[test]
fn new_is_pristine() {
    let (p, t) = Project::boxed();
    assert_eq!(project_status(&p, t), ProjectStatus::Pristine);
}

#[test]
fn edit_back_to_the_slot_is_clean() {
    let (mut p, t) = Project::boxed();
    let a = PartId::ALL[0];
    load(&mut p, t, src(a, SlotId::ALL[0])).unwrap();
    assert_eq!(status(&p, a), PartStatus::Clean);
    let was = p.part(a).sound.params.filter.cutoff;
    p.edit_part(a).sound.params.filter.cutoff = was * 0.5;
    assert_eq!(status(&p, a), PartStatus::Edited);
    p.edit_part(a).sound.params.filter.cutoff = was;
    assert_eq!(status(&p, a), PartStatus::Clean);
}

#[test]
fn save_over_a_slot_stales_the_other_unedited_users() {
    let (mut p, t) = Project::boxed();
    let s = SlotId::ALL[0];
    let [a, b, c] = [PartId::ALL[0], PartId::ALL[3], PartId::ALL[5]];
    for part in [a, b, c] {
        load(&mut p, t, src(part, s)).unwrap();
    }
    edit(&mut p, c, 0.5);
    edit(&mut p, a, 0.25);
    let over = part_actions(&p, a)
        .iter()
        .find(|x| x.kind() == PartActionKind::OverSlot(s))
        .unwrap();
    assert_eq!(p.apply_part_action(over), Ok(PartSet::EMPTY.with(b)));
    assert_eq!(status(&p, a), PartStatus::Clean);
    assert_eq!(status(&p, b), PartStatus::Stale(s));
    assert_eq!(status(&p, c), PartStatus::Edited);
    assert!(p.pool().get(s).unwrap().bits_eq(&p.part(a).sound));
}

#[test]
fn save_part_to_names_only_the_stale_users() {
    let (mut p, t) = Project::boxed();
    let s = SlotId::ALL[1];
    let [a, b, c] = [PartId::ALL[1], PartId::ALL[2], PartId::ALL[4]];
    for part in [a, b, c] {
        load(&mut p, t, src(part, s)).unwrap();
    }
    edit(&mut p, b, 0.5);
    edit(&mut p, a, 0.25);
    assert_eq!(save_part_to(&mut p, a, s), PartSet::EMPTY.with(c));
    // Saving again moves the slot; c is still as loaded, so still named.
    assert_eq!(save_part_to(&mut p, a, s), PartSet::EMPTY.with(c));
}

#[test]
fn new_slot_saves_to_the_free_slot_and_cleans_the_part() {
    let (mut p, t) = Project::boxed();
    let a = PartId::ALL[0];
    load(&mut p, t, src(a, SlotId::ALL[4])).unwrap();
    edit(&mut p, a, 0.5);
    let free = p.pool().first_free().unwrap();
    let new = part_actions(&p, a)
        .iter()
        .find(|x| x.kind() == PartActionKind::NewSlot(free))
        .unwrap();
    assert_eq!(p.apply_part_action(new), Ok(PartSet::EMPTY));
    assert!(p.pool().get(free).unwrap().bits_eq(&p.part(a).sound));
    assert_eq!(status(&p, a), PartStatus::Clean);
    match p.part(a).origin() {
        Origin::Slot { slot, crc, .. } => {
            assert_eq!((slot, crc), (free, sound_crc(&p.part(a).sound)));
        }
        o => panic!("origin {o:?}"),
    }
    assert!(p.users(SlotId::ALL[4]).is_empty());
}

#[test]
fn revert_is_bit_exact() {
    let (mut p, t) = Project::boxed();
    let (a, s) = (PartId::ALL[0], SlotId::ALL[2]);
    load(&mut p, t, src(a, s)).unwrap();
    let mix = p.part(a).mix;
    edit(&mut p, a, 0.5);
    p.edit_part(a).mix.pan = 0.25;
    let rev = part_actions(&p, a)
        .iter()
        .find(|x| x.kind() == PartActionKind::Revert(s))
        .unwrap();
    assert_eq!(p.apply_part_action(rev), Ok(PartSet::EMPTY));
    assert!(p.part(a).sound.bits_eq(p.pool().get(s).unwrap()));
    assert_eq!(status(&p, a), PartStatus::Clean);
    // A revert copies the Sound; the mix isn't the slot's.
    assert_ne!(p.part(a).mix, mix);
    assert_eq!(p.part(a).mix.pan, 0.25);
}

#[test]
fn update_brings_a_stale_part_to_its_slot() {
    let (mut p, t) = Project::boxed();
    let s = SlotId::ALL[6];
    let [a, b] = [PartId::ALL[0], PartId::ALL[1]];
    load(&mut p, t, src(a, s)).unwrap();
    load(&mut p, t, src(b, s)).unwrap();
    edit(&mut p, a, 0.5);
    save_part_to(&mut p, a, s);
    assert_eq!(status(&p, b), PartStatus::Stale(s));
    let update = part_actions(&p, b).iter().next().unwrap();
    assert_eq!(update.kind(), PartActionKind::Revert(s));
    assert_eq!(p.apply_part_action(update), Ok(PartSet::EMPTY));
    assert_eq!(status(&p, b), PartStatus::Clean);
    assert!(p.part(b).sound.bits_eq(&p.part(a).sound));
}

#[test]
fn init_origin_never_stale() {
    let (mut p, t) = Project::boxed();
    let a = PartId::ALL[1];
    assert_eq!(status(&p, a), PartStatus::Clean);
    // The INIT slot changes under it.
    p.pool_store(SlotId::ALL[8], factory_sound(0).unwrap());
    assert_eq!(status(&p, a), PartStatus::Clean);
    edit(&mut p, a, 0.5);
    assert_eq!(status(&p, a), PartStatus::Edited);
    load_anyway(&mut p, t, init(a, EngineType::Modal)).unwrap();
    assert_eq!(status(&p, a), PartStatus::Clean);
}

/// Review Focus 2: an edited Part, saved with its project and reloaded,
/// then its slot overwritten by another Part, is Edited, never Stale, so
/// UPDATE can't drop its edits without a prompt.
#[test]
fn edited_part_never_stale_after_reload() {
    let (mut p, t) = Project::boxed();
    let s = SlotId::ALL[5];
    let [a, b] = [PartId::ALL[0], PartId::ALL[2]];
    load(&mut p, t, src(a, s)).unwrap();
    edit(&mut p, a, 0.5);
    let (mut q, tq) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    assert_eq!(status(&q, a), PartStatus::Edited);
    match q.part(a).origin() {
        Origin::Slot { slot, crc, .. } => {
            assert_eq!(slot, s);
            assert_eq!(crc, sound_crc(q.pool().get(s).unwrap()));
            assert_ne!(crc, sound_crc(&q.part(a).sound));
        }
        o => panic!("origin {o:?}"),
    }
    load(&mut q, tq, src(b, s)).unwrap();
    edit(&mut q, b, 0.25);
    assert_eq!(save_part_to(&mut q, b, s), PartSet::EMPTY);
    assert_eq!(status(&q, a), PartStatus::Edited);
    assert_eq!(
        kinds(part_actions(&q, a)),
        [
            PartActionKind::OverSlot(s),
            PartActionKind::NewSlot(SlotId::ALL[10]),
            PartActionKind::Revert(s),
        ]
    );
}

#[test]
fn stale_reloads_as_edited() {
    let (mut p, t) = Project::boxed();
    let s = SlotId::ALL[0];
    let [a, b] = [PartId::ALL[3], PartId::ALL[4]];
    load(&mut p, t, src(a, s)).unwrap();
    load(&mut p, t, src(b, s)).unwrap();
    edit(&mut p, b, 0.5);
    assert_eq!(save_part_to(&mut p, b, s), PartSet::EMPTY.with(a));
    assert_eq!(status(&p, a), PartStatus::Stale(s));
    let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    assert_eq!(status(&q, a), PartStatus::Edited);
    assert_eq!(status(&q, b), PartStatus::Clean);
    assert!(q.part(a).sound.bits_eq(&p.part(a).sound));
}

#[test]
fn project_marks() {
    let (mut p, t) = Project::boxed();
    let st = |p: &Project| project_status(p, t);
    p.set_name(ProjectName::new("ACID PARTY").unwrap());
    assert_eq!(st(&p), ProjectStatus::Modified);
    p.mark_saved_for_test();
    assert_eq!(p.meta().saved_crc(), Some(project_crc(&p)));
    assert_eq!(st(&p), ProjectStatus::Saved);
    p.pool_store(SlotId::ALL[20], Sound::init(EngineType::Modal));
    assert_eq!(st(&p), ProjectStatus::Modified);
    p.pool_clear(SlotId::ALL[20]).unwrap();
    assert_eq!(st(&p), ProjectStatus::Saved);
    let was = p.perf().fx.reverb.mix;
    p.edit_fx().reverb.mix = 0.7;
    assert_eq!(st(&p), ProjectStatus::Modified);
    p.edit_fx().reverb.mix = was;
    assert_eq!(st(&p), ProjectStatus::Saved);
    let pan = p.part(PartId::ALL[2]).mix.pan;
    p.edit_part(PartId::ALL[2]).mix.pan = -0.5;
    assert_eq!(st(&p), ProjectStatus::Modified);
    p.edit_part(PartId::ALL[2]).mix.pan = pan;
    assert_eq!(st(&p), ProjectStatus::Saved);
    edit(&mut p, PartId::ALL[2], 0.5);
    assert_eq!(st(&p), ProjectStatus::Modified);
    // NEW again, though saved: Pristine wins.
    let (new, _) = Project::boxed();
    decode(&encode(&new), &mut p).unwrap();
    assert_eq!(st(&p), ProjectStatus::Pristine);
    // THEME can't mark it: ThemeSettings isn't reachable from `Project`.
}

#[test]
fn actions_offer_only_what_applies() {
    use PartActionKind::{NewSlot, OverSlot, Revert};
    let (mut p, t) = Project::boxed();
    let s = SlotId::ALL[0];
    let free = SlotId::ALL[10];
    let [
        clean_init,
        clean_slot,
        edited_slot,
        edited_init,
        stale,
        saver,
    ] = PartId::ALL;
    load(&mut p, t, src(clean_slot, s)).unwrap();
    load(&mut p, t, src(stale, s)).unwrap();
    load(&mut p, t, src(saver, s)).unwrap();
    edit(&mut p, saver, 0.5);
    save_part_to(&mut p, saver, s);
    load(&mut p, t, src(clean_slot, s)).unwrap();
    load(&mut p, t, src(edited_slot, s)).unwrap();
    edit(&mut p, edited_slot, 0.25);
    edit(&mut p, edited_init, 0.5);

    let rows = [
        (clean_init, PartStatus::Clean),
        (clean_slot, PartStatus::Clean),
        (edited_slot, PartStatus::Edited),
        (edited_init, PartStatus::Edited),
        (stale, PartStatus::Stale(s)),
    ];
    for (part, want) in rows {
        assert_eq!(status(&p, part), want, "Part {}", part.index());
    }
    let table = |p: &Project| rows.map(|(part, _)| kinds(part_actions(p, part)));
    assert_eq!(
        table(&p),
        [
            vec![NewSlot(free)],
            vec![NewSlot(free)],
            vec![OverSlot(s), NewSlot(free), Revert(s)],
            vec![NewSlot(free)],
            vec![Revert(s), NewSlot(free)],
        ]
    );

    // A full pool: NewSlot drops out of every row.
    for slot in SlotId::ALL.into_iter().skip(free.index()) {
        p.pool_store(slot, Sound::init(EngineType::Modal));
    }
    assert_eq!(p.pool().first_free(), None);
    assert_eq!(
        table(&p),
        [
            vec![],
            vec![],
            vec![OverSlot(s), Revert(s)],
            vec![],
            vec![Revert(s)],
        ]
    );

    // A Revert whose Part has since gone to INIT is refused by its Origin,
    // and changes nothing. The slot can be emptied only once its Part left
    // it, so this never reaches the empty-slot refusal.
    let rev = part_actions(&p, edited_slot)
        .iter()
        .find(|x| x.kind() == Revert(s))
        .unwrap();
    for part in [clean_slot, stale, saver] {
        load(&mut p, t, init(part, EngineType::Algo)).unwrap();
    }
    load_anyway(&mut p, t, init(edited_slot, EngineType::Algo)).unwrap();
    p.pool_clear(s).unwrap();
    let crc = project_crc(&p);
    assert_eq!(p.apply_part_action(rev), Err(ActionGone));
    assert_eq!(project_crc(&p), crc);
}

#[test]
fn an_action_is_gone_once_its_part_or_slot_moves() {
    let (mut p, t) = Project::boxed();
    let s = SlotId::ALL[3];
    let a = PartId::ALL[0];
    load(&mut p, t, src(a, s)).unwrap();
    let was = p.part(a).sound.params.filter.cutoff;
    edit(&mut p, a, 0.5);
    let pick =
        |p: &Project, k: PartActionKind| part_actions(p, a).iter().find(|x| x.kind() == k).unwrap();
    let over = pick(&p, PartActionKind::OverSlot(s));
    let free = p.pool().first_free().unwrap();
    let new = pick(&p, PartActionKind::NewSlot(free));
    let rev = pick(&p, PartActionKind::Revert(s));

    // Status: edited back to Clean, none of the Edited offers apply.
    p.edit_part(a).sound.params.filter.cutoff = was;
    let crc = project_crc(&p);
    for x in [over, new, rev] {
        assert_eq!(p.apply_part_action(x), Err(ActionGone));
    }
    assert_eq!(project_crc(&p), crc);

    // Origin: the Part reloaded from another slot and edited.
    load(&mut p, t, src(a, SlotId::ALL[4])).unwrap();
    edit(&mut p, a, 0.5);
    assert_eq!(p.apply_part_action(over), Err(ActionGone));
    assert_eq!(p.apply_part_action(rev), Err(ActionGone));

    // The NewSlot's slot filled.
    let new = pick(&p, PartActionKind::NewSlot(free));
    p.pool_store(free, Sound::init(EngineType::Algo));
    let crc = project_crc(&p);
    assert_eq!(p.apply_part_action(new), Err(ActionGone));
    assert_eq!(project_crc(&p), crc);

    // A NewSlot whose slot is free, though no longer the first, applies.
    let later = p.pool().first_free().unwrap();
    let new = pick(&p, PartActionKind::NewSlot(later));
    p.pool_clear(free).unwrap();
    assert_eq!(p.apply_part_action(new), Ok(PartSet::EMPTY));
    assert!(p.pool().get(later).unwrap().bits_eq(&p.part(a).sound));
}

/// An INIT Part's action from before a project load can't apply to the
/// loaded project's Part, though its Origin and status read the same.
#[test]
fn an_action_from_before_a_load_is_gone() {
    let a = PartId::ALL[2];
    let (mut q, tq) = Project::boxed();
    load(&mut q, tq, init(a, EngineType::Modal)).unwrap();
    edit(&mut q, a, 0.25);
    let (mut p, t) = Project::boxed();
    load(&mut p, t, init(a, EngineType::Modal)).unwrap();
    edit(&mut p, a, 0.5);
    let new = part_actions(&p, a).iter().next().unwrap();
    assert!(matches!(new.kind(), PartActionKind::NewSlot(_)));
    decode(&encode(&q), &mut p).unwrap();
    assert_eq!(p.part(a).origin(), Origin::Init(EngineType::Modal));
    assert_eq!(status(&p, a), PartStatus::Edited);
    let crc = project_crc(&p);
    assert_eq!(p.apply_part_action(new), Err(ActionGone));
    assert_eq!(project_crc(&p), crc);
}
