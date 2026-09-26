//! Locks the parser's byte-stream behaviour (spec §3, Review Focus 2):
//! running status, real-time bytes interleaved mid-message, sysex, system
//! common, truncated messages and hot-plug junk must all resync cleanly.

use chimera_hal::midi::MidiParser;
use chimera_hal::{MidiChannel, MidiMessage, MidiNote, Velocity};

fn ch(c: u8) -> MidiChannel {
    MidiChannel::new(c).unwrap()
}

fn on(c: u8, n: u8, v: u8) -> MidiMessage {
    MidiMessage::NoteOn {
        channel: ch(c),
        note: MidiNote::new(n).unwrap(),
        velocity: Velocity::new(v).unwrap(),
    }
}

fn off(c: u8, n: u8, v: u8) -> MidiMessage {
    MidiMessage::NoteOff {
        channel: ch(c),
        note: MidiNote::new(n).unwrap(),
        velocity: v,
    }
}

fn cc(c: u8, number: u8, value: u8) -> MidiMessage {
    MidiMessage::ControlChange {
        channel: ch(c),
        cc: number,
        value,
    }
}

fn bend(c: u8, value: i16) -> MidiMessage {
    MidiMessage::PitchBend {
        channel: ch(c),
        value,
    }
}

fn cases() -> Vec<(&'static str, Vec<u8>, Vec<MidiMessage>)> {
    vec![
        (
            "running_status",
            vec![0x90, 60, 100, 62, 100],
            vec![on(0, 60, 100), on(0, 62, 100)],
        ),
        (
            "realtime_mid_message",
            vec![0x90, 60, 0xF8, 100, 0xFE, 0x92, 0xFA, 61, 0xFC, 1],
            vec![on(0, 60, 100), on(2, 61, 1)],
        ),
        (
            "sysex_with_note_like_bytes",
            vec![0xF0, 0x7E, 60, 100, 0xF7, 60, 100, 0x90, 60, 100],
            vec![on(0, 60, 100)],
        ),
        (
            "system_common_then_stray_data",
            vec![0xF3, 5, 60, 100, 0xF2, 0x10, 0x20, 60, 100],
            vec![],
        ),
        (
            "velocity_zero_is_note_off",
            vec![0x93, 64, 0],
            vec![off(3, 64, 0)],
        ),
        (
            "program_change_takes_one_byte",
            vec![0xC0, 5, 6, 7, 0x90, 60, 100],
            vec![on(0, 60, 100)],
        ),
        (
            "channel_pressure_takes_one_byte",
            vec![0xD2, 0x40, 0x41, 0x92, 60, 1],
            vec![on(2, 60, 1)],
        ),
        (
            "pitch_bend",
            vec![0xE5, 0x00, 0x40, 0xE0, 0x7F, 0x7F, 0xE0, 0, 0],
            vec![bend(5, 0), bend(0, 8191), bend(0, -8192)],
        ),
        (
            "control_change_on_channel_16",
            vec![0xBF, 7, 127],
            vec![cc(15, 7, 127)],
        ),
        (
            "junk_before_first_status",
            vec![60, 100, 0x3C, 0x90, 60, 100],
            vec![on(0, 60, 100)],
        ),
        (
            "truncated_message_resyncs_on_next_status",
            vec![0x90, 60, 0x80, 64, 0],
            vec![off(0, 64, 0)],
        ),
        (
            "hot_plug_junk_then_note",
            vec![0x3C, 0xF7, 0x40, 0x92, 0x3C, 0x40],
            vec![on(2, 60, 64)],
        ),
    ]
}

#[test]
fn parser_behaviour_table() {
    for (name, bytes, want) in cases() {
        let mut p = MidiParser::new();
        let got: Vec<MidiMessage> = bytes.iter().filter_map(|&b| p.feed(b)).collect();
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn a_parser_can_live_in_a_static() {
    static PARSER: MidiParser = MidiParser::new();
    let _ = &PARSER;
}
