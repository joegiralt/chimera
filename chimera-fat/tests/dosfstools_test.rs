//! dosfstools, a second implementation: `mkfs.fat` images must parse and
//! read as it laid them out, and `fsck.fat -n` must pass every image our
//! layer wrote. Ignored by a plain `cargo test`; `just test-fat-tools` (in
//! `just test` and `just check`) runs them, and fails without dosfstools.

#[path = "common/ab.rs"]
mod ab;
#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;
#[path = "common/tools.rs"]
mod tools;

use chimera_fat::blocks::BLOCK;
use chimera_fat::fat::FatCache;
use chimera_fat::fs::Fs;
use chimera_fat::volume::{FsKind, first_partition, layout};
use chimera_hal::store::{Dir, FileName, Store, StoreError, VolumeId};
use core::cell::RefCell;
use embedded_sdmmc::{Mode, VolumeIdx};
use image::{
    PART_LBA, RamDisk, Rec, fat16, fat32, layout_of, pattern, sdmmc, sdmmc_write, with_clusters,
};
use probe::{Probed, Sink, XorShift, fats, free, log, probed, suite};
use std::collections::BTreeMap;
use std::rc::Rc;
use tools::{fsck, mkfs};

/// (kind, disk blocks, sectors per cluster, serial)
const IMAGES: [(FsKind, u32, u8, u32); 4] = [
    (FsKind::Fat16, 30_000, 1, 0x1600_0001),
    (FsKind::Fat16, 40_000, 4, 0x1600_0004),
    (FsKind::Fat32, 70_000, 1, 0x3200_0001),
    (FsKind::Fat32, 90_000, 1, 0x3200_0002),
];

#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn mkfs_layouts_match() {
    for (kind, blocks, spc, serial) in IMAGES {
        let what = format!("{kind:?} {blocks} blocks -s {spc}");
        let disk = mkfs(kind, blocks, spc, serial);
        let p = first_partition(&disk.block(0)).unwrap();
        let (l, id) = layout(&disk.block(PART_LBA), p).unwrap();
        assert_eq!(l.kind(), kind, "{what}");
        assert_eq!(l.blocks_per_cluster(), u32::from(spc), "{what}");
        let per_sector = match kind {
            FsKind::Fat16 => 256,
            FsKind::Fat32 => 128,
        };
        assert_ne!(
            (l.clusters() + 2) % per_sector,
            0,
            "{what}: a partial last FAT sector"
        );
        let f = fsck(&disk);
        assert_eq!(f.code, 0, "{what}: {}", f.out);
        assert_eq!(l.clusters(), f.total, "{what}");
        assert_eq!((id.serial, &id.label), (serial, b"CHIMERA    "), "{what}");
    }
}

#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn mkfs_images_read_back() {
    for (kind, blocks, spc, serial) in IMAGES {
        let what = format!("{kind:?} {blocks} blocks -s {spc}");
        let disk = mkfs(kind, blocks, spc, serial);
        let cluster = usize::from(spc) * BLOCK;
        let specs = [
            (Dir::Projects, "F0", "CHP", 0),
            (Dir::Projects, "F1", "CHP", 1),
            (Dir::Projects, "F2", "CHP", 511),
            (Dir::Projects, "F3", "CHP", 512),
            (Dir::Projects, "F4", "CHP", 513),
            (Dir::Sounds, "F5", "SND", 5_000),
            (Dir::Chimera, "F6", "BIN", 3 * cluster),
        ];
        let names: Vec<String> = specs
            .iter()
            .map(|(_, s, e, _)| format!("{s}.{e}"))
            .collect();
        let bodies: Vec<Vec<u8>> = specs
            .iter()
            .enumerate()
            .map(|(i, &(.., n))| pattern(i as u8, n))
            .collect();
        let files: Vec<(Dir, &str, &[u8])> = specs
            .iter()
            .zip(&names)
            .zip(&bodies)
            .map(|((&(dir, ..), n), b)| (dir, n.as_str(), b.as_slice()))
            .collect();
        let disk = sdmmc_write(disk, &files);

        let p = first_partition(&disk.block(0)).unwrap();
        let (l, _) = layout(&disk.block(PART_LBA), p).unwrap();
        let mut rec = Rec::new(&disk);
        let (mut fat, mut buf, mut hint) = (FatCache::new(), [0; BLOCK], None);
        let mut fs = Fs::new(&mut rec, l, &mut fat, &mut buf, &mut hint);
        for dir in [Dir::Chimera, Dir::Projects, Dir::Sounds] {
            let mut got = Vec::new();
            fs.list(dir, &mut |n, len| got.push((n, len))).unwrap();
            got.sort();
            let mut want = Vec::new();
            for (&(d, stem, ext, _), body) in specs.iter().zip(&bodies).filter(|(s, _)| s.0 == dir)
            {
                let n = FileName::new(d, stem.as_bytes(), ext.as_bytes()).unwrap();
                want.push((n, body.len() as u32));
                let mut sink = Sink::default();
                fs.read(n, &mut sink).unwrap();
                assert!(sink.0 == *body, "{what}: {stem}.{ext}");
            }
            want.sort();
            assert_eq!(got, want, "{what}: {dir:?}");
        }
        assert_eq!(rec.writes, [], "{what}");
        let f = fsck(&disk);
        assert_eq!(f.code, 0, "{what}: {}", f.out);
        assert!(f.used > 0, "{what}: fsck counts the files");
    }
}

/// The block and offset of the first entry named `name83`.
fn entry(disk: &RamDisk, name83: &[u8; 11]) -> (usize, usize) {
    let blocks = disk.0.borrow();
    (PART_LBA as usize..blocks.len())
        .flat_map(|b| (0..512).step_by(32).map(move |at| (b, at)))
        .find(|&(b, at)| &blocks[b][at..at + 11] == name83)
        .expect("entry on the image")
}

/// `fsck.fat` really runs: a cross-link fails it.
#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn fsck_harness_catches_damage() {
    let body = pattern(1, 3 * BLOCK);
    let files = [
        (Dir::Projects, "A.CHP", &body[..]),
        (Dir::Projects, "B.CHP", &body[..]),
    ];
    let disk = sdmmc_write(mkfs(FsKind::Fat16, 30_000, 1, 1), &files);
    assert_eq!(fsck(&disk).code, 0, "clean before");
    let (a, at_a) = entry(&disk, b"A       CHP");
    let (b, at_b) = entry(&disk, b"B       CHP");
    let start_a: [u8; 2] = disk.block(a as u32)[at_a + 26..at_a + 28]
        .try_into()
        .unwrap();
    disk.0.borrow_mut()[b][at_b + 26..at_b + 28].copy_from_slice(&start_a);
    let f = fsck(&disk);
    assert_ne!(f.code, 0, "a cross-link: {}", f.out);
}

/// The RAM-image builders every other test starts from are FAT a computer
/// accepts, at each classification edge.
#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn builder_images_pass_fsck() {
    let images = [
        ("fat16(16 384)", fat16(16_384, 1)),
        ("fat32", fat32(1)),
        ("with_clusters(4 085)", with_clusters(4_085, 1)),
        ("with_clusters(65 524)", with_clusters(65_524, 1)),
        ("with_clusters(65 525)", with_clusters(65_525, 1)),
    ];
    for (what, disk) in images {
        let f = fsck(&disk);
        assert_eq!(f.code, 0, "{what}: {}", f.out);
    }
}

fn copy(disk: &RamDisk) -> RamDisk {
    RamDisk(RefCell::new(disk.0.borrow().clone()))
}

/// Every file on the card, with its bytes.
type Model = BTreeMap<FileName, Vec<u8>>;

/// `fsck.fat -n` passes `disk`, counts the clusters in use as we do, and
/// `embedded-sdmmc` reads every file in `model` as it is there.
fn verify(what: &str, disk: &RamDisk, model: &Model) {
    let f = fsck(disk);
    assert_eq!(f.code, 0, "{what}: {}", f.out);
    let l = layout_of(disk);
    assert_eq!(
        f.used,
        l.clusters() - free(disk).0,
        "{what}: clusters in use"
    );
    sdmmc(copy(disk), |vm| {
        let vol = vm.open_volume(VolumeIdx(0)).unwrap();
        let root = vol.open_root_dir().unwrap();
        for (file, body) in model {
            let mut d = root.open_dir("CHIMERA").unwrap();
            match file.dir() {
                Dir::Chimera => {}
                Dir::Projects => d.change_dir("PROJECTS").unwrap(),
                Dir::Sounds => d.change_dir("SOUNDS").unwrap(),
            }
            let stem = String::from_utf8_lossy(file.stem());
            let ext = String::from_utf8_lossy(file.ext());
            let f = d
                .open_file_in_dir(format!("{stem}.{ext}").as_str(), Mode::ReadOnly)
                .unwrap();
            let (mut got, mut buf) = (Vec::new(), [0; 512]);
            loop {
                let n = f.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
            }
            assert!(got == *body, "{what}: {stem}.{ext}");
        }
    });
}

fn read_back(s: &mut Probed<RamDisk>, vol: VolumeId, f: FileName) -> Vec<u8> {
    let mut sink = Sink::default();
    s.read(vol, f, &mut sink).unwrap();
    sink.0
}

/// A card's life on an mkfs image, checked after each phase: make the
/// directories, write 40 files, overwrite each at another size, delete
/// every third, fill the card, delete two, write again.
fn workload(what: &str, disk: RamDisk, seed: u64) {
    let disk = Rc::new(disk);
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    let mut rng = XorShift(seed);
    let mut model = Model::new();
    let dirs = [Dir::Chimera, Dir::Projects, Dir::Sounds];
    let put = |s: &mut Probed<RamDisk>, model: &mut Model, f: FileName, body: Vec<u8>| {
        assert_eq!(
            s.write(vol, f, &mut |w| w.put(&body)),
            Ok(body.len() as u32),
            "{what}"
        );
        model.insert(f, body);
    };

    for d in dirs {
        s.make_dir(vol, d).unwrap();
    }
    verify(&format!("{what}, directories"), &disk, &model);

    let files: Vec<FileName> = (0..40)
        .map(|i| FileName::new(dirs[i % 3], format!("F{i}").as_bytes(), b"DAT").unwrap())
        .collect();
    for (i, &f) in files.iter().enumerate() {
        put(&mut s, &mut model, f, pattern(i as u8, rng.below(20_001)));
    }
    verify(&format!("{what}, 40 files"), &disk, &model);

    for (i, &f) in files.iter().enumerate() {
        put(
            &mut s,
            &mut model,
            f,
            pattern(!(i as u8), rng.below(20_001)),
        );
    }
    verify(&format!("{what}, overwritten"), &disk, &model);

    for &f in files.iter().step_by(3) {
        s.delete(vol, f).unwrap();
        model.remove(&f);
    }
    verify(&format!("{what}, every third deleted"), &disk, &model);

    for i in 0.. {
        let f = FileName::new(Dir::Sounds, format!("FULL{i}").as_bytes(), b"DAT").unwrap();
        let body = pattern(i as u8, 1 << 20);
        match s.write(vol, f, &mut |w| w.put(&body)) {
            Ok(n) => assert_eq!(n, 1 << 20, "{what}"),
            Err(StoreError::Full) => {
                let got = read_back(&mut s, vol, f);
                assert!(body.starts_with(&got), "{what}: the partial file");
                model.insert(f, got);
                break;
            }
            Err(e) => panic!("{what}: {e:?}"),
        }
        model.insert(f, body);
    }
    assert_eq!(free(&disk), (0, 0), "{what}: full");
    verify(&format!("{what}, full"), &disk, &model);

    let full0 = FileName::new(Dir::Sounds, b"FULL0", b"DAT").unwrap();
    for f in [files[1], full0] {
        s.delete(vol, f).unwrap();
        model.remove(&f);
    }
    verify(&format!("{what}, two deleted"), &disk, &model);

    for (i, stem) in [b"AGAIN1", b"AGAIN2"].into_iter().enumerate() {
        let f = FileName::new(Dir::Projects, stem, b"DAT").unwrap();
        put(&mut s, &mut model, f, pattern(i as u8, rng.below(20_001)));
    }
    verify(&format!("{what}, written again"), &disk, &model);
}

#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn mkfs_images_pass_suite_and_fsck() {
    for (kind, blocks, spc, serial) in IMAGES {
        let what = format!("{kind:?} {blocks} blocks -s {spc}");
        for (disk, _) in suite(|n| mkfs(kind, blocks, spc, serial + n)) {
            let f = fsck(&disk);
            assert_eq!(f.code, 0, "{what}, after the suite: {}", f.out);
        }
        workload(&what, mkfs(kind, blocks, spc, serial), u64::from(serial));
    }
}

#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn builder_images_pass_fsck_after_the_suite() {
    for (what, image) in [
        (
            "fat16(16 384)",
            (|n| fat16(16_384, n)) as fn(u32) -> RamDisk,
        ),
        ("fat32", fat32),
    ] {
        for (i, (disk, _)) in suite(image).iter().enumerate() {
            let f = fsck(disk);
            assert_eq!(f.code, 0, "{what}, card {i}: {}", f.out);
            let l = layout_of(disk);
            assert_eq!(f.used, l.clusters() - free(disk).0, "{what}, card {i}");
        }
    }
}

/// The cut `fat_store_test` makes, FAT 1 written and FAT 2's write failed,
/// seen by `fsck.fat`: it reports the FATs differ, and after one following
/// change, one that touches no FAT entry of its own, it passes.
#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn a_failed_fat2_write_heals_for_fsck() {
    let disk = Rc::new(mkfs(FsKind::Fat16, 30_000, 1, 1));
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    let data = FileName::new(Dir::Chimera, b"DATA", b"BIN").unwrap();
    s.write(vol, data, &mut |w| w.put(&[1; 3 * BLOCK])).unwrap();
    let empty = FileName::new(Dir::Chimera, b"EMPTY", b"BIN").unwrap();
    s.write(vol, empty, &mut |_| Ok(())).unwrap();
    let (_, fat2) = fats(&layout_of(&disk));
    log(&s).fail_write.set(Some(PART_LBA + fat2));
    assert_eq!(
        s.write(vol, data, &mut |w| w.put(&[2; 2 * BLOCK])),
        Err(StoreError::Io)
    );
    log(&s).fail_write.set(None);
    let f = fsck(&disk);
    assert_ne!(f.code, 0, "the FATs differ: {}", f.out);
    s.delete(vol, empty).unwrap();
    let f = fsck(&disk);
    assert_eq!(f.code, 0, "healed: {}", f.out);
}

/// Findings `fsck.fat -n` may make on an image a power cut left: lost
/// clusters, and FATs that differ where the cut hit FAT 2. Never a
/// cross-link, a bad chain or a wrong size.
fn allowed_finding(line: &str) -> bool {
    let line = line.trim();
    line.is_empty()
        || line.starts_with("fsck.fat ")
        || (line.contains(" files, ") && line.ends_with(" clusters"))
        || (line.starts_with("Reclaimed ") && line.contains(" unused cluster"))
        || line == "FATs differ but appear to be intact."
        || line == "Using first FAT."
        || line == "Leaving filesystem unchanged."
}

/// The power-cut claim checked by a second implementation: every image a
/// cut in saves 2 (creates B), 3 (truncates A) and 4 (truncates B) leaves,
/// and the image after the next save, has only lost clusters or a stale
/// FAT 2 block for `fsck.fat -n` to find.
#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn cut_images_pass_fsck() {
    use image::Cut;
    let base = ab::slot(mkfs(FsKind::Fat16, 30_000, 1, 0x1600_00AB));
    ab::make_dir(&base);
    ab::save(&base, 1).0.unwrap();
    let (mut clean, mut findings) = (0, 0);
    for n in 2..=4 {
        let f = fsck(&base.inner);
        assert_eq!(f.code, 0, "before save {n}: {}", f.out);
        let before = copy(&base.inner);
        let dry = ab::slot(copy(&before));
        ab::save(&dry, n).0.unwrap();
        for k in 0..dry.writes.get() {
            let what = format!("save {n} cut at write {k}");
            let cut = ab::slot(copy(&before));
            cut.cut.set(Cut::After(k));
            assert!(ab::save(&cut, n).0.is_err(), "{what}");
            cut.cut.set(Cut::Never);
            for when in ["after the cut", "after the next save"] {
                let f = fsck(&cut.inner);
                let bad: Vec<&str> = f.out.lines().filter(|l| !allowed_finding(l)).collect();
                assert!(bad.is_empty(), "{what}, {when}: {bad:?}\n{}", f.out);
                if f.code == 0 {
                    clean += 1;
                } else {
                    findings += 1;
                }
                if when == "after the cut" {
                    let next = 100 * n + k;
                    ab::save(&cut, next).0.unwrap();
                    assert!(ab::load(&cut).unwrap().bits_eq(&ab::sound(next)), "{what}");
                }
            }
        }
        ab::save(&base, n).0.unwrap();
    }
    println!("cut images: {clean} clean, {findings} with lost clusters or a stale FAT 2");
}
