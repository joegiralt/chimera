#[path = "common/image.rs"]
mod image;

use chimera_fat::FixedTime;
use chimera_fat::volume::{
    FsKind, Layout, Link, PartitionType, Root, boot_sector, first_partition, layout,
};
use chimera_hal::store::{Unsupported, VolumeId};
use core::num::NonZeroU8;
use core::ops::ControlFlow;
use embedded_sdmmc::{VolumeIdx, VolumeManager};
use image::{
    PART_LBA, RamDisk, exfat, exfat_superfloppy, fat16, fat32, fat32_layout, layout_of,
    superfloppy, with_clusters,
};

/// The first partition, then its boot sector.
fn parse(disk: &RamDisk) -> Result<(FsKind, VolumeId), Unsupported> {
    let p = first_partition(&disk.block(0))?;
    boot_sector(&disk.block(p.lba), p.kind)
}

/// `disk`'s MBR with the first entry's type byte set to `kind`, parsed.
fn kind_as(disk: &RamDisk, kind: u8) -> PartitionType {
    let mut mbr = disk.block(0);
    mbr[446 + 4] = kind;
    first_partition(&mbr).unwrap().kind
}

fn id(serial: u32) -> VolumeId {
    VolumeId {
        serial,
        label: *b"CHIMERA    ",
    }
}

#[test]
fn fat16_serial_and_label() {
    let disk = fat16(16_384, 0xDEAD_BEEF);
    let p = first_partition(&disk.block(0)).unwrap();
    assert_eq!(p.lba, PART_LBA);
    assert!(matches!(p.kind, PartitionType::Fat(_)));
    assert_eq!(parse(&disk), Ok((FsKind::Fat16, id(0xDEAD_BEEF))));
}

#[test]
fn fat32_serial_and_label() {
    assert_eq!(
        parse(&fat32(0x0BAD_F00D)),
        Ok((FsKind::Fat32, id(0x0BAD_F00D)))
    );
}

#[test]
fn exfat_is_unsupported() {
    let disk = exfat();
    let p = first_partition(&disk.block(0)).unwrap();
    assert_eq!(p.kind, PartitionType::ExfatOrNtfs);
    assert_eq!(
        boot_sector(&disk.block(p.lba), p.kind),
        Err(Unsupported::Exfat)
    );
}

#[test]
fn exfat_superfloppy_is_exfat() {
    assert_eq!(
        first_partition(&exfat_superfloppy().block(0)),
        Err(Unsupported::Exfat)
    );
}

#[test]
fn type_07_without_exfat_oem_is_not_fat() {
    let disk = fat16(16_384, 1);
    assert_eq!(
        boot_sector(&disk.block(PART_LBA), PartitionType::ExfatOrNtfs),
        Err(Unsupported::NotFat(0x07))
    );
}

#[test]
fn superfloppy_is_no_partition_table() {
    assert_eq!(
        first_partition(&superfloppy().block(0)),
        Err(Unsupported::NoPartitionTable)
    );
}

/// A boot sector whose boot code happens to form a plausible partition entry
/// is still no partition table.
#[test]
fn superfloppy_with_plausible_entry_is_no_partition_table() {
    let mut b = superfloppy().block(0);
    b[446 + 4] = 0x0E;
    b[446 + 8..446 + 12].copy_from_slice(&2048u32.to_le_bytes());
    assert_eq!(first_partition(&b), Err(Unsupported::NoPartitionTable));
}

#[test]
fn bad_signature_is_no_partition_table() {
    assert_eq!(
        first_partition(&[0; 512]),
        Err(Unsupported::NoPartitionTable)
    );
}

#[test]
fn empty_first_entry_is_no_partition_table() {
    let mut b = fat16(16_384, 1).block(0);
    b[446 + 4] = 0;
    assert_eq!(first_partition(&b), Err(Unsupported::NoPartitionTable));
}

#[test]
fn bad_status_is_no_partition_table() {
    let mut b = fat16(16_384, 1).block(0);
    b[446] = 0x12;
    assert_eq!(first_partition(&b), Err(Unsupported::NoPartitionTable));
}

#[test]
fn other_partition_type() {
    let disk = fat16(16_384, 1);
    let kind = kind_as(&disk, 0x83);
    assert_eq!(kind, PartitionType::Other(NonZeroU8::new(0x83).unwrap()));
    assert_eq!(
        boot_sector(&disk.block(PART_LBA), kind),
        Err(Unsupported::NotFat(0x83))
    );
}

#[test]
fn fat_type_without_boot_signature_is_not_fat() {
    let kind = kind_as(&fat16(16_384, 1), 0x0C);
    assert_eq!(boot_sector(&[0; 512], kind), Err(Unsupported::NotFat(0x0C)));
}

#[test]
fn every_fat_partition_type_is_accepted() {
    let disk = fat16(16_384, 7);
    for byte in [0x04, 0x06, 0x0E, 0x0B, 0x0C] {
        let kind = kind_as(&disk, byte);
        assert!(matches!(kind, PartitionType::Fat(t) if t.get() == byte));
        assert_eq!(
            boot_sector(&disk.block(PART_LBA), kind).map(|r| r.0),
            Ok(FsKind::Fat16)
        );
    }
}

#[test]
fn corrupt_boot_sector_is_rejected_not_panic() {
    type Mutation = fn(&mut [u8; 512]);
    let cases: [(&str, RamDisk, Mutation); 13] = [
        ("bytes per sector 1024", fat16(16_384, 1), |b| {
            b[11..13].copy_from_slice(&1024u16.to_le_bytes())
        }),
        ("sectors per cluster 0", fat16(16_384, 1), |b| b[13] = 0),
        ("sectors per cluster 3", fat16(16_384, 1), |b| b[13] = 3),
        ("reserved 0", fat16(16_384, 1), |b| b[14..16].fill(0)),
        ("FAT count 0", fat16(16_384, 1), |b| b[16] = 0),
        ("FAT count 3", fat16(16_384, 1), |b| b[16] = 3),
        ("FAT size 0", fat16(16_384, 1), |b| {
            b[22..24].fill(0);
            b[36..40].fill(0);
        }),
        ("total below the metadata", fat16(16_384, 1), |b| {
            b[19..21].fill(0);
            b[32..36].copy_from_slice(&10u32.to_le_bytes());
        }),
        ("FAT32 FS version 1", fat32(1), |b| {
            b[42..44].copy_from_slice(&1u16.to_le_bytes())
        }),
        ("FAT32 root cluster 0", fat32(1), |b| b[44..48].fill(0)),
        ("FAT32 FSInfo in the data area", fat32(1), |b| {
            b[48..50].copy_from_slice(&32u16.to_le_bytes())
        }),
        ("FAT too small for the clusters", fat16(16_384, 1), |b| {
            b[22..24].copy_from_slice(&10u16.to_le_bytes())
        }),
        ("FAT32 clusters past 0x0FFF_FFF5", fat32(1), |b| {
            b[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
            b[36..40].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        }),
    ];
    for (what, disk, mutate) in cases {
        let kind = first_partition(&disk.block(0)).unwrap().kind;
        let mut bs = disk.block(PART_LBA);
        assert!(boot_sector(&bs, kind).is_ok(), "{what}: valid before");
        mutate(&mut bs);
        assert_eq!(
            boot_sector(&bs, kind),
            Err(Unsupported::BadBootSector),
            "{what}"
        );
    }

    let fat12 = with_clusters(4_084, 1);
    let kind = kind_as(&fat12, 0x06);
    assert_eq!(
        boot_sector(&fat12.block(PART_LBA), kind),
        Err(Unsupported::NotFat(0x06))
    );
}

/// The count decides, not `BPB_FATSz16 == 0`: `embedded-sdmmc` reads a
/// FAT32 layout under 65 525 clusters as FAT16, with no root directory.
#[test]
fn fat32_layout_below_fat32_count_is_bad() {
    assert_eq!(
        parse(&fat32_layout(65_524, 1)),
        Err(Unsupported::BadBootSector)
    );
}

#[test]
fn fat32_type_byte_with_fat16_layout_is_fat16() {
    let disk = fat16(16_384, 0xCAFE_0016);
    let kind = kind_as(&disk, 0x0C);
    assert_eq!(
        boot_sector(&disk.block(PART_LBA), kind),
        Ok((FsKind::Fat16, id(0xCAFE_0016)))
    );
}

/// Makes `CHIMERA` through `embedded-sdmmc`, finds it again with
/// `iterate_dir`, and returns the FAT type the library used, read from the
/// width of the FAT entry it wrote for the new directory's cluster.
fn sdmmc_kind(disk: RamDisk) -> FsKind {
    let mgr: VolumeManager<_, _> = VolumeManager::new(disk, FixedTime);
    {
        let vol = mgr.open_volume(VolumeIdx(0)).unwrap();
        let root = vol.open_root_dir().unwrap();
        root.make_dir_in_dir("CHIMERA").unwrap();
        let mut found = false;
        root.iterate_dir(|e| {
            found |= e.name.to_string() == "CHIMERA";
            ControlFlow::Continue(())
        })
        .unwrap();
        assert!(found, "CHIMERA not listed");
    }
    let (disk, _) = mgr.free();
    let bs = disk.block(PART_LBA);
    let fat = disk.block(PART_LBA + u32::from(u16::from_le_bytes([bs[14], bs[15]])));
    // FAT16 gives the new directory cluster 2 (a 16-bit entry at byte 4);
    // FAT32 gives it cluster 3, after the root (a 32-bit entry at byte 12).
    if u32::from_le_bytes(fat[12..16].try_into().unwrap()) & 0x0FFF_FFFF >= 0x0FFF_FFF8 {
        FsKind::Fat32
    } else {
        assert!(
            u16::from_le_bytes([fat[4], fat[5]]) >= 0xFFF8,
            "no FAT entry written"
        );
        FsKind::Fat16
    }
}

#[test]
fn classification_matches_embedded_sdmmc() {
    for clusters in [4_085, 65_524, 65_525] {
        let disk = with_clusters(clusters, 1);
        let (ours, _) = parse(&disk).unwrap();
        assert_eq!(ours, sdmmc_kind(disk), "{clusters} clusters");
    }
}

#[test]
fn images_open_in_embedded_sdmmc() {
    assert_eq!(sdmmc_kind(fat16(16_384, 1)), FsKind::Fat16);
    assert_eq!(sdmmc_kind(fat32(2)), FsKind::Fat32);
}

/// Cluster `c`'s entry set to `v` in a FAT block, read back as a link.
fn link_of(l: &Layout, c: u32, v: u32) -> Link {
    let mut block = [0; 512];
    let at = (c * if l.kind() == FsKind::Fat16 { 2 } else { 4 }) as usize % 512;
    match l.kind() {
        FsKind::Fat16 => block[at..at + 2].copy_from_slice(&(v as u16).to_le_bytes()),
        FsKind::Fat32 => block[at..at + 4].copy_from_slice(&v.to_le_bytes()),
    }
    l.link(&block, c)
}

#[test]
fn layout_reads_links_only_onto_the_volume() {
    let l = layout_of(&with_clusters(4_094, 1));
    assert_eq!(
        (l.kind(), l.clusters(), l.cluster_bytes(), l.root()),
        (
            FsKind::Fat16,
            4_094,
            512,
            Root::Fixed {
                first: 33,
                blocks: 32
            }
        )
    );
    assert_eq!(l.fat_block(255), 1);
    assert_eq!(l.fat_block(256), 2);
    for (v, link) in [
        (0, Link::Broken),
        (1, Link::Broken),
        (2, Link::Next(2)),
        (4_095, Link::Next(4_095)),
        (4_096, Link::Broken),
        (0xFFF7, Link::Broken),
        (0xFFF8, Link::End),
        (0xFFFF, Link::End),
    ] {
        assert_eq!(link_of(&l, 300, v), link, "FAT16 {v:#x}");
    }

    let l = layout_of(&fat32(1));
    assert_eq!((l.kind(), l.root()), (FsKind::Fat32, Root::Cluster(2)));
    assert_eq!(l.fat_block(127), 32);
    assert_eq!(l.fat_block(128), 33);
    for (v, link) in [
        (0, Link::Broken),
        (1, Link::Broken),
        (0xF000_0003, Link::Next(3)),
        (66_001, Link::Next(66_001)),
        (66_002, Link::Broken),
        (0x0FFF_FFF7, Link::Broken),
        (0x0FFF_FFF8, Link::End),
    ] {
        assert_eq!(link_of(&l, 700, v), link, "FAT32 {v:#x}");
    }
}

#[test]
fn start_cluster_takes_the_high_half_on_fat32_only() {
    let mut block = [0; 512];
    block[64 + 20..64 + 22].copy_from_slice(&1u16.to_le_bytes());
    block[64 + 26..64 + 28].copy_from_slice(&5u16.to_le_bytes());
    assert_eq!(layout_of(&fat16(16_384, 1)).start_cluster(&block, 64), 5);
    assert_eq!(layout_of(&fat32(1)).start_cluster(&block, 64), 0x1_0005);
}

/// The MBR entry's count bounds the volume: a BPB one block larger is bad.
#[test]
fn partition_bounds_the_volume() {
    for disk in [fat16(16_384, 1), fat32(1)] {
        let p = first_partition(&disk.block(0)).unwrap();
        let mut bs = disk.block(PART_LBA);
        let total = u32::from_le_bytes(bs[32..36].try_into().unwrap());
        assert_eq!(total, p.blocks, "the builder fills its partition");
        assert_eq!(layout(&bs, p).unwrap().0.blocks(), total);
        bs[32..36].copy_from_slice(&(total + 1).to_le_bytes());
        assert_eq!(layout(&bs, p), Err(Unsupported::BadBootSector));
    }
}

#[test]
fn layout_regions() {
    // FAT16 over 14 336 blocks: 1 reserved, two 57-block FATs, a 32-block
    // root, then 14 189 one-block clusters.
    let l = layout_of(&fat16(16_384, 1));
    assert_eq!(
        (l.blocks(), l.clusters(), l.blocks_per_cluster()),
        (14_336, 14_189, 1)
    );
    assert_eq!(l.fat_copies(1).collect::<Vec<_>>(), [1, 58]);
    assert_eq!(l.fat_copies(57).collect::<Vec<_>>(), [57, 114]);
    assert_eq!(
        l.root(),
        Root::Fixed {
            first: 115,
            blocks: 32
        }
    );
    assert_eq!(l.fs_info(), None);
    assert_eq!(l.cluster_block(2), Some(147));
    assert_eq!(l.cluster_block(14_190), Some(14_335));
    assert_eq!(l.cluster_block(14_191), None);
    assert_eq!(l.cluster_block(1), None);
    assert_eq!(l.cluster_block(0), None);

    // FAT32 of 66 000 clusters: 32 reserved, two 516-block FATs.
    let l = layout_of(&fat32(1));
    assert_eq!((l.blocks(), l.clusters()), (67_064, 66_000));
    assert_eq!(l.fat_copies(32).collect::<Vec<_>>(), [32, 548]);
    assert_eq!(l.root(), Root::Cluster(2));
    assert_eq!(l.fs_info(), Some(1));
    assert_eq!(l.cluster_block(2), Some(1_064));
    assert_eq!(l.cluster_block(66_001), Some(67_063));
    assert_eq!(l.cluster_block(66_002), None);
    assert_eq!(l.cluster_block(1), None);
    assert_eq!(l.cluster_block(u32::MAX), None);
}

#[test]
fn entry_widths() {
    let l = layout_of(&fat16(16_384, 1));
    let mut block = [0xAA; 512];
    l.put_entry(&mut block, 300, 0x1234);
    assert_eq!(block[88..90], [0x34, 0x12], "cluster 300: byte 600 mod 512");
    assert_eq!((block[87], block[90]), (0xAA, 0xAA), "neighbours kept");
    assert_eq!(l.entry(&block, 300), 0x1234);
    assert_eq!(l.link(&block, 300), Link::Next(0x1234));

    let l = layout_of(&fat32(1));
    let mut block = [0; 512];
    block[240..244].copy_from_slice(&0xF000_0000u32.to_le_bytes());
    l.put_entry(&mut block, 700, 0x5ABC_DEF0);
    assert_eq!(
        u32::from_le_bytes(block[240..244].try_into().unwrap()),
        0xFABC_DEF0,
        "cluster 700: byte 2 800 mod 512; the top nibble is the card's"
    );
    l.put_entry(&mut block, 700, 5);
    assert_eq!(
        u32::from_le_bytes(block[240..244].try_into().unwrap()),
        0xF000_0005
    );
    assert_eq!(l.entry(&block, 700), 5, "masked to 28 bits");
}

/// BPB_FSInfo 0 or 0xFFFF: the volume has no FSInfo, and still mounts.
#[test]
fn no_fs_info_mounts_without_one() {
    let disk = fat32(1);
    let p = first_partition(&disk.block(0)).unwrap();
    for v in [0u16, 0xFFFF] {
        let mut bs = disk.block(PART_LBA);
        bs[48..50].copy_from_slice(&v.to_le_bytes());
        let (l, _) = layout(&bs, p).unwrap();
        assert_eq!(l.fs_info(), None, "BPB_FSInfo {v:#x}");
    }
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "FAT16 entry")]
fn fat16_entry_over_16_bits_is_a_bug() {
    let l = layout_of(&fat16(16_384, 1));
    l.put_entry(&mut [0; 512], 300, 0x1_0000);
}
