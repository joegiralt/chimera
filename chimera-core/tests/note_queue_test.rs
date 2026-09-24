//! Lock-free SPSC note queue from the UI/input thread to audio
//! (instrument-core spec § Threading).

use chimera_core::note_queue::{NoteEvent, NoteKind, NoteQueue, NOTE_QUEUE_LEN};
use chimera_core::{MidiChannel, MidiNote, Velocity};

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
fn empty_queue_pops_nothing() {
    let q = NoteQueue::new();
    assert_eq!(q.pop(), None);
}

#[test]
fn events_come_out_in_order_and_intact() {
    let q = NoteQueue::new();
    let evs = [ev(0, 60, 100), ev(15, 127, 127), ev(9, 0, 1), ev(3, 64, 0)];
    for e in evs {
        assert!(q.push(e));
    }
    for e in evs {
        assert_eq!(q.pop(), Some(e));
    }
    assert_eq!(q.pop(), None);
}

#[test]
fn full_queue_drops_and_counts() {
    let q = NoteQueue::new();
    for i in 0..NOTE_QUEUE_LEN {
        assert!(q.push(ev(0, i as u8, 100)), "slot {i}");
    }
    assert!(!q.push(ev(0, 100, 100)));
    assert!(!q.push(ev(0, 101, 100)));
    assert_eq!(q.dropped(), 2);
    // The queued events are untouched by the drops.
    assert_eq!(q.pop(), Some(ev(0, 0, 100)));
    assert!(q.push(ev(0, 102, 100)), "room again after a pop");
}

#[test]
fn indices_wrap_around() {
    let q = NoteQueue::new();
    for i in 0..10 * NOTE_QUEUE_LEN {
        let e = ev((i % 16) as u8, (i % 128) as u8, (i % 128) as u8);
        assert!(q.push(e));
        assert_eq!(q.pop(), Some(e), "event {i}");
    }
    assert_eq!(q.dropped(), 0);
}

/// One producer thread, one consumer thread: every event arrives once, in order.
#[test]
fn producer_and_consumer_threads() {
    const N: usize = 20_000;
    let q = std::sync::Arc::new(NoteQueue::new());
    let producer = {
        let q = std::sync::Arc::clone(&q);
        std::thread::spawn(move || {
            for i in 0..N {
                while !q.push(ev(0, (i % 128) as u8, 1 + (i % 127) as u8)) {
                    std::thread::yield_now();
                }
            }
        })
    };
    let mut got = 0;
    while got < N {
        if let Some(e) = q.pop() {
            assert_eq!(e, ev(0, (got % 128) as u8, 1 + (got % 127) as u8));
            got += 1;
        }
    }
    producer.join().unwrap();
    assert_eq!(q.pop(), None);
}
