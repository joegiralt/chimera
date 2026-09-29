use chimera_hal::store::{Dir, FileName, StoreError, Unsupported, VolumeId};
use chimera_hal::testkit::{MemStore, store_suite};

#[test]
fn mem_store_passes_suite() {
    store_suite(
        &mut || MemStore::new(1),
        &mut |s| s.swap(2),
        &mut |s| s.eject(),
        &mut |_| {},
    );
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

#[test]
fn file_names() {
    let n = |stem: &[u8], ext: &[u8]| FileName::new(Dir::Sounds, stem, ext);
    let ok = n(b"PATCH01", b"SND").unwrap();
    assert_eq!(
        (ok.dir(), ok.stem(), ok.ext()),
        (Dir::Sounds, &b"PATCH01"[..], &b"SND"[..])
    );
    assert!(n(b"12345678", b"ABC").is_some(), "full 8.3");
    assert!(
        n(b"A", b"").is_some_and(|f| f.ext().is_empty()),
        "empty extension"
    );
    assert!(n(b"abc", b"BIN").is_none(), "lowercase");
    assert!(n(b"ABC", b"bin").is_none(), "lowercase extension");
    assert!(n(b"", b"BIN").is_none(), "empty stem");
    assert!(n(b"123456789", b"BIN").is_none(), "9-byte stem");
    assert!(n(b"ABC", b"ABCD").is_none(), "4-byte extension");
    assert!(n(b"A B", b"BIN").is_none(), "space");
    assert!(n(b"A.B", b"BIN").is_none(), "dot");
}
