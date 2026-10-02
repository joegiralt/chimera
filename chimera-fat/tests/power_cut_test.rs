//! A/B saves on our FAT layer, the card pulled at every block (ADR 0045,
//! ADR 0048): a generation always loads, the kept side is never touched,
//! and nothing is cross-linked or longer than its chain. FAT16 images: they
//! are cheap to copy per cut.

#[path = "common/ab.rs"]
mod ab;
#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;

use ab::{
    Slot, assert_kept, copy, file, load, load_side, name83, save, save_on, slot, sound, target,
};
use chimera_core::addr::BlockRef;
use chimera_core::block::{Block, DiskCode};
use chimera_core::mod_path::MAX_REGISTRY_DESTS;
use chimera_core::params::EngineType;
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::project::test_support::{confirm_overwrite, full, same, save_at};
use chimera_core::project::{
    PartId, Project, ProjectDecoder, ProjectNote, clear_project, list_projects, project_crc,
    project_file,
};
use chimera_core::storage::{
    Card, CardError, CardEvent, FileKind, Generation, Header, InPlaceError, ProjectId, RecordTag,
    RecordWriter, SaveError, Side, SoundCheck, SoundDecoder, encode_sound, load_ab,
    load_ab_in_place, save_ab, write_file,
};
use chimera_hal::store::{ByteSink, Dir, FileName, Store, StoreError};
use image::{Cut, CutDisk, FatEntry, RamDisk, Tear, fat_check, with_clusters};
use probe::{Probed, XorShift, free, log, probed};
use std::collections::HashSet;

/// A FAT16 card of 512 B clusters: every file of the pair and `/CHIMERA`
/// sit in FAT sector 0.
const CLUSTERS: u32 = 4_200;

/// The kept side's entry, as it is now.
fn kept(slot: &Slot) -> FatEntry {
    let side = target(slot).other();
    fat_check(&slot.inner)
        .entry(&name83(side))
        .cloned()
        .expect("the kept side has an entry")
}

/// A card with `/CHIMERA` and save 1 on side A.
fn first_card() -> RamDisk {
    let s = slot(with_clusters(CLUSTERS, 0xC0DE));
    ab::make_dir(&s);
    save(&s, 1).0.unwrap();
    copy(&s.inner)
}

/// What a cut may leave: no cross-link, no length over its chain, the kept
/// side's entry and chain as they were, and FAT copies that differ only in
/// FAT 2 blocks a cut hit (FAT 1's write landed, FAT 2's didn't).
fn assert_safe(what: &str, disk: &RamDisk, kept: &FatEntry, cuts: &[u32]) {
    let r = fat_check(disk);
    assert!(
        r.cross_linked.is_empty(),
        "{what}: cross-linked {:?}",
        r.cross_linked
    );
    assert!(
        r.short.is_empty(),
        "{what}: over their chains {:?}",
        r.short
    );
    assert_eq!(r.entry(&kept.name), Some(kept), "{what}: the kept side");
    assert!(
        r.fats_differ.iter().all(|b| cuts.contains(b)),
        "{what}: FATs differ at {:?}",
        r.fats_differ
    );
}

/// The next save after a cut lands and loads, and rewrites FAT sector 0,
/// which the pair shares, so both FATs agree again.
fn assert_recovers(what: &str, slot: &Slot, n: u32) {
    assert!(save(slot, n).0.is_ok(), "{what}: the next save");
    let got = load(slot).unwrap_or_else(|e| panic!("{what}: after the next save, {e:?}"));
    assert!(got.bits_eq(&sound(n)), "{what}: the next save loads");
    let r = fat_check(&slot.inner);
    assert!(
        r.cross_linked.is_empty() && r.short.is_empty() && r.fats_differ.is_empty(),
        "{what}: after the next save, {r:?}"
    );
}

/// Blocks a save of `n` on `disk` writes, in order.
fn writes_of(disk: &RamDisk, n: u32) -> Vec<u32> {
    let dry = slot(copy(disk));
    let mut s = probed(&dry);
    save_on(&mut s, n, &|_| Ok(())).unwrap();
    let blocks: Vec<u32> = log(&s).writes.borrow().iter().map(|&(b, _)| b).collect();
    assert_eq!(blocks.len() as u32, dry.writes.get(), "one block per write");
    blocks
}

#[test]
fn cut_at_every_block_write_keeps_a_generation() {
    let base = slot(first_card());
    // Save 2 creates B, 3 truncates A, 4 truncates B.
    for n in 2..=4 {
        let before = copy(&base.inner);
        let kept = kept(&base);
        let kept_side = target(&base).other();
        let blocks = writes_of(&before, n);
        for (k, &block) in blocks.iter().enumerate() {
            let what = format!(
                "save {n} cut at write {k} of {} (block {block})",
                blocks.len()
            );
            let cut = slot(copy(&before));
            cut.cut.set(Cut::After(k as u32));
            let (r, failed) = save(&cut, n);
            assert!(r.is_err(), "{what}");
            assert_eq!(failed, Some(block), "{what}");
            cut.cut.set(Cut::Never);
            assert_safe(&what, &cut.inner, &kept, &[block]);
            assert_kept(
                &what,
                &cut,
                kept_side,
                Generation::new(n - 1),
                &sound(n - 1),
            );
            let got = load(&cut).unwrap_or_else(|e| panic!("{what}: {e:?}"));
            assert!(
                got.bits_eq(&sound(n - 1)) || got.bits_eq(&sound(n)),
                "{what}: loaded {:?}",
                got.name
            );
            assert_recovers(&what, &cut, 100 + n);
        }
        save(&base, n).0.unwrap();
    }
}

#[test]
fn repeated_cuts_keep_a_generation() {
    let base = first_card();
    let (mut cuts, mut landed) = (0, 0);
    for seed in 0..200u32 {
        let mut rng = XorShift(u64::from(seed) * 0x9E37_79B9 + 1);
        let card = slot(copy(&base));
        let mut current = sound(1);
        // A FAT 2 block a cut hit stays stale until a save rewrites it: the
        // store that knew is gone with the power.
        let mut hit = Vec::new();
        for round in 0..6 {
            let n = seed * 10 + round + 2;
            let what = format!("seed {seed} round {round}");
            let kept = kept(&card);
            let kept_side = target(&card).other();
            let (kept_gen, kept_snd) = load_side(&card, kept_side).unwrap();
            card.writes.set(0);
            card.cut.set(Cut::After(rng.below(24) as u32));
            let (r, failed) = save(&card, n);
            card.cut.set(Cut::Never);
            hit.extend(failed);
            assert_safe(&what, &card.inner, &kept, &hit);
            assert_kept(&what, &card, kept_side, kept_gen, &kept_snd);
            let got = load(&card).unwrap_or_else(|e| panic!("{what}: {e:?}"));
            if r.is_ok() {
                landed += 1;
                assert!(got.bits_eq(&sound(n)), "{what}: a landed save loads");
            } else {
                cuts += 1;
                assert!(
                    got.bits_eq(&current) || got.bits_eq(&sound(n)),
                    "{what}: loaded {:?}",
                    got.name
                );
            }
            current = got;
        }
        assert_recovers(&format!("seed {seed}"), &card, 9_999_000 + seed);
    }
    println!("repeated cuts: {cuts} cut, {landed} landed");
    assert!(cuts > 0 && landed > 0, "{cuts} cut, {landed} landed");
}

/// A torn 512 B write (ADR 0045's atomic-block assumption broken). A torn
/// data block, or one side's own entry, never loses the kept side, and no
/// tear ever loads wrong data. A tear can lose the pair only in a block
/// both sides use: the directory block holding both entries, or a FAT
/// block holding both chains.
#[test]
fn torn_block_breaks_only_shared_sectors() {
    let base = slot(first_card());
    save(&base, 2).0.unwrap();
    let before = copy(&base.inner);
    let r = fat_check(&before);
    let (a, b) = (
        r.entry(&name83(Side::A)).unwrap(),
        r.entry(&name83(Side::B)).unwrap(),
    );
    let fat_b: HashSet<u32> = r.fat_blocks(&b.chain).into_iter().collect();
    let mut shared: HashSet<u32> = r
        .fat_blocks(&a.chain)
        .into_iter()
        .filter(|x| fat_b.contains(x))
        .collect();
    if a.block == b.block {
        shared.insert(a.block);
    }
    let blocks = writes_of(&before, 3);
    let (mut loaded, mut lost) = (0, Vec::new());
    for (k, &block) in blocks.iter().enumerate() {
        for tear in [Tear::HalfOld, Tear::Garbage] {
            let what = format!("save 3 torn at write {k} (block {block}), {tear:?}");
            let cut = slot(copy(&before));
            cut.cut.set(Cut::TornAfter(k as u32, tear));
            assert!(save(&cut, 3).0.is_err(), "{what}");
            cut.cut.set(Cut::Never);
            match load(&cut) {
                Ok(got) => {
                    assert!(
                        got.bits_eq(&sound(2)) || got.bits_eq(&sound(3)),
                        "{what}: loaded {:?}",
                        got.name
                    );
                    loaded += 1;
                }
                Err(e) => {
                    assert!(
                        shared.contains(&block),
                        "{what}: lost the pair ({e:?}) on an unshared block"
                    );
                    lost.push((k, block, tear));
                }
            }
        }
    }
    println!(
        "torn writes: {} writes x 2 tears; {loaded} load a generation, {} lose the pair, \
         all on shared blocks {shared:?}: {lost:?}",
        blocks.len(),
        lost.len()
    );
}

#[test]
fn cut_then_reinsert_loads_previous_generation() {
    let disk = slot(first_card());
    let mut s = probed(&disk);
    let mut card = Card::new();
    let v = s.mount().unwrap();
    let out = card.run(&mut s, |_, _| Ok::<_, StoreError>(())).unwrap();
    assert_eq!(out.event, CardEvent::Mounted);

    disk.cut.set(Cut::After(3));
    let snd = sound(2);
    let out = card
        .run(&mut s, |s, r| {
            save_ab(
                s,
                r,
                file(),
                &mut SoundCheck::new(),
                Some(snd.name),
                &mut |w| encode_sound(&snd, w),
            )
        })
        .unwrap();
    assert_eq!(out.result, Err(SaveError::Store(StoreError::Io)));
    assert_eq!(
        card,
        Card::Failed {
            err: CardError::Io,
            last: Some(v)
        }
    );
    assert!(log(&s).reinits.get() > 0, "the cut re-inits the card");

    disk.cut.set(Cut::Never);
    let mut t = Sound::neutral(EngineType::Algo);
    let out = card
        .run(&mut s, |s, r| {
            load_ab(s, r, file(), &mut SoundDecoder::new(&mut t))
        })
        .unwrap();
    assert_eq!(out.event, CardEvent::Same);
    assert_eq!(out.result.map(|h| h.generation), Ok(Generation::FIRST));
    assert!(t.bits_eq(&sound(1)));
    assert_eq!(card, Card::Ready(v));
    assert_kept("reinserted", &disk, Side::A, Generation::FIRST, &sound(1));
}

struct Bytes(Vec<u8>);

impl ByteSink for Bytes {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(b);
        Ok(())
    }
}

/// A = generation 1, valid; B = generation 2 under a good CRC, which the
/// decoder rejects (a Registry past `MAX_REGISTRY_DESTS`). Save 2 writes
/// over B, never A: A loads at every cut.
#[test]
fn decoder_invalid_newest_is_written_over_at_every_cut() {
    let base = slot(first_card());
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::new(2),
        name: None,
    };
    let mut bad = Bytes(Vec::new());
    write_file(&mut bad, &h, &mut |w| {
        w.put(RecordTag::Engine, &[EngineType::Algo.disk_code()])?;
        w.put(RecordTag::Registry, &[0; (MAX_REGISTRY_DESTS + 1) * 10])
    })
    .unwrap();
    ab::op(&mut probed(&base), |s, r| {
        s.write(r.volume(), file().side(Side::B), &mut |w| w.put(&bad.0))
    })
    .unwrap();
    assert!(load(&base).unwrap().bits_eq(&sound(1)), "A loads");
    assert_eq!(target(&base), Side::B, "B is invalid to save too");

    let before = copy(&base.inner);
    let kept = kept(&base);
    let blocks = writes_of(&before, 2);
    for (k, &block) in blocks.iter().enumerate() {
        let what = format!("save 2 over an invalid B cut at write {k} (block {block})");
        let cut = slot(copy(&before));
        cut.cut.set(Cut::After(k as u32));
        let (r, failed) = save(&cut, 2);
        assert!(r.is_err(), "{what}");
        cut.cut.set(Cut::Never);
        assert_safe(&what, &cut.inner, &kept, &[block]);
        assert_eq!(failed, Some(block), "{what}");
        assert_kept(&what, &cut, Side::A, Generation::FIRST, &sound(1));
        let got = load(&cut).unwrap_or_else(|e| panic!("{what}: {e:?}"));
        assert!(
            got.bits_eq(&sound(1)) || got.bits_eq(&sound(2)),
            "{what}: loaded {:?}",
            got.name
        );
        assert_recovers(&what, &cut, 100 + k as u32);
    }
}

/// 8 non-critical `Block` records of block codes this firmware doesn't
/// know, 4 120 B in all: a reader skips them.
fn padding(w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    for code in 0xE0..0xE8u8 {
        assert!(
            BlockRef::from_disk_code(code).is_none(),
            "{code:#x} is a block"
        );
        let mut p = vec![code];
        for id in 0..102u8 {
            p.extend_from_slice(&[id, 0, 0, 0, 0]);
        }
        w.put(RecordTag::Block, &p)?;
    }
    Ok(())
}

#[test]
fn full_card_keeps_previous_generation() {
    let disk = slot(first_card());
    save(&disk, 2).0.unwrap();
    let mut s = probed(&disk);
    let v = s.mount().unwrap();
    let pad = FileName::new(Dir::Chimera, b"PAD", b"BIN").unwrap();
    let spare = free(&disk.inner).0 as usize;
    s.write(v, pad, &mut |w| w.put(&vec![0xAB; (spare - 1) * 512]))
        .unwrap();
    assert_eq!(free(&disk.inner), (1, 1), "one free cluster");

    let side = target(&disk);
    let kept = kept(&disk);
    let old = fat_check(&disk.inner)
        .entry(&name83(side))
        .unwrap()
        .chain
        .len() as u32;
    assert!(4_096 / 512 > old + 1, "the padding alone needs more");

    // Through a `Card`: a full card is a file-level condition, not a card
    // fault. It stays `Ready`, nothing re-inits, and generation 2 loads in
    // the same session.
    let mut s = probed(&disk);
    let mut card = Card::Ready(v);
    let snd = sound(3);
    let out = card
        .run(&mut s, |s, r| {
            save_ab(
                s,
                r,
                file(),
                &mut SoundCheck::new(),
                Some(snd.name),
                &mut |w| {
                    encode_sound(&snd, w)?;
                    padding(w)
                },
            )
        })
        .unwrap();
    assert_eq!(out.result, Err(SaveError::Store(StoreError::Full)));
    assert_eq!(card, Card::Ready(v), "Full is no card fault");
    assert_eq!(log(&s).reinits.get(), 0, "Full re-inits nothing");
    assert_eq!(free(&disk.inner), (0, 0), "the last free cluster was used");
    assert_safe("full", &disk.inner, &kept, &[]);
    assert_kept("full", &disk, side.other(), Generation::new(2), &sound(2));
    let mut t = Sound::neutral(EngineType::Algo);
    let out = card
        .run(&mut s, |s, r| {
            load_ab(s, r, file(), &mut SoundDecoder::new(&mut t))
        })
        .unwrap();
    assert_eq!(out.event, CardEvent::Same);
    assert_eq!(out.result.map(|h| h.generation), Ok(Generation::new(2)));
    assert!(t.bits_eq(&sound(2)));
    assert_eq!(card, Card::Ready(v));

    let mut s = probed(&disk);
    s.delete(v, file().side(side)).unwrap();
    assert_eq!(free(&disk.inner), (1 + old, 1 + old), "Full leaked nothing");
    assert!(load(&disk).unwrap().bits_eq(&sound(2)));
}

/// `full()` with Part 1's LEVEL at save `n`'s value: each save differs.
fn project_gen(n: u32) -> Box<Project> {
    let (mut p, _) = full();
    p.edit_part(PartId::ALL[0])
        .mix
        .set(PartParams::LEVEL, 0.1 * n as f32);
    p
}

fn project_id() -> ProjectId {
    ProjectId::new(1).unwrap()
}

/// Save `n` over the pair, as listed (a SAVE OVER), or to it fresh.
fn save_project_on(s: &mut Probed<CutDisk>, n: u32) -> ProjectNote {
    save_at(&mut Card::new(), s, &mut project_gen(n), project_id().get())
}

/// CLEAR of the pair as listed now, from a project not loaded from it.
fn clear_project_on(s: &mut Probed<CutDisk>) -> Result<(), ProjectNote> {
    let mut card = Card::new();
    let mut listed = None;
    let out = list_projects(&mut card, s, &mut |e| listed = Some(e));
    assert_eq!(out.note, None);
    let c = confirm_overwrite(&listed.expect("listed"));
    clear_project(&mut card, s, &Project::boxed().0, c).out
}

fn load_project_in_place(slot: &Slot) -> Result<Box<Project>, InPlaceError> {
    let (mut q, _) = Project::boxed();
    ab::op(&mut probed(slot), |s, r| {
        load_ab_in_place(
            s,
            r,
            project_file(project_id()),
            &mut ProjectDecoder::new_for_test(&mut q),
        )
    })?;
    Ok(q)
}

/// A project (about 44 KB, 90-odd blocks a side) cut at every block
/// write of its third save: the in-place load always gets save 2 or 3
/// whole, never an error, and the next save lands.
#[test]
fn project_cut_keeps_a_generation() {
    let base = slot(with_clusters(CLUSTERS, 0xC0DE));
    for n in 1..=2 {
        let note = save_project_on(&mut probed(&base), n);
        assert!(matches!(note, ProjectNote::Saved(_)), "save {n}: {note:?}");
    }
    let before = copy(&base.inner);
    let blocks = {
        let dry = slot(copy(&before));
        let mut s = probed(&dry);
        assert!(matches!(save_project_on(&mut s, 3), ProjectNote::Saved(_)));
        let blocks: Vec<u32> = log(&s).writes.borrow().iter().map(|&(b, _)| b).collect();
        blocks
    };
    assert!(blocks.len() > 80, "{} writes", blocks.len());
    let (two, three) = (project_gen(2), project_gen(3));
    let (crc2, crc3) = (project_crc(&two), project_crc(&three));
    assert_ne!(crc2, crc3);
    let mut loaded = [0; 2];
    for k in 0..blocks.len() {
        let what = format!("project save 3 cut at write {k} of {}", blocks.len());
        let cut = slot(copy(&before));
        cut.cut.set(Cut::After(k as u32));
        let note = save_project_on(&mut probed(&cut), 3);
        assert!(!matches!(note, ProjectNote::Saved(_)), "{what}: {note:?}");
        cut.cut.set(Cut::Never);
        let r = fat_check(&cut.inner);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty(),
            "{what}: {r:?}"
        );
        let got = load_project_in_place(&cut).unwrap_or_else(|e| panic!("{what}: {e:?}"));
        if project_crc(&got) == crc3 {
            same(&three, &got);
            loaded[1] += 1;
        } else {
            same(&two, &got);
            loaded[0] += 1;
        }
        let note = save_project_on(&mut probed(&cut), 4);
        assert!(
            matches!(note, ProjectNote::Saved(_)),
            "{what}: next save {note:?}"
        );
        same(&project_gen(4), &load_project_in_place(&cut).unwrap());
    }
    println!(
        "project cuts: {} writes; save 2 loads at {}, save 3 at {}",
        blocks.len(),
        loaded[0],
        loaded[1]
    );
}

/// CLEAR cut at every block write: the load gets save 2 or NEW whole,
/// never an error, and a CLEAR run again lands and loads Pristine.
#[test]
fn project_clear_cut_keeps_a_generation() {
    let base = slot(with_clusters(CLUSTERS, 0xC0DE));
    for n in 1..=2 {
        let note = save_project_on(&mut probed(&base), n);
        assert!(matches!(note, ProjectNote::Saved(_)), "save {n}: {note:?}");
    }
    let before = copy(&base.inner);
    let blocks = {
        let dry = slot(copy(&before));
        let mut s = probed(&dry);
        assert_eq!(clear_project_on(&mut s), Ok(()));
        let blocks: Vec<u32> = log(&s).writes.borrow().iter().map(|&(b, _)| b).collect();
        blocks
    };
    assert!(blocks.len() > 20, "{} writes", blocks.len());
    let (new, t) = Project::boxed();
    let two = project_gen(2);
    let mut loaded = [0; 2];
    for k in 0..blocks.len() {
        let what = format!("clear cut at write {k} of {}", blocks.len());
        let cut = slot(copy(&before));
        cut.cut.set(Cut::After(k as u32));
        assert!(clear_project_on(&mut probed(&cut)).is_err(), "{what}");
        cut.cut.set(Cut::Never);
        let r = fat_check(&cut.inner);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty(),
            "{what}: {r:?}"
        );
        let got = load_project_in_place(&cut).unwrap_or_else(|e| panic!("{what}: {e:?}"));
        if project_crc(&got) == t.get() {
            same(&new, &got);
            loaded[1] += 1;
        } else {
            same(&two, &got);
            loaded[0] += 1;
        }
        assert_eq!(clear_project_on(&mut probed(&cut)), Ok(()), "{what}: again");
        let got = load_project_in_place(&cut).unwrap();
        assert_eq!(project_crc(&got), t.get(), "{what}: Pristine");
    }
    println!(
        "clear cuts: {} writes; save 2 loads at {}, NEW at {}",
        blocks.len(),
        loaded[0],
        loaded[1]
    );
}
