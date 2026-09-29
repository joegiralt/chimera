//! RAM-disk card images: FAT16, FAT32, exFAT and the superfloppies; `Rec`,
//! a recording view of one's partition; `Overlay`, a copy-on-write view of
//! one; and `embedded-sdmmc`, a second FAT implementation, over them.
// Each test binary uses its own part of this module.
#![allow(dead_code)]

use chimera_fat::Medium;
use chimera_fat::blocks::{BLOCK, Blocks};
use chimera_fat::volume::Root;
use chimera_fat::volume::{Layout, first_partition, layout};
use chimera_hal::store::Dir;
use core::cell::{Cell, RefCell};
use embedded_sdmmc::{
    Block, BlockCount, BlockDevice, BlockIdx, Mode, TimeSource, Timestamp, VolumeIdx, VolumeManager,
};
use std::collections::HashMap;
use std::rc::Rc;

pub const PART_LBA: u32 = 2048;

#[derive(Debug, PartialEq, Eq)]
pub struct DiskError;

impl core::fmt::Display for DiskError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("disk error")
    }
}

impl core::error::Error for DiskError {}

pub struct RamDisk(pub RefCell<Vec<[u8; 512]>>);

impl RamDisk {
    pub fn zeroed(blocks: u32) -> Self {
        Self(RefCell::new(vec![[0; 512]; blocks as usize]))
    }

    pub fn block(&self, idx: u32) -> [u8; 512] {
        self.0.borrow()[idx as usize]
    }
}

impl BlockDevice for RamDisk {
    type Error = DiskError;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), DiskError> {
        let disk = self.0.borrow();
        for (i, b) in blocks.iter_mut().enumerate() {
            b.contents = *disk.get(start.0 as usize + i).ok_or(DiskError)?;
        }
        Ok(())
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), DiskError> {
        let mut disk = self.0.borrow_mut();
        for (i, b) in blocks.iter().enumerate() {
            *disk.get_mut(start.0 as usize + i).ok_or(DiskError)? = b.contents;
        }
        Ok(())
    }

    fn num_blocks(&self) -> Result<BlockCount, DiskError> {
        Ok(BlockCount(self.0.borrow().len() as u32))
    }
}

impl Medium for RamDisk {}

/// Where a `CutDisk` is pulled, counted in blocks, not calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    Never,
    /// Blocks `0..k` land whole; block `k` and every one after fail.
    After(u32),
    /// Blocks `0..k` land whole; block `k` lands torn and fails, and every
    /// one after fails.
    TornAfter(u32, Tear),
}

/// What a torn block holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tear {
    /// The first 256 B new, the rest old.
    HalfOld,
    /// Noise.
    Garbage,
}

/// A card pulled mid-write. Reads keep working, so a test can look at
/// what the cut left.
pub struct CutDisk {
    pub inner: RamDisk,
    /// Blocks the card took, a torn one included.
    pub writes: Cell<u32>,
    pub cut: Cell<Cut>,
}

impl CutDisk {
    pub fn new(inner: RamDisk, cut: Cut) -> Self {
        Self {
            inner,
            writes: Cell::new(0),
            cut: Cell::new(cut),
        }
    }
}

/// Block `new` torn over `old` as `t` says; the noise is seeded by `lba`.
fn tear(old: &[u8; 512], new: &[u8; 512], t: Tear, lba: u32) -> [u8; 512] {
    let mut b = *old;
    match t {
        Tear::HalfOld => b[..256].copy_from_slice(&new[..256]),
        Tear::Garbage => {
            let mut x = u64::from(lba).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            for v in &mut b {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                *v = x as u8;
            }
        }
    }
    b
}

impl BlockDevice for CutDisk {
    type Error = DiskError;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), DiskError> {
        self.inner.read(blocks, start)
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), DiskError> {
        for (i, b) in blocks.iter().enumerate() {
            let (n, at) = (self.writes.get(), start.0 + i as u32);
            let torn = match self.cut.get() {
                Cut::After(k) if n >= k => return Err(DiskError),
                Cut::TornAfter(k, _) if n > k => return Err(DiskError),
                Cut::TornAfter(k, t) if n == k => Some(t),
                _ => None,
            };
            let contents = match torn {
                Some(t) => tear(&self.inner.block(at), &b.contents, t, at),
                None => b.contents,
            };
            self.inner.write(&[Block { contents }], BlockIdx(at))?;
            self.writes.set(n + 1);
            if torn.is_some() {
                return Err(DiskError);
            }
        }
        Ok(())
    }

    fn num_blocks(&self) -> Result<BlockCount, DiskError> {
        self.inner.num_blocks()
    }
}

impl Medium for CutDisk {}

/// A shared base image with this view's own changes over it, so a fuzz
/// seed on a 33 MB FAT32 card copies nothing.
pub struct Overlay {
    base: Rc<RamDisk>,
    changed: RefCell<HashMap<u32, [u8; 512]>>,
}

impl Overlay {
    pub fn new(base: &Rc<RamDisk>) -> Self {
        Self {
            base: base.clone(),
            changed: RefCell::new(HashMap::new()),
        }
    }

    /// Block `idx`, if the card has it.
    pub fn block(&self, idx: u32) -> Option<[u8; 512]> {
        let changed = self.changed.borrow().get(&idx).copied();
        changed.or_else(|| self.base.0.borrow().get(idx as usize).copied())
    }

    /// Sets byte `at` of block `idx`, which the card has.
    pub fn poke(&self, idx: u32, at: usize, v: u8) {
        let mut b = self.block(idx).expect("a block on the card");
        b[at] = v;
        self.changed.borrow_mut().insert(idx, b);
    }
}

impl BlockDevice for Overlay {
    type Error = DiskError;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), DiskError> {
        for (i, b) in blocks.iter_mut().enumerate() {
            b.contents = self.block(start.0 + i as u32).ok_or(DiskError)?;
        }
        Ok(())
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), DiskError> {
        for (i, b) in blocks.iter().enumerate() {
            let idx = start.0 + i as u32;
            self.block(idx).ok_or(DiskError)?;
            self.changed.borrow_mut().insert(idx, b.contents);
        }
        Ok(())
    }

    fn num_blocks(&self) -> Result<BlockCount, DiskError> {
        self.base.num_blocks()
    }
}

impl Medium for Overlay {}

fn put16(b: &mut [u8; 512], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut [u8; 512], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// One partition from `PART_LBA` to the disk's end, of type `kind`.
pub fn mbr(disk: &mut [[u8; 512]], kind: u8) {
    let len = disk.len() as u32 - PART_LBA;
    let b = &mut disk[0];
    b[446 + 1..446 + 4].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    b[446 + 4] = kind;
    b[446 + 5..446 + 8].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    put32(b, 446 + 8, PART_LBA);
    put32(b, 446 + 12, len);
    put16(b, 510, 0xAA55);
}

/// The BPB common to FAT16 and FAT32: 512 B sectors, 1 sector per cluster,
/// 2 FATs.
fn bpb(b: &mut [u8; 512], jump: u8, reserved: u16, hidden: u32, total: u32) {
    b[0..3].copy_from_slice(&[0xEB, jump, 0x90]);
    b[3..11].copy_from_slice(b"CHIMERA ");
    put16(b, 11, 512);
    b[13] = 1;
    put16(b, 14, reserved);
    b[16] = 2;
    b[21] = 0xF8;
    put16(b, 24, 63);
    put16(b, 26, 255);
    put32(b, 28, hidden);
    put32(b, 32, total);
    put16(b, 510, 0xAA55);
}

/// The root's volume label entry, matching the boot sector's.
fn label(root: &mut [u8; 512]) {
    root[..11].copy_from_slice(b"CHIMERA    ");
    root[11] = 0x08;
}

/// The extended BPB: drive, signature, serial, label, FS type.
fn ebpb(b: &mut [u8; 512], at: usize, serial: u32, fs: &[u8; 8]) {
    b[at] = 0x80;
    b[at + 2] = 0x29;
    put32(b, at + 3, serial);
    b[at + 7..at + 18].copy_from_slice(b"CHIMERA    ");
    b[at + 18..at + 26].copy_from_slice(fs);
}

const RESERVED16: u32 = 1;
const ROOT_ENTRIES: u32 = 512;
const ROOT_SECTORS: u32 = ROOT_ENTRIES * 32 / 512;
const RESERVED32: u32 = 32;

fn fat16_size(clusters: u32) -> u32 {
    ((clusters + 2) * 2).div_ceil(512)
}

fn fat32_size(clusters: u32) -> u32 {
    ((clusters + 2) * 4).div_ceil(512)
}

/// A FAT16-layout volume over all of `vol` (first block at LBA `hidden`),
/// with FATs of `fat_sz` sectors.
fn fat16_volume(vol: &mut [[u8; 512]], hidden: u32, serial: u32, fat_sz: u32) {
    let total = vol.len() as u32;
    let b = &mut vol[0];
    bpb(b, 0x3C, RESERVED16 as u16, hidden, total);
    put16(b, 17, ROOT_ENTRIES as u16);
    put16(b, 22, fat_sz as u16);
    ebpb(b, 0x24, serial, b"FAT16   ");
    for fat in 0..2 {
        let f = &mut vol[(RESERVED16 + fat * fat_sz) as usize];
        put32(f, 0, 0xFFFF_FFF8);
    }
    label(&mut vol[(RESERVED16 + 2 * fat_sz) as usize]);
}

/// A FAT32-layout volume over all of `vol`, with FATs of `fat_sz` sectors.
fn fat32_volume(vol: &mut [[u8; 512]], hidden: u32, serial: u32, fat_sz: u32) {
    let total = vol.len() as u32;
    let b = &mut vol[0];
    bpb(b, 0x58, RESERVED32 as u16, hidden, total);
    put32(b, 36, fat_sz);
    put32(b, 44, 2); // root directory cluster
    put16(b, 48, 1); // FSInfo sector
    put16(b, 50, 6); // backup boot sector
    ebpb(b, 0x40, serial, b"FAT32   ");
    vol[6] = vol[0];
    let info = &mut vol[1];
    put32(info, 0, 0x4161_5252);
    put32(info, 484, 0x6141_7272);
    put32(info, 488, 0xFFFF_FFFF);
    put32(info, 492, 0xFFFF_FFFF);
    put32(info, 508, 0xAA55_0000);
    for fat in 0..2 {
        let f = &mut vol[(RESERVED32 + fat * fat_sz) as usize];
        put32(f, 0, 0x0FFF_FFF8);
        put32(f, 4, 0x0FFF_FFFF);
        put32(f, 8, 0x0FFF_FFFF); // root directory: one cluster, end of chain
    }
    label(&mut vol[(RESERVED32 + 2 * fat_sz) as usize]);
}

/// A FAT16 card of `blocks` blocks, type 0x0E.
pub fn fat16(blocks: u32, serial: u32) -> RamDisk {
    let d = RamDisk::zeroed(blocks);
    {
        let mut disk = d.0.borrow_mut();
        mbr(&mut disk, 0x0E);
        let fat_sz = fat16_size(blocks - PART_LBA);
        fat16_volume(&mut disk[PART_LBA as usize..], PART_LBA, serial, fat_sz);
    }
    d
}

/// A card of exactly `clusters` clusters: FAT16 layout (type 0x0E) below
/// 65 525, FAT32 layout (type 0x0C) from it. Below 4 085 it is FAT12-sized.
pub fn with_clusters(clusters: u32, serial: u32) -> RamDisk {
    if clusters >= 65_525 {
        return fat32_layout(clusters, serial);
    }
    let fat_sz = fat16_size(clusters);
    let d = RamDisk::zeroed(PART_LBA + RESERVED16 + 2 * fat_sz + ROOT_SECTORS + clusters);
    {
        let mut disk = d.0.borrow_mut();
        mbr(&mut disk, 0x0E);
        fat16_volume(&mut disk[PART_LBA as usize..], PART_LBA, serial, fat_sz);
    }
    d
}

/// A FAT32-layout card (type 0x0C) of exactly `clusters` clusters, whatever
/// the count.
pub fn fat32_layout(clusters: u32, serial: u32) -> RamDisk {
    let fat_sz = fat32_size(clusters);
    let d = RamDisk::zeroed(PART_LBA + RESERVED32 + 2 * fat_sz + clusters);
    {
        let mut disk = d.0.borrow_mut();
        mbr(&mut disk, 0x0C);
        fat32_volume(&mut disk[PART_LBA as usize..], PART_LBA, serial, fat_sz);
    }
    d
}

/// A FAT32 card of 66 000 clusters, over the 65 525 floor.
pub fn fat32(serial: u32) -> RamDisk {
    with_clusters(66_000, serial)
}

fn exfat_boot_sector(b: &mut [u8; 512]) {
    b[0..3].copy_from_slice(&[0xEB, 0x76, 0x90]);
    b[3..11].copy_from_slice(b"EXFAT   ");
    put16(b, 510, 0xAA55);
}

/// Partition type 0x07 holding an exFAT boot sector.
pub fn exfat() -> RamDisk {
    let d = RamDisk::zeroed(PART_LBA + 64);
    {
        let mut disk = d.0.borrow_mut();
        mbr(&mut disk, 0x07);
        exfat_boot_sector(&mut disk[PART_LBA as usize]);
    }
    d
}

/// exFAT written from block 0, with no MBR.
pub fn exfat_superfloppy() -> RamDisk {
    let d = RamDisk::zeroed(64);
    exfat_boot_sector(&mut d.0.borrow_mut()[0]);
    d
}

/// FAT16 written from block 0, with no MBR.
pub fn superfloppy() -> RamDisk {
    let d = RamDisk::zeroed(16_384);
    fat16_volume(&mut d.0.borrow_mut(), 0, 0x1234_5678, fat16_size(16_384));
    d
}

/// The first partition's `Layout`.
pub fn layout_of(disk: &RamDisk) -> Layout {
    let p = first_partition(&disk.block(0)).unwrap();
    layout(&disk.block(p.lba), p).unwrap().0
}

/// The first partition of a `RamDisk` as `Blocks`, logging every access.
/// I/O at or past `bound` (the partition's end, unless a test narrows it)
/// panics.
pub struct Rec<'a> {
    disk: &'a RamDisk,
    lba: u32,
    pub bound: u32,
    pub reads: u32,
    /// Every write fails while set.
    pub fail_writes: bool,
    /// Each write's block, and whether it held these bytes already.
    pub writes: Vec<(u32, bool)>,
}

impl<'a> Rec<'a> {
    pub fn new(disk: &'a RamDisk) -> Self {
        let p = first_partition(&disk.block(0)).unwrap();
        Self {
            disk,
            lba: p.lba,
            bound: p.blocks,
            reads: 0,
            fail_writes: false,
            writes: Vec::new(),
        }
    }

    fn at(&self, lba: u32) -> usize {
        assert!(lba < self.bound, "block {lba} at or past {}", self.bound);
        (self.lba + lba) as usize
    }
}

impl Blocks for Rec<'_> {
    type Error = DiskError;

    fn read(&mut self, lba: u32, buf: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = self.at(lba);
        self.reads += 1;
        *buf = *self.disk.0.borrow().get(at).ok_or(DiskError)?;
        Ok(())
    }

    fn write(&mut self, lba: u32, buf: &[u8; BLOCK]) -> Result<(), DiskError> {
        let at = self.at(lba);
        if self.fail_writes {
            return Err(DiskError);
        }
        let mut disk = self.disk.0.borrow_mut();
        let block = disk.get_mut(at).ok_or(DiskError)?;
        self.writes.push((lba, block == buf));
        *block = *buf;
        Ok(())
    }
}

/// `n` bytes no two files share at the same offset.
pub fn pattern(seed: u8, n: usize) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(31) ^ seed).collect()
}

/// 2026-01-01 00:00, for `embedded-sdmmc`'s stamps.
pub struct FixedTime;

impl TimeSource for FixedTime {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

pub type Sdmmc = VolumeManager<RamDisk, FixedTime>;

/// Runs `f` on `embedded-sdmmc` over `disk`. The library (0.10) takes the
/// root's volume label entry `CHIMERA` for a file of that name, and then
/// can't open or make the directory `/CHIMERA`, so the label's first byte
/// is changed meanwhile.
pub fn sdmmc<R>(disk: RamDisk, f: impl FnOnce(&Sdmmc) -> R) -> (RamDisk, R) {
    let l = layout_of(&disk);
    let root = PART_LBA
        + match l.root() {
            Root::Fixed { first, .. } => first,
            Root::Cluster(c) => l.cluster_block(c).unwrap(),
        };
    let at = disk
        .block(root)
        .as_chunks::<32>()
        .0
        .iter()
        .position(|e| e[..11] == *b"CHIMERA    " && e[11] & 0x08 != 0);
    let poke = |disk: &RamDisk, v| {
        if let Some(at) = at {
            disk.0.borrow_mut()[root as usize][at * 32] = v;
        }
    };
    poke(&disk, b'X');
    let vm = VolumeManager::new(disk, FixedTime);
    let r = f(&vm);
    let disk = vm.free().0;
    poke(&disk, b'C');
    (disk, r)
}

/// Writes each `(dir, name, bytes)` through `embedded-sdmmc`, making the
/// directories it needs.
pub fn sdmmc_write(disk: RamDisk, files: &[(Dir, &str, &[u8])]) -> RamDisk {
    sdmmc(disk, |vm| {
        let vol = vm.open_volume(VolumeIdx(0)).unwrap();
        let root = vol.open_root_dir().unwrap();
        for &(dir, name, bytes) in files {
            let _ = root.make_dir_in_dir("CHIMERA");
            let mut d = root.open_dir("CHIMERA").unwrap();
            let sub = match dir {
                Dir::Chimera => None,
                Dir::Projects => Some("PROJECTS"),
                Dir::Sounds => Some("SOUNDS"),
            };
            if let Some(sub) = sub {
                let _ = d.make_dir_in_dir(sub);
                d.change_dir(sub).unwrap();
            }
            let f = d
                .open_file_in_dir(name, Mode::ReadWriteCreateOrTruncate)
                .unwrap();
            for chunk in bytes.chunks(BLOCK) {
                f.write(chunk).unwrap();
            }
        }
    })
    .0
}

/// One live entry of `/CHIMERA`, as `fat_check` found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FatEntry {
    pub name: [u8; 11],
    pub raw: [u8; 32],
    /// The absolute block holding the entry.
    pub block: u32,
    /// Its clusters, as far as the chain goes.
    pub chain: Vec<u32>,
}

/// What `fat_check` found. It reads the image by its own BPB parse and
/// chain walk, not through `chimera_fat`: the code under test isn't its
/// own judge.
#[derive(Debug)]
pub struct FatReport {
    /// Clusters in more than one chain.
    pub cross_linked: Vec<u32>,
    /// Files whose length is over their chain, or whose chain breaks (a
    /// free, reserved or out-of-range link, or a loop) before an end mark.
    pub short: Vec<[u8; 11]>,
    /// Absolute blocks of FAT 2 that differ from FAT 1's.
    pub fats_differ: Vec<u32>,
    pub entries: Vec<FatEntry>,
    /// Absolute first blocks of FAT 1 and FAT 2, and bytes per FAT entry.
    fats: [u32; 2],
    entry_bytes: u32,
}

impl FatReport {
    pub fn entry(&self, name: &[u8; 11]) -> Option<&FatEntry> {
        self.entries.iter().find(|e| e.name == *name)
    }

    /// The absolute FAT 1 and FAT 2 blocks holding `chain`'s entries.
    pub fn fat_blocks(&self, chain: &[u32]) -> Vec<u32> {
        let mut v: Vec<u32> = chain
            .iter()
            .flat_map(|&c| self.fats.map(|f| f + c * self.entry_bytes / 512))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// Walks `disk`'s FATs, root and `/CHIMERA`: see `FatReport`.
pub fn fat_check(disk: &RamDisk) -> FatReport {
    let u16_at = |b: &[u8; 512], at: usize| u32::from(u16::from_le_bytes([b[at], b[at + 1]]));
    let u32_at = |b: &[u8; 512], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let part = u32_at(&disk.block(0), 446 + 8);
    let bpb = disk.block(part);
    assert_eq!(u16_at(&bpb, 11), 512, "512 B sectors");
    let spc = u32::from(bpb[13]);
    let reserved = u16_at(&bpb, 14);
    let copies = u32::from(bpb[16]);
    let root_entries = u16_at(&bpb, 17);
    let total = match u16_at(&bpb, 19) {
        0 => u32_at(&bpb, 32),
        n => n,
    };
    let fat_size = match u16_at(&bpb, 22) {
        0 => u32_at(&bpb, 36),
        n => n,
    };
    let root_blocks = (root_entries * 32).div_ceil(512);
    let data = reserved + copies * fat_size + root_blocks;
    let clusters = (total - data) / spc;
    let fat32 = clusters >= 65_525;
    let (entry_bytes, end_mark, mask) = if fat32 {
        (4, 0x0FFF_FFF8, 0x0FFF_FFFF)
    } else {
        (2, 0xFFF8, 0xFFFF)
    };
    let fats = [part + reserved, part + reserved + fat_size];
    let link = |c: u32| {
        let b = disk.block(fats[0] + c * entry_bytes / 512);
        let at = (c * entry_bytes % 512) as usize;
        let v = if fat32 {
            u32_at(&b, at)
        } else {
            u16_at(&b, at)
        };
        v & mask
    };
    let held = |c: u32| (2..clusters + 2).contains(&c);
    // The chain from `start` (0 is none), and whether it ended on an end mark.
    let walk = |start: u32| {
        let mut chain = Vec::new();
        if start == 0 {
            return (chain, true);
        }
        let mut c = start;
        loop {
            // Longer than the volume: a loop.
            if !held(c) || chain.len() as u32 == clusters {
                return (chain, false);
            }
            chain.push(c);
            match link(c) {
                v if v >= end_mark => return (chain, true),
                v => c = v,
            }
        }
    };
    let first_block = |c: u32| part + data + (c - 2) * spc;
    let blocks_of = |chain: &[u32]| -> Vec<u32> {
        chain
            .iter()
            .flat_map(|&c| (0..spc).map(move |i| first_block(c) + i))
            .collect()
    };
    let entries_in = |blocks: &[u32]| -> Vec<(u32, [u8; 32])> {
        let mut out = Vec::new();
        for &b in blocks {
            for e in disk.block(b).as_chunks::<32>().0 {
                match e[0] {
                    0 => return out,
                    0xE5 | b'.' => {}
                    _ if e[11] == 0x0F || e[11] & 0x08 != 0 => {}
                    _ => out.push((b, *e)),
                }
            }
        }
        out
    };
    let start_of = |e: &[u8; 32]| {
        let hi = if fat32 {
            u32::from(u16::from_le_bytes([e[20], e[21]])) << 16
        } else {
            0
        };
        hi | u32::from(u16::from_le_bytes([e[26], e[27]]))
    };

    let mut used: HashMap<u32, u32> = HashMap::new();
    let mut count = |chain: &[u32]| {
        for &c in chain {
            *used.entry(c).or_default() += 1;
        }
    };
    let root = if fat32 {
        let (chain, _) = walk(u32_at(&bpb, 44));
        count(&chain);
        blocks_of(&chain)
    } else {
        (0..root_blocks)
            .map(|i| part + reserved + copies * fat_size + i)
            .collect()
    };
    let chimera = entries_in(&root)
        .into_iter()
        .find(|(_, e)| e[..11] == *b"CHIMERA    " && e[11] & 0x10 != 0);
    let (mut entries, mut short) = (Vec::new(), Vec::new());
    if let Some((_, dir)) = chimera {
        let (dir_chain, _) = walk(start_of(&dir));
        count(&dir_chain);
        for (block, raw) in entries_in(&blocks_of(&dir_chain)) {
            let name: [u8; 11] = raw[..11].try_into().unwrap();
            let (chain, ended) = walk(start_of(&raw));
            count(&chain);
            let is_dir = raw[11] & 0x10 != 0;
            let len = u32::from_le_bytes(raw[28..32].try_into().unwrap());
            let need = len.div_ceil(spc * 512) as usize;
            if !ended || (!is_dir && chain.len() < need) {
                short.push(name);
            }
            entries.push(FatEntry {
                name,
                raw,
                block,
                chain,
            });
        }
    }
    let mut cross_linked: Vec<u32> = used
        .into_iter()
        .filter(|&(_, n)| n > 1)
        .map(|(c, _)| c)
        .collect();
    cross_linked.sort_unstable();
    let fats_differ = (0..fat_size)
        .filter(|&i| disk.block(fats[0] + i) != disk.block(fats[1] + i))
        .map(|i| fats[1] + i)
        .collect();
    FatReport {
        cross_linked,
        short,
        fats_differ,
        entries,
        fats,
        entry_bytes,
    }
}
