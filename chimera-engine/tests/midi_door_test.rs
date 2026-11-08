//! MIDI in's bank-1 side (spec § 7.1): `midi_in::feed` and `idle` reach
//! the pipe through the door's `MidiDoor`; closed, MIDI in is deaf, notes
//! included (spec § 13), and nothing panics. The host builds its own door
//! (`open_host`). One test at a time: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use std::sync::Mutex;

use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::midi_in::{self, CtrlEvent, CtrlRing, DinSinks, PipeSlot};
use chimera_engine::note_queue::{
    ChannelMode, ChannelModeEvent, InEvent, NoteEvent, NoteKind, NoteQueue, RealTime,
};
use chimera_engine::{MidiChannel, MidiNote, Velocity};
use chimera_midi::ctl::{Ctl, FreeCc};
use chimera_midi::value::U7;

static DOOR: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct Rec {
    notes: Vec<InEvent>,
    ctrl: Vec<CtrlEvent>,
}

/// `bytes` and, if `idle`, a receiver timeout through the shells' sinks
/// (a note queue and a control ring), as the ISR does.
fn through(p: &mut PipeSlot, bytes: &[u8], idle: bool) -> Rec {
    let q = NoteQueue::new();
    let (mut notes, mut audio) = q.split().unwrap();
    let ctrl = CtrlRing::new();
    let mut sinks = DinSinks {
        notes: &mut notes,
        ctrl: &ctrl,
        sysex: None,
    };
    midi_in::feed(p, bytes, &mut sinks);
    if idle {
        midi_in::idle(p, &mut sinks);
    }
    Rec {
        notes: core::iter::from_fn(|| audio.pop()).collect(),
        ctrl: ctrl.drain().collect(),
    }
}

/// A note on and off, a clock, CC 74 and Active Sensing.
const BYTES: &[u8] = &[0x91, 60, 100, 0xF8, 60, 0, 0xB1, 74, 33, 0xFE];

fn run() -> (Rec, PipeSlot) {
    let mut p = PipeSlot::new();
    (through(&mut p, BYTES, true), p)
}

#[test]
fn open_routes_like_before() {
    let _g = DOOR.lock().unwrap();
    DOOR_CELL.open_host();
    let (r, p) = run();
    DOOR_CELL.close();
    let ch = MidiChannel::new(1).unwrap();
    let note = |kind| {
        InEvent::Note(NoteEvent {
            channel: ch,
            note: MidiNote::new(60).unwrap(),
            kind,
        })
    };
    assert_eq!(
        r.notes[..3],
        [
            note(NoteKind::On(Velocity::new(100).unwrap())),
            InEvent::Rt(RealTime::Clock),
            note(NoteKind::Off),
        ]
    );
    assert_eq!(r.notes.len(), 3 + 16, "idle after FE: notes off × 16");
    for (c, e) in r.notes[3..].iter().enumerate() {
        let want = InEvent::Mode(ChannelModeEvent {
            channel: MidiChannel::clamped(c as u8),
            mode: ChannelMode::NotesOff,
        });
        assert_eq!(*e, want);
    }
    assert_eq!(
        r.ctrl,
        [
            CtrlEvent::Ctl(
                ch,
                Ctl::Cc {
                    cc: FreeCc::new(74).unwrap(),
                    value: U7::new(33).unwrap()
                }
            ),
            CtrlEvent::SensingLost,
        ]
    );
    assert_eq!(p.stats().refused, 0);
}

#[test]
fn closed_is_deaf() {
    let _g = DOOR.lock().unwrap();
    DOOR_CELL.close();
    let (r, p) = run();
    assert!(r.notes.is_empty(), "notes too: no bank-1 parser");
    assert!(r.ctrl.is_empty());
    assert!(p.pipe().is_none(), "the bytes never reached a pipe");
    assert_eq!(p.stats(), Default::default());
    // Opening later starts from a pipe that heard nothing.
    DOOR_CELL.open_host();
    let mut p = p;
    let r = through(&mut p, &[0x90, 61, 1], false);
    DOOR_CELL.close();
    assert_eq!(r.notes.len(), 1);
}
