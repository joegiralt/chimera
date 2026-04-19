//! MIDI input via USART1 at 31250 baud.

use chimera_hal::{MidiIn, MidiMessage};
use stm32h7xx_hal::serial::Serial;

type SerialType = Serial<stm32h7xx_hal::pac::USART1>;

pub struct Stm32Midi {
    serial: SerialType,
    state: MidiParseState,
    running_status: u8,
    data: [u8; 2],
    data_idx: usize,
}

#[derive(Clone, Copy)]
enum MidiParseState {
    WaitingStatus,
    WaitingData,
}

impl Stm32Midi {
    pub fn new(serial: SerialType) -> Self {
        Self {
            serial,
            state: MidiParseState::WaitingStatus,
            running_status: 0,
            data: [0; 2],
            data_idx: 0,
        }
    }

    fn parse_byte(&mut self, byte: u8) -> Option<MidiMessage> {
        if byte >= 0x80 {
            // Status byte
            if byte >= 0xF8 {
                // Real-time messages — ignore for now
                return None;
            }
            self.running_status = byte;
            self.data_idx = 0;
            self.state = MidiParseState::WaitingData;
            return None;
        }

        // Data byte
        match self.state {
            MidiParseState::WaitingStatus => {
                // Running status
                if self.running_status >= 0x80 {
                    self.data_idx = 0;
                    self.state = MidiParseState::WaitingData;
                    self.data[0] = byte;
                    self.data_idx = 1;

                    let expected = Self::data_bytes_for_status(self.running_status);
                    if self.data_idx >= expected {
                        self.state = MidiParseState::WaitingStatus;
                        return self.make_message();
                    }
                }
                None
            }
            MidiParseState::WaitingData => {
                self.data[self.data_idx] = byte;
                self.data_idx += 1;

                let expected = Self::data_bytes_for_status(self.running_status);
                if self.data_idx >= expected {
                    self.state = MidiParseState::WaitingStatus;
                    return self.make_message();
                }
                None
            }
        }
    }

    fn data_bytes_for_status(status: u8) -> usize {
        match status & 0xF0 {
            0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 2,
            0xC0 | 0xD0 => 1,
            _ => 2,
        }
    }

    fn make_message(&self) -> Option<MidiMessage> {
        let channel = self.running_status & 0x0F;
        match self.running_status & 0xF0 {
            0x90 => {
                if self.data[1] == 0 {
                    // Note on with velocity 0 = note off
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

impl MidiIn for Stm32Midi {
    fn read(&mut self) -> Option<MidiMessage> {
        // Try to read available bytes from UART
        // The stm32h7xx-hal serial read is non-blocking
        match self.serial.read() {
            Ok(byte) => self.parse_byte(byte),
            Err(_) => None,
        }
    }
}
