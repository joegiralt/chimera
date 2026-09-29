#[path = "common/image.rs"]
mod image;

use chimera_fat::{FatStore, FixedTime, Medium};
use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError, Unsupported, VolumeId};
use chimera_hal::testkit::store_suite;
use core::cell::{Cell, RefCell};
use core::ops::ControlFlow;
use embedded_sdmmc::{
    Block, BlockCount, BlockDevice, BlockIdx, Error, Mode, VolumeIdx, VolumeManager,
};
use image::{
    CutDisk, DiskError, PART_LBA, RamDisk, exfat, exfat_superfloppy, fat16, fat32, superfloppy,
    with_clusters,
};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

/// What a `Probe` saw, and whether its card is in the slot.
struct Log {
    present: Cell<bool>,
    /// Fail this many reads, first to last.
    fail_reads: Cell<u32>,
    reads: Cell<u32>,
    reinits: Cell<u32>,
}

impl Log {
    fn new() -> Rc<Log> {
        Rc::new(Log {
            present: Cell::new(true),
            fail_reads: Cell::new(0),
            reads: Cell::new(0),
            reinits: Cell::new(0),
        })
    }
}

/// A card in a slot the test can empty, sharing its image with the test.
struct Probe<D> {
    disk: Rc<D>,
    log: Rc<Log>,
}

impl<D: Medium<Error = DiskError>> BlockDevice for Probe<D> {
    type Error = DiskError;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), DiskError> {
        let log = &self.log;
        log.reads.set(log.reads.get() + 1);
        if !log.present.get() {
            return Err(DiskError);
        }
        if log.fail_reads.get() > 0 {
            log.fail_reads.set(log.fail_reads.get() - 1);
            return Err(DiskError);
        }
        self.disk.read(blocks, start)
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), DiskError> {
        if !self.log.present.get() {
            return Err(DiskError);
        }
        self.disk.write(blocks, start)
    }

    fn num_blocks(&self) -> Result<BlockCount, DiskError> {
        self.disk.num_blocks()
    }
}

impl<D: Medium<Error = DiskError>> Medium for Probe<D> {
    fn reinit(&self) {
        self.log.reinits.set(self.log.reinits.get() + 1);
    }

    fn classify(&self, e: &DiskError) -> StoreError {
        if self.log.present.get() {
            self.disk.classify(e)
        } else {
            StoreError::NoCard
        }
    }

    fn fault(&self) -> Option<StoreError> {
        self.disk.fault()
    }
}

type Probed<D> = FatStore<Probe<D>, FixedTime>;

fn probed<D: Medium<Error = DiskError>>(disk: &Rc<D>) -> (Probed<D>, Rc<Log>) {
    let log = Log::new();
    let probe = Probe {
        disk: disk.clone(),
        log: log.clone(),
    };
    (FatStore::new(probe, FixedTime), log)
}

fn suite(image: fn(u32) -> RamDisk) {
    let log = Log::new();
    let slot = |serial| {
        let probe = Probe {
            disk: Rc::new(image(serial)),
            log: log.clone(),
        };
        FatStore::new(probe, FixedTime)
    };
    store_suite(
        &mut || {
            log.present.set(true);
            slot(1)
        },
        &mut |s| *s = slot(2),
        &mut |_| log.present.set(false),
        &mut |s| assert!(!s.has_open_handles()),
    );
}

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
    let mut s = FatStore::new(exfat(), FixedTime);
    assert_eq!(s.mount(), Err(StoreError::Unsupported(Unsupported::Exfat)));
}

#[test]
fn exfat_superfloppy_mount_is_unsupported() {
    let mut s = FatStore::new(exfat_superfloppy(), FixedTime);
    assert_eq!(s.mount(), Err(StoreError::Unsupported(Unsupported::Exfat)));
}

#[test]
fn superfloppy_mount_is_unsupported() {
    let mut s = FatStore::new(superfloppy(), FixedTime);
    assert_eq!(
        s.mount(),
        Err(StoreError::Unsupported(Unsupported::NoPartitionTable))
    );
}

/// The library divides by sectors per cluster: reaching `open_raw_volume`
/// with 0 would panic.
#[test]
fn corrupt_bpb_mount_is_unsupported() {
    let disk = fat16(16_384, 1);
    disk.0.borrow_mut()[PART_LBA as usize][13] = 0;
    let mut s = FatStore::new(disk, FixedTime);
    assert_eq!(
        s.mount(),
        Err(StoreError::Unsupported(Unsupported::BadBootSector))
    );
}

// Images for the file-level tests.

/// FAT16 with a FAT ending on a sector boundary (4 094 + 2 entries = 16
/// sectors), so no zero entries lie past the last cluster: the library's
/// free-cluster scan would hand those out.
const CLUSTERS: u32 = 4_094;

fn copy(disk: &RamDisk) -> RamDisk {
    RamDisk(RefCell::new(disk.0.borrow().clone()))
}

fn name(stem: &[u8]) -> FileName {
    FileName::new(Dir::Chimera, stem, b"BIN").unwrap()
}

/// A small FAT16 card with `/CHIMERA` made, and its id.
fn card() -> (Rc<RamDisk>, VolumeId) {
    let disk = Rc::new(with_clusters(CLUSTERS, 7));
    let (mut s, _) = probed(&disk);
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    (disk, vol)
}

/// Where the FATs and the root directory start: (FAT 1, FAT size, root).
fn layout(disk: &RamDisk) -> (usize, usize, usize) {
    let bs = disk.block(PART_LBA);
    let reserved = u16::from_le_bytes([bs[14], bs[15]]) as usize;
    let fat_sz = u16::from_le_bytes([bs[22], bs[23]]) as usize;
    let fat1 = PART_LBA as usize + reserved;
    (fat1, fat_sz, fat1 + 2 * fat_sz)
}

/// Marks every free cluster but the first `keep` bad, in both FATs.
fn fill(disk: &RamDisk, keep: usize) {
    let (fat1, fat_sz, _) = layout(disk);
    let mut blocks = disk.0.borrow_mut();
    let mut kept = 0;
    for entry in 2..CLUSTERS as usize + 2 {
        let (blk, at) = (fat1 + entry * 2 / 512, entry * 2 % 512);
        if blocks[blk][at..at + 2] != [0, 0] {
            continue;
        }
        if kept < keep {
            kept += 1;
            continue;
        }
        for b in [blk, blk + fat_sz] {
            blocks[b][at..at + 2].copy_from_slice(&0xFFF7u16.to_le_bytes());
        }
    }
    assert_eq!(kept, keep);
}

/// The block and offset of the directory entry named `name83` (space
/// padded, no dot).
fn entry(disk: &RamDisk, name83: &[u8; 11]) -> (usize, usize) {
    let blocks = disk.0.borrow();
    (PART_LBA as usize..blocks.len())
        .flat_map(|b| (0..512).step_by(32).map(move |at| (b, at)))
        .find(|&(b, at)| &blocks[b][at..at + 11] == name83)
        .expect("entry on the image")
}

/// Opens `/CHIMERA` on `disk` with the library alone, for pinning what it
/// reports.
fn raw<D: BlockDevice, R>(
    disk: D,
    f: impl FnOnce(&VolumeManager<D, FixedTime>, embedded_sdmmc::RawDirectory) -> R,
) -> R {
    let vm = VolumeManager::new(disk, FixedTime);
    let v = vm.open_raw_volume(VolumeIdx(0)).unwrap();
    let root = vm.open_root_dir(v).unwrap();
    let dir = vm.open_dir(root, "CHIMERA").unwrap();
    f(&vm, dir)
}

/// Writes `n` bytes to a new file with the library alone.
fn raw_write<D: BlockDevice>(disk: D, n: usize) -> Result<(), Error<D::Error>> {
    raw(disk, |vm, dir| {
        let f = vm
            .open_file_in_dir(dir, "NEW.BIN", Mode::ReadWriteCreateOrTruncate)
            .unwrap();
        (0..n)
            .step_by(512)
            .try_for_each(|at| vm.write(f, &[0x5A; 512][..(n - at).min(512)]))
    })
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

#[test]
fn mutated_images_never_panic() {
    let (base, vol) = card();
    let file = name(b"DATA");
    let (mut s, _) = probed(&base);
    s.write(vol, file, &mut |w| w.put(&[0xA5; 3 * 512]))
        .unwrap();
    let (fat1, fat_sz, root) = layout(&base);
    // The blocks holding the MBR, the boot sector, each FAT's used entries,
    // the root directory's entries and `/CHIMERA`'s (cluster 2, right after
    // the root).
    let targets = [0, PART_LBA as usize, fat1, fat1 + fat_sz, root, root + 32];
    for seed in 1..=2_000u64 {
        let mut rng = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let disk = copy(&base);
        for _ in 0..1 + rng.below(8) {
            let blk = targets[rng.below(targets.len())];
            let at = rng.below(512);
            disk.0.borrow_mut()[blk][at] = rng.next() as u8;
        }
        let run = catch_unwind(AssertUnwindSafe(|| {
            let mut s = FatStore::new(disk, FixedTime);
            let vol = s.mount().unwrap_or(vol);
            let _ = s.list(vol, Dir::Chimera, &mut |_, _| {});
            let _ = s.read(vol, file, &mut Sink::default());
            let _ = s.write(vol, name(b"NEW"), &mut |w| w.put(&[0x5A; 700]));
        }));
        assert!(run.is_ok(), "seed {seed} panicked");
    }
}

#[test]
fn error_calls_reinit() {
    let (disk, vol) = card();
    let cut = Rc::new(CutDisk {
        inner: copy(&disk),
        writes_left: Cell::new(Some(0)),
    });
    let (mut s, log) = probed(&cut);
    assert_eq!(
        s.write(vol, name(b"F"), &mut |w| w.put(b"x")),
        Err(StoreError::Io)
    );
    assert_eq!(log.reinits.get(), 1);
    assert_eq!(s.mount(), Ok(vol));
}

/// Review Focus 4: a card swapped while idle fails the first read.
#[test]
fn mount_retries_once_after_reinit() {
    let (disk, vol) = card();
    let (mut s, log) = probed(&disk);
    log.fail_reads.set(1);
    assert_eq!(s.mount(), Ok(vol));
    assert_eq!(log.reinits.get(), 1);

    let (mut s, log) = probed(&disk);
    log.fail_reads.set(2);
    assert_eq!(s.mount(), Err(StoreError::Io));
    assert_eq!(log.reinits.get(), 2, "the retry's own error re-inits too");
    assert_eq!(log.reads.get(), 2, "no third read");
}

/// Review Focus 5. The library never hands out the last free cluster: its
/// next-free scan after an allocation fails, and fails the allocation.
#[test]
fn full_maps_both_library_errors() {
    let (disk, vol) = card();
    fill(&disk, 0);
    assert!(matches!(
        raw_write(copy(&disk), 1),
        Err(Error::NotEnoughSpace)
    ));
    let (mut s, log) = probed(&disk);
    assert_eq!(
        s.write(vol, name(b"NEW"), &mut |w| w.put(b"x")),
        Err(StoreError::Full)
    );
    assert_eq!(log.reinits.get(), 0, "a full card is no fault");

    let (disk, vol) = card();
    fill(&disk, 2);
    assert!(matches!(raw_write(copy(&disk), 1024), Err(Error::DiskFull)));
    let (mut s, log) = probed(&disk);
    assert_eq!(
        s.write(vol, name(b"NEW"), &mut |w| w.put(&[1; 1024])),
        Err(StoreError::Full)
    );
    assert_eq!(log.reinits.get(), 0);
}

/// Review Focus 5: a card pulled during allocation is a fault, not a full
/// card.
#[test]
fn alloc_device_error_is_not_full() {
    let (disk, vol) = card();
    // A 2-cluster file: its entry, the first cluster's FAT entry (twice) and
    // data block, then the second cluster's FAT entry, which is cut.
    let cut_at = Some(4);
    let cut = || CutDisk {
        inner: copy(&disk),
        writes_left: Cell::new(cut_at),
    };
    assert!(matches!(raw_write(cut(), 1024), Err(Error::DiskFull)));
    let cut = Rc::new(cut());
    let (mut s, log) = probed(&cut);
    assert_eq!(
        s.write(vol, name(b"NEW"), &mut |w| w.put(&[1; 1024])),
        Err(StoreError::Io)
    );
    assert_eq!(log.reinits.get(), 1);
}

/// Sets cluster `c`'s entry in both FAT16 FATs.
fn link(disk: &RamDisk, c: usize, to: u16) {
    let (fat1, fat_sz, _) = layout(disk);
    for fat in [fat1, fat1 + fat_sz] {
        let (blk, at) = (fat + c * 2 / 512, c * 2 % 512);
        disk.0.borrow_mut()[blk][at..at + 2].copy_from_slice(&to.to_le_bytes());
    }
}

/// Chains the library would walk off the data area (a panic in debug
/// builds, a stray block in release) or around forever are refused first.
#[test]
fn broken_chains_are_corrupt_before_the_library_walks_them() {
    // `/CHIMERA` is cluster 2 and `DATA.BIN` clusters 3 to 5.
    let (base, vol) = card();
    let (mut s, _) = probed(&base);
    s.write(vol, name(b"DATA"), &mut |w| w.put(&[7; 3 * 512]))
        .unwrap();
    let broken: [(&str, usize, u16); 4] = [
        ("file links to a free cluster", 3, 0x00CA),
        ("file links to reserved cluster 1", 4, 1),
        ("file links past the volume", 4, (CLUSTERS + 2) as u16),
        ("directory loops", 2, 2),
    ];
    for (what, c, to) in broken {
        let disk = Rc::new(copy(&base));
        link(&disk, c, to);
        let (mut s, log) = probed(&disk);
        let mut sink = Sink::default();
        assert_eq!(
            s.read(vol, name(b"DATA"), &mut sink),
            Err(StoreError::Corrupt),
            "{what}"
        );
        assert!(sink.0.is_empty(), "{what}: nothing reaches the sink");
        assert_eq!(log.reinits.get(), 0, "{what}");
    }

    // FAT32's root directory is a chain too.
    let disk = Rc::new(fat32(1));
    let (mut s, _) = probed(&disk);
    let vol = s.mount().unwrap();
    let (fat1, _, _) = layout(&disk);
    disk.0.borrow_mut()[fat1][8..12].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(s.make_dir(vol, Dir::Chimera), Err(StoreError::Corrupt));
}

/// Review Focus 2's torn truncate: an old length over a short chain.
#[test]
fn chain_errors_read_as_corrupt() {
    let (disk, vol) = card();
    let (mut s, log) = probed(&disk);
    s.write(vol, name(b"TORN"), &mut |w| w.put(&[3; 512]))
        .unwrap();
    let (b, at) = entry(&disk, b"TORN    BIN");
    disk.0.borrow_mut()[b][at + 28..at + 32].copy_from_slice(&(3 * 512u32).to_le_bytes());
    assert_eq!(
        s.read(vol, name(b"TORN"), &mut Sink::default()),
        Err(StoreError::Corrupt)
    );
    assert_eq!(log.reinits.get(), 0);
}
