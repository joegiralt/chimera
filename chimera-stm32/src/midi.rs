//! MIDI input via USART1 at 31250 baud.
//! This module is not directly used in main.rs — MIDI rx is split
//! from the serial peripheral and polled directly.
//! This file provides the MIDI parser for future use.

use chimera_hal::MidiMessage;

pub struct MidiParser {
    running_status: u8,
    data: [u8; 2],
    data_idx: usize,
}

impl MidiParser {
    pub fn new() -> Self {
        Self {
            running_status: 0,
            data: [0; 2],
            data_idx: 0,
        }
    }

    /// Feed a byte from the UART. Returns a message when complete.
    pub fn feed(&mut self, byte: u8) -> Option<MidiMessage> {
        if byte >= 0xF8 {
            return None; // Real-time — ignore
        }

        if byte >= 0x80 {
            // Status byte
            self.running_status = byte;
            self.data_idx = 0;
            return None;
        }

        // Data byte
        if self.running_status < 0x80 {
            return None; // No status yet
        }

        self.data[self.data_idx] = byte;
        self.data_idx += 1;

        let expected = match self.running_status & 0xF0 {
            0xC0 | 0xD0 => 1,
            _ => 2,
        };

        if self.data_idx >= expected {
            self.data_idx = 0;
            return self.make_message();
        }

        None
    }

    fn make_message(&self) -> Option<MidiMessage> {
        let channel = self.running_status & 0x0F;
        match self.running_status & 0xF0 {
            0x90 => {
                if self.data[1] == 0 {
                    Some(MidiMessage::NoteOff {
                        channel,
                        note: self.data[0],
                        velocity: 0,
                    })
                } else {
                    Some(MidiMessage::NoteOn {
                        channel,
                        note: self.data[0],
                        velocity: self.data[1],
                    })
                }
            }
            0x80 => Some(MidiMessage::NoteOff {
                channel,
                note: self.data[0],
                velocity: self.data[1],
            }),
            0xB0 => Some(MidiMessage::ControlChange {
                channel,
                cc: self.data[0],
                value: self.data[1],
            }),
            0xE0 => Some(MidiMessage::PitchBend {
                channel,
                value: ((self.data[1] as i16) << 7 | self.data[0] as i16) - 8192,
            }),
            _ => None,
        }
    }
}
