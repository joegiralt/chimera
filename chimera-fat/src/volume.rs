//! The MBR and partition boot sector: FS type and volume serial, which
//! `embedded-sdmmc` doesn't expose. Pure; never panics.

use chimera_hal::store::{Unsupported, VolumeId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsKind {
    Fat16,
    Fat32,
}

/// The first MBR partition entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partition {
    pub lba: u32,
    pub kind: u8,
}

const ENTRY: usize = 446;

fn u16_at(b: &[u8; 512], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8; 512], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn signed(b: &[u8; 512]) -> bool {
    u16_at(b, 510) == 0xAA55
}

/// A FAT boot sector (jump, then `FAT` in the FAT16 or FAT32 FS-type field).
fn is_fat_boot_sector(b: &[u8; 512]) -> bool {
    matches!(b[0], 0xEB | 0xE9) && (&b[0x36..0x39] == b"FAT" || &b[0x52..0x55] == b"FAT")
}

/// Block 0's first partition. A missing signature, a FAT boot sector in
/// block 0 (a "superfloppy") or an invalid first entry is `NoPartitionTable`.
pub fn first_partition(mbr: &[u8; 512]) -> Result<Partition, Unsupported> {
    let status = mbr[ENTRY];
    let kind = mbr[ENTRY + 4];
    let lba = u32_at(mbr, ENTRY + 8);
    if !signed(mbr) || is_fat_boot_sector(mbr) || status & 0x7F != 0 || kind == 0 || lba == 0 {
        return Err(Unsupported::NoPartitionTable);
    }
    Ok(Partition { lba, kind })
}

/// The partition's FS type and identity, from its boot sector and the
/// MBR's type byte `part_kind`.
pub fn boot_sector(bs: &[u8; 512], part_kind: u8) -> Result<(FsKind, VolumeId), Unsupported> {
    match part_kind {
        0x04 | 0x06 | 0x0E | 0x0B | 0x0C => {}
        0x07 if &bs[3..11] == b"EXFAT   " => return Err(Unsupported::Exfat),
        k => return Err(Unsupported::NotFat(k)),
    }
    if !signed(bs) {
        return Err(Unsupported::NotFat(part_kind));
    }
    // BPB_FATSz16 is zero on FAT32 only.
    let (kind, at) = if u16_at(bs, 22) == 0 {
        (FsKind::Fat32, 0x43)
    } else {
        (FsKind::Fat16, 0x27)
    };
    let mut label = [0; 11];
    label.copy_from_slice(&bs[at + 4..at + 15]);
    Ok((
        kind,
        VolumeId {
            serial: u32_at(bs, at),
            label,
        },
    ))
}
