//! USB MIDI 1.0 event packets (class spec table 4-1) to and from MIDI bytes,
//! and the port's state between polls: the IN queue and Configured's edge.

use usb_device::UsbError;
use usb_device::device::UsbDeviceState;

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

/// Each whole packet's bytes if cable 0's, `None` for any other cable
/// and the reserved CINs; a trailing part packet is ignored.
pub fn cable_zero(packets: &[u8]) -> impl Iterator<Item = Option<Bytes>> + '_ {
    packets.as_chunks::<4>().0.iter().map(|&p| match unpack(p) {
        Some((Cable::ZERO, b)) => Some(b),
        _ => None,
    })
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

/// How USB left Configured, as its poll saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Left {
    /// A bus reset (or unplug): the host is gone.
    Reset,
    /// A suspend: an idle host's autosuspend as often as a sleep.
    Suspend,
}

/// Configured, as the last poll saw it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Edge(bool);

impl Edge {
    pub const fn new() -> Self {
        Self(false)
    }

    /// The device's state now; `Some` only on leaving Configured.
    pub fn poll(&mut self, state: UsbDeviceState) -> Option<Left> {
        let up = state == UsbDeviceState::Configured;
        let was = core::mem::replace(&mut self.0, up);
        if !was || up {
            return None;
        }
        Some(match state {
            UsbDeviceState::Suspend => Left::Suspend,
            _ => Left::Reset,
        })
    }

    pub const fn up(&self) -> bool {
        self.0
    }
}

/// The UI's echo bytes as USB's out ring holds them: the queue's one
/// consumer side.
pub trait ByteRing {
    fn pop(&self) -> Option<u8>;
    fn is_empty(&self) -> bool;
    /// Everything waiting dropped, and the producer's losses forgotten.
    fn discard(&self);
    /// Messages the producer lost since the last ask.
    fn take_dropped(&self) -> u32;
}

/// Packets an IN write takes: one 64-byte bulk packet's worth.
const HELD: usize = 16;

/// USB MIDI out: the ring's bytes packed, and the packets the endpoint
/// hasn't taken.
#[derive(Clone, Copy, Debug)]
pub struct InQueue {
    packer: Packer,
    held: [[u8; 4]; HELD],
    n: usize,
}

impl Default for InQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl InQueue {
    /// Inlined, a habit for anything bank 1 might build: bank 2's
    /// `door::alloc` builds the port today.
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            packer: Packer::new(),
            held: [[0; 4]; HELD],
            n: 0,
        }
    }

    /// Configured, the ring's bytes packed and written while `write` takes
    /// them; busy, held for the next call; any other error, dropped. Not
    /// Configured, the ring and the held dropped, the packer reset, and
    /// none counted: a host that comes back hears nothing stale. The drops:
    /// the UI's lost echo messages and the refused packets.
    pub fn send(
        &mut self,
        ring: &impl ByteRing,
        up: bool,
        mut write: impl FnMut(&[u8]) -> usb_device::Result<usize>,
    ) -> u32 {
        if self.n == 0 && ring.is_empty() {
            return 0;
        }
        if !up {
            ring.discard();
            *self = Self::new();
            return 0;
        }
        let mut drops = ring.take_dropped();
        loop {
            while self.n < HELD
                && let Some(x) = ring.pop()
            {
                if let Some(p) = self.packer.push(x) {
                    self.held[self.n] = p;
                    self.n += 1;
                }
            }
            let (held, _) = self.held.as_flattened().split_at(self.n * 4);
            if held.is_empty() {
                return drops;
            }
            match write(held) {
                Err(UsbError::WouldBlock) => return drops,
                Err(_) => drops = drops.wrapping_add(self.n as u32),
                Ok(_) => {}
            }
            self.n = 0;
        }
    }
}
