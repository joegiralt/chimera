//! Lock-free SPSC note queue from the UI/input thread to audio
//! (instrument-core spec § Threading).

use chimera_core::note_queue::{
    InEvent, MAX_NOTE_SOURCES, NOTE_OFF_RESERVE, NOTE_QUEUE_LEN, NoteEvent, NoteKind, NoteQueue,
    NoteSources, RealTime, SourceId,
};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_midi::stream::{Ctrl, Voice};
use chimera_midi::value::{Bend, U7, U14};

fn ev(ch: u8, note: u8, vel: u8) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::new(ch).unwrap(),
        note: MidiNote::new(note).unwrap(),
        kind: match Velocity::new(vel) {
            Some(v) => NoteKind::On(v),
            None => NoteKind::Off,
        },
    }
}

#[test]
fn notes_bend_pedal_and_real_time_are_queued() {
    let c = MidiChannel::new(4).unwrap();
    let n = MidiNote::new(60).unwrap();
    let u7 = |v| U7::new(v).unwrap();
    let control = |cc, value| Voice::Control {
        cc: Ctrl::new(cc).unwrap(),
        value: u7(value),
    };
    let on = Voice::NoteOn {
        key: n,
        vel: Velocity::MAX,
    };
    let off = Voice::NoteOff {
        key: n,
        vel: u7(64),
    };
    assert_eq!(
        InEvent::from_event(c, &on),
        Some(InEvent::Note(ev(4, 60, 127)))
    );
    assert_eq!(
        InEvent::from_event(c, &off),
        Some(InEvent::Note(ev(4, 60, 0)))
    );
    assert_eq!(InEvent::from_event(c, &control(1, 2)), None);
    // Pitch bend and the pedal are queued; every other CC is the Part's
    // MIDI map's (#247).
    assert_eq!(
        InEvent::from_event(c, &Voice::PitchBend(Bend::from_u14(U14::new(0).unwrap()))),
        Some(InEvent::Bend(chimera_core::note_queue::BendEvent {
            channel: c,
            value: -8192
        }))
    );
    for (value, down) in [(0, false), (63, false), (64, true), (127, true)] {
        assert_eq!(
            InEvent::from_event(c, &control(64, value)),
            Some(InEvent::Sustain(chimera_core::note_queue::SustainEvent {
                channel: c,
                down
            }))
        );
    }
    for cc in [0, 1, 7, 10, 63, 65, 119] {
        assert_eq!(InEvent::from_event(c, &control(cc, 100)), None, "CC {cc}");
    }
}

#[test]
fn bend_and_pedal_survive_the_queue_and_a_pedal_up_has_the_reserve() {
    use chimera_core::note_queue::{BendEvent, NOTE_OFF_RESERVE, NOTE_QUEUE_LEN, SustainEvent};
    let c = MidiChannel::new(9).unwrap();
    let q = NoteQueue::new();
    let (mut tx, mut rx) = q.split().unwrap();
    for value in [-8192, -1, 0, 1, 8191] {
        assert!(tx.push(InEvent::Bend(BendEvent { channel: c, value })));
        assert_eq!(
            rx.pop(),
            Some(InEvent::Bend(BendEvent { channel: c, value }))
        );
    }
    for down in [true, false] {
        assert!(tx.push(InEvent::Sustain(SustainEvent { channel: c, down })));
        assert_eq!(
            rx.pop(),
            Some(InEvent::Sustain(SustainEvent { channel: c, down }))
        );
    }
    // Full but for the reserve: a bend is dropped, a pedal up is not.
    for _ in 0..NOTE_QUEUE_LEN - NOTE_OFF_RESERVE {
        assert!(tx.push(InEvent::Bend(BendEvent {
            channel: c,
            value: 5
        })));
    }
    assert!(!tx.push(InEvent::Bend(BendEvent {
        channel: c,
        value: 5
    })));
    assert!(!tx.push(InEvent::Sustain(SustainEvent {
        channel: c,
        down: true
    })));
    assert!(tx.push(InEvent::Sustain(SustainEvent {
        channel: c,
        down: false
    })));
}

#[test]
fn empty_queue_pops_nothing() {
    let q = NoteQueue::new();
    let (_, mut rx) = q.split().unwrap();
    assert_eq!(rx.pop(), None);
}

#[test]
fn events_come_out_in_order_and_intact() {
    let q = NoteQueue::new();
    let (mut tx, mut rx) = q.split().unwrap();
    let evs = [ev(0, 60, 100), ev(15, 127, 127), ev(9, 0, 1), ev(3, 64, 0)];
    for e in evs {
        assert!(tx.push(e));
    }
    for e in evs {
        assert_eq!(rx.pop(), Some(e.into()));
    }
    assert_eq!(rx.pop(), None);
}

#[test]
fn full_queue_drops_and_counts() {
    let q = NoteQueue::new();
    let (mut tx, mut rx) = q.split().unwrap();
    for i in 0..NOTE_QUEUE_LEN {
        assert!(tx.push(ev(0, i as u8, 0)), "slot {i}");
    }
    assert!(!tx.push(ev(0, 100, 0)));
    assert!(!tx.push(ev(0, 101, 0)));
    assert_eq!(q.dropped(), 2);
    // The queued events are untouched by the drops.
    assert_eq!(rx.pop(), Some(ev(0, 0, 0).into()));
    assert!(tx.push(ev(0, 102, 0)), "room again after a pop");
}

/// The last `NOTE_OFF_RESERVE` slots take note-offs only, never a note-on
/// or a clock tick: every note-on queued can still be followed by its
/// note-off, however fast the clock runs.
#[test]
fn note_ons_leave_room_for_note_offs() {
    let q = NoteQueue::new();
    let (mut tx, _rx) = q.split().unwrap();
    let ons = NOTE_QUEUE_LEN - NOTE_OFF_RESERVE;
    for i in 0..ons {
        assert!(tx.push(ev(0, i as u8, 100)), "note-on {i}");
    }
    assert!(!tx.push(ev(0, 100, 100)), "a note-on into the reserve");
    for _ in 0..NOTE_OFF_RESERVE {
        assert!(!tx.push(RealTime::Clock), "a tick into the reserve");
    }
    for i in 0..NOTE_OFF_RESERVE {
        assert!(tx.push(ev(0, i as u8, 0)), "note-off {i}");
    }
    assert!(!tx.push(ev(0, 99, 0)), "full");
    assert_eq!(q.dropped(), 2 + NOTE_OFF_RESERVE as u32);
}

#[test]
fn indices_wrap_around() {
    let q = NoteQueue::new();
    let (mut tx, mut rx) = q.split().unwrap();
    for i in 0..10 * NOTE_QUEUE_LEN {
        let e = ev((i % 16) as u8, (i % 128) as u8, (i % 128) as u8);
        assert!(tx.push(e));
        assert_eq!(rx.pop(), Some(e.into()), "event {i}");
    }
    assert_eq!(q.dropped(), 0);
}

/// One producer thread, one consumer thread: every event arrives once, in order.
#[test]
fn producer_and_consumer_threads() {
    const N: usize = 20_000;
    let q: &'static NoteQueue = Box::leak(Box::new(NoteQueue::new()));
    let (mut tx, mut rx) = q.split().unwrap();
    let producer = {
        std::thread::spawn(move || {
            for i in 0..N {
                while !tx.push(ev(0, (i % 128) as u8, 1 + (i % 127) as u8)) {
                    std::thread::yield_now();
                }
            }
        })
    };
    let mut got = 0;
    while got < N {
        if let Some(e) = rx.pop() {
            assert_eq!(e, ev(0, (got % 128) as u8, 1 + (got % 127) as u8).into());
            got += 1;
        }
    }
    producer.join().unwrap();
    assert_eq!(rx.pop(), None);
}

const A: SourceId<2> = SourceId::new(0);
const B: SourceId<2> = SourceId::new(1);

#[test]
fn drain_pops_every_source_in_fixed_order() {
    let s: NoteSources<2> = NoteSources::new();
    let (mut producers, mut drain) = s.split().unwrap();
    let (mut a, mut b) = (producers.take(A).unwrap(), producers.take(B).unwrap());
    b.push(ev(1, 61, 100));
    a.push(ev(0, 60, 100));
    b.push(ev(1, 62, 0));
    let mut got = Vec::new();
    drain.drain(|s, e| got.push((s, e)));
    let note = |s, e| (s, InEvent::Note(e));
    assert_eq!(
        got,
        [
            note(A, ev(0, 60, 100)),
            note(B, ev(1, 61, 100)),
            note(B, ev(1, 62, 0))
        ]
    );
    drain.drain(|_, _| panic!("already drained"));
}

#[test]
fn drops_are_counted_per_source() {
    let s: NoteSources<2> = NoteSources::new();
    let (mut producers, _) = s.split().unwrap();
    let mut b = producers.take(B).unwrap();
    for i in 0..NOTE_QUEUE_LEN + 3 {
        b.push(ev(0, (i % 128) as u8, 0));
    }
    assert_eq!(s.drops(), [0, 3]);
}

#[test]
fn source_ids_are_positions_in_the_drain_order() {
    assert_eq!((A.index(), B.index()), (0, 1));
    assert_eq!(MAX_NOTE_SOURCES, 2);
}

#[test]
#[should_panic(expected = "note source index out of range")]
fn a_runtime_source_id_past_n_panics() {
    let i = std::hint::black_box(2);
    let _ = SourceId::<2>::new(i);
}

/// Each end exists once (#116): a second split gets nothing, so no second
/// producer or consumer can share a queue.
#[test]
fn queues_and_sources_split_once() {
    let q = NoteQueue::new();
    assert!(q.split().is_some());
    assert!(q.split().is_none());
    let s: NoteSources<2> = NoteSources::new();
    let (mut producers, _) = s.split().unwrap();
    assert!(s.split().is_none());
    assert!(producers.take(A).is_some());
    assert!(producers.take(A).is_none());
}
