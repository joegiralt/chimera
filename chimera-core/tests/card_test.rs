use chimera_core::storage::{Card, CardError, CardEvent, Outcome, after_error, after_mount};
use chimera_hal::store::{ByteSink, Dir, FileName, Store, StoreError, Unsupported, VolumeId};
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
        // A mount error that isn't NO CARD fails closed, even a file error.
        (Card::Ready(v), Err(StoreError::NotFound), io(Some(v)), None),
        (Card::Ready(v), Err(StoreError::Corrupt), io(Some(v)), None),
        (Card::Absent, Err(StoreError::Corrupt), io(None), None),
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
    for e in [S::NoCard, S::NotFound, S::Corrupt, S::Full] {
        assert_eq!(CardError::from_store(e), None, "{e:?}");
    }
    let mut card = vec![
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

fn ok(_: &mut MemStore, _: &chimera_core::storage::Ready) -> Result<(), StoreError> {
    Ok(())
}

fn event(o: Result<Outcome<(), StoreError>, StoreError>) -> CardEvent {
    o.expect("mounted").event
}

#[test]
fn op_error_fails_card() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let o = card
        .run(&mut s, |_, _| Err::<(), _>(StoreError::Io))
        .unwrap();
    assert_eq!(o.result, Err(StoreError::Io));
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
    let o = card.run(&mut s, |_, ready| Ok::<_, StoreError>(ready.volume()));
    assert_eq!(
        o,
        Ok(Outcome {
            event: CardEvent::Mounted,
            result: Ok(vol(1))
        })
    );
    assert_eq!(card, Card::Ready(vol(1)));
}

#[test]
fn a_failed_op_still_reports_the_swap() {
    for e in [StoreError::NotFound, StoreError::Corrupt, StoreError::Io] {
        let mut s = MemStore::new(1);
        let mut card = Card::new();
        assert_eq!(event(card.run(&mut s, ok)), CardEvent::Mounted);
        s.swap(2);
        let o = card.run(&mut s, |_, _| Err::<(), _>(e)).unwrap();
        assert_eq!(o.event, CardEvent::Swapped { old: vol(1) }, "{e:?}");
        assert_eq!(o.result, Err(e));
    }
}

#[test]
fn eject_then_insert() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let name = FileName::new(Dir::Chimera, b"KEEP", b"").unwrap();
    let saved = card.run(&mut s, |s, r| {
        s.make_dir(r.volume(), Dir::Chimera)?;
        s.write(r.volume(), name, &mut |b: &mut dyn ByteSink| b.put(b"hi"))
    });
    assert_eq!(saved.unwrap().result, Ok(2));
    s.eject();
    assert_eq!(card.run(&mut s, ok), Err(StoreError::NoCard));
    assert_eq!(card, Card::Absent);
    s.insert();
    let listed = card.run(&mut s, |s, r| {
        let mut files = Vec::new();
        s.list(r.volume(), Dir::Chimera, &mut |f, len| files.push((f, len)))?;
        Ok::<_, StoreError>(files)
    });
    // Absent forgets the last volume, so the same card back is `Mounted`.
    assert_eq!(
        listed,
        Ok(Outcome {
            event: CardEvent::Mounted,
            result: Ok(vec![(name, 2)])
        })
    );
    assert_eq!(card, Card::Ready(vol(1)));
}

#[test]
fn swap_between_ops() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    assert_eq!(event(card.run(&mut s, ok)), CardEvent::Mounted);
    assert_eq!(event(card.run(&mut s, ok)), CardEvent::Same);
    s.swap(2);
    assert_eq!(
        event(card.run(&mut s, ok)),
        CardEvent::Swapped { old: vol(1) }
    );
    assert_eq!(card, Card::Ready(vol(2)));
}
