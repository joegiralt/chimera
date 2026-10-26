//! MIDI trust boundary (spec §3, Review Focus 4): only valid notes and
//! note-on velocities are representable. The parser that builds them is
//! `chimera_midi::stream`'s (its tests).

use chimera_core::{MidiNote, Velocity};

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
