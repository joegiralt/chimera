//! The project's status is cached on a revision every edit bumps (#257):
//! no frame hashes the project, and no frame bumps it.

mod common;
mod screen;

use chimera_core::name::ProjectName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::test_support::save_part_to;
use chimera_core::project::{
    LoadLink, PartFrom, PartId, PartSource, Project, ProjectStatus, SaveTo, SlotId, StatusCache,
    boot_project, load_project, new_project_id, part_actions, project_crc, save_project,
};
use chimera_core::storage::Card;
use chimera_core::ui::perf::PerfStats;
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use core::cell::Cell;
use screen::{Fb, Input, feed};

const P1: PartId = PartId::ALL[0];

macro_rules! bumps {
    ($p:expr, $name:literal, $op:expr) => {{
        let r = $p.rev();
        $op;
        assert_ne!($p.rev(), r, $name);
    }};
}

#[test]
fn every_mutation_bumps_the_revision() {
    let (mut p, t) = Project::boxed();
    let free = SlotId::ALL[SlotId::ALL.len() - 1];
    bumps!(p, "edit_part", {
        p.edit_part(P1);
    });
    bumps!(p, "edit_fx", {
        p.edit_fx();
    });
    bumps!(p, "set_name", p.set_name(ProjectName::new("REV").unwrap()));
    bumps!(
        p,
        "pool_store",
        p.pool_store(free, Sound::init(EngineType::Algo))
    );
    bumps!(p, "pool_clear", {
        let _ = p.pool_clear(free);
    });
    bumps!(
        p,
        "replace_part",
        common::project::load(
            &mut p,
            t,
            PartSource {
                part: P1,
                from: PartFrom::Init(EngineType::Modal),
            }
        )
        .unwrap()
    );
    p.edit_part(P1).sound.name = chimera_core::name::Name::new("EDITED").unwrap();
    let a = part_actions(&p, P1)
        .iter()
        .next()
        .expect("an Edited Part has an action");
    bumps!(p, "apply_part_action", {
        p.apply_part_action(a).unwrap();
    });
    bumps!(p, "save_part_to", {
        save_part_to(&mut p, P1, free);
    });

    let (mut card, mut store) = (Card::new(), MemStore::new(1));
    let fresh = new_project_id(&mut card, &mut store).out.unwrap();
    let f = fresh.file();
    bumps!(p, "save_project", {
        let _ = save_project(&mut card, &mut store, &mut p, SaveTo::Fresh(fresh)).out;
    });
    bumps!(p, "boot_project", {
        let _ = boot_project(&mut card, &mut store, Some(f.id()), &mut p);
    });
    let go = common::project::confirm_load(&p, t, f);
    bumps!(p, "load_project", {
        let _ = load_project(&mut card, &mut store, &mut p, go, &LoadLink::new());
    });
    bumps!(p, "mark_saved_for_test", p.mark_saved_for_test());
}

#[test]
fn the_cache_hashes_once_per_revision() {
    let (mut p, t) = Project::boxed();
    let calls = Cell::new(0);
    let crc = |q: &Project| {
        calls.set(calls.get() + 1);
        project_crc(q)
    };
    let mut cache = StatusCache::new();
    assert_eq!(cache.cached(), ProjectStatus::Pristine);
    for _ in 0..1000 {
        assert_eq!(cache.get_with(&p, t, crc), ProjectStatus::Pristine);
    }
    assert_eq!(calls.get(), 1, "one hash for 1000 reads of one revision");
    p.set_name(ProjectName::new("EDITED").unwrap());
    assert_eq!(cache.get_with(&p, t, crc), ProjectStatus::Modified);
    assert_eq!(cache.get_with(&p, t, crc), ProjectStatus::Modified);
    assert_eq!(calls.get(), 2, "a new revision hashes once");
    p.mark_saved_for_test();
    assert_eq!(cache.get_with(&p, t, crc), ProjectStatus::Saved);
    assert_eq!(cache.cached(), ProjectStatus::Saved);
    assert_eq!(calls.get(), 3);
}

#[test]
fn frames_do_not_bump_the_revision() {
    let mut ui = Box::new(chimera_core::ui::UiState::new());
    let r = ui.project().rev();
    let mut fb = Fb::new();
    for _ in 0..100 {
        feed(&mut ui, Input::default());
        ui.update();
        ui.render_with_scope(&mut fb, &PerfStats::zero(), &screen::scope_fixture());
        let _ = ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &screen::scope_fixture());
    }
    assert_eq!(ui.project().rev(), r);
}

#[test]
fn ui_status_follows_edits_and_saves() {
    let mut ui = Box::new(chimera_core::ui::UiState::new());
    ui.update();
    assert_eq!(ui.project_status(), ProjectStatus::Pristine);
    feed(&mut ui, Input::press(ButtonId::Plus));
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    ui.update();
    assert_eq!(ui.project_status(), ProjectStatus::Modified);
    let (mut card, mut store) = (Card::new(), MemStore::new(1));
    let (mut sync, mut set, _) = chimera_core::storage::SystemSync::boot(&mut card, &mut store);
    let f = new_project_id(&mut card, &mut store).out.unwrap();
    ui.save_project(&mut card, &mut store, &mut sync, &mut set, SaveTo::Fresh(f));
    ui.update();
    assert_eq!(ui.project_status(), ProjectStatus::Saved);
}

#[test]
fn a_whole_project_assigned_is_hashed_afresh() {
    let mut ui = Box::new(chimera_core::ui::UiState::new());
    for _ in 0..5 {
        ui.project_mut()
            .set_name(ProjectName::new("EDITED").unwrap());
    }
    ui.update();
    assert_eq!(ui.project_status(), ProjectStatus::Modified);
    // A saved project at the revision the cache last saw.
    let (mut q, _) = Project::boxed();
    q.set_name(ProjectName::new("OTHER").unwrap());
    while q.rev().wrapping_add(1) != ui.project().rev() {
        q.set_name(ProjectName::new("OTHER").unwrap());
    }
    q.mark_saved_for_test();
    assert_eq!(q.rev(), ui.project().rev());
    *ui.project_mut() = *q;
    ui.update();
    assert_eq!(ui.project_status(), ProjectStatus::Saved);
}

#[test]
fn the_ui_hashes_once_per_revision() {
    let mut ui = Box::new(chimera_core::ui::UiState::new());
    let mut fb = Fb::new();
    let frame = |ui: &mut chimera_core::ui::UiState, fb: &mut Fb, input: Input| {
        feed(ui, input);
        ui.update();
        let _ = ui.render_dirty_with_scope(fb, &PerfStats::zero(), &screen::scope_fixture());
    };
    for _ in 0..50 {
        frame(&mut ui, &mut fb, Input::default());
    }
    assert_eq!(
        ui.status_hashes_for_test(),
        1,
        "one hash for 50 idle frames"
    );
    frame(&mut ui, &mut fb, Input::press(ButtonId::Plus));
    frame(&mut ui, &mut fb, Input::turn(EncoderId::A, 1));
    for _ in 0..50 {
        frame(&mut ui, &mut fb, Input::default());
    }
    assert_eq!(ui.status_hashes_for_test(), 2, "one more for the edit");
}
