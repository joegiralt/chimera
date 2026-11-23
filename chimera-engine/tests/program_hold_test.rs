// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! #515 (owner decision 7): a program change holds its channel's notes in
//! the audio's drain till the UI has applied it, or 40 ms, so a sequencer's
//! downbeat plays the new Sound. One test at a time (`ONE`): `PROGRAM_IN`
//! and `PROGRAM_PENDING` are the process's.
// With `bank2` the hold runs through the door, closed on a host until a
// test opens it: `midi_door_test` covers that build.
#![cfg(not(feature = "bank2"))]

use std::sync::Mutex;
use std::sync::atomic::Ordering;

use chimera_engine::midi_in::{
    CtrlRing, DinPipe, DinSinks, HOLD_BLOCKS, PROGRAM_IN, PROGRAM_PENDING,
};
use chimera_engine::note_queue::{InEvent, NoteDrain, NoteEvent, NoteKind, NoteSources, SourceId};
use chimera_engine::{MidiChannel, MidiNote, Velocity};

static ONE: Mutex<()> = Mutex::new(());

fn note(c: u8, n: u8, on: bool) -> InEvent {
    InEvent::Note(NoteEvent {
        channel: MidiChannel::new(c).unwrap(),
        note: MidiNote::new(n).unwrap(),
        kind: if on {
            NoteKind::On(Velocity::new(100).unwrap())
        } else {
            NoteKind::Off
        },
    })
}

/// Every note a drain hands on: (channel, note, on).
fn notes<const N: usize>(d: &mut NoteDrain<'_, N>) -> Vec<(u8, u8, bool)> {
    let mut out = Vec::new();
    d.drain_held(&HOLD, |_, e| {
        if let InEvent::Note(n) = e {
            out.push((
                n.channel.get(),
                n.note.get(),
                matches!(n.kind, NoteKind::On(_)),
            ));
        }
    });
    out
}

/// Review of #891: a source's Ready ring full, what is left in its queue
/// waits for the next block's sort; played straight, a held note's
/// Note-Off overtook its Note-On and the note stuck.
/// A hold of the test's own.
static HOLD: chimera_engine::note_queue::ProgramHold =
    chimera_engine::note_queue::ProgramHold::new();

#[test]
fn a_full_ready_ring_never_lets_a_held_channel_through() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let src: &'static NoteSources<1> = Box::leak(Box::new(NoteSources::new()));
    let (mut prods, mut drain) = src.split().unwrap();
    let mut q = prods.take(SourceId::new(0)).unwrap();
    PROGRAM_PENDING.store(1 << 1, Ordering::Relaxed);
    assert!(q.push(note(1, 60, true)));
    assert_eq!(notes(&mut drain), [], "held");
    // A load gate kept the drain shut: a queue of another channel's notes,
    // then the held note's Note-Off.
    for n in 0..30 {
        assert!(q.push(note(2, n, true)));
    }
    assert!(q.push(note(1, 60, false)));
    let mut got = notes(&mut drain);
    got.extend(notes(&mut drain));
    PROGRAM_PENDING.store(0, Ordering::Relaxed);
    for _ in 0..4 {
        got.extend(notes(&mut drain));
    }
    let ch1: Vec<_> = got.iter().filter(|e| e.0 == 1).collect();
    assert_eq!(ch1, [&(1, 60, true), &(1, 60, false)], "On, then Off");
    assert_eq!(got.iter().filter(|e| e.0 == 2).count(), 30, "nothing lost");
}

/// Review of #891: a full hold whose oldest event's source has no Ready
/// room left drops nothing: the newer source waits in its queue.
#[test]
fn a_full_hold_drops_nothing() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let src: &'static NoteSources<2> = Box::leak(Box::new(NoteSources::new()));
    let (mut prods, mut drain) = src.split().unwrap();
    let mut a = prods.take(SourceId::new(0)).unwrap();
    let mut b = prods.take(SourceId::new(1)).unwrap();
    PROGRAM_PENDING.store(1 << 1, Ordering::Relaxed);
    for n in 0..16 {
        assert!(a.push(note(1, n, true)));
    }
    assert_eq!(notes(&mut drain), [], "the hold is full");
    for n in 0..16 {
        assert!(a.push(note(2, n, true)));
    }
    assert!(b.push(note(1, 99, true)));
    let mut got = notes(&mut drain);
    PROGRAM_PENDING.store(0, Ordering::Relaxed);
    for _ in 0..4 {
        got.extend(notes(&mut drain));
    }
    let ch1: Vec<_> = got.iter().filter(|e| e.0 == 1).map(|e| e.1).collect();
    let want: Vec<u8> = (0..16).chain([99]).collect();
    assert_eq!(ch1, want, "every held note, in order");
}

/// Every event a drain hands on, as (channel, note) of its note-ons.
fn drained(d: &mut chimera_engine::note_queue::NoteDrain<'_, 1>) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    d.drain_held(&HOLD, |_, e| {
        if let InEvent::Note(n) = e
            && matches!(n.kind, NoteKind::On(_))
        {
            out.push((n.channel.get(), n.note.get()));
        }
    });
    out
}

#[test]
fn a_program_change_holds_its_channel() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let src: &'static NoteSources<1> = Box::leak(Box::new(NoteSources::new()));
    let (mut prods, mut drain) = src.split().unwrap();
    let mut notes = prods.take(SourceId::new(0)).unwrap();
    let ring: CtrlRing = CtrlRing::new();
    let mut pipe = DinPipe::new();
    let mut feed = |b: &[u8]| {
        pipe.feed(
            b,
            &mut DinSinks {
                notes: &mut notes,
                ctrl: &ring,
                sysex: None,
            },
        )
    };

    // PROGRAM IN off: nothing waits.
    PROGRAM_IN.store(false, Ordering::Relaxed);
    feed(&[0xC1, 3, 0x91, 60, 100]);
    assert_eq!(PROGRAM_PENDING.load(Ordering::Relaxed), 0);
    assert_eq!(drained(&mut drain), [(1, 60)]);

    // On: channel 2's notes wait, channel 3's pass.
    PROGRAM_IN.store(true, Ordering::Relaxed);
    feed(&[0xC1, 4, 0x91, 62, 100, 0x92, 64, 100]);
    assert_eq!(PROGRAM_PENDING.load(Ordering::Relaxed), 1 << 1);
    assert_eq!(drained(&mut drain), [(2, 64)], "the other channel plays");
    assert_eq!(drained(&mut drain), [], "still waiting");
    // The UI applied it: the held note goes first, then what follows.
    PROGRAM_PENDING.fetch_and(!(1 << 1), Ordering::Relaxed);
    feed(&[0x91, 65, 100]);
    assert_eq!(drained(&mut drain), [(1, 62), (1, 65)]);

    // Never applied: let go after `HOLD_BLOCKS`, the bit cleared.
    feed(&[0xC1, 5, 0x91, 67, 100]);
    let mut blocks = 0;
    let got = loop {
        blocks += 1;
        let d = drained(&mut drain);
        if !d.is_empty() {
            break d;
        }
        assert!(blocks <= HOLD_BLOCKS + 1, "held past its time");
    };
    assert_eq!(got, [(1, 67)]);
    assert_eq!(blocks, HOLD_BLOCKS + 1);
    assert_eq!(PROGRAM_PENDING.load(Ordering::Relaxed), 0);

    // A full hold lets its oldest go ahead of each new one, in order.
    feed(&[0xC1, 6]);
    for n in 0..20u8 {
        feed(&[0x91, n, 100]);
    }
    let d = drained(&mut drain);
    assert_eq!(d, (0..4u8).map(|n| (1, n)).collect::<Vec<_>>());
    PROGRAM_PENDING.store(0, Ordering::Relaxed);
    assert_eq!(
        drained(&mut drain),
        (4..20u8).map(|n| (1, n)).collect::<Vec<_>>()
    );
    PROGRAM_IN.store(false, Ordering::Relaxed);
}
