//! The replace guard (projects spec § Types decide what's possible,
//! § Unsaved-state edge cases): a replace that can lose work asks first,
//! and a confirmation is refused once its target moved.

use chimera_core::name::ProjectName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{
    PartActionKind, PartFrom, PartId, PartSource, PartStatus, Project, ProjectSource, Prompt,
    ReplaceError, ReplaceGuard, SlotId, part_actions, part_status,
};
use chimera_core::storage::ProjectId;
use chimera_hal::store::{Store, VolumeId};
use chimera_hal::testkit::MemStore;

fn vol() -> VolumeId {
    MemStore::new(1).mount().unwrap()
}

fn slot(part: PartId, s: SlotId) -> PartSource {
    PartSource {
        part,
        from: PartFrom::Slot(s),
    }
}

fn edit(p: &mut Project, part: PartId) {
    p.edit_part(part).sound.params.filter.cutoff *= 0.5;
}

#[test]
fn part_replace_prompts_only_when_edited() {
    let (mut p, t) = Project::boxed();
    let r = slot(PartId::ALL[0], SlotId::ALL[4]);
    assert!(ReplaceGuard::check(&p, t, r).is_ok()); // Clean
    edit(&mut p, PartId::ALL[0]);
    assert_eq!(
        ReplaceGuard::check(&p, t, r).unwrap_err().prompt(),
        Prompt::SavePartFirst
    );

    // Stale: b saves over the slot a was loaded from.
    let (a, b, s) = (PartId::ALL[2], PartId::ALL[3], SlotId::ALL[1]);
    for part in [a, b] {
        let c = ReplaceGuard::check(&p, t, slot(part, s)).unwrap();
        p.replace_part(c).unwrap();
    }
    edit(&mut p, b);
    p.save_part_to(b, s);
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Stale(s));
    let c = ReplaceGuard::check(&p, t, slot(a, SlotId::ALL[5])).unwrap();
    assert_eq!(c.target(), slot(a, SlotId::ALL[5]));
    p.replace_part(c).unwrap();
    assert!(
        p.part(a)
            .sound
            .bits_eq(p.pool().get(SlotId::ALL[5]).unwrap())
    );
}

#[test]
fn project_replace_prompts_only_when_modified() {
    let (mut p, t) = Project::boxed();
    let file = ProjectSource::File {
        id: ProjectId::new(3).unwrap(),
        vol: vol(),
    };
    for src in [ProjectSource::New, file] {
        assert_eq!(ReplaceGuard::check(&p, t, src).unwrap().target(), src); // Pristine
    }
    p.set_name(ProjectName::new("X").unwrap());
    for src in [ProjectSource::New, file] {
        assert_eq!(
            ReplaceGuard::check(&p, t, src).unwrap_err().prompt(),
            Prompt::SaveProjectFirst
        );
    }
    p.mark_saved_for_test();
    assert!(ReplaceGuard::check(&p, t, ProjectSource::New).is_ok()); // Saved
}

#[test]
fn each_answer_does_what_it_says() {
    let (mut p, t) = Project::boxed();
    let (a, s) = (PartId::ALL[0], SlotId::ALL[4]);
    let r = slot(a, s);
    edit(&mut p, a);
    let edited = p.part(a).sound.clone();
    let pending = ReplaceGuard::check(&p, t, r).unwrap_err().into_pending();
    assert_eq!(pending.source(), r);
    // No save made: still at risk.
    let pending = pending.save_then(&p, t).unwrap_err().into_pending();
    assert!(p.part(a).sound.bits_eq(&edited));

    // SAVE PART FIRST
    let new = part_actions(&p, a)
        .iter()
        .find(|x| matches!(x.kind(), PartActionKind::NewSlot(_)))
        .unwrap();
    p.apply_part_action(new).unwrap();
    let c = pending.save_then(&p, t).unwrap();
    p.replace_part(c).unwrap();
    assert!(p.part(a).sound.bits_eq(p.pool().get(s).unwrap()));
    let PartActionKind::NewSlot(saved) = new.kind() else {
        unreachable!()
    };
    assert!(p.pool().get(saved).unwrap().bits_eq(&edited));

    // REPLACE
    edit(&mut p, a);
    let c = ReplaceGuard::check(&p, t, r)
        .unwrap_err()
        .into_pending()
        .anyway(&p);
    p.replace_part(c).unwrap();
    assert!(p.part(a).sound.bits_eq(p.pool().get(s).unwrap()));

    // CANCEL
    edit(&mut p, a);
    let edited = p.part(a).sound.clone();
    let _ = ReplaceGuard::check(&p, t, r).unwrap_err().into_pending();
    assert!(p.part(a).sound.bits_eq(&edited));
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Edited);
}

/// Review Focus 4: confirmed while Clean, edited before the replace.
#[test]
fn confirmed_then_edited_is_refused() {
    let (mut p, t) = Project::boxed();
    let a = PartId::ALL[1];
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Clean);
    let c = ReplaceGuard::check(&p, t, slot(a, SlotId::ALL[3])).unwrap();
    edit(&mut p, a);
    let edited = p.part(a).sound.clone();
    assert!(!c.holds(&p));
    assert_eq!(p.replace_part(c), Err(ReplaceError::Changed));
    assert!(p.part(a).sound.bits_eq(&edited));
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Edited);
}

/// REPLACE confirms the edit as it was: a further edit needs asking again.
#[test]
fn anyway_then_edited_again_is_refused() {
    let (mut p, t) = Project::boxed();
    let a = PartId::ALL[2];
    edit(&mut p, a);
    let c = ReplaceGuard::check(&p, t, slot(a, SlotId::ALL[0]))
        .unwrap_err()
        .into_pending()
        .anyway(&p);
    assert!(c.holds(&p));
    edit(&mut p, a);
    let edited = p.part(a).sound.clone();
    assert_eq!(p.replace_part(c), Err(ReplaceError::Changed));
    assert!(p.part(a).sound.bits_eq(&edited));
}

#[test]
fn confirmed_slot_emptied_before_replace() {
    let (mut p, t) = Project::boxed();
    let s = SlotId::ALL[20];
    let a = PartId::ALL[1];
    p.pool_store(s, Sound::init(EngineType::Modal));
    let c = ReplaceGuard::check(&p, t, slot(a, s)).unwrap();
    p.pool_clear(s).unwrap();
    let before = p.part(a).sound.clone();
    assert_eq!(p.replace_part(c), Err(ReplaceError::SlotEmpty));
    assert!(p.part(a).sound.bits_eq(&before));
}

#[test]
fn a_project_confirmation_holds_until_the_project_moves() {
    let (mut p, t) = Project::boxed();
    let c = ReplaceGuard::check(&p, t, ProjectSource::New).unwrap();
    assert!(c.holds(&p));
    p.edit_fx().reverb.mix = 0.7;
    assert!(!c.holds(&p));
    // SAVE THEN LOAD: still Modified until the save lands.
    let pending = ReplaceGuard::check(&p, t, ProjectSource::New)
        .unwrap_err()
        .into_pending();
    let pending = pending.save_then(&p, t).unwrap_err().into_pending();
    p.mark_saved_for_test();
    let c = pending.save_then(&p, t).unwrap();
    assert!(c.holds(&p));
}
