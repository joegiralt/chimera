//! Projects on the card (projects spec § Storage, § Errors; ADR 0046's
//! in-place load): save, load, list and delete, and the notes they leave.

mod common;

use chimera_core::block::Block;
use chimera_core::name::ProjectName;
use chimera_core::params::FilterParams;
use chimera_core::project::test_support::{
    FailOnSecondRead, FlipOnSecondRead, FullOnWrite, full, project_store_suite, same,
};
use chimera_core::project::{
    Differ, LoadLink, PartFrom, PartId, PartSource, PartStatus, Project, ProjectFile, ProjectNote,
    ProjectSource, ProjectStatus, ReplaceGuard, SlotId, Subject, TemplateCrc, delete_project,
    list_projects, load_project, new_project_id, part_status, project_crc, project_file,
    project_status, save_project,
};
use chimera_core::storage::{Card, CardEvent, FileError, ProjectId, Side};
use chimera_hal::store::{Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use common::codec_util::{encode_project, fix_crc};

fn id(n: u32) -> ProjectId {
    ProjectId::new(n).unwrap()
}

fn name(s: &str) -> ProjectName {
    ProjectName::new(s).unwrap()
}

fn vol(s: &mut impl Store) -> VolumeId {
    s.mount().unwrap()
}

/// `P000000n` on the card in the slot now.
fn at(s: &mut impl Store, n: u32) -> ProjectFile {
    ProjectFile {
        id: id(n),
        vol: vol(s),
    }
}

/// Saves `p` as `P000000n` on the card in the slot.
fn save_at(card: &mut Card, s: &mut impl Store, p: &mut Project, n: u32) -> ProjectNote {
    let f = at(s, n);
    save_project(card, s, p, f)
}

/// A slot into a Clean Part, then an edit: an `Edited` Part from that slot.
fn edited_from(p: &mut Project, t: TemplateCrc, part: usize, slot: usize) {
    let src = PartSource {
        part: PartId::ALL[part],
        from: PartFrom::Slot(SlotId::ALL[slot]),
    };
    let c = ReplaceGuard::check(p, t, src).expect("a Clean Part");
    p.replace_part(c).unwrap();
    p.edit_part(PartId::ALL[part])
        .sound
        .params
        .filter
        .set(FilterParams::RESONANCE, 0.7);
}

fn load_file(
    card: &mut Card,
    s: &mut impl Store,
    q: &mut Project,
    t: TemplateCrc,
    id: ProjectId,
    link: &LoadLink,
) -> chimera_core::project::LoadOutcome {
    let v = vol(s);
    let go = ReplaceGuard::check(q, t, ProjectSource::File { id, vol: v }).expect("no prompt");
    load_project(card, s, q, go, link)
}

#[test]
fn suite_on_memstore() {
    project_store_suite(&mut || MemStore::new(1));
}

#[test]
fn full_card_save_stays_modified() {
    let mut s = FullOnWrite(MemStore::new(1), false);
    let mut card = Card::new();
    let (mut p, t) = full();
    let file = new_project_id(&mut card, &mut s).unwrap();
    let id = file.id;
    let _ = save_project(&mut card, &mut s, &mut p, file);
    let saved = p.meta().saved_crc();
    assert!(saved.is_some());
    p.edit_fx().delay.mix = 0.9;
    s.1 = true;
    assert_eq!(
        save_project(&mut card, &mut s, &mut p, file),
        ProjectNote::Card {
            err: StoreError::Full,
            subject: Some(Subject::Name(p.meta().name())),
        }
    );
    assert_eq!(
        (p.meta().saved_crc(), project_status(&p, t)),
        (saved, ProjectStatus::Modified)
    );
    assert!(matches!(card, Card::Ready(_)), "Full is no card fault");
    s.1 = false;
    let (mut q, t) = Project::boxed();
    let out = load_file(&mut card, &mut s, &mut q, t, id, &LoadLink::new());
    assert!(out.swap.is_some() && out.note.is_none(), "{:?}", out.note);
    let (first, _) = full();
    same(&first, &q);
}

/// Review Focus 3: the card swapped between the prompt and LOAD ANYWAY.
/// The new card has its own `P0000001`; the load must not touch RAM.
#[test]
fn swapped_card_refuses_a_pending_load() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let link = LoadLink::new();
    let (mut a, _) = full();
    let first = new_project_id(&mut card, &mut s).unwrap();
    let one = first.id;
    assert!(matches!(
        save_project(&mut card, &mut s, &mut a, first),
        ProjectNote::Saved(_)
    ));
    let mut listed = None;
    let out = list_projects(&mut card, &mut s, &mut |e| listed = Some(e));
    assert_eq!(out.note, None);
    let listed = listed.unwrap();
    assert_eq!(listed.id, one);

    let (mut q, t) = Project::boxed();
    q.edit_fx().delay.mix = 0.9;
    let src = ProjectSource::File {
        id: one,
        vol: listed.vol,
    };
    let pending = ReplaceGuard::check(&q, t, src)
        .expect_err("a Modified project asks")
        .into_pending();

    s.swap(2);
    let (mut other, _) = Project::boxed();
    other.set_name(name("OTHER CARD"));
    let on_b = new_project_id(&mut card, &mut s).unwrap();
    assert_eq!(on_b.id, one, "the new card has its own P0000001");
    assert!(matches!(
        save_project(&mut card, &mut s, &mut other, on_b),
        ProjectNote::Saved(_)
    ));
    let before = project_crc(&q);

    let go = pending.anyway(&q);
    let out = load_project(&mut card, &mut s, &mut q, go, &link);
    assert!(
        matches!(
            out.note,
            Some(ProjectNote::Card {
                err: StoreError::VolumeChanged(v),
                subject: Some(Subject::File(f)),
            }) if v == vol(&mut s) && f == one
        ),
        "{:?}",
        out.note
    );
    assert!(out.swap.is_none());
    assert_eq!(out.event, Some(CardEvent::Same));
    assert_eq!(project_crc(&q), before, "RAM untouched");
    assert_eq!(q.meta().name(), name("NEW PROJECT"));
    assert_eq!(link.epoch(), 0, "no bump");
    assert!(matches!(card, Card::Ready(_)), "the new card is fine");
}

#[test]
fn saved_toast_names_differing_parts() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut p, t) = Project::boxed();
    let one = at(&mut s, 1);
    assert_eq!(
        save_project(&mut card, &mut s, &mut p, one),
        ProjectNote::Saved(Differ::None)
    );

    // An edited INIT-origin Part isn't counted.
    p.edit_part(PartId::ALL[0])
        .sound
        .params
        .filter
        .set(FilterParams::RESONANCE, 0.7);
    edited_from(&mut p, t, 1, 2);
    assert_eq!(
        save_project(&mut card, &mut s, &mut p, one),
        ProjectNote::Saved(Differ::One(PartId::ALL[1], SlotId::ALL[2]))
    );
    assert_eq!(
        ProjectNote::Saved(Differ::One(PartId::ALL[1], SlotId::ALL[2]))
            .line()
            .as_str(),
        "SAVED: P2 DIFFERS FROM SLOT 03"
    );

    edited_from(&mut p, t, 3, 5);
    edited_from(&mut p, t, 4, 9);
    assert_eq!(
        save_project(&mut card, &mut s, &mut p, one),
        ProjectNote::Saved(Differ::Many(3))
    );
    assert_eq!(project_status(&p, t), ProjectStatus::Saved);
}

/// A side as `bytes` on both sides of `P0000001`.
fn both_sides(s: &mut MemStore, bytes: &[u8]) {
    let v = vol(s);
    s.make_dir(v, chimera_hal::store::Dir::Chimera).unwrap();
    s.make_dir(v, chimera_hal::store::Dir::Projects).unwrap();
    for side in [Side::A, Side::B] {
        s.write(v, project_file(id(1)).side(side), &mut |w| w.put(bytes))
            .unwrap();
    }
}

#[test]
fn pass_one_failure_leaves_ram() {
    let (src, _) = full();
    let good = encode_project(&src);
    // Cut mid-body, the CRC fails; shorter than a header and trailer,
    // the file is cut short.
    let half = good[..good.len() / 2].to_vec();
    let truncated = good[..20].to_vec();
    let mut bad_crc = good.clone();
    bad_crc[good.len() / 2] ^= 0x40;
    let mut bad_magic = good.clone();
    bad_magic[0] = b'X';
    fix_crc(&mut bad_magic);
    for (bytes, err) in [
        (half, FileError::BadCrc),
        (truncated, FileError::Truncated),
        (bad_crc, FileError::BadCrc),
        (bad_magic, FileError::BadMagic),
    ] {
        let mut s = MemStore::new(1);
        let mut card = Card::new();
        let link = LoadLink::new();
        both_sides(&mut s, &bytes);
        let (mut q, t) = Project::boxed();
        q.edit_fx().reverb.mix = 0.6;
        q.mark_saved_for_test();
        let before = project_crc(&q);
        let out = load_file(&mut card, &mut s, &mut q, t, id(1), &link);
        assert_eq!(
            out.note,
            Some(ProjectNote::File {
                err,
                subject: Subject::File(id(1)),
            })
        );
        assert!(out.swap.is_none(), "{err:?}");
        assert_eq!(project_crc(&q), before, "{err:?}: RAM untouched");
        assert_eq!(q.meta().saved_crc(), Some(before));
        assert_eq!(link.epoch(), 0);
    }
}

/// Review Focus 1: pass 1 passes, then the card changes under pass 2,
/// which has already written the project. It ends NEW, never half of
/// each, and still bumps so the audio gets its epoch.
#[test]
fn pass_two_failure_falls_back_to_new() {
    let mut mem = MemStore::new(1);
    let mut card = Card::new();
    let (mut p, _) = full();
    assert!(matches!(
        save_at(&mut card, &mut mem, &mut p, 1),
        ProjectNote::Saved(_)
    ));
    let mut s = FlipOnSecondRead::new(mem, project_file(id(1)).side(Side::A));
    let link = LoadLink::new();
    let (mut q, t) = Project::boxed();
    q.set_name(name("OLD"));
    q.mark_saved_for_test();
    let out = load_file(&mut card, &mut s, &mut q, t, id(1), &link);
    assert_eq!(
        out.note,
        Some(ProjectNote::LoadFailed(Subject::File(id(1))))
    );
    assert!(out.swap.is_some(), "a failed pass 2 still swaps");
    assert_eq!(link.epoch(), 1);
    assert_eq!(project_crc(&q), t.get(), "NEW");
    assert_eq!(project_status(&q, t), ProjectStatus::Pristine);
    assert_eq!(q.meta().id(), None);
    assert_eq!(q.meta().name(), name("NEW PROJECT"));
    assert_eq!(s.reads(), 2, "pass 1 and pass 2 read side A");

    // The card itself is fine: a load without the flip lands.
    let mut mem = s.inner;
    let out = load_file(&mut card, &mut mem, &mut q, t, id(1), &link);
    assert!(out.note.is_none() && out.swap.is_some());
    same(&p, &q);
}

#[test]
fn stale_confirmation_is_refused() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let link = LoadLink::new();
    let (mut q, t) = Project::boxed();
    q.set_name(name("SAVED ONE"));
    q.mark_saved_for_test();
    assert_eq!(project_status(&q, t), ProjectStatus::Saved);
    let go = ReplaceGuard::check(&q, t, ProjectSource::New).expect("Saved never asks");
    q.set_name(name("RENAMED"));
    let before = project_crc(&q);
    let out = load_project(&mut card, &mut s, &mut q, go, &link);
    assert_eq!(out.note, Some(ProjectNote::Changed));
    assert!(out.swap.is_none() && out.event.is_none());
    assert_eq!(project_crc(&q), before);
    assert_eq!(q.meta().name(), name("RENAMED"));
    assert_eq!(link.epoch(), 0);

    // Unchanged, NEW resets and bumps, with no note.
    let go = ReplaceGuard::check(&q, t, ProjectSource::New).expect_err("Modified");
    let go = go.into_pending().anyway(&q);
    let out = load_project(&mut card, &mut s, &mut q, go, &link);
    assert_eq!(out.note, None);
    assert!(out.swap.is_some());
    assert_eq!(project_crc(&q), t.get());
    assert_eq!(link.epoch(), 1);
}

#[test]
fn no_card_note() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let link = LoadLink::new();
    let (mut p, t) = full();
    assert!(matches!(
        save_at(&mut card, &mut s, &mut p, 1),
        ProjectNote::Saved(_)
    ));
    let v = vol(&mut s);
    let home = p.meta().file().unwrap();
    assert_eq!(home, ProjectFile { id: id(1), vol: v });
    s.eject();
    let no_card = |subject| ProjectNote::Card {
        err: StoreError::NoCard,
        subject,
    };

    p.edit_fx().delay.mix = 0.8;
    let saved = p.meta().saved_crc();
    assert_eq!(
        save_project(&mut card, &mut s, &mut p, home),
        no_card(Some(Subject::Name(name("FULL"))))
    );
    assert_eq!(p.meta().saved_crc(), saved);
    assert_eq!(card, Card::Absent);

    let (mut q, _) = Project::boxed();
    let before = project_crc(&q);
    let go = ReplaceGuard::check(&q, t, ProjectSource::File { id: id(1), vol: v }).unwrap();
    let out = load_project(&mut card, &mut s, &mut q, go, &link);
    assert_eq!(out.note, Some(no_card(Some(Subject::File(id(1))))));
    assert!(out.swap.is_none() && out.event.is_none());
    assert_eq!(project_crc(&q), before);

    let out = list_projects(&mut card, &mut s, &mut |_| panic!("no entries"));
    assert_eq!((out.event, out.note), (None, Some(no_card(None))));
    assert_eq!(new_project_id(&mut card, &mut s), Err(no_card(None)));
    assert_eq!(
        delete_project(&mut card, &mut s, &q, home),
        Err(no_card(Some(Subject::File(id(1)))))
    );
    assert_eq!(card, Card::Absent);
    assert_eq!(no_card(None).line().as_str(), "NO CARD");
}

#[test]
fn note_lines() {
    let acid = Subject::Name(name("ACID PARTY"));
    let seven = Subject::File(id(7));
    let other = VolumeId {
        serial: 9,
        label: *b"OTHER      ",
    };
    let rows = [
        (ProjectNote::Saved(Differ::None), "SAVED"),
        (
            ProjectNote::Saved(Differ::One(PartId::ALL[1], SlotId::ALL[2])),
            "SAVED: P2 DIFFERS FROM SLOT 03",
        ),
        (
            ProjectNote::Saved(Differ::Many(3)),
            "SAVED: 3 PARTS DIFFER FROM SLOTS",
        ),
        (
            ProjectNote::Card {
                err: StoreError::Full,
                subject: Some(acid),
            },
            "CARD FULL: ACID PARTY",
        ),
        (
            ProjectNote::Card {
                err: StoreError::NoCard,
                subject: None,
            },
            "NO CARD",
        ),
        (
            ProjectNote::Card {
                err: StoreError::VolumeChanged(other),
                subject: Some(seven),
            },
            "CARD CHANGED: P0000007",
        ),
        (
            ProjectNote::File {
                err: FileError::BadCrc,
                subject: seven,
            },
            "FILE CHECKSUM FAILED: P0000007",
        ),
        (
            ProjectNote::File {
                err: FileError::NeedsNewerFirmware,
                subject: acid,
            },
            "NEEDS NEWER FIRMWARE: ACID PARTY",
        ),
        (ProjectNote::Missing(seven), "PROJECT NOT FOUND: P0000007"),
        (ProjectNote::LoadFailed(seven), "LOAD FAILED: P0000007"),
        (ProjectNote::Changed, "CHANGED SINCE ASKED: TRY AGAIN"),
        (ProjectNote::IsLoaded, "CAN NOT DELETE THE LOADED PROJECT"),
        (ProjectNote::NoIds, "NO PROJECT IDS LEFT"),
        (ProjectNote::NewProject, "NEW PROJECT"),
    ];
    for (note, want) in rows {
        assert_eq!(note.line().as_str(), want, "{note:?}");
    }
}

/// A line past `LINE_LEN` is cut, never a panic.
#[test]
fn a_long_line_is_cut() {
    use chimera_core::project::{LINE_LEN, Line};
    let note = ProjectNote::Card {
        err: StoreError::Unsupported(chimera_hal::store::Unsupported::FatNotMirrored),
        subject: Some(Subject::Name(name("SIXTEEN CHARS 16"))),
    };
    let line = note.line();
    assert!(line.as_str().len() <= LINE_LEN);
    // The message gives way; the name survives.
    assert_eq!(
        line.as_str(),
        "CARD FAT NOT MIRRORED: FORMAT: SIXTEEN CHARS 16"
    );
    assert_eq!(Line::new(&"X".repeat(100)).as_str().len(), LINE_LEN);
}

/// I2: a project loaded from card A, card B (with its own `P0000001`) in
/// the slot, then SAVE: refused, and nothing on B changes.
#[test]
fn save_after_a_swap_is_refused() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let link = LoadLink::new();
    let (mut a, _) = full();
    assert!(matches!(
        save_at(&mut card, &mut s, &mut a, 1),
        ProjectNote::Saved(_)
    ));
    let (mut q, t) = Project::boxed();
    let out = load_file(&mut card, &mut s, &mut q, t, id(1), &link);
    assert!(out.note.is_none());
    let home = q.meta().file().unwrap();
    assert_eq!(home, at(&mut s, 1));
    // On its own card, SAVE lands.
    q.edit_fx().reverb.mix = 0.2;
    assert!(matches!(
        save_project(&mut card, &mut s, &mut q, home),
        ProjectNote::Saved(_)
    ));
    assert_eq!(project_status(&q, t), ProjectStatus::Saved);

    s.swap(2);
    let (mut b, tb) = Project::boxed();
    b.set_name(name("CARD B"));
    assert!(matches!(
        save_at(&mut card, &mut s, &mut b, 1),
        ProjectNote::Saved(_)
    ));

    q.edit_fx().delay.mix = 0.9;
    let before = *q.meta();
    let note = save_project(&mut card, &mut s, &mut q, home);
    assert!(
        matches!(note, ProjectNote::Card {
            err: StoreError::VolumeChanged(v),
            subject: Some(Subject::Name(n)),
        } if v == vol(&mut s) && n == name("FULL")),
        "{note:?}"
    );
    assert_eq!(*q.meta(), before, "meta untouched");
    assert_eq!(project_status(&q, t), ProjectStatus::Modified);
    assert!(matches!(card, Card::Ready(_)), "card B is fine");

    // B's P0000001 is still B's.
    let (mut r, _) = Project::boxed();
    let out = load_file(&mut card, &mut s, &mut r, tb, id(1), &link);
    assert!(out.note.is_none());
    same(&b, &r);
}

/// I3: a delete confirmed on card A, run after card B went in: refused.
#[test]
fn delete_after_a_swap_is_refused() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut a, _) = Project::boxed();
    let on_a = at(&mut s, 1);
    assert!(matches!(
        save_project(&mut card, &mut s, &mut a, on_a),
        ProjectNote::Saved(_)
    ));
    s.swap(2);
    let (mut b, _) = Project::boxed();
    let on_b = at(&mut s, 1);
    assert!(matches!(
        save_project(&mut card, &mut s, &mut b, on_b),
        ProjectNote::Saved(_)
    ));
    let (loaded, _) = Project::boxed();
    let got = delete_project(&mut card, &mut s, &loaded, on_a);
    assert!(
        matches!(got, Err(ProjectNote::Card {
            err: StoreError::VolumeChanged(v),
            subject: Some(Subject::File(f)),
        }) if v == on_b.vol && f == id(1)),
        "{got:?}"
    );
    assert!(matches!(card, Card::Ready(_)));
    let mut ids = Vec::new();
    let out = list_projects(&mut card, &mut s, &mut |e| ids.push(e.file()));
    assert_eq!(out.note, None);
    assert_eq!(ids, [on_b], "B's file stays");
    // On its own card the delete runs; B's own project guards its file.
    assert_eq!(
        delete_project(&mut card, &mut s, &b, on_b),
        Err(ProjectNote::IsLoaded)
    );
    assert_eq!(delete_project(&mut card, &mut s, &loaded, on_b), Ok(()));
}

/// I1: the card goes (or changes) during pass 2: a store error, not a
/// CRC. The project is part written, so it falls back to NEW and swaps.
#[test]
fn pass_two_store_error_falls_back_to_new() {
    let other = VolumeId {
        serial: 9,
        label: *b"OTHER      ",
    };
    for err in [StoreError::NoCard, StoreError::VolumeChanged(other)] {
        let mut mem = MemStore::new(1);
        let mut card = Card::new();
        let (mut p, _) = full();
        assert!(matches!(
            save_at(&mut card, &mut mem, &mut p, 1),
            ProjectNote::Saved(_)
        ));
        let mut s = FailOnSecondRead::new(mem, project_file(id(1)).side(Side::A), err);
        let link = LoadLink::new();
        let (mut q, t) = Project::boxed();
        q.set_name(name("OLD"));
        q.mark_saved_for_test();
        let out = load_file(&mut card, &mut s, &mut q, t, id(1), &link);
        assert_eq!(s.reads(), 2, "{err:?}: pass 2 ran");
        assert_eq!(
            out.note,
            Some(ProjectNote::LoadFailed(Subject::File(id(1)))),
            "{err:?}"
        );
        assert!(out.swap.is_some(), "{err:?}");
        assert_eq!(link.epoch(), 1);
        assert_eq!(project_crc(&q), t.get(), "{err:?}: NEW");
        assert_eq!(q.meta().file(), None);
        let failed = err == StoreError::NoCard;
        assert_eq!(card == Card::Absent, failed, "{err:?}: {card:?}");
    }
}

/// A Part left `Stale` by another Part's save over its slot isn't counted:
/// only `Edited` ones are.
#[test]
fn saved_toast_skips_stale_parts() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut p, t) = Project::boxed();
    let slot = SlotId::ALL[4];
    for part in [0, 1] {
        let src = PartSource {
            part: PartId::ALL[part],
            from: PartFrom::Slot(slot),
        };
        let c = ReplaceGuard::check(&p, t, src).unwrap();
        p.replace_part(c).unwrap();
    }
    p.edit_part(PartId::ALL[0])
        .sound
        .params
        .filter
        .set(FilterParams::RESONANCE, 0.77);
    let stale = p.save_part_to(PartId::ALL[0], slot);
    assert!(stale.contains(PartId::ALL[1]));
    assert_eq!(
        part_status(p.part(PartId::ALL[1]), p.pool()),
        PartStatus::Stale(slot)
    );
    assert_eq!(
        save_at(&mut card, &mut s, &mut p, 1),
        ProjectNote::Saved(Differ::None)
    );
    edited_from(&mut p, t, 2, 6);
    assert_eq!(
        save_at(&mut card, &mut s, &mut p, 1),
        ProjectNote::Saved(Differ::One(PartId::ALL[2], SlotId::ALL[6]))
    );
}
