//! The MBR and partition boot sector: FS type and volume serial, which
//! `embedded-sdmmc` doesn't expose. Pure; never panics.

use chimera_hal::store::{Unsupported, VolumeId};
use core::num::NonZeroU8;
use core::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsKind {
    Fat16,
    Fat32,
}

/// A FAT partition type byte (0x04, 0x06, 0x0E, 0x0B or 0x0C). Only
/// `first_partition` builds one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FatTypeByte(u8);

impl FatTypeByte {
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// The MBR type byte of a present partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionType {
    Fat(FatTypeByte),
    /// 0x07: the boot sector decides.
    ExfatOrNtfs,
    Other(NonZeroU8),
}

impl PartitionType {
    /// `None` for an empty entry (0).
    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x04 | 0x06 | 0x0E | 0x0B | 0x0C => Some(Self::Fat(FatTypeByte(b))),
            EXFAT_TYPE => Some(Self::ExfatOrNtfs),
            b => NonZeroU8::new(b).map(Self::Other),
        }
    }
}

/// The first MBR partition entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partition {
    pub lba: u32,
    pub kind: PartitionType,
}

const SECTOR: usize = 512;
const SIGNATURE: usize = 510;
const BOOT_SIGNATURE: u16 = 0xAA55;
const OEM_NAME: Range<usize> = 3..11;
const EXFAT_OEM: &[u8] = b"EXFAT   ";
const EXFAT_TYPE: u8 = 0x07;
const BS_JUMP: usize = 0;
const JMP_SHORT: u8 = 0xEB;
const JMP_NEAR: u8 = 0xE9;

const MBR_ENTRY: usize = 446;
const MBR_STATUS: usize = MBR_ENTRY;
const MBR_TYPE: usize = MBR_ENTRY + 4;
const MBR_LBA: usize = MBR_ENTRY + 8;
const MBR_BOOTABLE: u8 = 0x80;

const BPB_BYTES_PER_SECTOR: usize = 11;
const BPB_SECTORS_PER_CLUSTER: usize = 13;
const BPB_RESERVED: usize = 14;
const BPB_NUM_FATS: usize = 16;
const BPB_ROOT_ENTRIES: usize = 17;
const BPB_TOTAL16: usize = 19;
const BPB_FAT_SIZE16: usize = 22;
const BPB_TOTAL32: usize = 32;
const BPB_FAT_SIZE32: usize = 36;
const BPB_FS_VER: usize = 42;
const BPB_ROOT_CLUSTER: usize = 44;
const BPB_FS_INFO: usize = 48;
const BS_VOL_ID16: usize = 0x27;
const BS_VOL_ID32: usize = 0x43;
const BS_LABEL16: usize = 0x2B;
const BS_LABEL32: usize = 0x47;
const LABEL_LEN: usize = 11;
const BS_FS_TYPE16: usize = 0x36;
const BS_FS_TYPE32: usize = 0x52;
const DIR_ENTRY: u32 = 32;

/// Fewer clusters is FAT12.
const FAT12_CLUSTERS: u32 = 4085;
/// Fewer clusters is FAT16; this many or more is FAT32.
const FAT16_CLUSTERS: u32 = 65_525;

fn u16_at(b: &[u8; SECTOR], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8; SECTOR], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn signed(b: &[u8; SECTOR]) -> bool {
    u16_at(b, SIGNATURE) == BOOT_SIGNATURE
}

/// A FAT boot sector (jump, then `FAT` in the FAT16 or FAT32 FS-type field).
fn is_fat_boot_sector(b: &[u8; SECTOR]) -> bool {
    let fat_at = |at: usize| &b[at..at + 3] == b"FAT";
    matches!(b[BS_JUMP], JMP_SHORT | JMP_NEAR) && (fat_at(BS_FS_TYPE16) || fat_at(BS_FS_TYPE32))
}

/// Block 0's first partition. An exFAT boot sector in block 0 is `Exfat`.
/// A missing signature, a FAT boot sector in block 0 (a "superfloppy") or
/// an invalid first entry is `NoPartitionTable`.
pub fn first_partition(mbr: &[u8; SECTOR]) -> Result<Partition, Unsupported> {
    if &mbr[OEM_NAME] == EXFAT_OEM {
        return Err(Unsupported::Exfat);
    }
    let status = mbr[MBR_STATUS];
    let lba = u32_at(mbr, MBR_LBA);
    let kind = PartitionType::from_byte(mbr[MBR_TYPE]);
    match kind {
        Some(kind)
            if signed(mbr)
                && !is_fat_boot_sector(mbr)
                && status & !MBR_BOOTABLE == 0
                && lba != 0 =>
        {
            Ok(Partition { lba, kind })
        }
        _ => Err(Unsupported::NoPartitionTable),
    }
}

/// The partition's FS type and identity, from its boot sector and the
/// MBR's type.
pub fn boot_sector(
    bs: &[u8; SECTOR],
    kind: PartitionType,
) -> Result<(FsKind, VolumeId), Unsupported> {
    layout(bs, kind).map(|(l, id)| (l.kind, id))
}

/// Where a volume `boot_sector` accepts keeps its FAT, and how far its
/// clusters run. Only `layout` builds one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    kind: FsKind,
    /// FAT 1's first block, from the partition's start.
    fat: u32,
    clusters: u32,
    cluster_bytes: u32,
    /// FAT32's root directory cluster.
    root: Option<u32>,
}

/// A FAT entry, read as a link in a chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Next(u32),
    End,
    /// Free, reserved, bad, or past the last cluster.
    Broken,
}

const FAT16_EOC: u32 = 0xFFF8;
const FAT32_EOC: u32 = 0x0FFF_FFF8;
const FAT32_MASK: u32 = 0x0FFF_FFFF;
const FIRST_CLUSTER: u32 = 2;
const DIR_CLUSTER_HI: usize = 20;
const DIR_CLUSTER_LO: usize = 26;

impl Layout {
    pub const fn kind(&self) -> FsKind {
        self.kind
    }

    pub const fn clusters(&self) -> u32 {
        self.clusters
    }

    pub const fn cluster_bytes(&self) -> u32 {
        self.cluster_bytes
    }

    /// FAT32's root directory cluster; FAT16's root is outside the clusters.
    pub const fn root(&self) -> Option<u32> {
        self.root
    }

    /// A data cluster of this volume.
    pub const fn holds(&self, cluster: u32) -> bool {
        cluster >= FIRST_CLUSTER && cluster - FIRST_CLUSTER < self.clusters
    }

    fn entry_bytes(&self) -> u32 {
        match self.kind {
            FsKind::Fat16 => 2,
            FsKind::Fat32 => 4,
        }
    }

    /// The FAT 1 block holding `cluster`'s entry, from the partition's start.
    pub fn fat_block(&self, cluster: u32) -> u32 {
        let at = u64::from(cluster) * u64::from(self.entry_bytes()) / SECTOR as u64;
        u32::try_from(u64::from(self.fat) + at).unwrap_or(u32::MAX)
    }

    /// `cluster`'s entry in `block`, its `fat_block`.
    pub fn link(&self, block: &[u8; SECTOR], cluster: u32) -> Link {
        // Wrapping keeps the offset: 2^32 is a multiple of the sector.
        let at = cluster.wrapping_mul(self.entry_bytes()) as usize % SECTOR;
        let (v, eoc) = match self.kind {
            FsKind::Fat16 => (u32::from(u16_at(block, at)), FAT16_EOC),
            FsKind::Fat32 => (u32_at(block, at) & FAT32_MASK, FAT32_EOC),
        };
        if v >= eoc {
            Link::End
        } else if self.holds(v) {
            Link::Next(v)
        } else {
            Link::Broken
        }
    }

    /// The first cluster of the directory entry at `at` in `block` (0: none).
    pub fn start_cluster(&self, block: &[u8; SECTOR], at: usize) -> u32 {
        let at = (at % SECTOR) & !(DIR_ENTRY as usize - 1);
        let lo = u32::from(u16_at(block, at + DIR_CLUSTER_LO));
        match self.kind {
            FsKind::Fat16 => lo,
            FsKind::Fat32 => lo | u32::from(u16_at(block, at + DIR_CLUSTER_HI)) << 16,
        }
    }
}

/// `boot_sector`, with the volume's `Layout`.
pub fn layout(bs: &[u8; SECTOR], kind: PartitionType) -> Result<(Layout, VolumeId), Unsupported> {
    let byte = match kind {
        PartitionType::Fat(b) => b.get(),
        PartitionType::ExfatOrNtfs if &bs[OEM_NAME] == EXFAT_OEM => return Err(Unsupported::Exfat),
        PartitionType::ExfatOrNtfs => return Err(Unsupported::NotFat(EXFAT_TYPE)),
        PartitionType::Other(k) => return Err(Unsupported::NotFat(k.get())),
    };
    if !signed(bs) {
        return Err(Unsupported::NotFat(byte));
    }
    let bad = Err(Unsupported::BadBootSector);
    let spc = bs[BPB_SECTORS_PER_CLUSTER];
    let reserved = u32::from(u16_at(bs, BPB_RESERVED));
    let fats = bs[BPB_NUM_FATS];
    let fat_size = match u16_at(bs, BPB_FAT_SIZE16) {
        0 => u32_at(bs, BPB_FAT_SIZE32),
        n => u32::from(n),
    };
    let total = match u16_at(bs, BPB_TOTAL16) {
        0 => u32_at(bs, BPB_TOTAL32),
        n => u32::from(n),
    };
    let root_entries = u16_at(bs, BPB_ROOT_ENTRIES);
    let root_sectors = (u32::from(root_entries) * DIR_ENTRY).div_ceil(SECTOR as u32);
    // u64: FATs × FAT size overflows u32 on a hostile sector.
    let meta =
        u64::from(reserved) + u64::from(fats) * u64::from(fat_size) + u64::from(root_sectors);
    if usize::from(u16_at(bs, BPB_BYTES_PER_SECTOR)) != SECTOR
        || !spc.is_power_of_two()
        || reserved == 0
        || !matches!(fats, 1 | 2)
        || fat_size == 0
        || u64::from(total) <= meta
    {
        return bad;
    }
    // As embedded-sdmmc 0.10 counts: meta < total, so this fits in u32.
    let clusters = (total - meta as u32) / u32::from(spc);
    let (kind, serial_at, label_at, root) = if clusters < FAT12_CLUSTERS {
        return Err(Unsupported::NotFat(byte));
    } else if clusters < FAT16_CLUSTERS {
        if root_entries == 0 {
            return bad;
        }
        (FsKind::Fat16, BS_VOL_ID16, BS_LABEL16, None)
    } else {
        let root_cluster = u32_at(bs, BPB_ROOT_CLUSTER);
        let fs_info = u32::from(u16_at(bs, BPB_FS_INFO));
        if u16_at(bs, BPB_FS_VER) != 0
            || !(2..clusters.saturating_add(2)).contains(&root_cluster)
            || !(1..reserved).contains(&fs_info)
        {
            return bad;
        }
        (FsKind::Fat32, BS_VOL_ID32, BS_LABEL32, Some(root_cluster))
    };
    let mut label = [0; LABEL_LEN];
    label.copy_from_slice(&bs[label_at..label_at + LABEL_LEN]);
    let layout = Layout {
        kind,
        fat: reserved,
        clusters,
        cluster_bytes: u32::from(spc) * SECTOR as u32,
        root,
    };
    Ok((
        layout,
        VolumeId {
            serial: u32_at(bs, serial_at),
            label,
        },
    ))
}
