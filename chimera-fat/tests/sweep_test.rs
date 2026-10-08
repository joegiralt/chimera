//! The lost-cluster sweep (#360): a 128-slot project save cut at every
//! write leaves nothing a sweep can't free; a dry run writes nothing; a
//! computer's tree beside `/CHIMERA` keeps every byte; every doubt stops
//! the sweep before its first write; and a sweep stopped by a fault or a
//! cut leaves nothing crossed or short.

#[path = "common/ab.rs"]
mod ab;
#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;
#[path = "common/tree.rs"]
mod tree;

use ab::{copy, op, slot};
use chimera_core::block::Block as _;
use chimera_core::part::PartParams;
use chimera_core::project::test_support::{full, full_dense_slots, save_at};
use chimera_core::project::{PartId, Project, ProjectNote};
use chimera_core::storage::Card;
use chimera_fat::FatStore;
use chimera_fat::sweep::{MAX_DEPTH, MAX_DIRS, max_lost};
use chimera_fat::volume::{FsKind, Layout, Root};
use chimera_hal::store::{
    Dir, Doubt, FileName, MAX_CHAIN_BYTES, Store, StoreError, Sweep, SweepSeen, Swept, VolumeId,
};
use core::cell::Cell;
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};
use image::{
    Cut, CutDisk, DiskError, PART_LBA, RamDisk, fat_check, fat32, layout_of, with_clusters,
};
use probe::{XorShift, log, marks_for, probed, sweep_on, swept, swept_clean};
use std::rc::Rc;
use tree::{DOT, Tree, lfn_run, raw, short_name};

/// FAT16, 512 B clusters: a 128-slot project spans many.
const CLUSTERS: u32 = 4_200;

// The project saves the cut tests start from.

/// The 128-slot project the saves start from: `full()`, or 175 KB of
/// dense Sounds whose chain crosses FAT sectors.
type Start = fn() -> Box<Project>;

fn sparse() -> Box<Project> {
    full().0
}

fn dense() -> Box<Project> {
    full_dense_slots(128).0
}

fn project_gen(base: Start, n: u32) -> Box<Project> {
    let mut p = base();
    p.edit_part(PartId::ALL[0])
        .mix
        .set(PartParams::LEVEL, 0.1 * n as f32);
    p
}

fn save_gen<S: Store>(base: Start, s: &mut S, n: u32) -> ProjectNote {
    save_at(&mut Card::new(), s, &mut project_gen(base, n), 1)
}

/// A card holding saves 1 and 2.
fn two_saves(base: Start) -> RamDisk {
    let mut s = FatStore::new(with_clusters(CLUSTERS, 0xC0DE));
    for n in 1..=2 {
        let note = save_gen(base, &mut s, n);
        assert!(matches!(note, ProjectNote::Saved(_)), "save {n}: {note:?}");
    }
    copy(s.device())
}

/// `f` on the card save 3 left when cut at each of its writes, the cut
/// lifted, as at the next boot.
fn each_cut(base: Start, mut f: impl FnMut(&str, &Rc<CutDisk>)) {
    let before = two_saves(base);
    let writes = {
        let dry = slot(copy(&before));
        let mut s = probed(&dry);
        assert!(matches!(save_gen(base, &mut s, 3), ProjectNote::Saved(_)));
        log(&s).writes.borrow().len()
    };
    // 59 since the fixed matrix columns made the Sounds smaller.
    assert!(writes > 50, "{writes} writes");
    for k in 0..writes {
        let what = format!("cut at write {k} of {writes}");
        let cut = slot(copy(&before));
        cut.cut.set(Cut::After(k as u32));
        let note = op(&mut probed(&cut), |s, _| {
            Ok::<_, StoreError>(save_gen(base, s, 3))
        })
        .unwrap();
        assert!(!matches!(note, ProjectNote::Saved(_)), "{what}");
        cut.cut.set(Cut::Never);
        f(&what, &cut);
    }
}

// What the card holds.

/// `c`'s value in FAT `copy` (0: FAT 1).
fn fat_value(disk: &RamDisk, l: &Layout, copy: usize, c: u32) -> u32 {
    let at = l.fat_copies(l.fat_block(c)).nth(copy).unwrap();
    l.entry(&disk.block(PART_LBA + at), c)
}

/// A value that holds a cluster: a link or an end mark.
fn holds(l: &Layout, v: u32) -> bool {
    v != 0 && (l.holds(v) || v >= l.end_mark() - 7)
}

/// `fat_check`'s lost clusters that FAT 2 holds too: what a sweep frees
/// on a card whose every directory `fat_check` walks.
fn lost_in_both(disk: &RamDisk) -> Vec<u32> {
    let l = layout_of(disk);
    fat_check(disk)
        .lost
        .into_iter()
        .filter(|&c| holds(&l, fat_value(disk, &l, 1, c)))
        .collect()
}

fn blocks(disk: &RamDisk) -> Vec<[u8; 512]> {
    disk.0.borrow().clone()
}

/// Every block outside the FATs and FSInfo.
fn outside_fats(disk: &RamDisk) -> Vec<[u8; 512]> {
    let l = layout_of(disk);
    let fats = l.fat1().start..l.fat1().start + 2 * l.fat1().len() as u32;
    disk.0
        .borrow()
        .iter()
        .enumerate()
        .filter(|&(i, _)| {
            let lba = (i as u32).wrapping_sub(PART_LBA);
            !(fats.contains(&lba) || l.fs_info() == Some(lba))
        })
        .map(|(_, b)| *b)
        .collect()
}

/// `swept_clean`'s boots, for a card `fat_check` can't judge (a tree
/// beside `/CHIMERA`). Returns what they freed.
fn sweep_until_clean<D: BlockDevice<Error = DiskError>>(what: &str, disk: &Rc<D>) -> u32 {
    let mut freed = 0;
    for _ in 0..3 {
        let s = swept(disk);
        freed += s.freed;
        if s.freed == 0 && !s.healed {
            return freed;
        }
    }
    panic!("{what}: three sweeps and still finding");
}

/// Each entry's chain and every data block as `before` had them.
fn entries_kept(what: &str, before: &RamDisk, after: &RamDisk) {
    let (b, a) = (fat_check(before), fat_check(after));
    assert_eq!(a.entries, b.entries, "{what}: entries");
    let l = layout_of(before);
    for e in &b.entries {
        for &c in &e.chain {
            let first = PART_LBA + l.cluster_block(c).unwrap();
            for blk in first..first + l.blocks_per_cluster() {
                assert_eq!(after.block(blk), before.block(blk), "{what}: block {blk}");
            }
        }
    }
}

#[test]
fn sweep_frees_every_cut_leak() {
    for base in [sparse as Start, dense] {
        cut_leaks_freed(base);
    }
}

fn cut_leaks_freed(base: Start) {
    let (mut leaks, mut most) = (0, 0);
    each_cut(base, |what, cut| {
        let before = copy(&cut.inner);
        let both = lost_in_both(&before);
        let lost = fat_check(&before).lost.len();
        (leaks, most) = (leaks + usize::from(lost > 0), most.max(lost));
        let first = swept(cut);
        assert_eq!(first.lost as usize, both.len(), "{what}: lost");
        assert_eq!(first.freed, first.lost, "{what}: freed");
        swept_clean(what, cut, &cut.inner);
        entries_kept(what, &before, &cut.inner);
    });
    // The cuts between the FAT's flush and the entry's write leak the
    // whole new chain.
    println!("{leaks} cuts leaked, the most {most} clusters");
    assert!(
        leaks >= 2 && most >= 40,
        "{leaks} cuts leaked, the most {most}"
    );
}

/// A delete of a file crossing a FAT sector, its FAT 2 write in the first
/// sector landing then erring: the store keeps that sector stale and the
/// rest of the chain as an orphan, both for the next change to heal.
fn arm_stale_and_orphan(what: &str, s: &mut probe::Probed<CutDisk>, vol: VolumeId) {
    let arm = FileName::new(Dir::Chimera, b"ARM", b"BIN").unwrap();
    s.write(vol, arm, &mut |w| w.put(&[0xA5; 300 * 512]))
        .unwrap_or_else(|e| panic!("{what}: the arming file, {e:?}"));
    let n = log(s).writes.borrow().len();
    // The delete writes the entry's block, then FAT 1's and FAT 2's first
    // sector: the third write lands and errs.
    log(s).land_then_fail.set(Some(n + 2));
    assert!(s.delete(vol, arm).is_err(), "{what}: the arming delete");
    log(s).land_then_fail.set(None);
}

#[test]
fn dry_run_writes_nothing() {
    each_cut(sparse, |what, cut| {
        let mut s = probed(cut);
        let vol = s.mount().unwrap();
        let held_before = fat_check(&cut.inner).lost.len();
        arm_stale_and_orphan(what, &mut s, vol);
        let armed = copy(&cut.inner);
        assert!(
            fat_check(&armed).lost.len() >= held_before + 40,
            "{what}: the orphan holds the chain's rest"
        );
        let writes = log(&s).writes.borrow().len();
        let got = s
            .sweep(vol, Sweep::DryRun, &mut marks_for(&**cut))
            .unwrap_or_else(|e| panic!("{what}: {e:?}"));
        assert_eq!(got.lost as usize, lost_in_both(&armed).len(), "{what}");
        assert_eq!(got.freed, 0, "{what}");
        assert_eq!(log(&s).writes.borrow().len(), writes, "{what}: a write");
        assert_eq!(blocks(&cut.inner), blocks(&armed), "{what}: the image");
    });
}

#[test]
fn sweep_is_idempotent() {
    each_cut(sparse, |what, cut| {
        sweep_until_clean(what, cut);
        let mut s = probed(cut);
        let vol = s.mount().unwrap();
        let again = s
            .sweep(vol, Sweep::Free, &mut marks_for(&**cut))
            .unwrap_or_else(|e| panic!("{what}: {e:?}"));
        assert_eq!(
            (again.lost, again.freed, again.healed),
            (0, 0, false),
            "{what}"
        );
        assert!(log(&s).writes.borrow().is_empty(), "{what}: a write");
    });
}

// A computer's tree beside `/CHIMERA`.

const ATTR_HIDDEN_FILE: u8 = 0x22;

/// `/CHIMERA` through the store, then a computer's files around it, then
/// `lost` chains: the clusters they hold.
fn foreign_card(disk: RamDisk, lost: &[u32], rng: &mut XorShift) -> (Rc<RamDisk>, Vec<u32>) {
    let disk = Rc::new(disk);
    {
        let mut s = probed(&disk);
        let vol = s.mount().unwrap();
        for (i, d) in [Dir::Chimera, Dir::Projects, Dir::Sounds]
            .into_iter()
            .enumerate()
        {
            s.make_dir(vol, d).unwrap();
            let f = FileName::new(d, format!("K{i}").as_bytes(), b"BIN").unwrap();
            s.write(vol, f, &mut |w| w.put(&[i as u8; 3_000])).unwrap();
        }
    }
    let mut t = Tree::new(&disk);
    let mut root = t.root();
    let mut svi = t.mkdir(
        &mut root,
        *b"SYSTEM~1   ",
        Some("System Volume Information"),
    );
    t.file(
        &mut svi,
        *b"INDEXE~1   ",
        Some("IndexerVolumeGuid"),
        (76, 0),
        1,
        None,
    );
    let mut trashes = t.mkdir(&mut root, *b"TRASHE~1   ", Some(".Trashes"));
    t.file(
        &mut trashes,
        *b"_~1     TXT",
        Some("._song.txt"),
        (4_096, 0),
        2,
        Some(rng),
    );
    let mut music = t.mkdir(&mut root, *b"MY MUSIC   ", None);
    let mut deep = t.mkdir(
        &mut music,
        [
            0x05, b'L', b'E', b'V', b'1', b' ', b' ', b' ', b' ', b' ', b' ',
        ],
        None,
    );
    let mut deeper = t.mkdir(&mut deep, *b"$LEV2~1    ", Some("$lev2 with a long name"));
    for (i, name) in [
        *b"SONG~1  WAV",
        *b"_SONG_  WAV",
        *b"$TMP$   TMP",
        *b"A B     TXT",
        [
            0x05, b'X', b' ', b' ', b' ', b' ', b' ', b' ', b'T', b'X', b'T',
        ],
        [
            0x82, 0xA0, 0x81, b'Z', b' ', b' ', b' ', b' ', b' ', b' ', b' ',
        ],
        *b"BANK_1  BNK",
    ]
    .into_iter()
    .enumerate()
    {
        let dir = match i % 4 {
            0 => &mut root,
            1 => &mut music,
            2 => &mut deep,
            _ => &mut deeper,
        };
        let long = (i % 2 == 0).then(|| format!("A long name number {i}.wav"));
        t.file(
            dir,
            name,
            long.as_deref(),
            (700 * (i as u32 + 1), 0),
            i as u8,
            Some(rng),
        );
    }
    let mut dotfile = raw(*b"_~2     TXT", ATTR_HIDDEN_FILE, 0, 0);
    let c = t.take(1, None);
    t.fill(&c, Some(9));
    dotfile[26..28].copy_from_slice(&(c[0] as u16).to_le_bytes());
    dotfile[20..22].copy_from_slice(&((c[0] >> 16) as u16).to_le_bytes());
    dotfile[28..32].copy_from_slice(&4u32.to_le_bytes());
    t.put_named(&mut root, dotfile, Some("._DS_Store"));
    // A deleted entry whose stale start is a live file's cluster.
    let mut gone = raw(*b"XGONE   TXT", 0x20, c[0], 4);
    gone[0] = 0xE5;
    t.put(&mut root, gone);
    let lost: Vec<u32> = lost.iter().flat_map(|&n| t.lose(n, Some(rng))).collect();
    (disk, lost)
}

#[test]
fn sweep_keeps_foreign_trees() {
    for (seed, disk) in [with_clusters(CLUSTERS, 0x1616), fat32(0x3232)]
        .into_iter()
        .enumerate()
    {
        let mut rng = XorShift(0x5EED + seed as u64);
        let (disk, lost) = foreign_card(disk, &[1, 5, 30], &mut rng);
        let l = layout_of(&disk);
        let before = copy(&disk);
        let got = swept(&disk);
        assert_eq!((got.lost, got.freed), (36, 36), "{:?}", l.kind());
        assert!(!got.healed);
        for c in 2..2 + l.clusters() {
            let want = match lost.contains(&c) {
                true => 0,
                false => fat_value(&before, &l, 0, c),
            };
            for copy in 0..2 {
                assert_eq!(
                    fat_value(&disk, &l, copy, c),
                    want,
                    "cluster {c} in FAT {copy}"
                );
            }
        }
        assert_eq!(outside_fats(&disk), outside_fats(&before), "a data block");
        assert_eq!(swept(&disk).freed, 0, "a second sweep");
    }
}

/// A FAT32 first-level `..` may name the root cluster.
#[test]
fn a_first_level_dot_dot_may_name_the_root_cluster() {
    let disk = fat32(0x3233);
    let mut t = Tree::new(&disk);
    let mut root = t.root();
    let d = t.mkdir(&mut root, *b"UP2ROOT    ", None);
    let first = layout_of(&disk).cluster_block(d.start).unwrap();
    disk.0.borrow_mut()[(PART_LBA + first) as usize][32 + 26] = 2;
    let disk = Rc::new(disk);
    assert_eq!(swept(&disk).freed, 0);
}

// Every doubt, before the first write.

/// A small tree on `disk`: a file, a directory two deep holding two more,
/// and one lost chain. Returns the tree's handles.
struct Base {
    disk: RamDisk,
    file: Vec<u32>,
    sub: tree::DirAt,
    subsub: tree::DirAt,
    other: Vec<u32>,
}

fn base(kind: FsKind) -> Base {
    let disk = match kind {
        FsKind::Fat16 => with_clusters(CLUSTERS, 0xD0B7),
        FsKind::Fat32 => fat32(0xD0B7),
    };
    let mut t = Tree::new(&disk);
    let mut root = t.root();
    let file = t
        .file(&mut root, *b"FILE    BIN", None, (3 * 512, 0), 1, None)
        .chain;
    let mut sub = t.mkdir(&mut root, *b"SUB        ", Some("sub"));
    let other = t
        .file(&mut sub, *b"OTHER   BIN", None, (2 * 512, 0), 2, None)
        .chain;
    let subsub = t.mkdir(&mut sub, *b"SUBSUB     ", None);
    t.lose(3, None);
    let (sub, subsub) = (sub.clone(), subsub.clone());
    Base {
        disk,
        file,
        sub,
        subsub,
        other,
    }
}

/// Slot `i` of `d`'s first block, as an absolute block and byte offset.
fn slot_at(l: &Layout, d: &tree::DirAt, i: usize) -> (usize, usize) {
    let b = l.cluster_block(d.chain[0]).unwrap();
    ((PART_LBA + b) as usize, i * 32)
}

fn root_slot(l: &Layout, i: usize) -> (usize, usize) {
    let b = match l.root() {
        Root::Fixed { first, .. } => first,
        Root::Cluster(c) => l.cluster_block(c).unwrap(),
    };
    ((PART_LBA + b) as usize, i * 32)
}

fn poke(disk: &RamDisk, (b, at): (usize, usize), bytes: &[u8]) {
    disk.0.borrow_mut()[b][at..at + bytes.len()].copy_from_slice(bytes);
}

fn set_fat(disk: &RamDisk, l: &Layout, c: u32, v: u32) {
    for copy in l.fat_copies(l.fat_block(c)) {
        let mut d = disk.0.borrow_mut();
        l.put_entry(&mut d[(PART_LBA + copy) as usize], c, v);
    }
}

/// A broken directory, made in a fresh `Base`.
type Shape = fn(&Base, &Layout);

/// The sweep on `disk` in `Free`: the doubt, nothing written, the image as
/// it was.
fn refused(what: &str, disk: RamDisk, want: Doubt) {
    let before = blocks(&disk);
    let disk = Rc::new(disk);
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    let got = s.sweep(vol, Sweep::Free, &mut marks_for(&*disk));
    assert_eq!(got.map_err(|(d, _)| d), Err(want), "{what}");
    assert!(log(&s).writes.borrow().is_empty(), "{what}: a write");
    assert_eq!(blocks(&disk), before, "{what}: the image");
}

/// A chain of `n` directories, each in the last, under the root.
fn nested(disk: &RamDisk, n: usize) {
    let mut t = Tree::new(disk);
    let mut d = t.root();
    for i in 0..n {
        d = t.mkdir(&mut d, short_name(0, i as u32), None);
    }
}

/// `n` directories side by side under one in the root.
fn wide(disk: &RamDisk, n: usize) {
    let mut t = Tree::new(disk);
    let mut root = t.root();
    let mut top = t.mkdir(&mut root, *b"WIDE       ", None);
    for i in 1..n {
        t.mkdir(&mut top, short_name(0, i as u32), None);
    }
}

#[test]
fn sweep_aborts_on_doubt() {
    for kind in [FsKind::Fat16, FsKind::Fat32] {
        let k = format!("{kind:?}");
        let fresh = || base(kind);
        let l = layout_of(&fresh().disk);
        let bad = l.end_mark() - 8;
        // The control: no doubt.
        let b = fresh();
        assert_eq!(
            sweep_on(&Rc::new(b.disk), Sweep::DryRun).map(|s| s.lost),
            Ok(3),
            "{k}"
        );

        // Rule 1: two sectors differ; one sends a reached cluster two ways.
        let b = fresh();
        let fat2 = l.fat_copies(l.fat1().start).nth(1).unwrap();
        poke(&b.disk, ((PART_LBA + fat2) as usize, 500), &[1]);
        poke(&b.disk, ((PART_LBA + fat2 + 1) as usize, 500), &[1]);
        refused(&format!("{k}: two sectors"), b.disk, Doubt::FatsDiffer);
        let b = fresh();
        let at = l.fat_copies(l.fat_block(b.file[0])).nth(1).unwrap();
        let mut blk = b.disk.block(PART_LBA + at);
        l.put_entry(&mut blk, b.file[0], b.other[0]);
        b.disk.0.borrow_mut()[(PART_LBA + at) as usize] = blk;
        refused(&format!("{k}: a split link"), b.disk, Doubt::FatsDiffer);

        // Rule 3, each shape.
        for v in [0, 1, bad, l.clusters() + 5] {
            let b = fresh();
            set_fat(&b.disk, &l, b.sub.start, v);
            refused(
                &format!("{k}: a dir chain to {v:#x}"),
                b.disk,
                Doubt::DirBroken,
            );
        }
        let shapes: [(&str, Shape); 11] = [
            ("no dot dot", |b, l| {
                let (blk, at) = slot_at(l, &b.sub, 1);
                poke(&b.disk, (blk, at), &[0xE5]);
            }),
            ("no dots", |b, l| {
                let (blk, _) = slot_at(l, &b.subsub, 0);
                poke(&b.disk, (blk, 0), &[0; 64]);
            }),
            ("a subdir at 0", |b, l| {
                let (blk, at) = slot_at(l, &b.sub, 3);
                poke(&b.disk, (blk, at + 26), &[0, 0]);
                poke(&b.disk, (blk, at + 20), &[0, 0]);
            }),
            ("a label with a cluster", |b, l| {
                let at = root_slot(l, 0);
                poke(&b.disk, (at.0, at.1 + 26), &[9, 0]);
            }),
            ("an LFN with a cluster", |b, l| {
                let mut e = lfn_run(b"X          ", "x")[0];
                e[26] = 7;
                poke(&b.disk, slot_at(l, &b.sub, 4), &e);
                poke(
                    &b.disk,
                    slot_at(l, &b.sub, 5),
                    &raw(*b"X          ", 0x20, 0, 0),
                );
            }),
            ("a label that is a dir", |b, l| {
                let at = root_slot(l, 0);
                poke(&b.disk, (at.0, at.1 + 11), &[0x18]);
            }),
            ("live after the end", |b, l| {
                poke(
                    &b.disk,
                    slot_at(l, &b.sub, 9),
                    &raw(*b"LATE    BIN", 0x20, 0, 0),
                );
            }),
            ("dot out of place", |b, l| {
                let dot = b.disk.block(slot_at(l, &b.sub, 0).0 as u32);
                let (blk, _) = slot_at(l, &b.sub, 0);
                poke(&b.disk, (blk, 0), &dot[32..64]);
                poke(&b.disk, (blk, 32), &dot[0..32]);
            }),
            ("dot dot naming a stranger", |b, l| {
                let (blk, at) = slot_at(l, &b.subsub, 1);
                poke(&b.disk, (blk, at + 26), &(b.file[0] as u16).to_le_bytes());
            }),
            ("dot naming a stranger", |b, l| {
                let (blk, at) = slot_at(l, &b.sub, 0);
                poke(&b.disk, (blk, at + 26), &(b.file[0] as u16).to_le_bytes());
            }),
            ("a dot in the root", |b, l| {
                poke(&b.disk, root_slot(l, 4), &raw(DOT, 0x10, 0, 0));
            }),
        ];
        for (what, shape) in shapes {
            let b = fresh();
            shape(&b, &l);
            refused(&format!("{k}: {what}"), b.disk, Doubt::DirBroken);
        }

        // Rule 4.
        let b = fresh();
        set_fat(&b.disk, &l, b.file[1], 0);
        refused(
            &format!("{k}: a file chain to a free cluster"),
            b.disk,
            Doubt::FileShort,
        );
        let b = fresh();
        set_fat(&b.disk, &l, b.file[1], l.end_mark());
        refused(
            &format!("{k}: a file chain short of its length"),
            b.disk,
            Doubt::FileShort,
        );
        let b = fresh();
        let mut t = Tree::new(&b.disk);
        let mut root = t.root();
        t.put(&mut root, raw(*b"NOCHAIN BIN", 0x20, 0, 10));
        refused(
            &format!("{k}: a length with no chain"),
            b.disk,
            Doubt::FileShort,
        );
        let b = fresh();
        let mut t = Tree::new(&b.disk);
        let mut root = t.root();
        t.put(&mut root, raw(*b"OUTSIDE BIN", 0x20, l.clusters() + 9, 10));
        refused(
            &format!("{k}: a start past the volume"),
            b.disk,
            Doubt::FileShort,
        );

        // Rule 5.
        let b = fresh();
        let mut t = Tree::new(&b.disk);
        let mut root = t.root();
        t.put(&mut root, raw(*b"TWIN    BIN", 0x20, b.other[0], 10));
        refused(
            &format!("{k}: a shared cluster"),
            b.disk,
            Doubt::CrossLinked,
        );
        let b = fresh();
        set_fat(&b.disk, &l, b.file[2], b.file[0]);
        refused(&format!("{k}: a loop"), b.disk, Doubt::CrossLinked);
        let b = fresh();
        let (blk, at) = slot_at(&l, &b.subsub, 2);
        poke(
            &b.disk,
            (blk, at),
            &raw(*b"UPAGAIN    ", 0x10, b.sub.start, 0),
        );
        refused(
            &format!("{k}: a loop to an ancestor"),
            b.disk,
            Doubt::CrossLinked,
        );

        // Rule 6.
        let d = fresh().disk;
        nested(&d, MAX_DEPTH);
        assert!(sweep_on(&Rc::new(d), Sweep::DryRun).is_ok(), "{k}: depth 8");
        let d = fresh().disk;
        nested(&d, MAX_DEPTH + 1);
        refused(&format!("{k}: depth 9"), d, Doubt::TooDeep);
        // `base` has 2 directories.
        let d = fresh().disk;
        wide(&d, MAX_DIRS - 2);
        assert!(
            sweep_on(&Rc::new(d), Sweep::DryRun).is_ok(),
            "{k}: 256 directories"
        );
        let d = fresh().disk;
        wide(&d, MAX_DIRS - 1);
        refused(&format!("{k}: 257 directories"), d, Doubt::TooDeep);

        // Rule 7: decided from the layout, with no block read past the
        // boot sector.
        let disk = Rc::new(fresh().disk);
        let mut s = probed(&disk);
        let vol = s.mount().unwrap();
        let reads = log(&s).reads.get();
        let mut marks = vec![0; (l.clusters() as usize).div_ceil(8) - 1];
        assert_eq!(
            s.sweep(vol, Sweep::Free, &mut marks).map_err(|(d, _)| d),
            Err(Doubt::NoRoom),
            "{k}"
        );
        assert_eq!(
            log(&s).reads.get() - reads,
            2,
            "{k}: the MBR and the boot sector"
        );

        // Rule 8: a swapped card.
        let disk = Rc::new(fresh().disk);
        let mut s = probed(&disk);
        let mut vol = s.mount().unwrap();
        vol.serial ^= 1;
        assert!(matches!(
            s.sweep(vol, Sweep::Free, &mut marks_for(&*disk)),
            Err((Doubt::Card(StoreError::VolumeChanged(_)), _))
        ));
    }
}

/// Sector D, FAT 2's copy behind FAT 1's: a file `A` whose last link only
/// FAT 1 holds (FAT 2 ends it early, its tail free), one loose cluster
/// each copy alone holds, and one both hold; `far`, lost in both outside
/// D. With `shape`, a reached cluster FAT 2 links past FAT 1 as well.
fn stale_d(kind: FsKind, shape: Option<&str>) -> (RamDisk, Layout, [u32; 6], u32) {
    let disk = match kind {
        FsKind::Fat16 => with_clusters(CLUSTERS, 0x0D0D),
        FsKind::Fat32 => fat32(0x0D0D),
    };
    let l = layout_of(&disk);
    let mut t = Tree::new(&disk);
    let mut root = t.root();
    if let Some(info) = l.fs_info() {
        // A count FSInfo trusts, for the free phase to mark unknown.
        disk.0.borrow_mut()[(PART_LBA + info) as usize][488..492].fill(0);
    }
    let a = t
        .file(&mut root, *b"A       BIN", None, (512, 1), 1, None)
        .chain;
    let b = t
        .file(&mut root, *b"B       BIN", None, (512, 1), 2, None)
        .chain;
    let only1 = t.lose(1, None)[0];
    let only2 = t.lose(1, None)[0];
    let both = t.lose(1, None)[0];
    let far = 2 + l.clusters() - 3;
    set_fat(&disk, &l, far, l.end_mark());
    let d = l.fat_block(a[0]);
    assert!(
        [a[1], b[0], b[1], only1, only2, both]
            .iter()
            .all(|&x| l.fat_block(x) == d)
    );
    assert_ne!(l.fat_block(far), d);
    let fat2 = l.fat_copies(d).nth(1).unwrap();
    {
        let mut blks = disk.0.borrow_mut();
        let f2 = &mut blks[(PART_LBA + fat2) as usize];
        l.put_entry(f2, a[0], l.end_mark());
        l.put_entry(f2, a[1], 0);
        l.put_entry(f2, only2, l.end_mark());
        l.put_entry(f2, only1, 0);
        let f1 = &mut blks[(PART_LBA + d) as usize];
        l.put_entry(f1, only2, 0);
        match shape {
            // B's last link only FAT 2 holds, its tail free in FAT 1.
            Some("tail") => {
                l.put_entry(f1, b[0], l.end_mark());
                l.put_entry(f1, b[1], 0);
            }
            // B's link only FAT 2 holds, its tail still held in FAT 1.
            Some("link") => l.put_entry(f1, b[0], l.end_mark()),
            // B's first cluster free in FAT 1: what a torn sector leaves.
            Some("free") => l.put_entry(f1, b[0], 0),
            _ => {}
        }
    }
    (disk, l, [a[0], a[1], b[0], b[1], only1, both], far)
}

/// A stale FAT 2 sector D: a reached cluster may differ only as a FAT
/// 1-first cut leaves it, FAT 1 newer, and then FAT 1 is followed, D
/// copied, and what both copies hold as lost freed; what only FAT 1 holds
/// goes at the next boot. FAT 2 linking past FAT 1 is `FatsDiffer`, with
/// nothing written.
#[test]
fn one_stale_sector_is_fat1_first_or_doubt() {
    for kind in [FsKind::Fat16, FsKind::Fat32] {
        let (disk, l, [a0, a1, b0, b1, only1, both], far) = stale_d(kind, None);
        let d = l.fat_block(a0);
        let fat2 = l.fat_copies(d).nth(1).unwrap();
        let disk = Rc::new(disk);
        let dry = sweep_on(&disk, Sweep::DryRun).unwrap();
        assert_eq!(
            (dry.lost, dry.stale, dry.healed, dry.settled()),
            (2, true, false, false),
            "{kind:?}: both and far; nothing healed dry"
        );
        let mut s = probed(&disk);
        let vol = s.mount().unwrap();
        let got = s.sweep(vol, Sweep::Free, &mut marks_for(&*disk)).unwrap();
        assert_eq!((got.lost, got.freed, got.healed), (2, 2, true), "{kind:?}");
        assert!(!got.settled(), "{kind:?}: only1 waits");
        let writes: Vec<u32> = log(&s).writes.borrow().iter().map(|&(b, _)| b).collect();
        assert_eq!(writes[0], PART_LBA + fat2, "{kind:?}: D's FAT 2 copy first");
        if let Some(info) = l.fs_info() {
            assert_eq!(writes[1], PART_LBA + info, "{kind:?}: then FSInfo");
        }
        for x in [a0, a1, b0, b1, only1] {
            assert!(holds(&l, fat_value(&disk, &l, 0, x)), "{kind:?}: {x} kept");
        }
        for x in [both, far] {
            assert_eq!(fat_value(&disk, &l, 0, x), 0, "{kind:?}: {x} freed");
        }
        // FAT 1 won the copy; what only it holds goes at the next boot.
        let next = swept(&disk);
        assert_eq!((next.freed, next.healed), (1, false), "{kind:?}: only1");
        assert!(next.settled(), "{kind:?}");
        assert_eq!(fat_value(&disk, &l, 0, only1), 0);
        assert_eq!(fat_value(&disk, &l, 1, only1), 0);
        assert!(holds(&l, fat_value(&disk, &l, 0, a1)), "{kind:?}: A's tail");

        for shape in ["tail", "link", "free"] {
            let (disk, ..) = stale_d(kind, Some(shape));
            refused(&format!("{kind:?}: B {shape}"), disk, Doubt::FatsDiffer);
        }
    }
}

/// A directory tail only FAT 2 links keeps every file in it, at this boot
/// and every later one.
#[test]
fn a_dir_tail_only_fat2_links_keeps_its_files() {
    let disk = fat32(0xD12E);
    let l = layout_of(&disk);
    let mut t = Tree::new(&disk);
    let mut root = t.root();
    let mut d = t.mkdir(&mut root, *b"HOSTDIR    ", None);
    for i in 0..14 {
        t.put(&mut d, raw(short_name(0, i), 0x20, 0, 0));
    }
    let f = t.file(
        &mut d,
        *b"TAIL    WAV",
        None,
        (3 * l.cluster_bytes(), 0),
        7,
        None,
    );
    assert_eq!(d.chain.len(), 2, "TAIL's entry sits in the second cluster");
    let s = l.fat_block(d.chain[0]);
    assert_eq!(s, l.fat_block(d.chain[1]));
    let mut blk = disk.block(PART_LBA + s);
    l.put_entry(&mut blk, d.chain[0], l.end_mark());
    l.put_entry(&mut blk, d.chain[1], 0);
    disk.0.borrow_mut()[(PART_LBA + s) as usize] = blk;
    let disk = Rc::new(disk);
    for boot in 0..3 {
        let got = sweep_on(&disk, Sweep::Free).map_err(|(d, _)| d);
        assert_eq!(got.map(|_| ()), Err(Doubt::FatsDiffer), "boot {boot}");
        for &c in &f.chain {
            assert_ne!(
                fat_value(&disk, &l, 0, c),
                0,
                "boot {boot}: TAIL's cluster {c} freed"
            );
        }
    }
}

/// A FAT 1 sector torn by a cut mid-write (its tail zeros, ones or noise,
/// FAT 2 the good copy) under a live chain: `FatsDiffer`, nothing written,
/// boot after boot. Copying it over FAT 2 would lose the only good copy.
#[test]
fn a_torn_fat1_sector_frees_nothing() {
    for kind in [FsKind::Fat16, FsKind::Fat32] {
        for tear in ["zeros", "ones", "noise"] {
            let fresh = || {
                let disk = match kind {
                    FsKind::Fat16 => with_clusters(CLUSTERS, 0x7EA2),
                    FsKind::Fat32 => fat32(0x7EA2),
                };
                let l = layout_of(&disk);
                let mut t = Tree::new(&disk);
                let mut root = t.root();
                // A chain over the whole of FAT 1's first sector.
                let per = match kind {
                    FsKind::Fat16 => 256,
                    FsKind::Fat32 => 128,
                };
                let f = t.file(&mut root, *b"LONG    WAV", None, (per * 512, 0), 5, None);
                assert!(f.chain.iter().any(|&c| l.fat_block(c) == l.fat1().start));
                t.lose(3, None);
                let mut rng = XorShift(0x7EA2);
                let mut d = disk.0.borrow_mut();
                let blk = &mut d[(PART_LBA + l.fat1().start) as usize];
                for b in &mut blk[256..] {
                    *b = match tear {
                        "zeros" => 0,
                        "ones" => 0xFF,
                        _ => rng.next() as u8,
                    };
                }
                drop(d);
                disk
            };
            let what = format!("{kind:?} {tear}");
            refused(&what, fresh(), Doubt::FatsDiffer);
            let disk = Rc::new(fresh());
            let before = blocks(&disk);
            for boot in 0..3 {
                let got = sweep_on(&disk, Sweep::Free).map_err(|(d, _)| d);
                assert_eq!(
                    got.map(|_| ()),
                    Err(Doubt::FatsDiffer),
                    "{what} boot {boot}"
                );
            }
            assert_eq!(blocks(&disk), before, "{what}: the image");
        }
    }
}

/// One run frees at most `max_lost`, two of the largest chains a cut can
/// strand: above it the free is refused, nothing written, while a dry run
/// still counts them all.
#[test]
fn a_sweep_frees_at_most_its_cap() {
    for (kind, clusters) in [(FsKind::Fat16, 20_000), (FsKind::Fat32, 66_000)] {
        let fresh = |extra: u32| {
            let disk = with_clusters(clusters, 0xCA9);
            let l = layout_of(&disk);
            let cap = max_lost(&l);
            assert_eq!(cap, 2 * ((MAX_CHAIN_BYTES).div_ceil(l.cluster_bytes()) + 1));
            let mut t = Tree::new(&disk);
            let mut root = t.root();
            t.file(&mut root, *b"KEPT    BIN", None, (2_000, 0), 1, None);
            t.lose(cap - 1 + extra, None);
            t.lose(1, None);
            (disk, cap)
        };
        let (disk, cap) = fresh(0);
        let disk = Rc::new(disk);
        assert_eq!(swept(&disk).freed, cap, "{kind:?}: the cap itself");
        let (disk, cap) = fresh(1);
        let dry = sweep_on(&Rc::new(copy(&disk)), Sweep::DryRun).unwrap();
        assert_eq!((dry.lost, dry.settled()), (cap + 1, false), "{kind:?}");
        refused(
            &format!("{kind:?}: one past"),
            disk,
            Doubt::TooMany(cap + 1),
        );
    }
}

/// A block device whose reads fail from the `after`th on.
struct FailAfter {
    inner: RamDisk,
    after: Cell<u32>,
    reads: Cell<u32>,
}

impl BlockDevice for FailAfter {
    type Error = DiskError;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), DiskError> {
        let n = self.reads.get();
        self.reads.set(n + 1);
        if n >= self.after.get() {
            return Err(DiskError);
        }
        self.inner.read(blocks, start)
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), DiskError> {
        self.inner.write(blocks, start)
    }

    fn num_blocks(&self) -> Result<BlockCount, DiskError> {
        self.inner.num_blocks()
    }
}

/// A card with lost clusters, a FAT 2 sector behind, and FSInfo: every
/// kind of write the free phase makes. Returns the lost clusters.
fn leaky(kind: FsKind) -> (RamDisk, Vec<u32>) {
    let disk = match kind {
        FsKind::Fat16 => with_clusters(CLUSTERS, 0x1EA4),
        FsKind::Fat32 => fat32(0x1EA4),
    };
    let mut rng = XorShift(0xBEEF);
    let (disk, lost) = foreign_card(disk, &[2, 300, 40], &mut rng);
    let l = layout_of(&disk);
    let last = (2..2 + l.clusters())
        .rev()
        .find(|&c| holds(&l, fat_value(&disk, &l, 0, c)))
        .unwrap();
    let fat2 = l.fat_copies(l.fat_block(last)).nth(1).unwrap();
    let mut d = disk.0.borrow_mut();
    l.put_entry(&mut d[(PART_LBA + fat2) as usize], last, 0);
    drop(d);
    (Rc::try_unwrap(disk).ok().unwrap(), lost)
}

#[test]
fn sweep_read_error_frees_nothing() {
    for kind in [FsKind::Fat16, FsKind::Fat32] {
        let (base, _) = leaky(kind);
        let total = {
            let d = Rc::new(FailAfter {
                inner: copy(&base),
                after: Cell::new(u32::MAX),
                reads: Cell::new(0),
            });
            let mut s = probed(&d);
            let vol = s.mount().unwrap();
            d.reads.set(0);
            s.sweep(vol, Sweep::Free, &mut marks_for(&*d)).unwrap();
            assert!(!log(&s).writes.borrow().is_empty());
            // The free phase's reads come after its first write: only
            // the read phases' count.
            d.reads.get()
        };
        for after in (0..total).step_by(7).chain([total - 1]) {
            let d = Rc::new(FailAfter {
                inner: copy(&base),
                after: Cell::new(u32::MAX),
                reads: Cell::new(0),
            });
            let mut s = probed(&d);
            let vol = s.mount().unwrap();
            d.reads.set(0);
            d.after.set(after);
            let got = s.sweep(vol, Sweep::Free, &mut marks_for(&*d));
            if let Err((Doubt::Unreadable, _)) = got {
                assert!(log(&s).writes.borrow().is_empty(), "{kind:?} read {after}");
                assert_eq!(blocks(&d.inner), blocks(&base), "{kind:?} read {after}");
            } else {
                // Past the read phases: the failure fell in the free phase.
                assert!(
                    matches!(got, Err((Doubt::Card(_), _))),
                    "{kind:?} read {after}: {got:?}"
                );
            }
        }
        for armed in ["fail_reads", "absent"] {
            let disk = Rc::new(copy(&base));
            let mut s = probed(&disk);
            let vol = s.mount().unwrap();
            match armed {
                "fail_reads" => log(&s).fail_reads.set(u32::MAX),
                _ => log(&s).absent.set(true),
            }
            let got = s.sweep(vol, Sweep::Free, &mut marks_for(&*disk));
            assert!(
                matches!(
                    got,
                    Err((Doubt::Unreadable | Doubt::Card(StoreError::NoCard), _))
                ),
                "{kind:?} {armed}: {got:?}"
            );
            assert!(log(&s).writes.borrow().is_empty());
            assert_eq!(blocks(&disk), blocks(&base));
        }
    }
}

/// FAT 1 as `before` had it, but `lost` clusters each as they were or
/// free, and only free once `done`; with `done`, FAT 2 the same.
fn fat_kept(what: &str, image: &RamDisk, before: &RamDisk, lost: &[u32], done: bool) {
    let l = layout_of(before);
    for c in 2..2 + l.clusters() {
        let (was, now) = (fat_value(before, &l, 0, c), fat_value(image, &l, 0, c));
        let ok = match (lost.contains(&c), done) {
            (false, _) => now == was,
            (true, false) => now == was || now == 0,
            (true, true) => now == 0,
        };
        assert!(ok, "{what}: cluster {c} was {was:#x}, now {now:#x}");
        if done {
            assert_eq!(
                fat_value(image, &l, 1, c),
                now,
                "{what}: cluster {c} in FAT 2"
            );
        }
    }
}

/// What a free phase stopped partway may leave: FAT 1 changed only where
/// it freed a lost cluster, no data block touched; then the next boots'
/// sweeps free the rest.
fn after_a_stopped_sweep<D: BlockDevice<Error = DiskError>>(
    what: &str,
    disk: &Rc<D>,
    image: &RamDisk,
    (before, lost): (&RamDisk, &[u32]),
    got: SweepSeen,
) {
    assert!(matches!(got, Err((Doubt::Card(_), _))), "{what}: {got:?}");
    fat_kept(what, image, before, lost, false);
    assert_eq!(
        outside_fats(image),
        outside_fats(before),
        "{what}: a data block"
    );
    sweep_until_clean(what, disk);
    fat_kept(what, image, before, lost, true);
    let r = fat_check(image);
    assert!(
        r.cross_linked.is_empty() && r.short.is_empty(),
        "{what}: {r:?}"
    );
}

/// The free phase's writes, in order.
fn free_writes(base: &RamDisk) -> Vec<u32> {
    let disk = Rc::new(copy(base));
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    let got: Swept = s.sweep(vol, Sweep::Free, &mut marks_for(&*disk)).unwrap();
    assert!(got.freed > 300 && got.healed);
    log(&s).writes.borrow().iter().map(|&(b, _)| b).collect()
}

#[test]
fn sweep_write_error_is_safe() {
    for kind in [FsKind::Fat16, FsKind::Fat32] {
        let (base, lost) = leaky(kind);
        let writes = free_writes(&base);
        for (k, &block) in writes.iter().enumerate() {
            for land in [false, true] {
                let what = format!(
                    "{kind:?} {} at write {k} (block {block})",
                    ["fail_write", "land_then_fail"][usize::from(land)]
                );
                let disk = Rc::new(copy(&base));
                let mut s = probed(&disk);
                let vol = s.mount().unwrap();
                match land {
                    false => log(&s).fail_write.set(Some(block)),
                    true => log(&s).land_then_fail.set(Some(k)),
                }
                let got = s.sweep(vol, Sweep::Free, &mut marks_for(&*disk));
                after_a_stopped_sweep(&what, &disk, &disk, (&base, &lost), got);
            }
        }
    }
}

#[test]
fn sweep_cut_is_safe() {
    for kind in [FsKind::Fat16, FsKind::Fat32] {
        let (base, lost) = leaky(kind);
        let writes = free_writes(&base);
        for k in 0..writes.len() {
            let what = format!("{kind:?} cut at write {k} of {}", writes.len());
            let cut = slot(copy(&base));
            cut.cut.set(Cut::After(k as u32));
            let got = sweep_on(&cut, Sweep::Free);
            cut.cut.set(Cut::Never);
            after_a_stopped_sweep(&what, &cut, &cut.inner, (&base, &lost), got);
        }
    }
}
