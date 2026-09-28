//! Lock-free SPSC note queue from the UI/input thread to audio
//! (instrument-core spec § Threading).

use chimera_core::note_queue::{
    MAX_NOTE_SOURCES, NOTE_QUEUE_LEN, NoteEvent, NoteKind, NoteQueue, NoteSources, SourceId,
};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::MidiMessage;

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
fn only_note_on_and_off_become_note_events() {
    let c = MidiChannel::new(4).unwrap();
    let n = MidiNote::new(60).unwrap();
    let on = MidiMessage::NoteOn {
        channel: c,
        note: n,
        velocity: Velocity::MAX,
    };
    let off = MidiMessage::NoteOff {
        channel: c,
        note: n,
        velocity: 64,
    };
    assert_eq!(NoteEvent::from_midi(on), Some(ev(4, 60, 127)));
    assert_eq!(NoteEvent::from_midi(off), Some(ev(4, 60, 0)));
    assert_eq!(
        NoteEvent::from_midi(MidiMessage::ControlChange {
            channel: c,
            cc: 1,
            value: 2
        }),
        None
    );
    assert_eq!(
        NoteEvent::from_midi(MidiMessage::PitchBend {
            channel: c,
            value: 0
        }),
        None
    );
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
        assert_eq!(rx.pop(), Some(e));
    }
    assert_eq!(rx.pop(), None);
}

#[test]
fn full_queue_drops_and_counts() {
    let q = NoteQueue::new();
    let (mut tx, mut rx) = q.split().unwrap();
    for i in 0..NOTE_QUEUE_LEN {
        assert!(tx.push(ev(0, i as u8, 100)), "slot {i}");
    }
    assert!(!tx.push(ev(0, 100, 100)));
    assert!(!tx.push(ev(0, 101, 100)));
    assert_eq!(q.dropped(), 2);
    // The queued events are untouched by the drops.
    assert_eq!(rx.pop(), Some(ev(0, 0, 100)));
    assert!(tx.push(ev(0, 102, 100)), "room again after a pop");
}

#[test]
fn indices_wrap_around() {
    let q = NoteQueue::new();
    let (mut tx, mut rx) = q.split().unwrap();
    for i in 0..10 * NOTE_QUEUE_LEN {
        let e = ev((i % 16) as u8, (i % 128) as u8, (i % 128) as u8);
        assert!(tx.push(e));
        assert_eq!(rx.pop(), Some(e), "event {i}");
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
            assert_eq!(e, ev(0, (got % 128) as u8, 1 + (got % 127) as u8));
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
    let ([mut a, mut b], mut drain) = s.split().unwrap();
    b.push(ev(1, 61, 100));
    a.push(ev(0, 60, 100));
    b.push(ev(1, 62, 0));
    let mut got = Vec::new();
    drain.drain(|e| got.push(e));
    assert_eq!(got, [ev(0, 60, 100), ev(1, 61, 100), ev(1, 62, 0)]);
    drain.drain(|_| panic!("already drained"));
}

#[test]
fn drops_are_counted_per_source() {
    let s: NoteSources<2> = NoteSources::new();
    let ([_, mut b], _) = s.split().unwrap();
    for i in 0..NOTE_QUEUE_LEN + 3 {
        b.push(ev(0, (i % 128) as u8, 100));
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
    assert!(s.split().is_some());
    assert!(s.split().is_none());
}
