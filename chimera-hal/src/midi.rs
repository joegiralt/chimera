//! MIDI byte-stream parser — the trust boundary where raw bytes become
//! `MidiNote`/`Velocity`. Hardware-independent (moved from chimera-stm32 so
//! it is host-testable); the firmware will feed it bytes from USART1 @ 31250.

use crate::{MidiMessage, MidiNote, Velocity};

pub struct MidiParser {
    running_status: u8,
    data: [u8; 2],
    data_idx: usize,
}

impl Default for MidiParser {
    fn default() -> Self {
        Self::new()
    }
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
                let note = MidiNote::new(self.data[0])?;
                match Velocity::new(self.data[1]) {
                    Some(velocity) => Some(MidiMessage::NoteOn { channel, note, velocity }),
                    // Note-on with velocity 0 is a note-off (MIDI 1.0 spec).
                    None => Some(MidiMessage::NoteOff { channel, note, velocity: 0 }),
                }
            }
            0x80 => Some(MidiMessage::NoteOff {
                channel,
                note: MidiNote::new(self.data[0])?,
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
