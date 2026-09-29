//! The FAT core on a recording RAM partition: the allocator's bounds, the
//! FAT copies, bounded chain walks, the entry codec and FSInfo.

#[path = "common/image.rs"]
mod image;

use chimera_fat::blocks::FsError;
use chimera_fat::dir::{Entry, ShortName, Slot, encode, lfn_checksum, parse};
use chimera_fat::fat::{FatCache, Table};
use chimera_fat::fsinfo;
use chimera_fat::volume::{FsKind, Layout};
use chimera_hal::store::{Dir, FileName};
use image::{PART_LBA, RamDisk, Rec, fat16, fat32, layout_of, with_clusters};
use std::collections::HashSet;

/// Cluster `c`'s entry, set to `v` in every FAT on the disk.
fn put(disk: &RamDisk, l: &Layout, c: u32, v: u32) {
    let mut blocks = disk.0.borrow_mut();
    for copy in l.fat_copies(l.fat_block(c)) {
        l.put_entry(&mut blocks[(PART_LBA + copy) as usize], c, v);
    }
}

/// Cluster `c`'s entry in each FAT, FAT 1 first.
fn entries(disk: &RamDisk, l: &Layout, c: u32) -> Vec<u32> {
    l.fat_copies(l.fat_block(c))
        .map(|copy| l.entry(&disk.block(PART_LBA + copy), c))
        .collect()
}

/// Every FAT block of every copy.
fn fats(disk: &RamDisk, l: &Layout) -> Vec<[u8; 512]> {
    let (first, last) = (l.fat_block(0), l.fat_block(l.clusters() + 1));
    (first..=last)
        .flat_map(|b| l.fat_copies(b).collect::<Vec<_>>())
        .map(|b| disk.block(PART_LBA + b))
        .collect()
}

/// Marks every free cluster bad, except `keep`.
fn fill_except(disk: &RamDisk, l: &Layout, keep: &[u32]) {
    let bad = match l.kind() {
        FsKind::Fat16 => 0xFFF7,
        FsKind::Fat32 => 0x0FFF_FFF7,
    };
    for c in 2..l.clusters() + 2 {
        if !keep.contains(&c) && entries(disk, l, c)[0] == 0 {
            put(disk, l, c, bad);
        }
    }
}

fn entries_per_sector(l: &Layout) -> u32 {
    match l.kind() {
        FsKind::Fat16 => 256,
        FsKind::Fat32 => 128,
    }
}

/// C1: the scan stops at `count + 2`, even inside the FAT's last sector,
/// whose tail the builder leaves zero.
#[test]
fn alloc_never_passes_the_last_cluster() {
    for (clusters, kind) in [
        (4_085, FsKind::Fat16),
        (5_000, FsKind::Fat16),
        (65_524, FsKind::Fat16),
        (65_525, FsKind::Fat32),
        (66_000, FsKind::Fat32),
    ] {
        let disk = with_clusters(clusters, 1);
        let l = layout_of(&disk);
        assert_eq!((l.kind(), l.clusters()), (kind, clusters));
        assert_ne!(
            (clusters + 2) % entries_per_sector(&l),
            0,
            "{clusters}: the last FAT sector is partial"
        );
        let mut rec = Rec::new(&disk);
        let mut cache = FatCache::new();
        let mut t = Table::new(&mut rec, &l, &mut cache);
        let free = t.free_count().unwrap();
        let (mut got, mut from) = (HashSet::new(), 2);
        loop {
            match t.alloc(from, None) {
                Ok(c) => {
                    assert!((2..clusters + 2).contains(&c), "{clusters}: cluster {c}");
                    assert!(got.insert(c), "{clusters}: cluster {c} twice");
                    from = c + 1;
                }
                Err(FsError::Full) => break,
                Err(e) => panic!("{clusters}: {e:?}"),
            }
        }
        t.flush().unwrap();
        assert_eq!(
            got.len() as u32,
            free,
            "{clusters}: every free cluster, once"
        );
        assert!(
            rec.writes.iter().all(|&(b, _)| b < l.blocks()),
            "{clusters}: a write past the volume"
        );
    }
}

#[test]
fn tail_zeros_are_not_free() {
    let disk = with_clusters(5_000, 1);
    let l = layout_of(&disk);
    fill_except(&disk, &l, &[]);
    let last = l.fat_block(l.clusters() + 1);
    assert_eq!(
        l.entry(&disk.block(PART_LBA + last), 5_002),
        0,
        "a zero tail"
    );
    let mut rec = Rec::new(&disk);
    let mut cache = FatCache::new();
    let mut t = Table::new(&mut rec, &l, &mut cache);
    for from in [2, 4_990, 5_001] {
        assert_eq!(t.alloc(from, None), Err(FsError::Full), "from {from}");
    }
    t.flush().unwrap();
    assert_eq!(rec.writes, []);
}

#[test]
fn last_free_cluster_is_allocated() {
    for disk in [with_clusters(5_000, 1), fat32(1)] {
        let l = layout_of(&disk);
        let last = l.clusters() + 1;
        fill_except(&disk, &l, &[last]);
        let mut rec = Rec::new(&disk);
        let mut cache = FatCache::new();
        let mut t = Table::new(&mut rec, &l, &mut cache);
        assert_eq!(t.alloc(2, None), Ok(last));
        t.flush().unwrap();
        let (before, writes) = (fats(&disk, &l), rec.writes.len());

        let mut t = Table::new(&mut rec, &l, &mut cache);
        assert_eq!(t.alloc(2, None), Err(FsError::Full));
        t.flush().unwrap();
        assert_eq!(rec.writes.len(), writes, "Full writes nothing");
        assert_eq!(fats(&disk, &l), before, "nothing leaked");
        assert_eq!(entries(&disk, &l, last), [l.end_mark(); 2]);
    }
}

#[test]
fn hint_is_only_a_start() {
    let disk = with_clusters(5_000, 1);
    let l = layout_of(&disk);
    for c in [10, 5_001] {
        put(&disk, &l, c, 0xFFFF);
    }
    let mut rec = Rec::new(&disk);
    let mut cache = FatCache::new();
    let mut t = Table::new(&mut rec, &l, &mut cache);
    assert_eq!(t.alloc(10, None), Ok(11), "from a used cluster");
    assert_eq!(
        t.alloc(5_001, None),
        Ok(2),
        "from the used last cluster: wraps"
    );
    assert_eq!(t.alloc(u32::MAX, None), Ok(3), "from past the volume");
    assert_eq!(t.alloc(1, None), Ok(4), "from a reserved cluster");
}

#[test]
fn alloc_links_prev() {
    let disk = fat16(16_384, 1);
    let l = layout_of(&disk);
    let mut rec = Rec::new(&disk);
    let mut cache = FatCache::new();
    let mut t = Table::new(&mut rec, &l, &mut cache);
    let a = t.alloc(2, None).unwrap();
    let b = t.alloc(a, Some(a)).unwrap();
    assert_eq!(t.alloc(2, Some(1)), Err(FsError::Corrupt), "an unheld prev");
    t.flush().unwrap();
    assert_eq!(t.chain_len(a), Ok(2));
    assert_eq!(
        (entries(&disk, &l, a), entries(&disk, &l, b)),
        (vec![b; 2], vec![0xFFFF; 2])
    );
}

#[test]
fn fat_copies_are_written_together() {
    let disk = fat16(16_384, 1);
    let l = layout_of(&disk);
    let (s300, s3000) = (l.fat_block(300), l.fat_block(3_000));
    let copies: Vec<u32> = l.fat_copies(s300).collect();
    let mut rec = Rec::new(&disk);
    let mut cache = FatCache::new();
    let mut t = Table::new(&mut rec, &l, &mut cache);

    t.set(300, 0x1234).unwrap();
    t.flush().unwrap();
    t.set(300, 0x1234).unwrap();
    t.flush().unwrap();
    t.set(301, 0).unwrap();
    t.flush().unwrap();
    // Dirty, then evicted by a read of another sector.
    t.set(300, 5).unwrap();
    assert_eq!(t.link(3_000).map(|_| ()), Ok(()));
    t.set(3_000, 9).unwrap();
    t.flush().unwrap();

    let s3000_copies: Vec<u32> = l.fat_copies(s3000).collect();
    let want: Vec<(u32, bool)> = [&copies, &copies, &s3000_copies]
        .iter()
        .flat_map(|c| c.iter().map(|&b| (b, false)))
        .collect();
    assert_eq!(rec.writes, want, "FAT 1 then FAT 2, and only on a change");
    for s in [s300, s3000] {
        let [a, b] = [0, 1].map(|i| disk.block(PART_LBA + l.fat_copies(s).nth(i).unwrap()));
        assert_eq!(a, b, "FAT sector {s} in both copies");
    }
    assert_eq!(entries(&disk, &l, 300), [5, 5]);
}

/// Cluster 3 → 4 → `to` in both FATs.
fn chain(disk: &RamDisk, l: &Layout, to: u32) {
    put(disk, l, 3, 4);
    put(disk, l, 4, to);
}

#[test]
fn broken_chains_are_bounded() {
    for disk in [with_clusters(5_000, 1), fat32(1)] {
        let l = layout_of(&disk);
        let (count, bad) = match l.kind() {
            FsKind::Fat16 => (5_000, 0xFFF7),
            FsKind::Fat32 => (66_000, 0x0FFF_FFF7),
        };
        // (what, 4's entry, clusters freed from 3)
        let cases = [
            ("links to a free cluster", 0, 1),
            ("links to reserved cluster 1", 1, 2),
            ("links past the volume", count + 2, 2),
            ("links to the bad mark", bad, 2),
            ("2-cycle", 3, 2),
        ];
        for (what, to, freed) in cases {
            let disk = RamDisk(disk.0.clone());
            chain(&disk, &l, to);
            // A sentinel chain beside it stays.
            put(&disk, &l, 5, l.end_mark());
            let mut rec = Rec::new(&disk);
            let mut cache = FatCache::new();
            let mut t = Table::new(&mut rec, &l, &mut cache);
            assert_eq!(t.chain_len(3), Err(FsError::Corrupt), "{what}");
            assert_eq!(t.free_chain(3), Ok(freed), "{what}");
            t.flush().unwrap();
            assert!(rec.reads <= count, "{what}: {} FAT reads", rec.reads);
            assert_eq!(entries(&disk, &l, 3), [0, 0], "{what}");
            assert_eq!(entries(&disk, &l, 4), [0, 0], "{what}");
            assert_eq!(entries(&disk, &l, 5), [l.end_mark(); 2], "{what}");
        }

        let disk = RamDisk(disk.0.clone());
        put(&disk, &l, 3, 3);
        let mut rec = Rec::new(&disk);
        let mut cache = FatCache::new();
        let mut t = Table::new(&mut rec, &l, &mut cache);
        assert_eq!(t.chain_len(3), Err(FsError::Corrupt), "self-loop");
        assert_eq!(t.free_chain(3), Ok(1), "self-loop");
        for start in [0, 1, count + 2, u32::MAX] {
            assert_eq!(t.chain_len(start), Err(FsError::Corrupt), "start {start}");
            assert_eq!(t.free_chain(start), Ok(0), "start {start}");
        }
        t.flush().unwrap();
        assert!(rec.reads <= count);
        assert_eq!(entries(&disk, &l, 3), [0, 0], "self-loop");
    }
}

/// A loop the length of the volume is still a loop: the walk stops at
/// `count` steps, one FAT read per sector.
#[test]
fn a_loop_through_every_cluster_is_corrupt() {
    let disk = with_clusters(5_000, 1);
    let l = layout_of(&disk);
    for c in 2..5_002 {
        put(&disk, &l, c, if c == 5_001 { 2 } else { c + 1 });
    }
    let mut rec = Rec::new(&disk);
    let mut cache = FatCache::new();
    let mut t = Table::new(&mut rec, &l, &mut cache);
    assert_eq!(t.chain_len(2), Err(FsError::Corrupt));
    assert_eq!(t.free_chain(2), Ok(5_000));
    assert_eq!(t.free_count(), Ok(5_000));
    t.flush().unwrap();
    let sectors = 5_002u32.div_ceil(256);
    assert!(rec.reads <= 4 * sectors, "{} FAT reads", rec.reads);
}

fn file(stem: &[u8], ext: &[u8]) -> FileName {
    FileName::new(Dir::Projects, stem, ext).unwrap()
}

/// A raw entry: an 11-byte name and an attribute.
fn raw(name: &[u8; 11], attr: u8) -> [u8; 32] {
    let mut r = [0; 32];
    r[..11].copy_from_slice(name);
    r[11] = attr;
    r
}

#[test]
fn entry_codec_round_trips() {
    let name = ShortName::file(&file(b"SONG1", b"CHP"));
    let e = Entry {
        name,
        start: 0x0001_2345,
        len: 5_000,
    };
    let mut r = [0xFF; 32];
    encode(&e, false, FsKind::Fat32, &mut r);
    assert_eq!(&r[..11], b"SONG1   CHP");
    assert_eq!(r[11], 0x20);
    assert_eq!(u16::from_le_bytes([r[24], r[25]]), 0x5C21, "2026-01-01");
    assert_eq!(parse(&r, FsKind::Fat32), Slot::File(e));
    assert_eq!(name.to_file(Dir::Projects), Some(file(b"SONG1", b"CHP")));

    let e16 = Entry { start: 0x2345, ..e };
    encode(&e16, false, FsKind::Fat16, &mut r);
    assert_eq!(r[20..22], [0, 0], "FAT16 has no high half");
    assert_eq!(parse(&r, FsKind::Fat16), Slot::File(e16));
    r[20] = 1;
    assert_eq!(
        parse(&r, FsKind::Fat16),
        Slot::File(e16),
        "FAT16 ignores it"
    );

    let d = Entry {
        name: ShortName::dir(Dir::Projects),
        start: 7,
        len: 0,
    };
    encode(&d, true, FsKind::Fat16, &mut r);
    assert_eq!((&r[..11], r[11]), (&b"PROJECTS   "[..], 0x10));
    assert_eq!(parse(&r, FsKind::Fat16), Slot::Dir(d));
    encode(&Entry { len: 99, ..d }, true, FsKind::Fat16, &mut r);
    assert_eq!(
        parse(&r, FsKind::Fat16),
        Slot::Dir(d),
        "a directory's length is 0"
    );

    for (what, r, slot) in [
        ("LFN", raw(b"A\0B\0C\0D\0E\0\0", 0x0F), Slot::Lfn),
        (
            "LFN, reserved bits set",
            raw(b"A\0B\0C\0D\0E\0\0", 0xCF),
            Slot::Lfn,
        ),
        (
            "LFN bits and archive",
            raw(b"A\0B\0C\0D\0E\0\0", 0x2F),
            Slot::Other,
        ),
        ("label", raw(b"CHIMERA    ", 0x08), Slot::Other),
        ("dot", raw(b".          ", 0x10), Slot::Other),
        ("dot dot", raw(b"..         ", 0x10), Slot::Other),
        ("deleted", raw(b"\xE5ONG1   CHP", 0x20), Slot::Free),
        ("end", raw(&[0; 11], 0), Slot::End),
        ("lowercase", raw(b"song1   chp", 0x20), Slot::Other),
        ("tilde", raw(b"SONG~1  CHP", 0x20), Slot::Other),
        ("inner space", raw(b"SO NG   CHP", 0x20), Slot::Other),
        ("0x05 lead byte", raw(b"\x05ONG1   CHP", 0x20), Slot::Other),
    ] {
        assert_eq!(parse(&r, FsKind::Fat32), slot, "{what}");
    }
}

#[test]
fn lfn_checksum_known_answer() {
    let name = ShortName::file(&FileName::new(Dir::Chimera, b"README", b"TXT").unwrap());
    assert_eq!(lfn_checksum(&name), 0x73);
}

#[test]
fn fsinfo_is_checked_before_its_hint() {
    let disk = fat32(1);
    let l = layout_of(&disk);
    let mut b = disk.block(PART_LBA + l.fs_info().unwrap());
    assert!(fsinfo::valid(&b));
    assert_eq!(fsinfo::hint(&b, &l), None, "0xFFFF_FFFF: no hint");
    for (v, hint) in [
        (100, Some(100)),
        (66_001, Some(66_001)),
        (66_002, None),
        (1, None),
    ] {
        b[492..496].copy_from_slice(&u32::to_le_bytes(v));
        assert_eq!(fsinfo::hint(&b, &l), hint, "next-free {v}");
    }
    b[492..496].copy_from_slice(&100u32.to_le_bytes());
    for at in [0, 484, 508] {
        let mut bad = b;
        bad[at] ^= 1;
        assert!(!fsinfo::valid(&bad), "signature at {at}");
        assert_eq!(fsinfo::hint(&bad, &l), None, "signature at {at}");
    }
}
