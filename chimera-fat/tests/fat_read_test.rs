//! `Fs::list` and `Fs::read` on a recording RAM partition: files
//! `embedded-sdmmc` wrote read back, chains are checked before the sink sees
//! a byte, odd entries are skipped, and nothing is ever written.

#[path = "common/image.rs"]
mod image;

use chimera_fat::blocks::{BLOCK, FsError};
use chimera_fat::fat::FatCache;
use chimera_fat::fs::Fs;
use chimera_fat::volume::{Layout, Root, first_partition, layout};
use chimera_hal::store::{Dir, FileName, ReadSink};
use core::ops::ControlFlow;
use image::{
    DiskError, PART_LBA, RamDisk, Rec, fat16, fat32, layout_of, pattern, sdmmc_write, with_clusters,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// What a read delivered; it breaks after `stop_after` chunks, if set.
#[derive(Default)]
struct Sink {
    began: Option<u32>,
    bytes: Vec<u8>,
    chunks: usize,
    stop_after: Option<usize>,
}

impl ReadSink for Sink {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        self.began = Some(len);
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        assert!(bytes.len() <= BLOCK);
        self.bytes.extend_from_slice(bytes);
        self.chunks += 1;
        if self.stop_after == Some(self.chunks) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// The buffers `FatStore` will own, and an `Fs` over them.
struct Bufs {
    fat: FatCache,
    buf: [u8; BLOCK],
    hint: Option<u32>,
}

impl Bufs {
    fn new() -> Self {
        Self {
            fat: FatCache::new(),
            buf: [0; BLOCK],
            hint: None,
        }
    }

    fn fs<'a, 'd>(&'a mut self, rec: &'a mut Rec<'d>, l: Layout) -> Fs<'a, Rec<'d>> {
        Fs::new(rec, l, &mut self.fat, &mut self.buf, &mut self.hint)
    }
}

fn name(dir: Dir, stem: &str, ext: &str) -> FileName {
    FileName::new(dir, stem.as_bytes(), ext.as_bytes()).unwrap()
}

type Files = Vec<(FileName, Vec<u8>)>;

/// `disk` with `(dir, stem, ext, len)` files written by `embedded-sdmmc`.
fn written(disk: RamDisk, files: &[(Dir, &str, &str, usize)]) -> (RamDisk, Files) {
    let bodies: Files = files
        .iter()
        .enumerate()
        .map(|(i, &(dir, stem, ext, len))| (name(dir, stem, ext), pattern(i as u8, len)))
        .collect();
    let names: Vec<String> = files
        .iter()
        .map(|&(_, stem, ext, _)| format!("{stem}.{ext}"))
        .collect();
    let args: Vec<(Dir, &str, &[u8])> = files
        .iter()
        .zip(&names)
        .zip(&bodies)
        .map(|((&(dir, ..), n), (_, b))| (dir, n.as_str(), b.as_slice()))
        .collect();
    (sdmmc_write(disk, &args), bodies)
}

fn listed(fs: &mut Fs<'_, Rec<'_>>, dir: Dir) -> Result<Vec<(FileName, u32)>, FsError<DiskError>> {
    let mut got = Vec::new();
    fs.list(dir, &mut |n, len| got.push((n, len)))?;
    got.sort();
    Ok(got)
}

fn read(fs: &mut Fs<'_, Rec<'_>>, file: FileName) -> (Result<(), FsError<DiskError>>, Sink) {
    let mut sink = Sink::default();
    (fs.read(file, &mut sink), sink)
}

/// Sounds is never made, so it is a missing directory. `BIG`'s chain spans
/// FAT sectors, so reading it moves the FAT cache.
const POPULATED: [(Dir, &str, &str, usize); 6] = [
    (Dir::Chimera, "TOP", "BIN", 700),
    (Dir::Projects, "EMPTY", "CHP", 0),
    (Dir::Projects, "A", "CHP", 1),
    (Dir::Projects, "B", "CHP", 5_000),
    (Dir::Projects, "C", "CHP", 1_536),
    (Dir::Projects, "BIG", "CHP", 300 * BLOCK),
];

#[test]
fn read_only_ops_write_nothing() {
    for disk in [fat16(16_384, 1), fat32(1)] {
        let (disk, files) = written(disk, &POPULATED);
        let l = layout_of(&disk);
        let mut rec = Rec::new(&disk);
        let mut bufs = Bufs::new();
        let mut fs = bufs.fs(&mut rec, l);
        assert_eq!(listed(&mut fs, Dir::Chimera).unwrap().len(), 1);
        assert_eq!(listed(&mut fs, Dir::Projects).unwrap().len(), 5);
        assert_eq!(listed(&mut fs, Dir::Sounds), Err(FsError::NotFound));
        for (file, body) in &files {
            let (r, sink) = read(&mut fs, *file);
            assert_eq!((r, &sink.bytes), (Ok(()), body));
            let mut sink = Sink {
                stop_after: Some(1),
                ..Sink::default()
            };
            assert_eq!(fs.read(*file, &mut sink), Ok(()));
            assert_eq!(sink.bytes, body[..body.len().min(BLOCK)]);
        }
        let missing = name(Dir::Projects, "NONE", "CHP");
        assert_eq!(read(&mut fs, missing).0, Err(FsError::NotFound));
        let in_missing_dir = name(Dir::Sounds, "A", "CHP");
        assert_eq!(read(&mut fs, in_missing_dir).0, Err(FsError::NotFound));
        assert!(rec.reads > 0);
        assert_eq!(rec.writes, [], "{:?}", l.kind());
    }
}

#[test]
fn reads_what_embedded_sdmmc_wrote() {
    for disk in [fat16(16_384, 1), fat32(1)] {
        let cluster = layout_of(&disk).cluster_bytes() as usize;
        let sizes = [0, 1, 511, 512, 513, 5_000, 3 * cluster];
        let stems = ["F0", "F1", "F2", "F3", "F4", "F5", "F6"];
        let specs: Vec<_> = stems
            .iter()
            .zip(sizes)
            .map(|(&s, n)| (Dir::Projects, s, "CHP", n))
            .collect();
        let (disk, mut files) = written(disk, &specs);
        let l = layout_of(&disk);
        let mut rec = Rec::new(&disk);
        let mut bufs = Bufs::new();
        let mut fs = bufs.fs(&mut rec, l);
        files.sort();
        let want: Vec<_> = files.iter().map(|(n, b)| (*n, b.len() as u32)).collect();
        assert_eq!(listed(&mut fs, Dir::Projects).unwrap(), want);
        for (file, body) in &files {
            let (r, sink) = read(&mut fs, *file);
            assert_eq!(r, Ok(()));
            assert_eq!(sink.began, Some(body.len() as u32));
            assert!(sink.bytes == *body, "{file:?}");
        }
    }
}

/// The block and offset of the entry named `name83` (space padded, no dot).
fn entry(disk: &RamDisk, name83: &[u8; 11]) -> (usize, usize) {
    let blocks = disk.0.borrow();
    (PART_LBA as usize..blocks.len())
        .flat_map(|b| (0..512).step_by(32).map(move |at| (b, at)))
        .find(|&(b, at)| &blocks[b][at..at + 11] == name83)
        .expect("entry on the image")
}

/// `name83`'s start cluster.
fn start(disk: &RamDisk, name83: &[u8; 11]) -> u32 {
    let (b, at) = entry(disk, name83);
    let e = disk.block(b as u32);
    u32::from(u16::from_le_bytes([e[at + 26], e[at + 27]]))
        | u32::from(u16::from_le_bytes([e[at + 20], e[at + 21]])) << 16
}

/// Sets cluster `c`'s entry in every FAT.
fn put(disk: &RamDisk, l: &Layout, c: u32, v: u32) {
    let mut blocks = disk.0.borrow_mut();
    for copy in l.fat_copies(l.fat_block(c)) {
        l.put_entry(&mut blocks[(PART_LBA + copy) as usize], c, v);
    }
}

fn copy(disk: &RamDisk) -> RamDisk {
    RamDisk(disk.0.clone())
}

#[test]
fn chain_errors_read_as_corrupt_before_the_sink() {
    for base in [fat16(16_384, 1), fat32(1)] {
        let files = [
            (Dir::Projects, "SHORT", "CHP", 512),
            (Dir::Projects, "LONG", "CHP", 3 * 512),
        ];
        let (base, _) = written(base, &files);
        let l = layout_of(&base);
        let count = l.clusters();
        let bad = l.end_mark() - 8;
        let long = start(&base, b"LONG    CHP");
        let fat = base.block(PART_LBA + l.fat_block(long));
        assert_eq!(l.entry(&fat, long), long + 1, "a contiguous chain");
        let mut cases: Vec<(String, RamDisk)> = Vec::new();

        let disk = copy(&base);
        let (b, at) = entry(&disk, b"SHORT   CHP");
        disk.0.borrow_mut()[b][at + 28..at + 32].copy_from_slice(&(3 * 512u32).to_le_bytes());
        cases.push(("3 clusters over a 1-cluster chain".into(), disk));

        let disk = copy(&base);
        let (b, at) = entry(&disk, b"LONG    CHP");
        disk.0.borrow_mut()[b][at + 20..at + 22].fill(0);
        disk.0.borrow_mut()[b][at + 26..at + 28].fill(0);
        cases.push(("a length over no chain".into(), disk));

        for (what, to) in [
            ("a free cluster", 0),
            ("reserved cluster 1", 1),
            ("past the volume", count + 2),
            ("the bad mark", bad),
            ("the first cluster (a 2-cycle)", long),
            ("itself", long + 1),
        ] {
            let disk = copy(&base);
            put(&disk, &l, long + 1, to);
            cases.push((format!("the second cluster links to {what}"), disk));
        }

        for (what, disk) in cases {
            let mut rec = Rec::new(&disk);
            let mut bufs = Bufs::new();
            let mut fs = bufs.fs(&mut rec, l);
            let file = if what.starts_with('3') {
                "SHORT"
            } else {
                "LONG"
            };
            let (r, sink) = read(&mut fs, name(Dir::Projects, file, "CHP"));
            assert_eq!(r, Err(FsError::Corrupt), "{what}");
            assert_eq!(sink.began, None, "{what}: begin");
            assert!(rec.reads <= count, "{what}");
            assert_eq!(rec.writes, [], "{what}");
        }
    }
}

/// A raw entry: an 11-byte name, an attribute and a length.
fn raw(name: &[u8; 11], attr: u8) -> [u8; 32] {
    let mut r = [0; 32];
    r[..11].copy_from_slice(name);
    r[11] = attr;
    r
}

#[test]
fn lfn_labels_and_odd_names_are_skipped() {
    let (disk, _) = written(fat16(16_384, 1), &[(Dir::Projects, "KEEP", "CHP", 10)]);
    let (b, at) = entry(&disk, b"KEEP    CHP");
    let odd = [
        raw(b"Bs\0o\0n\0g\0\0\0", 0x0F),
        raw(b"PROJECTS   ", 0x08),
        raw(b"lower   chp", 0x20),
        raw(b"SONG~1  CHP", 0x20),
        raw(b"SUBDIR     ", 0x10),
        raw(b"\xE5ONE    CHP", 0x20),
        raw(b"LAST    CHP", 0x20),
    ];
    for (i, r) in odd.iter().enumerate() {
        let at = at + 32 * (i + 1);
        disk.0.borrow_mut()[b][at..at + 32].copy_from_slice(r);
    }
    let l = layout_of(&disk);
    let mut rec = Rec::new(&disk);
    let mut bufs = Bufs::new();
    let mut fs = bufs.fs(&mut rec, l);
    let keep = name(Dir::Projects, "KEEP", "CHP");
    let last = name(Dir::Projects, "LAST", "CHP");
    assert_eq!(
        listed(&mut fs, Dir::Projects),
        Ok(vec![(keep, 10), (last, 0)])
    );
    let subdir = name(Dir::Projects, "SUBDIR", "");
    assert_eq!(
        read(&mut fs, subdir).0,
        Err(FsError::NotFound),
        "a directory, not a file"
    );
    let (r, sink) = read(&mut fs, last);
    assert_eq!((r, sink.began, sink.bytes.len()), (Ok(()), Some(0), 0));
}

#[test]
fn a_file_on_the_path_is_corrupt() {
    let disk = fat16(16_384, 1);
    let l = layout_of(&disk);
    let mut rec = Rec::new(&disk);
    let mut bufs = Bufs::new();
    let mut fs = bufs.fs(&mut rec, l);
    assert_eq!(listed(&mut fs, Dir::Chimera), Err(FsError::NotFound));
    let root = PART_LBA as usize + 115;
    disk.0.borrow_mut()[root][..32].copy_from_slice(&raw(b"CHIMERA    ", 0x20));
    let mut fs = bufs.fs(&mut rec, l);
    assert_eq!(listed(&mut fs, Dir::Chimera), Err(FsError::Corrupt));
    assert_eq!(listed(&mut fs, Dir::Projects), Err(FsError::Corrupt));
    let f = name(Dir::Sounds, "A", "SND");
    assert_eq!(read(&mut fs, f).0, Err(FsError::Corrupt));
}

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Mutated boot sectors, FATs (the tail sector included) and directories:
/// no panic, no write, and no block outside the partition (`Rec` panics on
/// one).
#[test]
fn mutated_images_read_within_the_partition() {
    let files = [
        (Dir::Chimera, "TOP", "BIN", 700),
        (Dir::Projects, "A", "CHP", 1_536),
        (Dir::Sounds, "S", "SND", 3_000),
    ];
    let (disk, bodies) = written(with_clusters(4_100, 1), &files);
    let l = layout_of(&disk);
    let Root::Fixed { first: root, .. } = l.root() else {
        unreachable!("FAT16")
    };
    let targets = [
        PART_LBA,
        PART_LBA + l.fat_block(0),
        PART_LBA + l.fat_block(l.clusters() + 1),
        PART_LBA + root,
        entry(&disk, b"A       CHP").0 as u32,
        entry(&disk, b"S       SND").0 as u32,
        entry(&disk, b"TOP     BIN").0 as u32,
    ];
    let part = first_partition(&disk.block(0)).unwrap();
    // Seeds whose image still mounts and reads a file: the fuzz isn't vacuous.
    let mut read_ok = 0;
    for seed in 1..=2_000u64 {
        let mut rng = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut undo = Vec::new();
        for _ in 0..1 + rng.below(8) {
            let blk = targets[rng.below(targets.len())] as usize;
            let at = rng.below(512);
            let mut d = disk.0.borrow_mut();
            undo.push((blk, at, d[blk][at]));
            d[blk][at] = rng.next() as u8;
        }
        let run = catch_unwind(AssertUnwindSafe(|| {
            let Ok((l, _)) = layout(&disk.block(PART_LBA), part) else {
                return false;
            };
            let mut rec = Rec::new(&disk);
            let mut bufs = Bufs::new();
            let mut fs = bufs.fs(&mut rec, l);
            for dir in [Dir::Chimera, Dir::Projects, Dir::Sounds] {
                let _ = fs.list(dir, &mut |_, _| {});
            }
            let mut any = false;
            for (file, _) in &bodies {
                any |= read(&mut fs, *file).0.is_ok();
            }
            assert_eq!(rec.writes, []);
            any
        }));
        read_ok += usize::from(run.unwrap_or_else(|_| panic!("seed {seed}")));
        let mut d = disk.0.borrow_mut();
        for (blk, at, v) in undo.into_iter().rev() {
            d[blk][at] = v;
        }
    }
    assert!(read_ok > 1_000, "{read_ok} of 2 000 seeds read a file");
}
