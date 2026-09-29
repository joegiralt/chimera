//! RAM-disk card images: FAT16, FAT32, exFAT and the superfloppies.

use chimera_fat::Medium;
use chimera_hal::store::StoreError;
use core::cell::{Cell, RefCell};
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};

pub const PART_LBA: u32 = 2048;

#[derive(Debug)]
pub struct DiskError;

impl core::fmt::Display for DiskError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("disk error")
    }
}

impl core::error::Error for DiskError {}

pub struct RamDisk(pub RefCell<Vec<[u8; 512]>>);

impl RamDisk {
    fn zeroed(blocks: u32) -> Self {
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

/// A card pulled mid-write: every write fails once `writes_left` reaches 0.
/// `None` never fails.
#[allow(dead_code)]
pub struct CutDisk {
    pub inner: RamDisk,
    pub writes_left: Cell<Option<u32>>,
}

#[allow(dead_code)]
impl BlockDevice for CutDisk {
    type Error = DiskError;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), DiskError> {
        self.inner.read(blocks, start)
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), DiskError> {
        match self.writes_left.get() {
            Some(0) => return Err(DiskError),
            Some(n) => self.writes_left.set(Some(n - 1)),
            None => {}
        }
        self.inner.write(blocks, start)
    }

    fn num_blocks(&self) -> Result<BlockCount, DiskError> {
        self.inner.num_blocks()
    }
}

#[allow(dead_code)]
impl Medium for CutDisk {
    /// Once cut, a fault, even where the library reports `DiskFull`.
    fn fault(&self) -> Option<StoreError> {
        (self.writes_left.get() == Some(0)).then_some(StoreError::Io)
    }
}

fn put16(b: &mut [u8; 512], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut [u8; 512], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn mbr(disk: &mut [[u8; 512]], kind: u8) {
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
