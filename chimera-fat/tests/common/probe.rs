//! `Probe`, a card in a slot the test can empty, recording what `FatStore`
//! does to it; and the checks every store test makes of a card.
// Each test binary uses its own part of this module.
#![allow(dead_code)]

use crate::image::{DiskError, PART_LBA, RamDisk, layout_of};
use chimera_fat::volume::Layout;
use chimera_fat::{FatStore, Medium};
use chimera_hal::store::{ReadSink, StoreError};
use chimera_hal::testkit::store_suite;
use core::cell::{Cell, RefCell};
use core::ops::ControlFlow;
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};
use std::rc::Rc;

/// Device calls a `Probe` allows: more is a loop.
pub const CALL_LIMIT: u32 = 1_000_000;

/// What a `Probe` saw, and what it fails.
#[derive(Default)]
pub struct Log {
    /// The card is out of the slot.
    pub absent: Cell<bool>,
    /// Fail this many reads, first to last.
    pub fail_reads: Cell<u32>,
    /// Every write to this block fails.
    pub fail_write: Cell<Option<u32>>,
    pub reads: Cell<u32>,
    pub calls: Cell<u32>,
    pub reinits: Cell<u32>,
    /// Each write's block, and whether it held those bytes already.
    pub writes: RefCell<Vec<(u32, bool)>>,
}

/// A card in a slot, sharing its image with the test.
pub struct Probe<D> {
    pub disk: Rc<D>,
    pub log: Rc<Log>,
}

impl<D> Probe<D> {
    fn call(&self) {
        let n = self.log.calls.get() + 1;
        assert!(n <= CALL_LIMIT, "{n} device calls: a loop");
        self.log.calls.set(n);
    }
}

impl<D: BlockDevice<Error = DiskError>> BlockDevice for Probe<D> {
    type Error = DiskError;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), DiskError> {
        self.call();
        let log = &self.log;
        log.reads.set(log.reads.get() + 1);
        if log.absent.get() {
            return Err(DiskError);
        }
        if log.fail_reads.get() > 0 {
            log.fail_reads.set(log.fail_reads.get() - 1);
            return Err(DiskError);
        }
        self.disk.read(blocks, start)
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), DiskError> {
        self.call();
        assert_eq!(blocks.len(), 1, "one block at a time");
        if self.log.absent.get() || self.log.fail_write.get() == Some(start.0) {
            return Err(DiskError);
        }
        let mut was = [Block::new()];
        self.disk.read(&mut was, start)?;
        let same = was[0].contents == blocks[0].contents;
        self.log.writes.borrow_mut().push((start.0, same));
        self.disk.write(blocks, start)
    }

    fn num_blocks(&self) -> Result<BlockCount, DiskError> {
        self.disk.num_blocks()
    }
}

impl<D: BlockDevice<Error = DiskError>> Medium for Probe<D> {
    fn reinit(&self) {
        self.log.reinits.set(self.log.reinits.get() + 1);
    }

    fn classify(&self, _e: &DiskError) -> StoreError {
        if self.log.absent.get() {
            StoreError::NoCard
        } else {
            StoreError::Io
        }
    }
}

pub type Probed<D> = FatStore<Probe<D>>;

pub fn probed<D: BlockDevice<Error = DiskError>>(disk: &Rc<D>) -> Probed<D> {
    FatStore::new(Probe {
        disk: disk.clone(),
        log: Rc::default(),
    })
}

pub fn log<D: BlockDevice<Error = DiskError>>(s: &Probed<D>) -> &Log {
    &s.device().log
}

/// FAT 1's and FAT 2's first blocks, from the partition's start.
pub fn fats(l: &Layout) -> (u32, u32) {
    let mut at = l.fat_copies(l.fat_block(0));
    (at.next().unwrap(), at.next().unwrap())
}

/// Free clusters in FAT 1 and in FAT 2.
pub fn free(disk: &RamDisk) -> (u32, u32) {
    let l = layout_of(disk);
    let (fat1, fat2) = fats(&l);
    let count = |fat: u32| {
        let blocks = disk.0.borrow();
        (2..l.clusters() + 2)
            .filter(|&c| {
                let b = &blocks[(PART_LBA + fat + l.fat_block(c) - fat1) as usize];
                l.entry(b, c) == 0
            })
            .count() as u32
    };
    (count(fat1), count(fat2))
}

/// FAT 1 and FAT 2 are byte-equal.
pub fn fats_equal(disk: &RamDisk) -> bool {
    let l = layout_of(disk);
    let (fat1, fat2) = fats(&l);
    let at = |b: u32| (PART_LBA + b) as usize;
    let blocks = disk.0.borrow();
    blocks[at(fat1)..at(fat2)] == blocks[at(fat2)..at(fat2 + fat2 - fat1)]
}

/// After every step: FAT 1 and FAT 2 are byte-equal (h), every write so far
/// lies in the partition (a), and none in the reserved or FAT region
/// rewrote a block with the bytes it held (d).
pub fn consistent(s: &Probed<RamDisk>) {
    let disk = &s.device().disk;
    let l = layout_of(disk);
    let (_, fat2) = fats(&l);
    let meta_end = PART_LBA + 2 * fat2 - l.fat_block(0);
    assert!(fats_equal(disk), "FAT 1 and FAT 2 differ");
    for &(lba, same) in log(s).writes.borrow().iter() {
        assert!(
            (PART_LBA..PART_LBA + l.blocks()).contains(&lba),
            "a write at {lba}, outside the partition"
        );
        assert!(
            !(same && lba < meta_end),
            "block {lba} rewritten with the bytes it held"
        );
    }
}

/// `store_suite` on cards from `image`, `consistent` after every step.
/// Returns each card it made, with its log.
pub fn suite(image: impl Fn(u32) -> RamDisk) -> Vec<(Rc<RamDisk>, Rc<Log>)> {
    let cards = RefCell::new(Vec::new());
    let slot = |serial| {
        let disk = Rc::new(image(serial));
        let s = probed(&disk);
        cards.borrow_mut().push((disk, s.device().log.clone()));
        s
    };
    store_suite(
        &mut || slot(1),
        &mut |s| *s = slot(2),
        &mut |s| log(s).absent.set(true),
        &mut consistent,
    );
    cards.into_inner()
}

/// What a read delivered.
#[derive(Default)]
pub struct Sink(pub Vec<u8>);

impl ReadSink for Sink {
    fn begin(&mut self, _len: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        self.0.extend_from_slice(bytes);
        ControlFlow::Continue(())
    }
}

pub struct XorShift(pub u64);

impl XorShift {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}
