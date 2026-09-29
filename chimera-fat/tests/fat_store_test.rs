#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;

use chimera_fat::FatStore;
use chimera_fat::dir::{ShortName, Slot, lfn_checksum, parse};
use chimera_fat::volume::{FsKind, Layout, Link, Root, first_partition, layout};
use chimera_hal::store::{Dir, FileName, Store, StoreError, Unsupported, VolumeId};
use core::cell::RefCell;
use embedded_sdmmc::BlockDevice;
use image::{
    Cut, CutDisk, DiskError, Overlay, PART_LBA, RamDisk, exfat, exfat_superfloppy, fat16, fat32,
    layout_of, pattern, superfloppy, with_clusters,
};
use probe::{Probed, Sink, XorShift, consistent, fats, fats_equal, free, log, probed, suite};
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

#[test]
fn fat16_passes_suite() {
    suite(|serial| fat16(16_384, serial));
}

#[test]
fn fat32_passes_suite() {
    suite(fat32);
}

#[test]
fn exfat_mount_is_unsupported() {
    let mut s = FatStore::new(exfat());
    assert_eq!(s.mount(), Err(StoreError::Unsupported(Unsupported::Exfat)));
}

#[test]
fn exfat_superfloppy_mount_is_unsupported() {
    let mut s = FatStore::new(exfat_superfloppy());
    assert_eq!(s.mount(), Err(StoreError::Unsupported(Unsupported::Exfat)));
}

#[test]
fn superfloppy_mount_is_unsupported() {
    let mut s = FatStore::new(superfloppy());
    assert_eq!(
        s.mount(),
        Err(StoreError::Unsupported(Unsupported::NoPartitionTable))
    );
}

/// Mirroring off: refused at mount, before anything is written.
#[test]
fn unmirrored_fat32_mount_is_unsupported() {
    let disk = Rc::new(fat32(1));
    disk.0.borrow_mut()[PART_LBA as usize][40] = 0x80;
    let mut s = probed(&disk);
    assert_eq!(
        s.mount(),
        Err(StoreError::Unsupported(Unsupported::FatNotMirrored))
    );
    assert_eq!(*log(&s).writes.borrow(), []);
}

/// Sectors per cluster 0 is refused before anything divides by it.
#[test]
fn corrupt_bpb_mount_is_unsupported() {
    let disk = fat16(16_384, 1);
    disk.0.borrow_mut()[PART_LBA as usize][13] = 0;
    let mut s = FatStore::new(disk);
    assert_eq!(
        s.mount(),
        Err(StoreError::Unsupported(Unsupported::BadBootSector))
    );
}

// Images for the file-level tests.

/// FAT16's fewest clusters: its FAT's last sector is partial, with zeros
/// past the last cluster.
const CLUSTERS: u32 = 4_085;

fn copy(disk: &RamDisk) -> RamDisk {
    RamDisk(RefCell::new(disk.0.borrow().clone()))
}

fn name(stem: &[u8]) -> FileName {
    FileName::new(Dir::Chimera, stem, b"BIN").unwrap()
}

/// A small FAT16 card with `/CHIMERA` made, and its id.
fn card() -> (Rc<RamDisk>, VolumeId) {
    let disk = Rc::new(with_clusters(CLUSTERS, 7));
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    (disk, vol)
}

/// FAT32 with a valid FSInfo holding a real free count.
fn fat32_counted(serial: u32) -> RamDisk {
    let d = fat32(serial);
    let l = layout_of(&d);
    let at = (PART_LBA + l.fs_info().unwrap()) as usize;
    d.0.borrow_mut()[at][488..492].copy_from_slice(&(l.clusters() - 1).to_le_bytes());
    d
}

/// Sets cluster `c`'s entry in every FAT.
fn put(disk: &RamDisk, c: u32, v: u32) {
    let l = layout_of(disk);
    let mut blocks = disk.0.borrow_mut();
    for at in l.fat_copies(l.fat_block(c)) {
        l.put_entry(&mut blocks[(PART_LBA + at) as usize], c, v);
    }
}

/// Marks every free cluster but the first `keep` bad, in every FAT.
fn fill(disk: &RamDisk, keep: u32) {
    let l = layout_of(disk);
    let mut kept = 0;
    for c in 2..l.clusters() + 2 {
        if l.entry(&disk.block(PART_LBA + l.fat_block(c)), c) != 0 {
            continue;
        }
        if kept < keep {
            kept += 1;
            continue;
        }
        put(disk, c, l.end_mark() - 8);
    }
    assert_eq!(kept, keep);
}

/// The block and offset of the file or directory entry named `name83`
/// (space padded, no dot); labels are skipped.
fn entry(disk: &RamDisk, name83: &[u8; 11]) -> (u32, usize) {
    let blocks = disk.0.borrow();
    (PART_LBA as usize..blocks.len())
        .flat_map(|b| (0..512).step_by(32).map(move |at| (b, at)))
        .find(|&(b, at)| &blocks[b][at..at + 11] == name83 && blocks[b][at + 11] & 0x08 == 0)
        .map(|(b, at)| (b as u32, at))
        .expect("entry on the image")
}

/// `name83`'s start cluster.
fn start(disk: &RamDisk, name83: &[u8; 11]) -> u32 {
    let (b, at) = entry(disk, name83);
    let e = disk.block(b);
    u32::from(u16::from_le_bytes([e[at + 26], e[at + 27]]))
        | u32::from(u16::from_le_bytes([e[at + 20], e[at + 21]])) << 16
}

/// Absolute block `block`'s slot at byte `at` becomes `raw`.
fn poke(disk: &RamDisk, block: u32, at: usize, raw: &[u8; 32]) {
    disk.0.borrow_mut()[block as usize][at..at + 32].copy_from_slice(raw);
}

/// A raw entry: an 11-byte name and an attribute.
fn raw(name: &[u8; 11], attr: u8) -> [u8; 32] {
    let mut r = [0; 32];
    r[..11].copy_from_slice(name);
    r[11] = attr;
    r
}

/// The root's first block, absolute.
fn root_block(disk: &RamDisk) -> u32 {
    let l = layout_of(disk);
    PART_LBA
        + match l.root() {
            Root::Fixed { first, .. } => first,
            Root::Cluster(c) => l.cluster_block(c).unwrap(),
        }
}

/// Every block of both FATs.
fn fat_region(disk: &RamDisk) -> Vec<[u8; 512]> {
    let l = layout_of(disk);
    let (fat1, fat2) = fats(&l);
    let at = |b: u32| (PART_LBA + b) as usize;
    disk.0.borrow()[at(fat1)..at(2 * fat2 - fat1)].to_vec()
}

fn read_back<D: BlockDevice<Error = DiskError>>(
    s: &mut Probed<D>,
    vol: VolumeId,
    f: FileName,
) -> Vec<u8> {
    let mut sink = Sink::default();
    s.read(vol, f, &mut sink).unwrap();
    sink.0
}

#[test]
fn fill_until_full_stays_in_the_partition() {
    let images = [
        ("FAT16, 4 085 clusters", with_clusters(4_085, 1)),
        ("FAT16, 5 000 clusters", with_clusters(5_000, 1)),
        ("FAT32, 66 000 clusters", fat32(1)),
    ];
    for (what, image) in images {
        let l = layout_of(&image);
        let per_sector = match l.kind() {
            FsKind::Fat16 => 256,
            FsKind::Fat32 => 128,
        };
        assert_ne!(
            (l.clusters() + 2) % per_sector,
            0,
            "{what}: a partial last FAT sector"
        );
        let disk = Rc::new(image);
        let mut s = probed(&disk);
        let vol = s.mount().unwrap();
        s.make_dir(vol, Dir::Chimera).unwrap();
        let cluster = l.cluster_bytes() as usize;
        // Most of the card in one file, so the fill is quick.
        let blocks = (l.clusters() - 400) * l.blocks_per_cluster();
        s.write(vol, name(b"BIG"), &mut |w| {
            (0..blocks).try_for_each(|_| w.put(&[1; 512]))
        })
        .unwrap();
        let mut i = 0;
        let (last, body) = loop {
            let body = pattern(i as u8, 2 * cluster);
            let f = name(format!("F{i}").as_bytes());
            match s.write(vol, f, &mut |w| w.put(&body)) {
                Ok(n) => assert_eq!(n as usize, body.len(), "{what}"),
                Err(StoreError::Full) => break (f, body),
                Err(e) => panic!("{what}: {e:?}"),
            }
            i += 1;
        };
        assert_eq!(
            free(&disk),
            (0, 0),
            "{what}: Full only once no cluster is free"
        );
        let got = read_back(&mut s, vol, last);
        assert!(
            body.starts_with(&got),
            "{what}: the partial file keeps its bytes"
        );
        consistent(&s);
        s.delete(vol, name(b"F0")).unwrap();
        let again = pattern(99, 2 * cluster);
        s.write(vol, name(b"AGAIN"), &mut |w| w.put(&again))
            .unwrap();
        assert_eq!(read_back(&mut s, vol, name(b"AGAIN")), again, "{what}");
        consistent(&s);
    }
}

/// One free cluster: a 2-cluster write takes it, keeps its bytes and is
/// `Full`; deleting that file gives the cluster back; a 1-cluster write then
/// takes it and a 1 B write is `Full`, leaking nothing.
#[test]
fn last_free_cluster_is_used() {
    let (disk, vol) = card();
    fill(&disk, 1);
    let mut s = probed(&disk);
    let body = pattern(1, 2 * 512);
    assert_eq!(
        s.write(vol, name(b"PART"), &mut |w| w.put(&body)),
        Err(StoreError::Full)
    );
    assert_eq!(read_back(&mut s, vol, name(b"PART")), body[..512]);
    assert_eq!(free(&disk), (0, 0));
    s.delete(vol, name(b"PART")).unwrap();
    assert_eq!(free(&disk), (1, 1), "no leak");

    assert_eq!(
        s.write(vol, name(b"ONE"), &mut |w| w.put(&[1; 512])),
        Ok(512)
    );
    assert_eq!(free(&disk), (0, 0));
    assert_eq!(
        s.write(vol, name(b"TWO"), &mut |w| w.put(b"x")),
        Err(StoreError::Full)
    );
    s.delete(vol, name(b"TWO")).unwrap();
    assert_eq!(free(&disk), (0, 0), "no leak");
    assert_eq!(read_back(&mut s, vol, name(b"ONE")), [1; 512]);
    consistent(&s);
}

#[test]
fn delete_frees_the_chain_in_every_fat() {
    for image in [fat16(16_384, 1), fat32(1)] {
        let disk = Rc::new(image);
        let mut s = probed(&disk);
        let vol = s.mount().unwrap();
        s.make_dir(vol, Dir::Chimera).unwrap();
        let fat = fat_region(&disk);
        let before = free(&disk);
        s.write(vol, name(b"DATA"), &mut |w| w.put(&pattern(1, 5_000)))
            .unwrap();
        let n = 5_000u32.div_ceil(layout_of(&disk).cluster_bytes());
        assert_eq!(free(&disk), (before.0 - n, before.1 - n));
        s.delete(vol, name(b"DATA")).unwrap();
        assert_eq!(free(&disk), before);
        assert!(fat_region(&disk) == fat, "both FATs as before");
        consistent(&s);
    }
}

#[test]
fn overwrite_frees_the_old_chain() {
    let (disk, vol) = card();
    let mut s = probed(&disk);
    let before = free(&disk);
    s.write(vol, name(b"DATA"), &mut |w| w.put(&pattern(1, 5_000)))
        .unwrap();
    s.write(vol, name(b"DATA"), &mut |w| w.put(&[9; 10]))
        .unwrap();
    assert_eq!(free(&disk), (before.0 - 1, before.1 - 1));
    assert_eq!(read_back(&mut s, vol, name(b"DATA")), [9; 10]);
    consistent(&s);
}

/// A fresh store (no hint) takes an overwritten file's clusters back in the
/// same order: the FAT ends as it began, and no sector is rewritten
/// unchanged on the way.
#[test]
fn overwrite_in_place_rewrites_no_fat_sector_unchanged() {
    let (disk, vol) = card();
    probed(&disk)
        .write(vol, name(b"DATA"), &mut |w| w.put(&[1; 3 * 512]))
        .unwrap();
    let (fat, first) = (fat_region(&disk), start(&disk, b"DATA    BIN"));
    let mut s = probed(&disk);
    s.write(vol, name(b"DATA"), &mut |w| w.put(&[2; 3 * 512]))
        .unwrap();
    assert_eq!(start(&disk, b"DATA    BIN"), first, "the same clusters");
    assert!(fat_region(&disk) == fat, "the same FAT");
    consistent(&s);
}

#[test]
fn read_only_ops_write_nothing() {
    for image in [fat16(16_384, 1), fat32_counted(1)] {
        let disk = Rc::new(image);
        let vol = {
            let mut s = probed(&disk);
            let vol = s.mount().unwrap();
            s.make_dir(vol, Dir::Chimera).unwrap();
            s.write(vol, name(b"DATA"), &mut |w| w.put(&pattern(1, 3_000)))
                .unwrap();
            vol
        };
        let mut s = probed(&disk);
        let other = VolumeId {
            serial: vol.serial ^ 1,
            ..vol
        };
        let in_projects = FileName::new(Dir::Projects, b"A", b"CHP").unwrap();
        let (data, nope) = (name(b"DATA"), name(b"NOPE"));
        let changed = Err(StoreError::VolumeChanged(vol));
        assert_eq!(s.mount(), Ok(vol));
        assert_eq!(s.list(vol, Dir::Chimera, &mut |_, _| {}), Ok(()));
        assert_eq!(read_back(&mut s, vol, data), pattern(1, 3_000));
        assert_eq!(s.make_dir(vol, Dir::Chimera), Ok(()), "made already");
        let not_found = Err(StoreError::NotFound);
        assert_eq!(s.list(vol, Dir::Projects, &mut |_, _| {}), not_found);
        assert_eq!(s.read(vol, nope, &mut Sink::default()), not_found);
        assert_eq!(
            s.write(vol, in_projects, &mut |w| w.put(b"x")),
            Err(StoreError::NotFound)
        );
        assert_eq!(s.delete(vol, nope), not_found);
        assert_eq!(s.make_dir(vol, Dir::Chimera), Ok(()));
        // The card isn't `other`.
        let mut s2 = probed(&disk);
        assert_eq!(s2.list(other, Dir::Chimera, &mut |_, _| {}), changed);
        assert_eq!(s2.read(other, data, &mut Sink::default()), changed);
        assert_eq!(
            s2.write(other, data, &mut |w| w.put(b"x")).map(|_| ()),
            changed
        );
        assert_eq!(s2.delete(other, data), changed);
        assert_eq!(s2.make_dir(other, Dir::Sounds), changed);
        assert_eq!(
            *log(&s).writes.borrow(),
            [],
            "{:?}",
            layout_of(&disk).kind()
        );
        assert_eq!(
            *log(&s2).writes.borrow(),
            [],
            "{:?}",
            layout_of(&disk).kind()
        );
    }
}

#[test]
fn fsinfo_is_written_at_most_once() {
    let fsi = PART_LBA + 1;
    let mut want = fat32_counted(1).block(fsi);
    want[488..492].fill(0xFF);
    let cards = suite(fat32_counted);
    assert_eq!(cards.len(), 3);
    for (i, (disk, log)) in cards.iter().enumerate() {
        let writes = log.writes.borrow();
        assert_eq!(writes.iter().filter(|w| w.0 == fsi).count(), 1, "card {i}");
        assert_eq!(writes[0].0, fsi, "card {i}: before anything else");
        assert_eq!(
            disk.block(fsi),
            want,
            "card {i}: only the free count changes"
        );
    }
    for (i, (_, log)) in suite(fat32).iter().enumerate() {
        let writes = log.writes.borrow();
        assert!(
            !writes.is_empty() && writes.iter().all(|w| w.0 != fsi),
            "card {i}: unknown already"
        );
    }
}

#[test]
fn bad_fsinfo_is_left_alone() {
    let fsi = PART_LBA + 1;
    let bad = |serial| {
        let d = fat32_counted(serial);
        d.0.borrow_mut()[fsi as usize][..4].fill(0);
        d
    };
    let was = bad(1).block(fsi);
    for (i, (disk, log)) in suite(bad).iter().enumerate() {
        assert!(log.writes.borrow().iter().all(|w| w.0 != fsi), "card {i}");
        assert_eq!(disk.block(fsi), was, "card {i}");
    }

    let clusters = layout_of(&fat32(1)).clusters();
    for (what, next) in [
        ("1", 1),
        ("count + 2", clusters + 2),
        ("the root's cluster", 2),
    ] {
        let disk = Rc::new(fat32(1));
        disk.0.borrow_mut()[fsi as usize][492..496].copy_from_slice(&u32::to_le_bytes(next));
        let mut s = probed(&disk);
        let vol = s.mount().unwrap();
        s.make_dir(vol, Dir::Chimera).unwrap();
        for (i, f) in [name(b"A"), name(b"B")].into_iter().enumerate() {
            s.write(vol, f, &mut |w| w.put(&pattern(i as u8, 5_000)))
                .unwrap();
        }
        for (i, f) in [name(b"A"), name(b"B")].into_iter().enumerate() {
            assert_eq!(read_back(&mut s, vol, f), pattern(i as u8, 5_000), "{what}");
        }
        let used = 1 + 1 + 2 * 10;
        assert_eq!(free(&disk).0, clusters - used, "{what}");
        consistent(&s);
    }
}

/// `got` is `want`, after `reinits` re-inits.
fn check<D: BlockDevice<Error = DiskError>>(
    what: &str,
    got: Result<(), StoreError>,
    want: StoreError,
    s: &Probed<D>,
    reinits: u32,
) {
    assert_eq!(got, Err(want), "{what}");
    assert_eq!(log(s).reinits.get(), reinits, "{what}: reinits");
}

fn list<D: BlockDevice<Error = DiskError>>(
    s: &mut Probed<D>,
    vol: VolumeId,
) -> Result<(), StoreError> {
    s.list(vol, Dir::Chimera, &mut |_, _| {})
}

#[test]
fn error_mapping_table() {
    let (disk, vol) = card();
    fill(&disk, 0);
    let mut s = probed(&disk);
    let r = s.write(vol, name(b"NEW"), &mut |w| w.put(b"x")).map(|_| ());
    check("a full card", r, StoreError::Full, &s, 0);
    assert_eq!(list(&mut s, vol), Ok(()));

    let (disk, vol) = card();
    let mut s = probed(&disk);
    s.write(vol, name(b"DATA"), &mut |w| w.put(&[1; 3 * 512]))
        .unwrap();
    put(&disk, start(&disk, b"DATA    BIN") + 1, 0);
    let r = s.read(vol, name(b"DATA"), &mut Sink::default());
    check("a broken chain", r, StoreError::Corrupt, &s, 0);
    assert_eq!(list(&mut s, vol), Ok(()));

    let (disk, vol) = card();
    let mut s = probed(&disk);
    let r = s.read(vol, name(b"NOPE"), &mut Sink::default());
    check("no such file", r, StoreError::NotFound, &s, 0);
    assert_eq!(list(&mut s, vol), Ok(()));

    let (disk, vol) = card();
    let cut = Rc::new(CutDisk::new(copy(&disk), Cut::After(0)));
    let mut s = probed(&cut);
    let r = s.write(vol, name(b"NEW"), &mut |w| w.put(b"x")).map(|_| ());
    check("a cut card", r, StoreError::Io, &s, 1);
    assert_eq!(s.mount(), Ok(vol));
    assert_eq!(list(&mut s, vol), Ok(()));

    let (disk, vol) = card();
    let mut s = probed(&disk);
    log(&s).absent.set(true);
    check(
        "an empty slot",
        list(&mut s, vol),
        StoreError::NoCard,
        &s,
        1,
    );
    log(&s).absent.set(false);
    assert_eq!(list(&mut s, vol), Ok(()));

    let (_, vol) = card();
    let other = Rc::new(with_clusters(CLUSTERS, 8));
    let mut s = probed(&other);
    let now = probed(&other).mount().unwrap();
    check(
        "a swapped card",
        list(&mut s, vol),
        StoreError::VolumeChanged(now),
        &s,
        0,
    );
    assert_eq!(s.mount(), Ok(now));
    assert_eq!(s.make_dir(now, Dir::Chimera), Ok(()));
}

/// The two free clusters sit either side of a FAT sector boundary, so the
/// allocation of the second flushes the first's sector: the cut lands in
/// the allocator. A device error there is `Io`, never `Full`.
#[test]
fn alloc_device_error_is_io() {
    let (disk, vol) = card();
    fill(&disk, 0);
    put(&disk, 255, 0);
    put(&disk, 256, 0);
    let l = layout_of(&disk);
    let (fat1, _) = fats(&l);
    let dry = Rc::new(copy(&disk));
    let mut s = probed(&dry);
    s.write(vol, name(b"NEW"), &mut |w| w.put(&[1; 1024]))
        .unwrap();
    let writes = log(&s).writes.borrow().clone();
    let first_fat = writes
        .iter()
        .position(|&(b, _)| b == PART_LBA + fat1)
        .expect("a FAT write");
    assert!(
        writes[first_fat + 2..]
            .iter()
            .any(|&(b, _)| b == PART_LBA + fat1 + 1),
        "the second cluster's sector is written after"
    );

    let cut = Rc::new(CutDisk::new(copy(&disk), Cut::After(first_fat as u32)));
    let mut s = probed(&cut);
    assert_eq!(
        s.write(vol, name(b"NEW"), &mut |w| w.put(&[1; 1024])),
        Err(StoreError::Io)
    );
    assert_eq!(log(&s).reinits.get(), 1);
}

/// A data write fails while the FAT cache holds the new clusters: the
/// operation ends there, `Io`, and the change is dropped, never flushed
/// later. The card keeps an empty file and no lost cluster, and the next
/// operation works.
#[test]
fn a_device_error_drops_the_unflushed_fat_change() {
    let (disk, vol) = card();
    let before = free(&disk);
    let (fat1, _) = fats(&layout_of(&disk));
    let dry = Rc::new(copy(&disk));
    let mut s = probed(&dry);
    s.write(vol, name(b"NEW"), &mut |w| w.put(&[1; 1024]))
        .unwrap();
    let first_fat = log(&s)
        .writes
        .borrow()
        .iter()
        .position(|&(b, _)| b == PART_LBA + fat1)
        .expect("a FAT write");

    let cut = Rc::new(CutDisk::new(copy(&disk), Cut::After(first_fat as u32 - 1)));
    let mut s = probed(&cut);
    assert_eq!(
        s.write(vol, name(b"NEW"), &mut |w| w.put(&[1; 1024])),
        Err(StoreError::Io)
    );
    cut.cut.set(Cut::Never);
    assert_eq!(read_back(&mut s, vol, name(b"NEW")), []);
    assert_eq!(free(&cut.inner), before, "nothing reached the FAT");
    s.write(vol, name(b"NEW"), &mut |w| w.put(&[2; 1024]))
        .unwrap();
    assert_eq!(read_back(&mut s, vol, name(b"NEW")), [2; 1024]);
    assert!(fats_equal(&cut.inner));
}

/// `DATA.BIN` (3 clusters) and an empty `EMPTY.BIN`, then an overwrite of
/// `DATA` whose FAT 2 write fails after FAT 1's landed. Returns the store
/// that saw the failure, and the free counts before it.
fn failed_fat2_write() -> (Rc<RamDisk>, VolumeId, Probed<RamDisk>, (u32, u32)) {
    let (disk, vol) = card();
    let mut s = probed(&disk);
    s.write(vol, name(b"DATA"), &mut |w| w.put(&[1; 3 * 512]))
        .unwrap();
    s.write(vol, name(b"EMPTY"), &mut |_| Ok(())).unwrap();
    let before = free(&disk);
    let (_, fat2) = fats(&layout_of(&disk));
    let mut s = probed(&disk);
    log(&s).fail_write.set(Some(PART_LBA + fat2));
    assert_eq!(
        s.write(vol, name(b"DATA"), &mut |w| w.put(&[2; 2 * 512])),
        Err(StoreError::Io)
    );
    assert_eq!(log(&s).reinits.get(), 1);
    assert!(!fats_equal(&disk), "FAT 2 missed the change");
    log(&s).fail_write.set(None);
    (disk, vol, s, before)
}

/// FAT 1's write lands, FAT 2's fails: the op is `Io`, and the store keeps
/// the sector. A read sees FAT 1's volume, consistent (the file empty, its
/// old chain free), and writes nothing. The next op that changes the
/// volume first copies the sector from FAT 1 to FAT 2, even when its own
/// change is elsewhere: deleting an empty file touches no FAT entry.
#[test]
fn a_failed_fat2_write_is_healed_by_the_next_change() {
    let (disk, vol, mut s, before) = failed_fat2_write();
    let writes = log(&s).writes.borrow().len();
    assert_eq!(read_back(&mut s, vol, name(b"DATA")), []);
    assert_eq!(free(&disk).0, before.0 + 3, "FAT 1: the old chain is free");
    assert_eq!(
        log(&s).writes.borrow().len(),
        writes,
        "a read heals nothing"
    );

    s.delete(vol, name(b"EMPTY")).unwrap();
    assert!(fats_equal(&disk), "healed");
    assert_eq!(free(&disk), (before.0 + 3, before.0 + 3));
    consistent(&s);
    let writes = log(&s).writes.borrow().len();
    s.delete(vol, name(b"DATA")).unwrap();
    let fat2 = PART_LBA + fats(&layout_of(&disk)).1;
    assert!(
        log(&s).writes.borrow()[writes..]
            .iter()
            .all(|w| w.0 != fat2),
        "healed once"
    );
}

/// The kept sector is its volume's: a card with another id isn't touched,
/// and the record is dropped.
#[test]
fn a_stale_fat_sector_is_healed_on_its_own_volume_only() {
    let (disk, _, mut s, _) = failed_fat2_write();
    // The same image under another serial: another volume.
    disk.0.borrow_mut()[PART_LBA as usize][0x27] ^= 1;
    let other = s.mount().unwrap();
    s.delete(other, name(b"EMPTY")).unwrap();
    assert!(!fats_equal(&disk), "not healed on another volume");
    disk.0.borrow_mut()[PART_LBA as usize][0x27] ^= 1;
    let vol = s.mount().unwrap();
    s.write(vol, name(b"NEW"), &mut |_| Ok(())).unwrap();
    assert!(!fats_equal(&disk), "the record went with the other volume");
}

/// An overwrite of a 3-cluster file with 2: the entry's reset comes before
/// any FAT write, each FAT sector goes to FAT 1 then FAT 2, and the entry's
/// final write comes after all of them.
#[test]
fn write_order_is_cut_safe() {
    let (disk, vol) = card();
    probed(&disk)
        .write(vol, name(b"DATA"), &mut |w| w.put(&[1; 3 * 512]))
        .unwrap();
    let before = copy(&disk);
    let (eb, at) = entry(&disk, b"DATA    BIN");
    let (fat1, fat2) = fats(&layout_of(&disk));
    let size = fat2 - fat1;
    let mut s = probed(&disk);
    s.write(vol, name(b"DATA"), &mut |w| w.put(&[2; 2 * 512]))
        .unwrap();
    let w: Vec<u32> = log(&s).writes.borrow().iter().map(|&(b, _)| b).collect();
    let is_fat = |b: u32| (PART_LBA + fat1..PART_LBA + fat2 + size).contains(&b);
    let fat: Vec<usize> = (0..w.len()).filter(|&i| is_fat(w[i])).collect();
    assert_eq!(w[0], eb, "the entry is reset first: {w:?}");
    assert_eq!(w[w.len() - 1], eb, "and set last: {w:?}");
    assert_eq!(fat.len(), 4, "the free, then the new chain: {w:?}");
    for pair in fat.chunks(2) {
        assert_eq!(pair[1], pair[0] + 1, "{w:?}");
        assert_eq!(w[pair[1]], w[pair[0]] + size, "FAT 1 then FAT 2: {w:?}");
    }
    assert!(fat[0] > 0 && fat[3] < w.len() - 1, "{w:?}");

    // Cut after the reset: an empty file, no chain.
    let cut = Rc::new(CutDisk::new(before, Cut::After(1)));
    let mut s = probed(&cut);
    assert_eq!(
        s.write(vol, name(b"DATA"), &mut |w| w.put(&[2; 2 * 512])),
        Err(StoreError::Io)
    );
    let e = cut.inner.block(eb);
    assert_eq!(e[at + 26..at + 32], [0; 6], "start 0, length 0");
}

#[test]
fn broken_chains_read_corrupt_and_stay_writable() {
    let (base, vol) = card();
    probed(&base)
        .write(vol, name(b"DATA"), &mut |w| w.put(&[7; 3 * 512]))
        .unwrap();
    let l = layout_of(&base);
    let first = start(&base, b"DATA    BIN");
    let bad = l.end_mark() - 8;
    let mut cases: Vec<(&str, RamDisk)> = Vec::new();
    for (what, to) in [
        ("links to a free cluster", 0),
        ("links to reserved cluster 1", 1),
        ("links past the volume", l.clusters() + 2),
        ("links to the bad mark", bad),
        ("is a 2-cycle", first),
        ("links to itself", first + 1),
    ] {
        let disk = copy(&base);
        put(&disk, first + 1, to);
        cases.push((what, disk));
    }
    let disk = copy(&base);
    let (b, at) = entry(&disk, b"DATA    BIN");
    disk.0.borrow_mut()[b as usize][at + 28..at + 32].copy_from_slice(&(5 * 512u32).to_le_bytes());
    cases.push(("is shorter than the length", disk));

    for (what, disk) in cases {
        let disk = Rc::new(disk);
        let mut s = probed(&disk);
        let mut sink = Sink::default();
        let r = s.read(vol, name(b"DATA"), &mut sink);
        assert_eq!(r, Err(StoreError::Corrupt), "the chain {what}");
        assert!(sink.0.is_empty(), "{what}: nothing reaches the sink");
        let body = pattern(3, 700);
        assert_eq!(
            s.write(vol, name(b"DATA"), &mut |w| w.put(&body)),
            Ok(700),
            "{what}"
        );
        assert_eq!(read_back(&mut s, vol, name(b"DATA")), body, "{what}");
        assert_eq!(s.delete(vol, name(b"DATA")), Ok(()), "{what}");
        assert_eq!(log(&s).reinits.get(), 0, "{what}");
        consistent(&s);
        if what.contains("bad mark") {
            assert_eq!(l.entry(&disk.block(PART_LBA + l.fat_block(bad)), bad), 0);
        }
    }

    // `/CHIMERA` is cluster 2: a loop.
    let disk = Rc::new(copy(&base));
    put(
        &disk,
        start(&disk, b"CHIMERA    "),
        start(&disk, b"CHIMERA    "),
    );
    let mut s = probed(&disk);
    assert_eq!(
        s.list(vol, Dir::Chimera, &mut |_, _| {}),
        Err(StoreError::Corrupt)
    );
    assert_eq!(
        s.write(vol, name(b"NEW"), &mut |w| w.put(b"x")),
        Err(StoreError::Corrupt)
    );
    assert_eq!(*log(&s).writes.borrow(), []);

    // FAT32's root is a chain too.
    let disk = Rc::new(fat32(1));
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    put(&disk, 2, 0);
    assert_eq!(s.make_dir(vol, Dir::Chimera), Err(StoreError::Corrupt));
    assert_eq!(*log(&s).writes.borrow(), []);
}

/// A long-name slot of a run: its order byte and its short name's checksum.
fn lfn(order: u8, sum: u8) -> [u8; 32] {
    let mut r = raw(b"\0A\0B\0C\0D\0E\0", 0x0F);
    r[0] = order;
    r[13] = sum;
    r
}

/// `P0000001.A` in `/CHIMERA`'s second block, slot 1, after a 3-slot run
/// from the first block's last two slots.
#[test]
fn lfn_run_is_deleted_with_its_entry() {
    let file = FileName::new(Dir::Chimera, b"P0000001", b"A").unwrap();
    let sum = lfn_checksum(&ShortName::file(&file));
    for (what, run_sum, run_goes) in [("matching", sum, true), ("wrong", sum ^ 1, false)] {
        let (disk, vol) = card();
        let mut s = probed(&disk);
        // `.` and `..`, 12 files, then PAD1 and PAD2 fill the first block.
        for i in 2..14 {
            s.write(vol, name(format!("F{i}").as_bytes()), &mut |_| Ok(()))
                .unwrap();
        }
        for f in [b"PAD1", b"PAD2", b"PAD3"] {
            s.write(vol, name(f), &mut |_| Ok(())).unwrap();
        }
        s.write(vol, file, &mut |w| w.put(b"long")).unwrap();
        let slots = [
            entry(&disk, b"PAD1    BIN"),
            entry(&disk, b"PAD2    BIN"),
            entry(&disk, b"PAD3    BIN"),
        ];
        let e = entry(&disk, b"P0000001A  ");
        assert_eq!((slots[1].1, slots[2].1, e.1), (480, 0, 32));
        assert_ne!(slots[1].0, e.0, "the run crosses a block boundary");
        for f in [b"PAD1", b"PAD2", b"PAD3"] {
            s.delete(vol, name(f)).unwrap();
        }
        for (&(b, at), order) in slots.iter().zip([0x43, 0x02, 0x01]) {
            poke(&disk, b, at, &lfn(order, run_sum));
        }
        s.delete(vol, file).unwrap();
        assert_eq!(disk.block(e.0)[e.1], 0xE5, "{what}: the entry");
        for (&(b, at), order) in slots.iter().zip([0x43, 0x02, 0x01]) {
            let want = if run_goes { 0xE5 } else { order };
            assert_eq!(disk.block(b)[at], want, "{what}: the run");
        }
        consistent(&s);
    }
}

#[test]
fn wrong_kinds_are_refused() {
    let (disk, vol) = card();
    let l = layout_of(&disk);
    let dir = PART_LBA + l.cluster_block(start(&disk, b"CHIMERA    ")).unwrap();
    poke(&disk, dir, 64, &raw(b"SYSTEM  A  ", 0x10));
    let mut s = probed(&disk);
    let system = FileName::new(Dir::Chimera, b"SYSTEM", b"A").unwrap();
    assert_eq!(
        s.read(vol, system, &mut Sink::default()),
        Err(StoreError::NotFound)
    );
    assert_eq!(s.delete(vol, system), Err(StoreError::NotFound));
    assert_eq!(
        s.write(vol, system, &mut |w| w.put(b"x")),
        Err(StoreError::Corrupt)
    );
    assert_eq!(*log(&s).writes.borrow(), []);

    let disk = Rc::new(fat16(16_384, 1));
    poke(&disk, root_block(&disk), 32, &raw(b"CHIMERA    ", 0x20));
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    assert_eq!(s.make_dir(vol, Dir::Chimera), Err(StoreError::Corrupt));
    assert_eq!(s.make_dir(vol, Dir::Projects), Err(StoreError::Corrupt));
    assert_eq!(
        s.list(vol, Dir::Chimera, &mut |_, _| {}),
        Err(StoreError::Corrupt)
    );
    assert_eq!(*log(&s).writes.borrow(), []);
}

#[test]
fn fat16_root_full_is_full() {
    let disk = Rc::new(fat16(16_384, 1));
    let Root::Fixed { first, blocks } = layout_of(&disk).root() else {
        panic!("a FAT16 root")
    };
    for i in 1..blocks as usize * 16 {
        let mut n = *b"R0000000BIN";
        n[1..8].copy_from_slice(format!("{i:07}").as_bytes());
        poke(
            &disk,
            PART_LBA + first + (i / 16) as u32,
            i % 16 * 32,
            &raw(&n, 0x20),
        );
    }
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    assert_eq!(s.make_dir(vol, Dir::Chimera), Err(StoreError::Full));
    assert_eq!(*log(&s).writes.borrow(), []);
}

/// FAT 1's chains, as the card's directory tree names them: each one held
/// and ended, and no cluster in two (no cross-link, no loop). `None` if the
/// card doesn't mount.
fn sound(disk: &Overlay) -> Option<bool> {
    let p = first_partition(&disk.block(0)?).ok()?;
    let (l, _) = layout(&disk.block(p.lba)?, p).ok()?;
    Some(walk(disk, p.lba, &l).is_some())
}

/// `None` at the first unsound chain.
fn walk(disk: &Overlay, lba: u32, l: &Layout) -> Option<()> {
    let block = |b: u32| disk.block(lba.checked_add(b)?);
    let mut seen = HashSet::new();
    // A chain's blocks, its clusters marked seen.
    let mut chain = |start: u32| -> Option<Vec<u32>> {
        let (mut c, mut blocks) = (start, Vec::new());
        loop {
            if !seen.insert(c) {
                return None;
            }
            let first = l.cluster_block(c)?;
            blocks.extend(first..first + l.blocks_per_cluster());
            match l.link(&block(l.fat_block(c))?, c) {
                Link::End => return Some(blocks),
                Link::Next(next) => c = next,
                Link::Broken => return None,
            }
        }
    };
    let mut dirs = vec![match l.root() {
        Root::Fixed { first, blocks } => (first..first + blocks).collect(),
        Root::Cluster(c) => chain(c)?,
    }];
    while let Some(blocks) = dirs.pop() {
        'dir: for b in blocks {
            for raw in block(b)?.as_chunks::<32>().0 {
                match parse(raw, l.kind()) {
                    Slot::End => break 'dir,
                    Slot::File(e) if e.start != 0 => _ = chain(e.start)?,
                    Slot::Dir(e) => dirs.push(chain(e.start)?),
                    _ => {}
                }
            }
        }
    }
    Some(())
}

/// `seeds` seeds of 1-8 byte mutations in the MBR, the boot sector,
/// FSInfo, FAT 1, FAT 2, the FAT's last sector, the root and `/CHIMERA`,
/// then every operation, on an `Overlay` of `image`. Nothing panics or
/// loops, every write is in the partition, and a card whose chains were
/// sound (`sound`) still has sound chains after.
fn fuzz(what: &str, image: RamDisk, seeds: u64) {
    let base = Rc::new(image);
    let vol = {
        let mut s = probed(&base);
        let vol = s.mount().unwrap();
        s.make_dir(vol, Dir::Chimera).unwrap();
        s.write(vol, name(b"DATA"), &mut |w| w.put(&[0xA5; 3 * 512]))
            .unwrap();
        vol
    };
    let l = layout_of(&base);
    let (fat1, fat2) = fats(&l);
    let chimera = l.cluster_block(start(&base, b"CHIMERA    ")).unwrap();
    let mut targets = vec![
        0,
        PART_LBA,
        PART_LBA + fat1,
        PART_LBA + fat2,
        PART_LBA + l.fat_block(l.clusters() + 1),
        root_block(&base),
        PART_LBA + chimera,
    ];
    targets.extend(l.fs_info().map(|b| PART_LBA + b));
    let (mut wrote, mut sound_seeds) = (0, 0);
    for seed in 1..=seeds {
        let mut rng = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let disk = Rc::new(Overlay::new(&base));
        for _ in 0..1 + rng.below(8) {
            let blk = targets[rng.below(targets.len())];
            disk.poke(blk, rng.below(512), rng.next() as u8);
        }
        // The partition the operations can accept.
        let bounds = disk.block(0).and_then(|mbr| {
            let p = first_partition(&mbr).ok()?;
            let (l, _) = layout(&disk.block(p.lba)?, p).ok()?;
            Some(u64::from(p.lba)..u64::from(p.lba) + u64::from(l.blocks()))
        });
        let was_sound = sound(&disk);
        let mut s = probed(&disk);
        let run = catch_unwind(AssertUnwindSafe(|| {
            let vol = s.mount().unwrap_or(vol);
            let _ = s.list(vol, Dir::Chimera, &mut |_, _| {});
            let _ = s.read(vol, name(b"DATA"), &mut Sink::default());
            let _ = s.write(vol, name(b"NEW"), &mut |w| w.put(&[0x5A; 700]));
            let _ = s.delete(vol, name(b"DATA"));
            let _ = s.make_dir(vol, Dir::Projects);
            let _ = s.write(vol, name(b"DATA"), &mut |w| w.put(&[0x3C; 1_500]));
        }));
        assert!(run.is_ok(), "{what}: seed {seed} panicked");
        if was_sound == Some(true) {
            sound_seeds += 1;
            assert_eq!(
                sound(&disk),
                Some(true),
                "{what}: seed {seed} broke FAT 1's chains"
            );
        }
        let writes = log(&s).writes.borrow();
        for &(lba, _) in writes.iter() {
            let inside = bounds.as_ref().is_some_and(|r| r.contains(&u64::from(lba)));
            assert!(
                inside && lba >= PART_LBA,
                "{what}: seed {seed} wrote block {lba}, outside {bounds:?}"
            );
        }
        wrote += u64::from(!writes.is_empty());
    }
    assert!(
        wrote > seeds / 2,
        "{what}: {wrote} of {seeds} seeds still write"
    );
    assert!(
        sound_seeds > seeds * 3 / 4,
        "{what}: {sound_seeds} of {seeds} seeds start sound"
    );
}

#[test]
fn mutated_images_never_panic() {
    fuzz("FAT16", with_clusters(CLUSTERS, 7), 2_000);
    fuzz("FAT32", fat32(1), 500);
}

#[test]
fn error_calls_reinit() {
    let (disk, vol) = card();
    let cut = Rc::new(CutDisk::new(copy(&disk), Cut::After(0)));
    let mut s = probed(&cut);
    assert_eq!(
        s.write(vol, name(b"F"), &mut |w| w.put(b"x")),
        Err(StoreError::Io)
    );
    assert_eq!(log(&s).reinits.get(), 1);
    assert_eq!(s.mount(), Ok(vol));
}

/// Review Focus 4: a card swapped while idle fails the first read.
#[test]
fn mount_retries_once_after_reinit() {
    let (disk, vol) = card();
    let mut s = probed(&disk);
    log(&s).fail_reads.set(1);
    assert_eq!(s.mount(), Ok(vol));
    assert_eq!(log(&s).reinits.get(), 1);

    let mut s = probed(&disk);
    log(&s).fail_reads.set(2);
    assert_eq!(s.mount(), Err(StoreError::Io));
    assert_eq!(
        log(&s).reinits.get(),
        2,
        "the retry's own error re-inits too"
    );
    assert_eq!(log(&s).reads.get(), 2, "no third read");
}

/// Review Focus 2's torn truncate: an old length over a short chain.
#[test]
fn chain_errors_read_as_corrupt() {
    let (disk, vol) = card();
    let mut s = probed(&disk);
    s.write(vol, name(b"TORN"), &mut |w| w.put(&[3; 512]))
        .unwrap();
    let (b, at) = entry(&disk, b"TORN    BIN");
    disk.0.borrow_mut()[b as usize][at + 28..at + 32].copy_from_slice(&(3 * 512u32).to_le_bytes());
    assert_eq!(
        s.read(vol, name(b"TORN"), &mut Sink::default()),
        Err(StoreError::Corrupt)
    );
    assert_eq!(log(&s).reinits.get(), 0);
}
