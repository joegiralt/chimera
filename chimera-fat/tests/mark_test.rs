// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sweep mark (ADR 0094): a change that writes turns a clean mark
//! dirty before its first write and clean after its last; a cut, torn or
//! failed change anywhere in between leaves it owing a sweep; the boot
//! sweeps only then, and a sweep that settles makes it clean.

#[path = "common/ab.rs"]
mod ab;
#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;
#[path = "common/tree.rs"]
mod tree;

use ab::{copy, slot};
use chimera_core::console::{SweepAsk, ask_sweep};
use chimera_core::storage::boot_sweep;
use chimera_hal::store::{
    DOUBT_HALT, Dir, FileName, Mark, Store, StoreError, Sweep, SweepRun, Then, Trigger, VolumeId,
};
use image::{Cut, CutDisk, DiskError, RamDisk, Tear, fat_check, with_clusters};
use probe::{log, marks_for, probed};
use std::rc::Rc;
use tree::Tree;

const CLUSTERS: u32 = 4_200;

fn name(stem: &[u8]) -> FileName {
    FileName::new(Dir::Chimera, stem, b"BIN").unwrap()
}

fn mark_file() -> FileName {
    FileName::new(Dir::Chimera, b"SWEEP", b"MRK").unwrap()
}

fn mark_of<D: embedded_sdmmc::BlockDevice<Error = DiskError>>(disk: &Rc<D>) -> Mark {
    let mut s = probed(disk);
    let vol = s.mount().unwrap();
    s.mark(vol).unwrap()
}

/// The boot's sweep, freeing.
fn boot<D: embedded_sdmmc::BlockDevice<Error = DiskError>>(disk: &Rc<D>) -> SweepRun {
    boot_sweep(&mut probed(disk), Sweep::Free, &mut marks_for(&**disk))
}

/// `/CHIMERA` holding `OLD.BIN`, its mark made clean by a settled sweep.
fn clean_card() -> RamDisk {
    let disk = Rc::new(with_clusters(CLUSTERS, 0x3A7C));
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    s.write(vol, name(b"OLD"), &mut |w| w.put(&[7; 3_000]))
        .unwrap();
    assert_eq!(s.mark(vol), Ok(Mark::Missing));
    let run = boot(&disk);
    assert!(
        matches!(
            run,
            SweepRun::Ran {
                then: Then::Cleared,
                ..
            }
        ),
        "{run:?}"
    );
    assert_eq!(mark_of(&disk), Mark::Clean);
    copy(&disk)
}

/// The changes a save or a put makes, each on `vol`.
type Change = fn(&mut probe::Probed<CutDisk>, VolumeId) -> Result<(), StoreError>;

const CHANGES: [(&str, Change); 4] = [
    ("a new file", |s, v| {
        s.write(v, name(b"NEW"), &mut |w| w.put(&[1; 40 * 512]))
            .map(|_| ())
    }),
    ("a rewrite", |s, v| {
        s.write(v, name(b"OLD"), &mut |w| w.put(&[2; 30 * 512]))
            .map(|_| ())
    }),
    ("a delete", |s, v| s.delete(v, name(b"OLD"))),
    ("a directory", |s, v| s.make_dir(v, Dir::Projects)),
];

/// How many writes `change` makes on a clean card, the mark's two
/// included.
fn writes_of(change: Change) -> usize {
    let disk = slot(clean_card());
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    change(&mut s, vol).unwrap();
    assert_eq!(mark_of(&disk), Mark::Clean);
    let w = log(&s).writes.borrow();
    assert_eq!(w[0].0, w[w.len() - 1].0, "the mark first and last");
    w.len()
}

#[test]
fn a_card_chimera_never_wrote_is_unused_then_missing() {
    let disk = Rc::new(with_clusters(CLUSTERS, 0x0A0A));
    assert_eq!(mark_of(&disk), Mark::Unused);
    assert!(matches!(boot(&disk), SweepRun::Skipped(Mark::Unused)));
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    assert_eq!(s.mark(vol), Ok(Mark::Missing));
    // A missing mark is made by no change, only by a settled sweep.
    s.write(vol, name(b"A"), &mut |w| w.put(b"x")).unwrap();
    assert_eq!(s.mark(vol), Ok(Mark::Missing));
}

#[test]
fn only_a_settled_sweep_clears_the_mark() {
    let disk = {
        let d = Rc::new(with_clusters(CLUSTERS, 0x5E77));
        let mut s = probed(&d);
        let vol = s.mount().unwrap();
        s.make_dir(vol, Dir::Chimera).unwrap();
        copy(&d)
    };
    Tree::new(&disk).lose(5, None);
    let disk = Rc::new(disk);
    // A dry run that finds lost clusters keeps the mark, and writes nothing.
    let mut s = probed(&disk);
    let run = boot_sweep(&mut s, Sweep::DryRun, &mut marks_for(&*disk));
    assert!(
        matches!(
            run,
            SweepRun::Ran {
                why: Trigger::Owed(Mark::Missing),
                seen: Ok(sw),
                then: Then::Kept,
                ..
            } if sw.lost == 5
        ),
        "{run:?}"
    );
    assert!(log(&s).writes.borrow().is_empty(), "a dry run wrote");
    assert_eq!(mark_of(&disk), Mark::Missing);
    // Freeing them settles it, and the next boot skips.
    let run = boot(&disk);
    assert!(
        matches!(run, SweepRun::Ran { seen: Ok(sw), then: Then::Cleared, .. } if sw.freed == 5),
        "{run:?}"
    );
    assert_eq!(mark_of(&disk), Mark::Clean);
    assert!(fat_check(&disk).lost.is_empty());
    assert!(matches!(boot(&disk), SweepRun::Skipped(Mark::Clean)));
}

#[test]
fn a_finished_change_leaves_it_clean() {
    for (what, change) in CHANGES {
        let n = writes_of(change);
        assert!(n >= 3, "{what}: {n} writes");
    }
}

#[test]
fn a_change_that_writes_nothing_writes_no_mark() {
    let disk = slot(clean_card());
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    assert_eq!(s.delete(vol, name(b"NOPE")), Err(StoreError::NotFound));
    s.make_dir(vol, Dir::Chimera).unwrap();
    assert!(log(&s).writes.borrow().is_empty());
    assert_eq!(s.mark(vol), Ok(Mark::Clean));
}

/// Cut, or torn, at every write of every change: once anything of it
/// landed the mark owes a sweep, and the boot's sweep leaves no lost
/// cluster and a clean mark.
#[test]
fn every_cut_of_a_change_owes_a_sweep() {
    let (mut settled, mut doubts) = (0, 0);
    for (what, change) in CHANGES {
        let n = writes_of(change) as u32;
        let cuts = (0..n).flat_map(|k| {
            [
                Cut::After(k),
                Cut::TornAfter(k, Tear::HalfOld),
                Cut::TornAfter(k, Tear::Garbage),
            ]
        });
        for cut in cuts {
            let what = format!("{what}, {cut:?}");
            let before = clean_card();
            let disk = Rc::new(CutDisk::new(copy(&before), cut));
            let mut s = probed(&disk);
            let vol = s.mount().unwrap();
            // Only the last write, the mark's `CLEAN`, fails the change
            // without failing it: the change itself is done.
            let r = change(&mut s, vol);
            let last = matches!(cut, Cut::After(k) | Cut::TornAfter(k, _) if k == n - 1);
            assert_eq!(r.is_ok(), last, "{what}");
            disk.cut.set(Cut::Never);
            let landed = !matches!(cut, Cut::After(0));
            match landed {
                // A torn directory block can hide the mark: `Missing`.
                true => assert!(mark_of(&disk).owes_sweep(), "{what}"),
                false => {
                    assert_eq!(*disk.inner.0.borrow(), *before.0.borrow(), "{what}");
                    assert_eq!(mark_of(&disk), Mark::Clean, "{what}");
                }
            }
            for _ in 0..3 {
                if !mark_of(&disk).owes_sweep() {
                    break;
                }
                let mut image = disk.inner.0.borrow().clone();
                match boot(&disk) {
                    SweepRun::Ran { seen: Ok(_), .. } => {}
                    // Noise in a directory block: a doubt, nothing
                    // written, the mark still owing.
                    SweepRun::Ran { seen: Err(_), .. } => {
                        assert!(matches!(cut, Cut::TornAfter(_, Tear::Garbage)), "{what}");
                        // Nothing but the mark's count, in place.
                        let now = disk.inner.0.borrow().clone();
                        let changed: Vec<_> =
                            (0..now.len()).filter(|&i| now[i] != image[i]).collect();
                        assert!(changed.len() <= 1, "{what}: a doubt wrote {changed:?}");
                        if let [i] = changed[..] {
                            image[i] = now[i];
                            assert!(
                                matches!(mark_of(&disk), Mark::Doubted(_)),
                                "{what}: block {i}"
                            );
                        }
                        assert!(mark_of(&disk).owes_sweep(), "{what}");
                        doubts += 1;
                        break;
                    }
                    run => panic!("{what}: {run:?}"),
                }
            }
            if !mark_of(&disk).owes_sweep() {
                assert_eq!(mark_of(&disk), Mark::Clean, "{what}: after the boots");
                let r = fat_check(&disk.inner);
                assert!(
                    r.lost.is_empty() && r.cross_linked.is_empty() && r.fats_differ.is_empty(),
                    "{what}: {r:?}"
                );
                settled += 1;
            }
        }
    }
    println!("{settled} cuts settled, {doubts} doubts");
    assert!(settled > 100, "{settled}");
}

#[test]
fn a_failed_change_keeps_it_dirty() {
    let disk = slot(clean_card());
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    // The third write, a data block of the new file, fails.
    log(&s).land_then_fail.set(Some(2));
    assert!(
        s.write(vol, name(b"NEW"), &mut |w| w.put(&[1; 20 * 512]))
            .is_err()
    );
    log(&s).land_then_fail.set(None);
    assert_eq!(s.mark(vol), Ok(Mark::Dirty));
}

#[test]
fn a_dirty_mark_outlives_finished_changes() {
    let disk = Rc::new(CutDisk::new(clean_card(), Cut::After(3)));
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    assert!(CHANGES[0].1(&mut s, vol).is_err());
    disk.cut.set(Cut::Never);
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    for (_, change) in &CHANGES[1..] {
        change(&mut s, vol).unwrap();
        assert_eq!(s.mark(vol), Ok(Mark::Dirty));
    }
    assert!(matches!(
        boot(&disk),
        SweepRun::Ran {
            why: Trigger::Owed(Mark::Dirty),
            then: Then::Cleared,
            ..
        }
    ));
    assert_eq!(mark_of(&disk), Mark::Clean);
}

#[test]
fn no_change_takes_the_mark_itself() {
    let disk = slot(clean_card());
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    assert_eq!(
        s.write(vol, mark_file(), &mut |w| w.put(b"x")),
        Err(StoreError::ReadOnly)
    );
    assert_eq!(s.delete(vol, mark_file()), Err(StoreError::ReadOnly));
    assert!(log(&s).writes.borrow().is_empty());
    assert_eq!(s.mark(vol), Ok(Mark::Clean));
}

/// A mark that isn't exactly the clean block (a computer's edit, the
/// wrong length) owes a sweep, and a settled one rewrites it.
#[test]
fn an_odd_mark_is_dirty_until_a_sweep_rewrites_it() {
    let disk = Rc::new(clean_card());
    {
        let mut s = probed(&disk);
        let vol = s.mount().unwrap();
        let mut at = None;
        s.runs(vol, mark_file(), &mut |b, _| at = Some(b)).unwrap();
        disk.0.borrow_mut()[at.unwrap() as usize][100] ^= 1;
        assert_eq!(s.mark(vol), Ok(Mark::Dirty));
    }
    assert!(matches!(
        boot(&disk),
        SweepRun::Ran {
            then: Then::Cleared,
            ..
        }
    ));
    assert_eq!(mark_of(&disk), Mark::Clean);
}

/// A clean boot reads the MBR, the boot sector, the root and `/CHIMERA`
/// up to their entries, a FAT sector and the mark: a few blocks.
#[test]
fn a_clean_boot_reads_a_few_blocks_and_writes_none() {
    let disk = Rc::new(clean_card());
    let mut s = probed(&disk);
    let run = boot_sweep(&mut s, Sweep::Free, &mut marks_for(&*disk));
    assert!(matches!(run, SweepRun::Skipped(Mark::Clean)), "{run:?}");
    let reads = log(&s).reads.get();
    println!("a clean boot: {reads} reads");
    assert!(reads <= 10, "{reads} reads");
    assert!(log(&s).writes.borrow().is_empty());
}

/// `ask_sweep` (the console's `sweep`): `Requested`, the mark `Asked`;
/// refused over unsaved changes or on a card with no `/CHIMERA`, nothing
/// written. The boot it restarts into sweeps a clean card and clears it.
#[test]
fn an_asked_sweep_runs_at_the_next_boot() {
    let disk = Rc::new(clean_card());
    let mut s = probed(&disk);
    assert_eq!(
        ask_sweep(chimera_core::errand::Errand::HOST, true, &mut s),
        SweepAsk::Unsaved
    );
    assert!(log(&s).writes.borrow().is_empty());
    assert_eq!(
        ask_sweep(chimera_core::errand::Errand::HOST, false, &mut s),
        SweepAsk::Requested
    );
    assert_eq!(log(&s).writes.borrow().len(), 1, "the mark, in place");
    assert_eq!(mark_of(&disk), Mark::Asked(0));
    let run = boot(&disk);
    assert!(
        matches!(
            run,
            SweepRun::Ran {
                why: Trigger::Asked,
                seen: Ok(sw),
                then: Then::Cleared,
                ..
            } if sw.lost == 0
        ),
        "{run:?}"
    );
    assert!(matches!(boot(&disk), SweepRun::Skipped(Mark::Clean)));
    // A card with no mark gets one; a card Chimera never wrote, none.
    let disk = Rc::new(with_clusters(CLUSTERS, 0xA5C));
    assert_eq!(
        ask_sweep(
            chimera_core::errand::Errand::HOST,
            false,
            &mut probed(&disk)
        ),
        SweepAsk::Unused
    );
    assert_eq!(mark_of(&disk), Mark::Unused);
    {
        let mut s = probed(&disk);
        let vol = s.mount().unwrap();
        s.make_dir(vol, Dir::Chimera).unwrap();
    }
    assert_eq!(
        ask_sweep(
            chimera_core::errand::Errand::HOST,
            false,
            &mut probed(&disk)
        ),
        SweepAsk::Requested
    );
    assert_eq!(mark_of(&disk), Mark::Asked(0));
    // A card that won't mount fails.
    let mut s = probed(&disk);
    log(&s).absent.set(true);
    assert!(matches!(
        ask_sweep(chimera_core::errand::Errand::HOST, false, &mut s),
        SweepAsk::Failed(_)
    ));
}

/// The request's write, cut or torn: the old mark, `Asked`, or plain
/// dirty, never clean.
#[test]
fn a_cut_request_never_reads_clean() {
    for cut in [
        Cut::After(0),
        Cut::TornAfter(0, Tear::HalfOld),
        Cut::TornAfter(0, Tear::Garbage),
    ] {
        let disk = Rc::new(CutDisk::new(clean_card(), cut));
        let _ = ask_sweep(
            chimera_core::errand::Errand::HOST,
            false,
            &mut probed(&disk),
        );
        disk.cut.set(Cut::Never);
        let m = mark_of(&disk);
        assert!(
            matches!(m, Mark::Clean | Mark::Asked(0) | Mark::Dirty),
            "{cut:?}: {m:?}"
        );
        // Only a write that never landed leaves it clean: nothing asked.
        assert_eq!(m == Mark::Clean, cut == Cut::After(0), "{cut:?}");
    }
}

/// The mark's own making, cut or torn at each write: the card still owes
/// a sweep, and the next boot makes it.
#[test]
fn a_cut_making_the_mark_owes_a_sweep() {
    let missing = || {
        let d = Rc::new(with_clusters(CLUSTERS, 0x3A4E));
        let mut s = probed(&d);
        let vol = s.mount().unwrap();
        s.make_dir(vol, Dir::Chimera).unwrap();
        s.write(vol, name(b"OLD"), &mut |w| w.put(&[7; 3_000]))
            .unwrap();
        copy(&d)
    };
    let n = {
        let d = slot(missing());
        let mut s = probed(&d);
        boot_sweep(&mut s, Sweep::Free, &mut marks_for(&*d));
        log(&s).writes.borrow().len() as u32
    };
    assert!(n >= 3, "{n} writes");
    for k in 0..n {
        for cut in [
            Cut::After(k),
            Cut::TornAfter(k, Tear::HalfOld),
            Cut::TornAfter(k, Tear::Garbage),
        ] {
            let disk = Rc::new(CutDisk::new(missing(), cut));
            boot(&disk);
            disk.cut.set(Cut::Never);
            match mark_of(&disk) {
                Mark::Clean => assert!(
                    matches!(cut, Cut::TornAfter(t, _) if t == n - 1) || k == n,
                    "{cut:?}: clean"
                ),
                m => assert!(m.owes_sweep(), "{cut:?}: {m:?}"),
            }
            for _ in 0..3 {
                if !mark_of(&disk).owes_sweep() {
                    break;
                }
                boot(&disk);
            }
            let m = mark_of(&disk);
            assert!(
                m == Mark::Clean || matches!(cut, Cut::TornAfter(_, Tear::Garbage)),
                "{cut:?}: {m:?} after the boots"
            );
        }
    }
}

// Doubting boots (owner ruling 2026-10-08): the third in a row stops the
// automatic sweep until a settled one.

/// `clean_card`, its mark dirty, and a cross-link in the root (`TWIN`
/// sharing `KEEP`'s first cluster): every sweep doubts.
fn doubting_card() -> Rc<RamDisk> {
    let disk = clean_card();
    {
        let mut t = Tree::new(&disk);
        let mut root = t.root();
        let f = t.file(&mut root, *b"KEEP    BIN", None, (1_000, 0), 3, None);
        t.put(&mut root, tree::raw(*b"TWIN    BIN", 0x20, f.chain[0], 1));
    }
    let disk = Rc::new(disk);
    set_mark(&disk, &chimera_fat::mark::DIRTY);
    disk
}

/// The mark's block, absolute.
fn mark_block(disk: &Rc<RamDisk>) -> usize {
    let mut s = probed(disk);
    let vol = s.mount().unwrap();
    let mut at = None;
    s.runs(vol, mark_file(), &mut |b, _| at = Some(b)).unwrap();
    at.unwrap() as usize
}

fn set_mark(disk: &Rc<RamDisk>, b: &[u8; 512]) {
    let at = mark_block(disk);
    disk.0.borrow_mut()[at] = *b;
}

/// `TWIN` deleted, as a computer's disk check would: the doubt gone.
fn fix(disk: &RamDisk) {
    let l = image::layout_of(disk);
    let first = match l.root() {
        chimera_fat::volume::Root::Fixed { first, .. } => first,
        chimera_fat::volume::Root::Cluster(c) => l.cluster_block(c).unwrap(),
    };
    let mut d = disk.0.borrow_mut();
    let blk = &mut d[(image::PART_LBA + first) as usize];
    let at = (0..16)
        .find(|i| &blk[i * 32..i * 32 + 11] == b"TWIN    BIN")
        .unwrap();
    blk[at * 32] = 0xE5;
}

#[test]
fn every_doubt_pattern_differs_from_clean_in_every_byte() {
    use chimera_fat::mark::{CLEAN, DIRTY, doubted};
    for n in 1..=DOUBT_HALT {
        let b = doubted(n);
        assert!(b.iter().zip(&CLEAN).all(|(x, y)| x != y), "doubted {n}");
        assert_ne!(b, DIRTY);
    }
}

#[test]
fn three_doubting_boots_stop_the_automatic_sweep() {
    let disk = doubting_card();
    for n in 1..=DOUBT_HALT {
        let run = boot(&disk);
        assert!(
            matches!(
                run,
                SweepRun::Ran { seen: Err(_), then: Then::Doubted(k), .. } if k == n
            ),
            "boot {n}: {run:?}"
        );
        assert_eq!(run.halted(), n == DOUBT_HALT, "boot {n}");
        assert_eq!(mark_of(&disk), Mark::Doubted(n));
    }
    // The fourth boot skips, writing nothing, and says why.
    let mut s = probed(&disk);
    let run = boot_sweep(&mut s, Sweep::Free, &mut marks_for(&*disk));
    assert_eq!(run, SweepRun::Skipped(Mark::Doubted(DOUBT_HALT)));
    assert!(run.halted());
    assert!(log(&s).writes.borrow().is_empty());
    // A changed card stays halted: changes leave a debt as it is.
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    s.write(vol, name(b"NEW"), &mut |w| w.put(&[4; 2_000]))
        .unwrap();
    assert_eq!(mark_of(&disk), Mark::Doubted(DOUBT_HALT));
    // An asked sweep runs at the next boot, halted or not; its doubt
    // counts no boot, and the count comes back.
    assert_eq!(
        ask_sweep(
            chimera_core::errand::Errand::HOST,
            false,
            &mut probed(&disk)
        ),
        SweepAsk::Requested
    );
    assert_eq!(mark_of(&disk), Mark::Asked(DOUBT_HALT));
    let run = boot(&disk);
    assert!(
        matches!(
            run,
            SweepRun::Ran {
                why: Trigger::Asked,
                seen: Err(_),
                then: Then::Doubted(DOUBT_HALT),
                ..
            }
        ),
        "{run:?}"
    );
    assert!(run.halted());
    assert_eq!(mark_of(&disk), Mark::Doubted(DOUBT_HALT));
    // Fixed, an asked sweep settles and clears it.
    fix(&disk);
    assert_eq!(
        ask_sweep(
            chimera_core::errand::Errand::HOST,
            false,
            &mut probed(&disk)
        ),
        SweepAsk::Requested
    );
    let run = boot(&disk);
    assert!(
        matches!(
            run,
            SweepRun::Ran {
                then: Then::Cleared,
                ..
            }
        ),
        "{run:?}"
    );
    assert!(matches!(boot(&disk), SweepRun::Skipped(Mark::Clean)));
}

/// A boot that runs without a doubt ends the streak, settled or not.
#[test]
fn a_boot_without_a_doubt_resets_the_count() {
    let disk = doubting_card();
    boot(&disk);
    boot(&disk);
    assert_eq!(mark_of(&disk), Mark::Doubted(2));
    fix(&disk);
    Tree::new(&disk).lose(4, None);
    // A dry run finds the lost chain: no doubt, not settled.
    let run = boot_sweep(&mut probed(&disk), Sweep::DryRun, &mut marks_for(&*disk));
    assert!(
        matches!(run, SweepRun::Ran { seen: Ok(sw), then: Then::Kept, .. } if sw.lost == 4),
        "{run:?}"
    );
    assert_eq!(mark_of(&disk), Mark::Dirty);
}

/// A card with no mark that doubts gets none: nothing allocates on a
/// card in doubt, so it keeps no count and goes on sweeping.
#[test]
fn a_doubt_on_a_card_with_no_mark_writes_nothing() {
    let disk = Rc::new(with_clusters(CLUSTERS, 0xD0B1));
    {
        let mut s = probed(&disk);
        let vol = s.mount().unwrap();
        s.make_dir(vol, Dir::Chimera).unwrap();
    }
    {
        let mut t = Tree::new(&disk);
        let mut root = t.root();
        let f = t.file(&mut root, *b"KEEP    BIN", None, (1_000, 0), 3, None);
        t.put(&mut root, tree::raw(*b"TWIN    BIN", 0x20, f.chain[0], 1));
    }
    let mut s = probed(&disk);
    let run = boot_sweep(&mut s, Sweep::Free, &mut marks_for(&*disk));
    assert!(
        matches!(
            run,
            SweepRun::Ran {
                seen: Err(_),
                then: Then::Kept,
                ..
            }
        ),
        "{run:?}"
    );
    assert!(log(&s).writes.borrow().is_empty());
    assert_eq!(mark_of(&disk), Mark::Missing);
}

/// The count's write, cut or torn: the mark reads the old count, the new
/// one, or plain dirty (a shorter streak: one more sweep), never clean.
#[test]
fn a_cut_count_write_never_reads_clean() {
    for n in 0..DOUBT_HALT {
        for cut in [
            Cut::After(0),
            Cut::TornAfter(0, Tear::HalfOld),
            Cut::TornAfter(0, Tear::Garbage),
        ] {
            let base = doubting_card();
            for _ in 0..n {
                boot(&base);
            }
            let was = mark_of(&base);
            let disk = Rc::new(CutDisk::new(copy(&base), cut));
            boot(&disk);
            disk.cut.set(Cut::Never);
            let m = mark_of(&disk);
            assert!(
                m == was || m == Mark::Doubted(n + 1) || m == Mark::Dirty,
                "{n} {cut:?}: {was:?} became {m:?}"
            );
            assert!(
                m.owes_sweep() || m == Mark::Doubted(DOUBT_HALT),
                "{n} {cut:?}: {m:?}"
            );
        }
    }
    // The streak's reset, cut or torn: the old count or plain dirty.
    for cut in [
        Cut::After(0),
        Cut::TornAfter(0, Tear::HalfOld),
        Cut::TornAfter(0, Tear::Garbage),
    ] {
        let base = doubting_card();
        boot(&base);
        boot(&base);
        fix(&base);
        Tree::new(&base).lose(4, None);
        let disk = Rc::new(CutDisk::new(copy(&base), cut));
        boot_sweep(&mut probed(&disk), Sweep::DryRun, &mut marks_for(&*disk));
        disk.cut.set(Cut::Never);
        let m = mark_of(&disk);
        assert!(
            matches!(m, Mark::Doubted(2) | Mark::Dirty),
            "reset {cut:?}: {m:?}"
        );
    }
}
