//! What the audio thread tells the UI about the notes played (`played`).
//! The statics are global: this binary's one test owns them.

use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, Instrument, NoteOrigin};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::played::{self, NoteIn, NoteOut};
use chimera_core::{MidiChannel, MidiNote, Velocity};

fn ev(note: u8, kind: NoteKind) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::new(0).unwrap(),
        note: MidiNote::new(note).unwrap(),
        kind,
    }
}

#[test]
fn notes_publish_the_pitch_the_voice_is_given() {
    let mut inst = Box::new(Instrument::new(48_000, SampleBudget::for_cpu(CPU_HZ_REV_V)));
    let mut shared = AudioShared::default();
    assert_eq!(played::last_in(), None);

    // PITCH +7 st and FINE −20 ct on Part 1: +680 ct on the key.
    shared.parts[0].params.pitch.pitch = 7.0;
    shared.parts[0].params.pitch.fine = -20.0;
    let vel = Velocity::new(96).unwrap();
    inst.handle(ev(60, NoteKind::On(vel)), &shared);
    assert_eq!(
        played::last_in(),
        Some(NoteIn {
            note: 60,
            vel: 96,
            part: 0
        })
    );
    let out = played::last_out().unwrap();
    assert_eq!(
        out,
        NoteOut {
            note: 60,
            cents: 680
        }
    );
    assert_eq!(out.total_cents(), 6680);
    assert_eq!(played::held::<8>().1, 1);

    // A second key: both held, the last in is the newer.
    inst.handle(ev(64, NoteKind::On(vel)), &shared);
    let (keys, n) = played::held::<8>();
    assert_eq!(&keys[..n], &[60, 64]);
    assert_eq!(played::last_in().unwrap().note, 64);

    inst.handle(ev(60, NoteKind::Off), &shared);
    let (keys, n) = played::held::<8>();
    assert_eq!(&keys[..n], &[64]);
    inst.handle(ev(64, NoteKind::Off), &shared);
    assert_eq!(played::held_count(), 0);
    // The last note stays for the readout after release.
    assert_eq!(played::last_in().unwrap().note, 64);

    // ORBIT's notes are not received notes.
    inst.handle_from(
        ev(72, NoteKind::On(vel)),
        &shared,
        NoteOrigin::Orbit(chimera_core::orbit::RingIdx::new(0).unwrap()),
    );
    assert_eq!(played::held_count(), 0);
    assert_eq!(played::last_in().unwrap().note, 64);
}
