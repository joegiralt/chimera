//! What the audio thread tells the UI about the notes played (`played`).
//! The statics are global: this binary's one test owns them.

mod common;

use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::NoteOrigin;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::played::{self, NoteIn, NoteOut};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use common::InstRig;

fn keys() -> Vec<u8> {
    let (k, n) = played::held::<8>();
    k[..n.min(8)].to_vec()
}

#[test]
fn notes_publish_the_pitch_the_voice_is_given() {
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    assert_eq!(played::last_in(), None);

    // PITCH +7 st and FINE -20 ct on Part 1: +680 ct on the key.
    rig.shared.parts[0].params.pitch.pitch = 7.0;
    rig.shared.parts[0].params.pitch.fine = -20.0;
    rig.shared.parts[0].mix.channel = MidiChannel::new(0).unwrap();
    rig.shared.parts[1].mix.channel = MidiChannel::new(1).unwrap();
    rig.note_on(0, 60);
    assert_eq!(
        played::last_in(),
        Some(NoteIn {
            note: 60,
            vel: Velocity::DEFAULT.get(),
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
    assert_eq!(keys(), [60]);

    // A second key: both held, the last in is the newer.
    rig.note_on(0, 64);
    assert_eq!(keys(), [60, 64]);
    assert_eq!(played::last_in().unwrap().note, 64);
    rig.note_off(0, 60);
    assert_eq!(keys(), [64]);
    rig.note_off(0, 64);
    assert!(keys().is_empty());
    // The last note stays for the readout after release.
    assert_eq!(played::last_in().unwrap().note, 64);

    // The same key on two Parts' channels: held until both let go; an
    // off on a channel no Part holds it on changes nothing.
    rig.note_on(0, 60);
    rig.note_on(1, 60);
    rig.note_off(5, 60);
    assert_eq!(keys(), [60]);
    rig.note_off(0, 60);
    assert_eq!(keys(), [60], "still held on channel 1");
    rig.note_off(1, 60);
    assert!(keys().is_empty());

    // ORBIT's notes are not received notes.
    let ev = NoteEvent {
        channel: MidiChannel::new(0).unwrap(),
        note: MidiNote::new(72).unwrap(),
        kind: NoteKind::On(Velocity::DEFAULT),
    };
    rig.inst.handle_from(
        ev,
        &rig.shared,
        NoteOrigin::Orbit(chimera_core::orbit::RingIdx::new(0).unwrap()),
    );
    assert!(keys().is_empty());
    assert_eq!(played::last_in().unwrap().note, 60);
    rig.inst.kill_all();
    rig.render(2);

    // A project load kills every voice: nothing is held after it.
    rig.note_on(0, 60);
    rig.note_on(0, 67);
    assert_eq!(keys(), [60, 67]);
    rig.inst.kill_all();
    assert!(keys().is_empty());

    // A key no voice holds (a note-off lost) goes at the next block.
    played::note_on(0, 0, 70, 96, 70, 0);
    assert_eq!(keys(), [70]);
    rig.render(1);
    assert!(keys().is_empty());

    // A held voice keeps its key through blocks.
    rig.note_on(0, 62);
    rig.render(4);
    assert_eq!(keys(), [62]);
    // More keys than the panel names are still counted.
    for k in 40..60 {
        played::note_on(0, 0, k, 96, k, 0);
    }
    let (_, total) = played::held::<8>();
    assert_eq!(total, 21);
}
