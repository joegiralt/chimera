//! ORBIT's notes in the voice pool (Task 4): `Instrument::handle_from` and
//! a note-off that matches its origin too (spec § 5.2).

mod common;

use chimera_core::hw::{CPU_HZ_REV_V, MAX_VOICES, SampleBudget};
use chimera_core::instrument::NoteOrigin;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::orbit::RingIdx;
use chimera_core::part::PartMode;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use common::InstRig;

const CH2: u8 = 1; // Part 2's default channel

fn ring(i: u8) -> NoteOrigin {
    NoteOrigin::Orbit(RingIdx::new(i).unwrap())
}

fn rig() -> InstRig {
    InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V))
}

fn ev(note: u8, on: bool) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::new(CH2).unwrap(),
        note: MidiNote::new(note).unwrap(),
        kind: if on {
            NoteKind::On(Velocity::DEFAULT)
        } else {
            NoteKind::Off
        },
    }
}

fn play(r: &mut InstRig, from: NoteOrigin, note: u8, on: bool) {
    r.inst.handle_from(ev(note, on), &r.shared, from);
}

/// Held voices on `note`.
fn held(r: &InstRig, note: u8) -> usize {
    r.inst
        .allocator()
        .slots()
        .iter()
        .filter(|s| s.held() && s.note() == MidiNote::new(note))
        .count()
}

#[test]
fn orbit_events_reach_parts_like_midi() {
    let run = |from: NoteOrigin| {
        let mut r = rig();
        let mut out = Vec::new();
        for (blocks, on) in [(40, true), (40, false)] {
            play(&mut r, from, 60, on);
            for _ in 0..blocks {
                r.render(1);
                out.extend_from_slice(&r.out().concat());
            }
        }
        out
    };
    let (orbit, live) = (run(ring(0)), run(NoteOrigin::Live));
    assert!(orbit.iter().any(|x| *x != 0.0), "a note sounded");
    assert!(
        orbit
            .iter()
            .zip(&live)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "bit-identical"
    );
}

#[test]
fn orbit_off_never_releases_a_held_key() {
    let mut r = rig();
    play(&mut r, NoteOrigin::Live, 60, true);
    for _ in 0..4 {
        play(&mut r, ring(0), 60, true);
        play(&mut r, ring(0), 60, false);
    }
    assert!(held(&r, 60) >= 1, "the live voice stays held");
    play(&mut r, NoteOrigin::Live, 60, false);
    assert_eq!(held(&r, 60), 0, "its own Off still releases it");
}

#[test]
fn two_rings_on_one_note_release_their_own() {
    let mut r = rig();
    play(&mut r, ring(0), 60, true);
    play(&mut r, ring(1), 60, true);
    assert_eq!(held(&r, 60), 2, "a voice each");
    play(&mut r, ring(0), 60, false);
    assert_eq!(held(&r, 60), 1, "ring 1's voice stays held");
    play(&mut r, ring(1), 60, false);
    assert_eq!(held(&r, 60), 0);

    // Mono: ring 1's retrigger takes the voice; ring 0's Off does nothing.
    let mut r = rig();
    r.shared.parts[CH2 as usize].mix.mode = PartMode::Mono;
    play(&mut r, ring(0), 60, true);
    play(&mut r, ring(1), 60, true);
    assert_eq!(held(&r, 60), 1);
    play(&mut r, ring(0), 60, false);
    assert_eq!(held(&r, 60), 1, "ring 0 no longer owns it");
    play(&mut r, ring(1), 60, false);
    assert_eq!(held(&r, 60), 0);
}

#[test]
fn live_off_never_releases_an_orbit_note() {
    let mut r = rig();
    play(&mut r, ring(0), 60, true);
    play(&mut r, NoteOrigin::Live, 60, true);
    play(&mut r, NoteOrigin::Live, 60, false);
    assert!(held(&r, 60) >= 1, "the ORBIT voice stays held");
    play(&mut r, ring(0), 60, false);
    assert_eq!(held(&r, 60), 0);
}

#[test]
fn orbit_shares_voices() {
    let mut r = rig();
    let chord = [48, 52, 55, 59];
    for n in chord {
        play(&mut r, NoteOrigin::Live, n, true);
    }
    // Five rings, notes that cycle past the pool: tails go first.
    let mut tail = 70;
    for i in 0..5u8 {
        play(&mut r, ring(i), tail + i, true);
        play(&mut r, ring(i), tail + i, false);
    }
    for _ in 0..2 {
        r.render(1);
    }
    for n in chord {
        assert_eq!(held(&r, n), 1, "held key {n} survives while tails exist");
    }
    // Fill the pool with held ORBIT notes; with no tails, the oldest held goes.
    tail = 80;
    for k in 0..MAX_VOICES as u8 {
        play(&mut r, ring(0), tail + k, true);
    }
    assert_eq!(held(&r, chord[0]), 0, "the oldest held note was stolen");
}
