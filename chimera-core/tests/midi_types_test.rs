//! MIDI trust boundary (spec §3, Review Focus 4): only valid notes and
//! note-on velocities are representable, and the parser builds them.

use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::MidiMessage;
use chimera_hal::midi::MidiParser;

#[test]
fn midi_note_accepts_0_to_127_only() {
    assert_eq!(MidiNote::new(0).map(MidiNote::get), Some(0));
    assert_eq!(MidiNote::new(127).map(MidiNote::get), Some(127));
    assert_eq!(MidiNote::new(128), None);
    assert_eq!(MidiNote::new(255), None);
}

#[test]
fn velocity_accepts_1_to_127_only() {
    assert_eq!(Velocity::new(0), None);
    assert_eq!(Velocity::new(1).map(Velocity::get), Some(1));
    assert_eq!(Velocity::new(127), Some(Velocity::MAX));
    assert_eq!(Velocity::new(128), None);
}

#[test]
fn velocity_unit_is_the_old_formula() {
    for v in 1..=127u8 {
        assert_eq!(Velocity::new(v).unwrap().unit(), v as f32 / 127.0);
    }
}

fn feed(p: &mut MidiParser, bytes: &[u8]) -> Vec<MidiMessage> {
    bytes.iter().filter_map(|&b| p.feed(b)).collect()
}

#[test]
fn parser_builds_note_on() {
    let msgs = feed(&mut MidiParser::new(), &[0x90, 60, 127]);
    assert_eq!(
        msgs,
        [MidiMessage::NoteOn {
            channel: MidiChannel::new(0).unwrap(),
            note: MidiNote::new(60).unwrap(),
            velocity: Velocity::MAX
        }]
    );
}

/// Velocity 0 is a note-off, never a zero-velocity note-on.
#[test]
fn parser_velocity_zero_is_note_off() {
    let msgs = feed(&mut MidiParser::new(), &[0x91, 127, 0]);
    assert_eq!(
        msgs,
        [MidiMessage::NoteOff {
            channel: MidiChannel::new(1).unwrap(),
            note: MidiNote::new(127).unwrap(),
            velocity: 0
        }]
    );
}

#[test]
fn parser_running_status_note_on_then_off() {
    let msgs = feed(&mut MidiParser::new(), &[0x90, 60, 100, 60, 0]);
    let n60 = MidiNote::new(60).unwrap();
    assert_eq!(
        msgs,
        [
            MidiMessage::NoteOn {
                channel: MidiChannel::new(0).unwrap(),
                note: n60,
                velocity: Velocity::DEFAULT
            },
            MidiMessage::NoteOff {
                channel: MidiChannel::new(0).unwrap(),
                note: n60,
                velocity: 0
            },
        ]
    );
}

#[test]
fn parser_note_off_keeps_release_velocity() {
    let msgs = feed(&mut MidiParser::new(), &[0x82, 64, 64]);
    assert_eq!(
        msgs,
        [MidiMessage::NoteOff {
            channel: MidiChannel::new(2).unwrap(),
            note: MidiNote::new(64).unwrap(),
            velocity: 64
        }]
    );
}
