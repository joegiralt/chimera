use chimera_core::storage::{Card, CardError, CardEvent, after_error, after_mount};
use chimera_hal::store::{StoreError, Unsupported, VolumeId};
use chimera_hal::testkit::MemStore;

fn vol(serial: u32) -> VolumeId {
    VolumeId {
        serial,
        label: *b"MEMSTORE   ",
    }
}

#[test]
fn transition_table() {
    let (v, w) = (vol(1), vol(2));
    let failed = |last| Card::Failed {
        err: CardError::Timeout,
        last,
    };
    let io = |last| Card::Failed {
        err: CardError::Io,
        last,
    };
    let cases = [
        (
            Card::Absent,
            Ok(v),
            Card::Ready(v),
            Some(CardEvent::Mounted),
        ),
        (Card::Ready(v), Ok(v), Card::Ready(v), Some(CardEvent::Same)),
        (
            Card::Ready(v),
            Ok(w),
            Card::Ready(w),
            Some(CardEvent::Swapped { old: v }),
        ),
        (
            failed(Some(v)),
            Ok(v),
            Card::Ready(v),
            Some(CardEvent::Same),
        ),
        (
            failed(Some(v)),
            Ok(w),
            Card::Ready(w),
            Some(CardEvent::Swapped { old: v }),
        ),
        (
            failed(None),
            Ok(v),
            Card::Ready(v),
            Some(CardEvent::Mounted),
        ),
        (Card::Absent, Err(StoreError::NoCard), Card::Absent, None),
        (Card::Ready(v), Err(StoreError::NoCard), Card::Absent, None),
        (failed(Some(v)), Err(StoreError::NoCard), Card::Absent, None),
        (failed(None), Err(StoreError::NoCard), Card::Absent, None),
        (Card::Ready(v), Err(StoreError::Io), io(Some(v)), None),
        (failed(Some(v)), Err(StoreError::Io), io(Some(v)), None),
        (failed(None), Err(StoreError::Io), io(None), None),
        (Card::Absent, Err(StoreError::Io), io(None), None),
    ];
    for (card, r, want, event) in cases {
        assert_eq!(after_mount(card, r), (want, event), "{card:?} + {r:?}");
    }
}

#[test]
fn file_errors_leave_the_card() {
    let v = vol(1);
    assert_eq!(
        after_error(Card::Ready(v), StoreError::NotFound),
        Card::Ready(v)
    );
    assert_eq!(
        after_error(Card::Ready(v), StoreError::Corrupt),
        Card::Ready(v)
    );
}

#[test]
fn from_store_table() {
    use StoreError as S;
    let unsupported = [
        Unsupported::Exfat,
        Unsupported::NoPartitionTable,
        Unsupported::NotFat(0x07),
        Unsupported::BadBootSector,
        Unsupported::FatNotMirrored,
    ];
    for e in [S::NoCard, S::NotFound, S::Corrupt] {
        assert_eq!(CardError::from_store(e), None, "{e:?}");
    }
    let mut card = vec![
        (S::Full, CardError::Full),
        (S::Timeout, CardError::Timeout),
        (S::Io, CardError::Io),
        (S::VolumeChanged(vol(9)), CardError::VolumeChanged(vol(9))),
    ];
    card.extend(unsupported.map(|u| (S::Unsupported(u), CardError::Unsupported(u))));
    for (e, c) in card {
        assert_eq!(CardError::from_store(e), Some(c), "{e:?}");
        assert_eq!(StoreError::from(c), e, "round trip {c:?}");
    }
}

#[test]
fn op_error_fails_card() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let r = card.run(&mut s, |_, _| Err::<(), _>(StoreError::Io));
    assert_eq!(r, Err(StoreError::Io));
    assert_eq!(
        card,
        Card::Failed {
            err: CardError::Io,
            last: Some(vol(1))
        }
    );
}

#[test]
fn ready_lends_the_mounted_volume() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let r = card.run(&mut s, |_, ready| Ok::<_, StoreError>(ready.volume()));
    assert_eq!(r, Ok((vol(1), CardEvent::Mounted)));
    assert_eq!(card, Card::Ready(vol(1)));
}

#[test]
fn eject_then_insert() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let ok = |_: &mut MemStore, _: &_| Ok::<_, StoreError>(());
    assert_eq!(card.run(&mut s, ok), Ok(((), CardEvent::Mounted)));
    s.eject();
    assert_eq!(card.run(&mut s, ok), Err(StoreError::NoCard));
    assert_eq!(card, Card::Absent);
    // MemStore has no insert: the same serial in the slot again.
    s.swap(1);
    assert_eq!(card.run(&mut s, ok), Ok(((), CardEvent::Mounted)));
    assert_eq!(card, Card::Ready(vol(1)));
}

#[test]
fn swap_between_ops() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let ok = |_: &mut MemStore, _: &_| Ok::<_, StoreError>(());
    assert_eq!(card.run(&mut s, ok), Ok(((), CardEvent::Mounted)));
    assert_eq!(card.run(&mut s, ok), Ok(((), CardEvent::Same)));
    s.swap(2);
    assert_eq!(
        card.run(&mut s, ok),
        Ok(((), CardEvent::Swapped { old: vol(1) }))
    );
    assert_eq!(card, Card::Ready(vol(2)));
}
