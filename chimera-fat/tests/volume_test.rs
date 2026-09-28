#[path = "common/image.rs"]
mod image;

use chimera_fat::volume::{FsKind, Partition, boot_sector, first_partition};
use chimera_hal::store::{Unsupported, VolumeId};
use embedded_sdmmc::{VolumeIdx, VolumeManager};
use image::{FixedTime, RamDisk, exfat, fat16, fat32, superfloppy};

/// The first partition, then its boot sector.
fn parse(disk: &RamDisk) -> Result<(FsKind, VolumeId), Unsupported> {
    let p = first_partition(&disk.block(0))?;
    boot_sector(&disk.block(p.lba), p.kind)
}

#[test]
fn fat16_serial_and_label() {
    let disk = fat16(16_384, 0xDEAD_BEEF);
    assert_eq!(
        first_partition(&disk.block(0)),
        Ok(Partition {
            lba: 2048,
            kind: 0x0E
        })
    );
    assert_eq!(
        parse(&disk),
        Ok((
            FsKind::Fat16,
            VolumeId {
                serial: 0xDEAD_BEEF,
                label: *b"CHIMERA    "
            }
        ))
    );
}

#[test]
fn fat32_serial_and_label() {
    let disk = fat32(0x0BAD_F00D);
    assert_eq!(
        parse(&disk),
        Ok((
            FsKind::Fat32,
            VolumeId {
                serial: 0x0BAD_F00D,
                label: *b"CHIMERA    "
            }
        ))
    );
}

#[test]
fn exfat_is_unsupported() {
    let disk = exfat();
    let p = first_partition(&disk.block(0)).unwrap();
    assert_eq!(p.kind, 0x07);
    assert_eq!(
        boot_sector(&disk.block(p.lba), p.kind),
        Err(Unsupported::Exfat)
    );
}

#[test]
fn type_07_without_exfat_oem_is_not_fat() {
    let disk = fat16(16_384, 1);
    assert_eq!(
        boot_sector(&disk.block(2048), 0x07),
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
    let mut mbr = disk.block(0);
    mbr[446 + 4] = 0x83;
    let p = first_partition(&mbr).unwrap();
    assert_eq!(p.kind, 0x83);
    assert_eq!(
        boot_sector(&disk.block(p.lba), p.kind),
        Err(Unsupported::NotFat(0x83))
    );
}

#[test]
fn fat_type_without_boot_signature_is_not_fat() {
    assert_eq!(boot_sector(&[0; 512], 0x0C), Err(Unsupported::NotFat(0x0C)));
}

#[test]
fn every_fat_partition_type_is_accepted() {
    let disk = fat16(16_384, 7);
    for kind in [0x04, 0x06, 0x0E, 0x0B, 0x0C] {
        assert_eq!(
            boot_sector(&disk.block(2048), kind).map(|r| r.0),
            Ok(FsKind::Fat16)
        );
    }
}

#[test]
fn images_open_in_embedded_sdmmc() {
    fn mkdir(disk: RamDisk) {
        let mgr: VolumeManager<_, _> = VolumeManager::new(disk, FixedTime);
        let vol = mgr.open_volume(VolumeIdx(0)).unwrap();
        let mut root = vol.open_root_dir().unwrap();
        root.make_dir_in_dir("CHIMERA").unwrap();
        root.change_dir("CHIMERA").unwrap();
    }
    mkdir(fat16(16_384, 1));
    mkdir(fat32(2));
}
