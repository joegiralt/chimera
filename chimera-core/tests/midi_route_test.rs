//! CCs ride their own ring, never the note queue (MIDI map spec § 1).

use chimera_core::midi_in::{CC_RING_LEN, Cc, CcEvent, CcRing, Routed, route};
use chimera_core::note_queue::{InEvent, NoteEvent, NoteKind, NoteQueue};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::MidiMessage;

fn ch(c: u8) -> MidiChannel {
    MidiChannel::new(c).unwrap()
}

fn cc(n: u8, value: u8) -> MidiMessage {
    MidiMessage::ControlChange {
        channel: ch(0),
        cc: n,
        value,
    }
}

fn note_on(n: u8) -> MidiMessage {
    MidiMessage::NoteOn {
        channel: ch(0),
        note: MidiNote::new(n).unwrap(),
        velocity: Velocity::new(100).unwrap(),
    }
}

fn ev(n: u8, value: u8) -> CcEvent {
    CcEvent {
        channel: ch(n % 16),
        cc: Cc::new(n).unwrap(),
        value,
    }
}

#[test]
fn route_splits_ccs() {
    for msg in [
        note_on(60),
        MidiMessage::PitchBend {
            channel: ch(0),
            value: 100,
        },
        cc(64, 127),
        MidiMessage::Clock,
    ] {
        assert!(matches!(route(msg), Routed::Queue(_)), "{msg:?}");
    }
    for n in [74, 7, 0] {
        assert_eq!(
            route(cc(n, 33)),
            Routed::Cc(CcEvent {
                channel: ch(0),
                cc: Cc::new(n).unwrap(),
                value: 33,
            })
        );
    }
}

#[test]
fn cc_newtype_and_reserved() {
    assert_eq!(Cc::new(127).map(Cc::get), Some(127));
    assert_eq!(Cc::new(128), None);
    let reserved: Vec<u8> = (0..=127)
        .filter(|&n| Cc::new(n).unwrap().is_reserved())
        .collect();
    let want: Vec<u8> = [0, 6, 32, 38, 64]
        .into_iter()
        .chain(96..=101)
        .chain(120..=127)
        .collect();
    assert_eq!(reserved, want);
}

#[test]
fn cc_event_packs_round_trip() {
    for e in [ev(0, 0), ev(127, 127), ev(74, 64), ev(15, 1)] {
        assert_eq!(CcEvent::unpack(e.pack()), Some(e));
    }
}

#[test]
fn ring_fifo_and_drop() {
    let ring = CcRing::new();
    for i in 0..CC_RING_LEN as u8 {
        assert!(ring.push(ev(i, i)));
    }
    assert!(!ring.push(ev(100, 1)));
    assert_eq!(ring.dropped(), 1);
    for i in 0..CC_RING_LEN as u8 {
        assert_eq!(ring.pop(), Some(ev(i, i)));
    }
    assert_eq!(ring.pop(), None);
    assert!(ring.push(ev(1, 2)));
    assert!(ring.push(ev(3, 4)));
    assert_eq!(ring.drain().collect::<Vec<_>>(), [ev(1, 2), ev(3, 4)]);
    assert_eq!(ring.pop(), None);
}

#[test]
fn cc_flood_never_drops_note_ons() {
    let queue = NoteQueue::new();
    let (mut notes, mut audio) = queue.split().unwrap();
    let ccs = CcRing::new();
    let mut sent = 0;
    let mut heard = 0;
    for i in 0..10_000u32 {
        let msg = if i % 300 == 0 {
            sent += 1;
            note_on(60)
        } else {
            cc(74, (i % 128) as u8)
        };
        match route(msg) {
            Routed::Queue(e) => assert!(notes.push(e)),
            Routed::Cc(e) => {
                ccs.push(e);
            }
            Routed::Drop => panic!("{msg:?} dropped"),
        }
        if i % 64 == 63 {
            while let Some(e) = audio.pop() {
                assert!(matches!(
                    e,
                    InEvent::Note(NoteEvent {
                        kind: NoteKind::On(_),
                        ..
                    })
                ));
                heard += 1;
            }
        }
    }
    while audio.pop().is_some() {
        heard += 1;
    }
    assert!(sent > 0);
    assert_eq!(heard, sent);
    assert_eq!(queue.dropped(), 0);
    assert!(
        ccs.dropped() > 0,
        "the undrained CC ring fills, not the queue"
    );
}
