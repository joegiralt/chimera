use chimera_hal::store::{StoreError, Unsupported, VolumeId};
use chimera_hal::testkit::{MemStore, store_suite};

#[test]
fn mem_store_passes_suite() {
    store_suite(&mut || MemStore::new(1), &mut |s| s.swap(2), &mut |_| {});
}

#[test]
fn messages() {
    let id = VolumeId {
        serial: 0,
        label: [b' '; 11],
    };
    assert_eq!(
        StoreError::Unsupported(Unsupported::Exfat).message(),
        "CARD IS EXFAT: FORMAT FAT32"
    );
    let all = [
        StoreError::NoCard,
        StoreError::Unsupported(Unsupported::Exfat),
        StoreError::Unsupported(Unsupported::NoPartitionTable),
        StoreError::Unsupported(Unsupported::NotFat(0x83)),
        StoreError::Unsupported(Unsupported::BadBootSector),
        StoreError::NotFound,
        StoreError::Full,
        StoreError::Timeout,
        StoreError::VolumeChanged(id),
        StoreError::Corrupt,
        StoreError::Io,
    ];
    for e in all {
        let m = e.message();
        assert!(!m.is_empty());
        assert!(
            m.bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b" :/".contains(&b)),
            "{m}"
        );
    }
}
