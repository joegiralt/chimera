//! The FAT core on a recording RAM partition: the allocator's bounds, the
//! FAT copies, bounded chain walks, the entry codec and FSInfo.

#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;

use chimera_fat::blocks::FsError;
use chimera_fat::dir::{Attr, Entry, ShortName, Slot, encode, lfn_checksum, parse, read_only};
use chimera_fat::fat::{FatCache, Table};
use chimera_fat::fsinfo;
use chimera_fat::volume::{FsKind, Layout};
use chimera_hal::store::{Dir, FileName, Store, VolumeId};
use image::{PART_LBA, RamDisk, Rec, fat_check, fat16, fat32, layout_of, with_clusters};
use probe::{log, probed};
use std::collections::HashSet;
use std::rc::Rc;

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
        // (what, 4's entry, clusters freed from 3, 4's entry after). A bad
        // or reserved mark is the cluster's own state, not a link: it stays.
        let cases = [
            ("links to a free cluster", 0, 1, 0),
            ("links to reserved cluster 1", 1, 2, 0),
            ("links past the volume", count + 2, 2, 0),
            ("is the bad mark", bad, 1, bad),
            ("is a reserved mark", bad - 7, 1, bad - 7),
            ("2-cycle", 3, 2, 0),
        ];
        for (what, to, freed, after) in cases {
            let disk = RamDisk(disk.0.clone());
            chain(&disk, &l, to);
            // A sentinel chain beside it stays.
            put(&disk, &l, 5, l.end_mark());
            let mut rec = Rec::new(&disk);
            let mut cache = FatCache::new();
            let mut t = Table::new(&mut rec, &l, &mut cache);
            assert_eq!(t.chain_len(3), Err(FsError::Corrupt), "{what}");
            assert_eq!(t.free_chain(3).map(|f| f.n), Ok(freed), "{what}");
            t.flush().unwrap();
            assert_eq!(rec.reads, 1, "{what}: one FAT sector, cached");
            assert_eq!(entries(&disk, &l, 3), [0, 0], "{what}");
            assert_eq!(entries(&disk, &l, 4), [after; 2], "{what}");
            assert_eq!(entries(&disk, &l, 5), [l.end_mark(); 2], "{what}");
        }

        let disk = RamDisk(disk.0.clone());
        put(&disk, &l, 3, 3);
        let mut rec = Rec::new(&disk);
        let mut cache = FatCache::new();
        let mut t = Table::new(&mut rec, &l, &mut cache);
        assert_eq!(t.chain_len(3), Err(FsError::Corrupt), "self-loop");
        assert_eq!(t.free_chain(3).map(|f| f.n), Ok(1), "self-loop");
        for start in [0, 1, count + 2, u32::MAX] {
            assert_eq!(t.chain_len(start), Err(FsError::Corrupt), "start {start}");
            assert_eq!(t.free_chain(start).map(|f| f.n), Ok(0), "start {start}");
        }
        t.flush().unwrap();
        assert_eq!(rec.reads, 1, "unheld starts read nothing");
        assert_eq!(entries(&disk, &l, 3), [0, 0], "self-loop");
    }
}

/// A loop the length of the volume is still a loop: the walk stops at
/// `count` steps, one FAT read per sector per pass.
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
    assert_eq!(t.free_chain(2).map(|f| f.n), Ok(5_000));
    assert_eq!(t.free_count(), Ok(5_000));
    t.flush().unwrap();
    // chain_len, free_chain and free_count each pass over the 20 sectors.
    let sectors = 5_002u32.div_ceil(256);
    assert_eq!(rec.reads, 3 * sectors);
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
    encode(&e, Attr::Sealed, FsKind::Fat32, &mut r);
    assert_eq!(r[11], 0x21, "a sealed file is read-only");
    assert!(read_only(&r));
    assert_eq!(parse(&r, FsKind::Fat32), Slot::File(e));
    encode(&e, Attr::File, FsKind::Fat32, &mut r);
    assert!(!read_only(&r));
    assert_eq!(&r[..11], b"SONG1   CHP");
    assert_eq!(r[11], 0x20);
    assert_eq!(u16::from_le_bytes([r[24], r[25]]), 0x5C21, "2026-01-01");
    assert_eq!(parse(&r, FsKind::Fat32), Slot::File(e));
    assert_eq!(name.to_file(Dir::Projects), Some(file(b"SONG1", b"CHP")));

    let e16 = Entry { start: 0x2345, ..e };
    encode(&e16, Attr::File, FsKind::Fat16, &mut r);
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
    encode(&d, Attr::Dir, FsKind::Fat16, &mut r);
    assert_eq!((&r[..11], r[11]), (&b"PROJECTS   "[..], 0x10));
    assert_eq!(parse(&r, FsKind::Fat16), Slot::Dir(d));
    encode(&Entry { len: 99, ..d }, Attr::Dir, FsKind::Fat16, &mut r);
    assert_eq!(
        parse(&r, FsKind::Fat16),
        Slot::Dir(d),
        "a directory's length is 0"
    );

    for (what, r, slot) in [
        ("LFN", raw(b"A\0B\0C\0D\0E\0\0", 0x0F), Slot::Lfn(0)),
        (
            "LFN, reserved bits set",
            raw(b"A\0B\0C\0D\0E\0\0", 0xCF),
            Slot::Lfn(0),
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
        ("inner space", raw(b"SO NG   CHP", 0x20), Slot::Other),
        ("0x05 lead byte", raw(b"\x05ONG1   CHP", 0x20), Slot::Other),
    ] {
        assert_eq!(parse(&r, FsKind::Fat32), slot, "{what}");
    }
    // A host's short name: an entry, which only the root lists.
    assert!(matches!(
        parse(&raw(b"SONG~1  CHP", 0x20), FsKind::Fat32),
        Slot::File(_)
    ));
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

/// A failed FAT write leaves the cache empty, not dirty: the card's state
/// is unknown, so the next access reads it again.
#[test]
fn failed_flush_empties_the_cache() {
    let disk = fat16(16_384, 1);
    let l = layout_of(&disk);
    let mut rec = Rec::new(&disk);
    let mut cache = FatCache::new();
    let mut t = Table::new(&mut rec, &l, &mut cache);
    t.set(300, 7).unwrap();
    t.blocks().fail_writes = true;
    assert!(matches!(t.flush(), Err(FsError::Dev(_))));
    t.blocks().fail_writes = false;
    assert_eq!(t.flush(), Ok(()), "nothing left to write");
    let reads = t.blocks().reads;
    assert_eq!(t.link(300).map(|_| ()), Ok(()));
    assert_eq!(t.blocks().reads, reads + 1, "read again");
    let _ = Table::new(&mut rec, &l, &mut cache);
    assert_eq!(entries(&disk, &l, 300), [0, 0]);
}

/// A `Table` dropped with a change unflushed is a bug the next one reports.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "unflushed FAT change")]
fn a_dirty_cache_is_not_dropped() {
    let disk = fat16(16_384, 1);
    let l = layout_of(&disk);
    let mut rec = Rec::new(&disk);
    let mut cache = FatCache::new();
    let mut t = Table::new(&mut rec, &l, &mut cache);
    t.set(300, 7).unwrap();
    let _ = Table::new(&mut rec, &l, &mut cache);
}

/// Clusters 250..=255 chained in both FATs: the tail of FAT sector 0 on
/// FAT16, so the next cluster, 256, is in sector 1.
fn sector0_tail(disk: &RamDisk, l: &Layout) {
    for c in 250..255 {
        put(disk, l, c, c + 1);
    }
    put(disk, l, 255, l.end_mark());
}

/// After a fault: the next operation's `heal`, with the card answering
/// again, then nothing held by no entry, crossed or short, and both FATs
/// alike.
fn healed_clean(what: &str, disk: &RamDisk, l: &Layout, cache: &mut FatCache) {
    let mut rec = Rec::new(disk);
    let mut t = Table::new(&mut rec, l, cache);
    t.heal().unwrap_or_else(|e| panic!("{what}: heal {e:?}"));
    t.flush().unwrap();
    let r = fat_check(disk);
    assert_eq!(r.lost, [] as [u32; 0], "{what}: lost");
    assert!(
        r.cross_linked.is_empty() && r.short.is_empty() && r.fats_differ.is_empty(),
        "{what}: {r:?}"
    );
}

/// #362: an alloc whose new cluster is in the next FAT sector, refused or
/// landed-then-erred at each of its four FAT writes (both copies of both
/// sectors): `drop_chain` frees the chain and the stranded cluster.
#[test]
fn drop_chain_frees_the_stranded_cluster() {
    let base = fat16(16_384, 1);
    let l = layout_of(&base);
    sector0_tail(&base, &l);
    assert_ne!(l.fat_block(255), l.fat_block(256));
    let writes = {
        let disk = RamDisk(base.0.clone());
        let mut rec = Rec::new(&disk);
        let mut cache = FatCache::new();
        let mut t = Table::new(&mut rec, &l, &mut cache);
        assert_eq!(t.alloc(256, Some(255)), Ok(256));
        t.flush().unwrap();
        rec.writes.iter().map(|&(b, _)| b).collect::<Vec<_>>()
    };
    let (s0, s1): (Vec<u32>, Vec<u32>) = (
        l.fat_copies(l.fat_block(255)).collect(),
        l.fat_copies(l.fat_block(256)).collect(),
    );
    assert_eq!(
        writes,
        [s1, s0].concat(),
        "sector 1 lands first, then the link"
    );
    for (k, &block) in writes.iter().enumerate() {
        for land in [false, true] {
            let what = match land {
                false => format!("fail_write at write {k} (block {block})"),
                true => format!("land_then_fail at write {k} (block {block})"),
            };
            let disk = RamDisk(base.0.clone());
            let mut cache = FatCache::new();
            {
                let mut rec = Rec::new(&disk);
                match land {
                    false => rec.fail_write = Some(block),
                    true => rec.land_then_fail = Some(k),
                }
                let mut t = Table::new(&mut rec, &l, &mut cache);
                let r = t.alloc(256, Some(255)).and_then(|_| t.flush());
                assert!(r.is_err(), "{what}: the fault reached the alloc");
                // No entry holds the chain: dropped as a failed write drops it.
                let _ = t.drop_chain(250);
            }
            healed_clean(&what, &disk, &l, &mut cache);
        }
    }
}

/// The next alloc's free cluster is in the sector holding the pending
/// link, and that sector's flush fails at FAT 1: the pending pair is
/// still the one `drop_chain` sees, so cluster 256 is freed once the card
/// answers.
#[test]
fn pending_link_sector_flush_fails() {
    let disk = fat16(16_384, 1);
    let l = layout_of(&disk);
    sector0_tail(&disk, &l);
    let fat1_s0 = l.fat_block(255);
    let mut cache = FatCache::new();
    {
        let mut rec = Rec::new(&disk);
        let mut t = Table::new(&mut rec, &l, &mut cache);
        assert_eq!(t.alloc(256, Some(255)), Ok(256));
        t.blocks().fail_write = Some(fat1_s0);
        assert!(t.alloc(100, Some(256)).is_err(), "the pending link's flush");
        let _ = t.drop_chain(250);
    }
    healed_clean("pending link", &disk, &l, &mut cache);
}

fn chimera_file(stem: &str) -> FileName {
    FileName::new(Dir::Chimera, stem.as_bytes(), b"BIN").unwrap()
}

/// `/CHIMERA`'s chain, raw from FAT 1: every cluster linked or ended, and
/// the last ended.
fn chimera_chain(disk: &RamDisk) -> Vec<u32> {
    let l = layout_of(disk);
    let start = fat_check(disk)
        .entries
        .iter()
        .find(|e| e.name == *b"FILL1   BIN")
        .map(|_| ())
        .and(Some(2))
        .unwrap();
    let mut chain = vec![start];
    loop {
        let c = *chain.last().unwrap();
        let v = entries(disk, &l, c)[0];
        assert_ne!(v, 0, "/CHIMERA links to a free cluster: {chain:?}");
        if v >= 0xFFF8 {
            return chain;
        }
        assert!(chain.len() < 8, "/CHIMERA's chain runs on: {chain:?}");
        chain.push(v);
    }
}

/// `/CHIMERA` full in cluster 2, a file to cluster 255: the next new file
/// grows the directory into cluster 256, across a FAT sector, then the
/// replace fails at each later write. The directory keeps its chain and
/// entries, nothing is cross-linked, and nothing it holds is freed.
#[test]
fn grow_then_failed_replace_keeps_the_directory() {
    let base = Rc::new(with_clusters(4_200, 7));
    let vol = {
        let mut s = probed(&base);
        let vol = s.mount().unwrap();
        s.make_dir(vol, Dir::Chimera).unwrap();
        s.write(vol, chimera_file("FILL1"), &mut |w| w.put(&[7; 253 * 512]))
            .unwrap();
        for i in 1..=13 {
            s.write(vol, chimera_file(&format!("F{i:02}")), &mut |_| Ok(()))
                .unwrap();
        }
        vol
    };
    let before = fat_check(&base);
    assert_eq!(
        before.entry(b"FILL1   BIN").unwrap().chain.last(),
        Some(&255)
    );
    assert_eq!(chimera_chain(&base), [2]);
    let body = [9u8; 3 * 512];
    let replace = |s: &mut probe::Probed<RamDisk>, vol: VolumeId| {
        s.write(vol, chimera_file("NEWF"), &mut |w| w.put(&body))
    };
    let writes: Vec<u32> = {
        let disk = Rc::new(RamDisk(base.0.clone()));
        let mut s = probed(&disk);
        replace(&mut s, vol).unwrap();
        assert_eq!(chimera_chain(&disk), [2, 256], "the directory grew");
        log(&s).writes.borrow().iter().map(|&(b, _)| b).collect()
    };
    for (k, &block) in writes.iter().enumerate() {
        for land in [false, true] {
            let what = match land {
                false => format!("fail_write at write {k} (block {block})"),
                true => format!("land_then_fail at write {k} (block {block})"),
            };
            let disk = Rc::new(RamDisk(base.0.clone()));
            let mut s = probed(&disk);
            match land {
                false => log(&s).fail_write.set(Some(block)),
                true => log(&s).land_then_fail.set(Some(k)),
            }
            let _ = replace(&mut s, vol);
            log(&s).fail_write.set(None);
            log(&s).land_then_fail.set(None);
            // The next change heals what the fault left.
            let vol = s.mount().unwrap();
            s.write(vol, chimera_file("F01"), &mut |_| Ok(())).unwrap();
            let chain = chimera_chain(&disk);
            assert!(chain == [2] || chain == [2, 256], "{what}: {chain:?}");
            let r = fat_check(&disk);
            assert!(
                r.cross_linked.is_empty() && r.short.is_empty(),
                "{what}: {r:?}"
            );
            for e in &before.entries {
                assert_eq!(r.entry(&e.name), Some(e), "{what}: an entry changed");
            }
        }
    }
}
