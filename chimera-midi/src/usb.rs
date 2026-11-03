//! USB MIDI 1.0 event packets (class spec table 4-1) to and from MIDI bytes.

/// The 1 to 3 MIDI bytes of one packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bytes {
    len: u8,
    b: [u8; 3],
}

impl Bytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.b[..self.len as usize]
    }
}

/// A virtual cable, 0 to 15: a packet's high nibble.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cable(u8);

impl Cable {
    /// The only cable Chimera's descriptors name.
    pub const ZERO: Cable = Cable(0);

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Cable and bytes of a packet; `None` for the reserved CINs 0 and 1.
pub fn unpack(p: [u8; 4]) -> Option<(Cable, Bytes)> {
    let len = match p[0] & 0x0F {
        0 | 1 => return None,
        5 | 0xF => 1,
        2 | 6 | 0xC | 0xD => 2,
        _ => 3,
    };
    Some((
        Cable(p[0] >> 4),
        Bytes {
            len,
            b: [p[1], p[2], p[3]],
        },
    ))
}

/// Bytes to cable-0 packets; realtime passes through SysEx at once.
#[derive(Clone, Copy, Debug, Default)]
pub struct Packer {
    b: [u8; 3],
    n: u8,
    /// Channel or system-common status being filled (running status kept
    /// for channel messages), 0 if none.
    status: u8,
    sysex: bool,
}

/// Total bytes of a message with this status; 0 for none.
fn size(status: u8) -> u8 {
    match status {
        0xC0..=0xDF | 0xF1 | 0xF3 => 2,
        0x80..=0xEF | 0xF2 => 3,
        _ => 0,
    }
}

impl Packer {
    pub const fn new() -> Self {
        Self {
            b: [0; 3],
            n: 0,
            status: 0,
            sysex: false,
        }
    }

    pub fn push(&mut self, b: u8) -> Option<[u8; 4]> {
        if b >= 0xF8 {
            return Some([0x0F, b, 0, 0]);
        }
        if b < 0x80 {
            return self.data(b);
        }
        let open = core::mem::take(&mut self.sysex);
        match b {
            0xF0 => {
                self.sysex = true;
                self.status = 0;
                self.b[0] = b;
                self.n = 1;
                None
            }
            0xF7 => {
                // Only closes an open SysEx; a stray one is dropped.
                if !open {
                    return None;
                }
                let n = self.n as usize;
                self.b[n] = b;
                self.b[n + 1..].fill(0);
                self.n = 0;
                Some([5 + n as u8, self.b[0], self.b[1], self.b[2]])
            }
            0xF6 => {
                self.status = 0;
                self.n = 0;
                Some([0x05, b, 0, 0])
            }
            0xF4 | 0xF5 => {
                self.status = 0;
                self.n = 0;
                None
            }
            _ => {
                self.status = b;
                self.b[0] = b;
                self.n = 1;
                None
            }
        }
    }

    fn data(&mut self, b: u8) -> Option<[u8; 4]> {
        if self.sysex {
            self.b[self.n as usize] = b;
            self.n += 1;
            if self.n < 3 {
                return None;
            }
            self.n = 0;
            return Some([0x04, self.b[0], self.b[1], self.b[2]]);
        }
        let need = size(self.status);
        if need == 0 {
            return None;
        }
        if self.n == 0 {
            // Running status.
            self.b[0] = self.status;
            self.n = 1;
        }
        self.b[self.n as usize] = b;
        self.n += 1;
        if self.n < need {
            return None;
        }
        self.n = 0;
        let s = self.status;
        let cin = if s >= 0xF0 { need } else { s >> 4 };
        if s >= 0xF0 {
            self.status = 0;
        }
        Some([
            cin,
            self.b[0],
            self.b[1],
            if need == 3 { self.b[2] } else { 0 },
        ])
    }
}
