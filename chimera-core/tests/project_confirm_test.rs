//! Typed overwrite and delete confirmations (#257): a confirmation holds the
//! file's newest generation as listed, and a file that moved since refuses
//! it. NEW streams without a second project in RAM.
//!
//! `fresh_file_only_from_new_project_id` is `FreshFile`'s doc test and
//! `save_part_to_is_not_public` is `Project::save_part_to`'s.

mod common;

use chimera_core::name::ProjectName;
use chimera_core::project::test_support::{confirm_delete, confirm_overwrite, full, same};
use chimera_core::project::{
    LoadLink, Project, ProjectEntry, ProjectNote, ProjectSource, ProjectStatus, ReplaceGuard,
    SaveTo, Subject, TemplateCrc, clear_project, delete_project, encode_new_project, list_projects,
    load_project, new_project_id, project_crc, project_status, save_project,
};
use chimera_core::storage::{Card, FileKind, Generation, Header, ProjectId, write_file};
use chimera_hal::testkit::MemStore;
use common::codec_util::{VecSink, decode_project, encode_project};

fn name(s: &str) -> ProjectName {
    ProjectName::new(s).unwrap()
}

fn listed(card: &mut Card, s: &mut MemStore) -> Vec<ProjectEntry> {
    let mut v = Vec::new();
    let out = list_projects(card, s, &mut |e| v.push(e));
    assert_eq!(out.note, None);
    v
}

fn entry(card: &mut Card, s: &mut MemStore, id: ProjectId) -> ProjectEntry {
    listed(card, s).into_iter().find(|e| e.id == id).unwrap()
}

fn saved(n: ProjectNote) {
    assert!(matches!(n, ProjectNote::Saved(_)), "{n:?}");
}

/// `p` saved to a new file; its id.
fn save_new(card: &mut Card, s: &mut MemStore, p: &mut Project) -> ProjectId {
    let f = new_project_id(card, s).unwrap();
    let id = f.file().id();
    saved(save_project(card, s, p, SaveTo::Fresh(f)));
    id
}

fn load(card: &mut Card, s: &mut MemStore, id: ProjectId) -> (Box<Project>, TemplateCrc) {
    let e = entry(card, s, id);
    let (mut q, t) = Project::boxed();
    let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol: e.vol }).unwrap();
    let out = load_project(card, s, &mut q, go, &LoadLink::new());
    assert!(out.swap.is_some() && out.note.is_none(), "{:?}", out.note);
    (q, t)
}

#[test]
fn encode_new_matches_project_new() {
    let (p, t) = Project::boxed();
    let h = Header {
        kind: FileKind::Project,
        generation: Generation::FIRST,
        name: Some(p.meta().name()),
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| encode_new_project(w)).unwrap();
    let bytes = sink.0;
    assert_eq!(bytes, encode_project(&p));
    let (mut q, _) = full();
    decode_project(&bytes, &mut q).unwrap();
    assert_eq!(project_crc(&q), t.get());
}

#[test]
fn confirmed_delete_after_resave_is_refused() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut a, _) = full();
    let id = save_new(&mut card, &mut s, &mut a);
    let c = confirm_delete(&entry(&mut card, &mut s, id));
    saved(save_project(&mut card, &mut s, &mut a, SaveTo::Own));
    let (other, _) = Project::boxed();
    let got = delete_project(&mut card, &mut s, &other, c);
    assert_eq!(
        got,
        Err(ProjectNote::FileChanged(Subject::Name(name("FULL"))))
    );
    assert_eq!(
        got.unwrap_err().line().as_str(),
        "CHANGED SINCE ASKED: FULL"
    );
    assert_eq!(listed(&mut card, &mut s).len(), 1, "still there");

    // Deleted since it was listed: a missing pair moved too.
    let e = entry(&mut card, &mut s, id);
    let stale = confirm_delete(&e);
    assert_eq!(
        delete_project(&mut card, &mut s, &other, confirm_delete(&e)),
        Ok(())
    );
    assert_eq!(
        delete_project(&mut card, &mut s, &other, stale),
        Err(ProjectNote::FileChanged(Subject::File(id)))
    );
}

#[test]
fn confirmed_delete_of_the_loaded_file_is_refused() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut a, _) = full();
    let id = save_new(&mut card, &mut s, &mut a);
    let c = confirm_delete(&entry(&mut card, &mut s, id));
    assert_eq!(
        delete_project(&mut card, &mut s, &a, c),
        Err(ProjectNote::IsLoaded)
    );
    assert_eq!(listed(&mut card, &mut s).len(), 1);
}

#[test]
fn confirmed_overwrite_after_resave_is_refused() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut a, _) = full();
    let id = save_new(&mut card, &mut s, &mut a);
    let c = confirm_overwrite(&entry(&mut card, &mut s, id));
    a.edit_fx().delay.mix = 0.8;
    saved(save_project(&mut card, &mut s, &mut a, SaveTo::Own));
    let (mut b, t) = Project::boxed();
    b.set_name(name("B"));
    assert_eq!(
        save_project(&mut card, &mut s, &mut b, SaveTo::Over(c)),
        ProjectNote::FileChanged(Subject::Name(name("FULL")))
    );
    assert_eq!(b.meta().file(), None);
    assert_eq!(project_status(&b, t), ProjectStatus::Modified);
    let (q, _) = load(&mut card, &mut s, id);
    same(&a, &q);
}

#[test]
fn overwrite_saves_over_and_becomes_that_file() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut a, _) = full();
    let id = save_new(&mut card, &mut s, &mut a);
    let e = entry(&mut card, &mut s, id);
    let (mut b, t) = Project::boxed();
    b.set_name(name("B"));
    saved(save_project(
        &mut card,
        &mut s,
        &mut b,
        SaveTo::Over(confirm_overwrite(&e)),
    ));
    assert_eq!(b.meta().file(), Some(e.file()));
    assert_eq!(project_status(&b, t), ProjectStatus::Saved);
    assert_eq!(entry(&mut card, &mut s, id).name, Some(name("B")));
    let (q, _) = load(&mut card, &mut s, id);
    same(&b, &q);

    // Over the project's own file: a SAVE.
    b.edit_fx().reverb.mix = 0.2;
    let own = confirm_overwrite(&entry(&mut card, &mut s, id));
    saved(save_project(&mut card, &mut s, &mut b, SaveTo::Over(own)));
    assert_eq!(project_status(&b, t), ProjectStatus::Saved);
}

#[test]
fn fresh_file_taken_since_is_refused() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let first = new_project_id(&mut card, &mut s).unwrap();
    let second = new_project_id(&mut card, &mut s).unwrap();
    assert_eq!(first.file(), second.file());
    let (mut a, _) = full();
    saved(save_project(
        &mut card,
        &mut s,
        &mut a,
        SaveTo::Fresh(first),
    ));
    let (mut b, _) = Project::boxed();
    assert_eq!(
        save_project(&mut card, &mut s, &mut b, SaveTo::Fresh(second)),
        ProjectNote::FileChanged(Subject::Name(name("FULL")))
    );
    assert_eq!(b.meta().file(), None);
}

#[test]
fn clear_other_makes_it_new() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut a, _) = full();
    let one = save_new(&mut card, &mut s, &mut a);
    let (mut b, _) = full();
    b.set_name(name("LOADED"));
    let two = save_new(&mut card, &mut s, &mut b);
    let e = entry(&mut card, &mut s, one);
    let stale = confirm_overwrite(&e);
    assert_eq!(
        clear_project(&mut card, &mut s, &b, confirm_overwrite(&e)),
        Ok(())
    );
    assert_eq!(
        entry(&mut card, &mut s, one).name,
        Some(name("NEW PROJECT"))
    );
    let (q, t) = load(&mut card, &mut s, one);
    assert_eq!(project_crc(&q), t.get());
    assert_eq!(project_status(&q, t), ProjectStatus::Pristine);
    assert_eq!(q.meta().name(), name("NEW PROJECT"));
    assert_eq!(
        clear_project(&mut card, &mut s, &b, stale),
        Err(ProjectNote::FileChanged(Subject::Name(name("NEW PROJECT"))))
    );
    let (r, _) = load(&mut card, &mut s, two);
    same(&b, &r);
}

#[test]
fn save_own_without_a_file_is_no_file() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut p, t) = Project::boxed();
    let n = save_project(&mut card, &mut s, &mut p, SaveTo::Own);
    assert_eq!(n, ProjectNote::NoFile);
    assert_eq!(n.line().as_str(), "NOT SAVED YET");
    assert_eq!(p.meta().file(), None);
    assert_eq!(project_status(&p, t), ProjectStatus::Pristine);
    assert!(listed(&mut card, &mut s).is_empty());
}
