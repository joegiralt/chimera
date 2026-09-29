//! The MBR, the partition boot sector and the volume's `Layout`: where the
//! FATs, the root and the clusters are, and FAT entry reads and writes.
//! Pure; never panics.

use chimera_hal::store::{Unsupported, VolumeId};
use core::num::NonZeroU8;
use core::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsKind {
    Fat16,
    Fat32,
}

impl FsKind {
    /// Bytes per FAT entry.
    const fn entry_bytes(self) -> u32 {
        match self {
            FsKind::Fat16 => 2,
            FsKind::Fat32 => 4,
        }
    }
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
    /// The entry's sector count: the volume must fit in it.
    pub blocks: u32,
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
const MBR_BLOCKS: usize = MBR_ENTRY + 12;
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
            Ok(Partition {
                lba,
                blocks: u32_at(mbr, MBR_BLOCKS),
                kind,
            })
        }
        _ => Err(Unsupported::NoPartitionTable),
    }
}

/// The partition's FS type and identity, from its boot sector and the
/// MBR's type, with no bound on the volume's size.
pub fn boot_sector(
    bs: &[u8; SECTOR],
    kind: PartitionType,
) -> Result<(FsKind, VolumeId), Unsupported> {
    let part = Partition {
        lba: 0,
        blocks: u32::MAX,
        kind,
    };
    layout(bs, part).map(|(l, id)| (l.kind, id))
}

/// Where the root directory is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    /// FAT16: a fixed region before the clusters.
    Fixed { first: u32, blocks: u32 },
    /// FAT32: a cluster chain.
    Cluster(u32),
}

/// Where a volume `layout` accepts keeps its FATs, root and clusters. Block
/// numbers are from the partition's start. Only `layout` builds one, so
/// every region lies below `blocks` and every FAT holds every cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    kind: FsKind,
    blocks: u32,
    /// FAT 1's first block.
    fat: u32,
    fat_blocks: u32,
    fats: u8,
    /// Cluster 2's first block.
    data: u32,
    clusters: u32,
    blocks_per_cluster: u32,
    root: Root,
    fs_info: Option<u32>,
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
/// FAT32's most clusters: the highest, 0x0FFF_FFF6, stays below the bad mark.
const FAT32_MAX_CLUSTERS: u32 = 0x0FFF_FFF5;
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

    /// The volume's blocks: every block it uses is below this.
    pub const fn blocks(&self) -> u32 {
        self.blocks
    }

    pub const fn blocks_per_cluster(&self) -> u32 {
        self.blocks_per_cluster
    }

    pub const fn cluster_bytes(&self) -> u32 {
        self.blocks_per_cluster * SECTOR as u32
    }

    pub const fn root(&self) -> Root {
        self.root
    }

    /// FAT32's FSInfo block.
    pub const fn fs_info(&self) -> Option<u32> {
        self.fs_info
    }

    /// `cluster`'s first block; `None` unless `holds(cluster)`.
    pub fn cluster_block(&self, cluster: u32) -> Option<u32> {
        // `layout` checked data + clusters × blocks per cluster ≤ blocks.
        self.holds(cluster)
            .then(|| self.data + (cluster - FIRST_CLUSTER) * self.blocks_per_cluster)
    }

    /// `fat1_block`, a FAT 1 block, in every FAT, FAT 1 first.
    pub fn fat_copies(&self, fat1_block: u32) -> impl Iterator<Item = u32> + use<> {
        let size = self.fat_blocks;
        (0..u32::from(self.fats)).map(move |i| fat1_block + i * size)
    }

    /// A data cluster of this volume.
    pub const fn holds(&self, cluster: u32) -> bool {
        cluster >= FIRST_CLUSTER && cluster - FIRST_CLUSTER < self.clusters
    }

    /// The FAT 1 block holding `cluster`'s entry, from the partition's start.
    pub fn fat_block(&self, cluster: u32) -> u32 {
        let at = u64::from(cluster) * u64::from(self.kind.entry_bytes()) / SECTOR as u64;
        u32::try_from(u64::from(self.fat) + at).unwrap_or(u32::MAX)
    }

    /// Where `cluster`'s entry sits in its `fat_block`.
    fn entry_at(&self, cluster: u32) -> usize {
        // Wrapping keeps the offset: 2^32 is a multiple of the sector.
        cluster.wrapping_mul(self.kind.entry_bytes()) as usize % SECTOR
    }

    /// `cluster`'s raw entry in `block`, its `fat_block`; FAT32's top
    /// nibble is masked off. 0 is free.
    pub fn entry(&self, block: &[u8; SECTOR], cluster: u32) -> u32 {
        let at = self.entry_at(cluster);
        match self.kind {
            FsKind::Fat16 => u32::from(u16_at(block, at)),
            FsKind::Fat32 => u32_at(block, at) & FAT32_MASK,
        }
    }

    /// Sets `cluster`'s entry in `block` to `v`. FAT32 keeps the entry's
    /// top nibble, as the spec asks.
    pub fn put_entry(&self, block: &mut [u8; SECTOR], cluster: u32, v: u32) {
        let at = self.entry_at(cluster);
        match self.kind {
            FsKind::Fat16 => block[at..at + 2].copy_from_slice(&(v as u16).to_le_bytes()),
            FsKind::Fat32 => {
                let v = (u32_at(block, at) & !FAT32_MASK) | (v & FAT32_MASK);
                block[at..at + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }

    /// The end-of-chain mark this FAT writes.
    pub const fn end_mark(&self) -> u32 {
        match self.kind {
            FsKind::Fat16 => 0xFFFF,
            FsKind::Fat32 => FAT32_MASK,
        }
    }

    /// `cluster`'s entry in `block`, its `fat_block`.
    pub fn link(&self, block: &[u8; SECTOR], cluster: u32) -> Link {
        self.as_link(self.entry(block, cluster))
    }

    /// A raw entry, read as a link.
    pub fn as_link(&self, v: u32) -> Link {
        let eoc = match self.kind {
            FsKind::Fat16 => FAT16_EOC,
            FsKind::Fat32 => FAT32_EOC,
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

/// `boot_sector`, with the volume's `Layout`. A volume larger than `part`
/// is `BadBootSector`.
pub fn layout(bs: &[u8; SECTOR], part: Partition) -> Result<(Layout, VolumeId), Unsupported> {
    let byte = match part.kind {
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
        || total > part.blocks
    {
        return bad;
    }
    // As embedded-sdmmc 0.10 counts: meta < total, so this fits in u32.
    let (data, spc) = (meta as u32, u32::from(spc));
    let clusters = (total - data) / spc;
    let (kind, serial_at, label_at, root, fs_info) = if clusters < FAT12_CLUSTERS {
        return Err(Unsupported::NotFat(byte));
    } else if clusters < FAT16_CLUSTERS {
        if root_entries == 0 {
            return bad;
        }
        let root = Root::Fixed {
            first: data - root_sectors,
            blocks: root_sectors,
        };
        (FsKind::Fat16, BS_VOL_ID16, BS_LABEL16, root, None)
    } else {
        let root_cluster = u32_at(bs, BPB_ROOT_CLUSTER);
        let fs_info = u32::from(u16_at(bs, BPB_FS_INFO));
        if u16_at(bs, BPB_FS_VER) != 0
            || !(2..clusters.saturating_add(2)).contains(&root_cluster)
            || !(1..reserved).contains(&fs_info)
            || clusters > FAT32_MAX_CLUSTERS
        {
            return bad;
        }
        let root = Root::Cluster(root_cluster);
        (FsKind::Fat32, BS_VOL_ID32, BS_LABEL32, root, Some(fs_info))
    };
    // Every cluster's entry, the two reserved ones included, is in each FAT.
    let entries = (u64::from(clusters) + 2) * u64::from(kind.entry_bytes());
    if u64::from(fat_size) * (SECTOR as u64) < entries {
        return bad;
    }
    let mut label = [0; LABEL_LEN];
    label.copy_from_slice(&bs[label_at..label_at + LABEL_LEN]);
    let layout = Layout {
        kind,
        blocks: total,
        fat: reserved,
        fat_blocks: fat_size,
        fats,
        data,
        clusters,
        blocks_per_cluster: spc,
        root,
        fs_info,
    };
    Ok((
        layout,
        VolumeId {
            serial: u32_at(bs, serial_at),
            label,
        },
    ))
}
