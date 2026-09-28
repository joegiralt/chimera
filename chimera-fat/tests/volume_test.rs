#[path = "common/image.rs"]
mod image;

use chimera_fat::volume::{FsKind, PartitionType, boot_sector, first_partition};
use chimera_hal::store::{Unsupported, VolumeId};
use core::num::NonZeroU8;
use core::ops::ControlFlow;
use embedded_sdmmc::{VolumeIdx, VolumeManager};
use image::{
    FixedTime, PART_LBA, RamDisk, exfat, exfat_superfloppy, fat16, fat32, fat32_layout,
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
    let cases: [(&str, RamDisk, Mutation); 11] = [
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
        ("FAT32 FSInfo 0", fat32(1), |b| b[48..50].fill(0)),
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
