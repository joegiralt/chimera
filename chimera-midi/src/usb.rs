// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! USB MIDI 1.0 event packets (class spec table 4-1) to and from MIDI bytes,
//! and the port's state between polls: the IN queue and Configured's edge.

use usb_device::UsbError;

use crate::value::Channel;
use usb_device::device::UsbDeviceState;

/// The 1 to 3 MIDI bytes of one packet, equal by those alone: the
/// padding is the host's.
#[derive(Clone, Copy, Debug, Eq)]
pub struct Bytes {
    len: u8,
    b: [u8; 3],
}

impl Bytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.b[..self.len as usize]
    }

    /// A channel message moved to channel `ch` (a Part cable's, #840);
    /// `None` for anything else, which is no Part's.
    pub fn on_channel(mut self, ch: Channel) -> Option<Bytes> {
        let s = self.b[0];
        (0x80..0xF0).contains(&s).then(|| {
            self.b[0] = s & 0xF0 | ch.get();
            self
        })
    }
}

impl PartialEq for Bytes {
    fn eq(&self, o: &Self) -> bool {
        self.as_slice() == o.as_slice()
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

/// Each whole packet's cable and bytes, `None` for the reserved CINs; a
/// trailing part packet is ignored.
pub fn cables(packets: &[u8]) -> impl Iterator<Item = Option<(Cable, Bytes)>> + '_ {
    packets.as_chunks::<4>().0.iter().map(|&p| unpack(p))
}

/// The Part cables (1 to `CABLES - 1`).
const PART_CABLES: usize = crate::usb_class::CABLES as usize - 1;

/// Where each Part cable's held keys and pedal went: a held key's
/// note-off and pressure, and a down pedal's up, go on the channel its
/// note-on or the pedal's down went on, though the Part's channel moved
/// since, so the voice releases (ADR 0243); a key the cable doesn't hold
/// and a pedal it hasn't put down go where the Part is now. A nibble and a
/// bit a key, 486 B.
#[derive(Clone, Debug)]
pub struct PartCables {
    keys: [[u8; 64]; PART_CABLES],
    held: [u128; PART_CABLES],
    /// The pedal's channel, or `UP`.
    pedal: [u8; PART_CABLES],
}

/// No pedal down.
const UP: u8 = 0xFF;

impl Default for PartCables {
    fn default() -> Self {
        Self::new()
    }
}

impl PartCables {
    pub const fn new() -> Self {
        Self {
            keys: [[0; 64]; PART_CABLES],
            held: [0; PART_CABLES],
            pedal: [UP; PART_CABLES],
        }
    }

    /// Cable `c`'s channel message on its Part's channel `now`, or the
    /// channel its held key or down pedal went on; `None` for anything
    /// else, or a cable no Part's.
    pub fn route(&mut self, c: Cable, b: Bytes, now: Channel) -> Option<Bytes> {
        let i = usize::from(c.get()).checked_sub(1)?;
        let keys = self.keys.get_mut(i)?;
        let (held, pedal) = (self.held.get_mut(i)?, self.pedal.get_mut(i)?);
        let [s, d1, d2] = b.b;
        let (k, bit) = (d1 & 0x7F, 1u128 << (d1 & 0x7F));
        let (key, shift) = (usize::from(k / 2), (k & 1) * 4);
        let was = || Channel::clamped(keys[key] >> shift & 0xF);
        let ch = match s & 0xF0 {
            0x90 if d2 > 0 => {
                keys[key] = keys[key] & !(0xF << shift) | now.get() << shift;
                *held |= bit;
                now
            }
            0x80 | 0x90 if *held & bit != 0 => {
                *held &= !bit;
                was()
            }
            0xA0 if *held & bit != 0 => was(),
            0xB0 if d1 == 64 && d2 >= 64 => {
                *pedal = now.get();
                now
            }
            0xB0 if d1 == 64 && *pedal != UP => Channel::clamped(core::mem::replace(pedal, UP)),
            _ => now,
        };
        b.on_channel(ch)
    }
}

/// Bytes to cable-0 packets; realtime passes through SysEx at once. Not
/// `Copy`: a copy would split one stream's state.
#[derive(Clone, Debug, Default)]
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
    /// Everything waiting dropped.
    fn clear(&self);
    /// Messages the producer lost since the last ask.
    fn take_dropped(&self) -> u32;
}

/// Packets an IN write takes: one 64-byte bulk packet's worth.
const HELD: usize = 16;

/// USB MIDI out: the ring's bytes packed, and the packets the endpoint
/// hasn't taken.
#[derive(Clone, Debug)]
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
    /// none counted: a host that comes back hears nothing stale. The UI
    /// lost an echo: the same, its losses counted. The drops: the UI's lost
    /// echo messages and the refused packets.
    pub fn send(
        &mut self,
        ring: &impl ByteRing,
        up: bool,
        mut write: impl FnMut(&[u8]) -> usb_device::Result<usize>,
    ) -> u32 {
        // Taken first, even with nothing waiting: the UI resends what it
        // lost only once its loss is taken (#842).
        let mut drops = ring.take_dropped();
        if !up || drops > 0 {
            // Gone, or the UI lost an echo: what waits is stale, and the
            // UI sends its Parts' values again once the ring is empty.
            ring.clear();
            *self = Self::new();
            return if up { drops } else { 0 };
        }
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
