//! dosfstools, a second implementation: `mkfs.fat` images must parse and
//! read as it laid them out, and `fsck.fat -n` must pass them. Ignored by a
//! plain `cargo test`; `just test-fat-tools` (in `just test` and `just
//! check`) runs them, and fails without dosfstools.

#[path = "common/image.rs"]
mod image;
#[path = "common/tools.rs"]
mod tools;

use chimera_fat::blocks::BLOCK;
use chimera_fat::fat::FatCache;
use chimera_fat::fs::Fs;
use chimera_fat::volume::{FsKind, first_partition, layout};
use chimera_hal::store::{Dir, FileName, ReadSink};
use core::ops::ControlFlow;
use image::{PART_LBA, RamDisk, Rec, pattern, sdmmc_write};
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

/// `sdmmc_write` on an mkfs image. `embedded-sdmmc` 0.10 matches the
/// volume label entry `CHIMERA` as a file of that name, so the label's
/// first byte is changed while it writes, then put back.
fn sdmmc_write_labelled(disk: RamDisk, files: &[(Dir, &str, &[u8])]) -> RamDisk {
    let (b, at) = entry(&disk, b"CHIMERA    ");
    assert_eq!(disk.block(b as u32)[at + 11], 0x08, "the label entry");
    disk.0.borrow_mut()[b][at] = b'X';
    let disk = sdmmc_write(disk, files);
    disk.0.borrow_mut()[b][at] = b'C';
    disk
}

#[derive(Default)]
struct Sink(Vec<u8>);

impl ReadSink for Sink {
    fn begin(&mut self, _len: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        self.0.extend_from_slice(bytes);
        ControlFlow::Continue(())
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
        let disk = sdmmc_write_labelled(disk, &files);

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
    let disk = sdmmc_write_labelled(mkfs(FsKind::Fat16, 30_000, 1, 1), &files);
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
