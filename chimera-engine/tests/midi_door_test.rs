// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI in's bank-1 side (spec § 7.1): `midi_in::feed` and `idle` reach
//! the pipe through the door's `MidiDoor`; closed, MIDI in is deaf, notes
//! included (spec § 13), and nothing panics. The host builds its own door
//! (`open_host`). One test at a time: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use std::sync::Mutex;

use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::midi_in::{self, CtrlEvent, CtrlRing, DinSinks, PipeSlot};
use chimera_engine::note_queue::{
    ChannelMode, ChannelModeEvent, Expr, ExprEvent, InEvent, NoteEvent, NoteKind, NoteQueue,
    RealTime,
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

/// A hold of the test's own.
static HOLD: chimera_engine::note_queue::ProgramHold =
    chimera_engine::note_queue::ProgramHold::new();

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
        r.notes[..4],
        [
            note(NoteKind::On(Velocity::new(100).unwrap())),
            InEvent::Rt(RealTime::Clock),
            note(NoteKind::Off),
            // CC 74 is SLIDE's too (ADR 0252).
            InEvent::Expr(ExprEvent {
                channel: ch,
                expr: Expr::Timbre(chimera_midi::value::U7::new(33).unwrap()),
            }),
        ]
    );
    assert_eq!(r.notes.len(), 4 + 16, "idle after FE: notes off × 16");
    for (c, e) in r.notes[4..].iter().enumerate() {
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

/// MPE's zone reads (ADR 0252) through the door: open, a member channel
/// names its master and its note's bend; closed, every channel is its own
/// and a voice keeps only its poly pressure.
#[test]
fn mpe_reads_go_through_the_door() {
    use chimera_engine::dsp::voice::Own;
    use chimera_engine::hw::MAX_VOICES;
    use chimera_engine::mpe::{self, OwnIn, Zones};
    let _g = DOOR.lock().unwrap();
    let z = Zones::OFF.with_lower(15);
    let ch = |c| MidiChannel::new(c).unwrap();
    let mut wheel = [0i16; 16];
    wheel[3] = 8191;
    let mut poly = [0u8; MAX_VOICES];
    poly[0] = 127;
    let channel = [ch(3); MAX_VOICES];
    let i = OwnIn {
        zones: z,
        channel: &channel,
        origin: &[chimera_engine::instrument::NoteOrigin::Live; MAX_VOICES],
        wheel: &wheel,
        expr: &[[0; 3]; 16],
        poly: &poly,
    };
    let mut out = [Own::default(); MAX_VOICES];
    DOOR_CELL.open_host();
    assert_eq!(mpe::master_of(z, ch(3)), ch(0));
    assert!(mpe::own(&i, &mut out));
    assert_eq!((out[0].bend, out[0].press), (48.0, 1.0));
    assert_eq!(mpe::write(z, 2, 4).upper(), 4);
    DOOR_CELL.close();
    assert_eq!(mpe::master_of(z, ch(3)), ch(3));
    assert!(
        !mpe::own(&i, &mut out),
        "closed: each voice its poly pressure"
    );
    assert_eq!(mpe::poly_only(poly[0]).press, 1.0);
    // Closed: stored as given, no shrink rule (a SYSTEM load keeps it).
    let w = mpe::write(z, 2, 4);
    assert_eq!((w.lower(), w.upper()), (15, 4));
}

/// #515: the drain's hold runs through the door's `hold`: open, a pending
/// channel's note waits; closed, it plays at once and nothing is held.
#[test]
fn the_hold_is_the_doors() {
    use chimera_engine::midi_in::PROGRAM_PENDING;
    use chimera_engine::note_queue::{NoteSources, SourceId};
    use std::sync::atomic::Ordering;
    let _g = DOOR.lock().unwrap();
    let src: &'static NoteSources<1> = Box::leak(Box::new(NoteSources::new()));
    let (mut prods, mut drain) = src.split().unwrap();
    let mut q = prods.take(SourceId::new(0)).unwrap();
    let on = InEvent::Note(NoteEvent {
        channel: MidiChannel::new(4).unwrap(),
        note: MidiNote::new(60).unwrap(),
        kind: NoteKind::On(Velocity::new(100).unwrap()),
    });
    let count = |d: &mut chimera_engine::note_queue::NoteDrain<'_, 1>| {
        let mut n = 0;
        d.drain_held(&HOLD, |_, _| n += 1);
        n
    };
    PROGRAM_PENDING.store(1 << 4, Ordering::Relaxed);
    DOOR_CELL.open_host();
    assert!(q.push(on));
    assert_eq!(count(&mut drain), 0, "open: held");
    PROGRAM_PENDING.store(0, Ordering::Relaxed);
    assert_eq!(count(&mut drain), 1, "applied: played");
    DOOR_CELL.close();
    PROGRAM_PENDING.store(1 << 4, Ordering::Relaxed);
    assert!(q.push(on));
    assert_eq!(count(&mut drain), 1, "closed: played at once");
    PROGRAM_PENDING.store(0, Ordering::Relaxed);
}
