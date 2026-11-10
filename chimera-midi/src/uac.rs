//! USB Audio 1.0 toward the host (ADR 0248): the stream's format, how
//! many frames each isochronous IN packet carries, their bytes, and the
//! frame each packet is aimed at. The board's door runs these from the
//! USB interrupt in bank 2.
//!
//! The device is an asynchronous source (USB 2.0 § 5.12.4.1): its clock
//! is the SAI's, and each packet carries what that clock made, so the host
//! needs no feedback. The ring's fill, smoothed, says how far the two
//! clocks have drifted; the packet gives or takes a few frames to hold it.

/// The stream: stereo, 48 kHz, 24-bit in 3-byte subframes.
pub const RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
pub const SUBFRAME: usize = 3;
pub const FRAME_BYTES: usize = CHANNELS * SUBFRAME;
/// Frames a 1 ms bus frame carries at the nominal rate.
pub const NOMINAL: usize = (RATE / 1_000) as usize;
/// Frames one packet may carry: half again the nominal, the room Linux's
/// snd-usb-audio gives each capture packet whatever wMaxPacketSize says
/// (sound/usb/endpoint.c: `freqmax = freqn + (freqn >> 1)`). A larger
/// packet overruns it and xHCI drops it whole. A frame the interrupt
/// missed is made up over the next packets.
pub const MAX_FRAMES: usize = NOMINAL + NOMINAL / 2;
/// The endpoint's wMaxPacketSize.
pub const PACKET: u16 = (MAX_FRAMES * FRAME_BYTES) as u16;
const _: () = assert!(PACKET <= 1023, "full speed's isochronous limit");

/// The ring's frames: room for `HIGH` and a block.
pub const RING: usize = 512;
/// The fill the pacer holds, smoothed: two audio blocks of slack under a
/// packet, about 2.7 ms of latency.
pub const TARGET: usize = 128;
/// A fill past this is a stream the host stopped reading with the stream
/// on: the ring drops back to `TARGET`, counted.
pub const HIGH: usize = 384;

/// The streaming counters, wrapping: `bench`'s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Packets armed.
    pub sent: u32,
    /// Bus frames a packet came too late for, owed to the next ones.
    pub missed: u32,
    /// Packets short of what they owed: the ring ran dry, and the stream
    /// primes again.
    pub short: u32,
    /// Drops back to `TARGET` from past `HIGH`.
    pub resyncs: u32,
    /// Armed packets a frame ended without, aimed again (`aim`).
    pub reaimed: u32,
}

/// What the next packet does: drop `skip` frames first, then carry `send`
/// (0: arm nothing this time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Next {
    pub skip: usize,
    pub send: usize,
}

/// The packet sizer. It waits until the ring holds `TARGET`, then sizes
/// each packet at `NOMINAL`, trimmed by the smoothed fill's error over 8
/// (a dead band of ±8 frames): at the SAI's ±100 ppm of drift a 49 or a
/// 47 every few hundred packets. A bus frame missed owes `NOMINAL` more,
/// paid as the packets' room allows and kept out of the fill's error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pacer {
    /// The fill, in 1/16 frame, one pole at 1/32.
    avg: i32,
    /// The bus frame the last packet was aimed at.
    last: u16,
    /// Frames owed for bus frames missed, not yet sent.
    owe: usize,
    primed: bool,
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Pacer {
    pub const fn new() -> Pacer {
        Pacer {
            avg: 0,
            last: 0,
            owe: 0,
            primed: false,
        }
    }

    /// The stream stopped: the next starts by priming.
    pub fn stop(&mut self) {
        self.primed = false;
        self.owe = 0;
    }

    /// The next packet from `fill` frames waiting, aimed at bus `frame`.
    pub fn next(&mut self, fill: usize, frame: u16, c: &mut Counts) -> Next {
        if self.primed {
            // Each bus frame skipped owes a frame's worth; a frame again
            // (re-aimed) or a stall past `HIGH` owes nothing.
            let gap = usize::from(frame.wrapping_sub(self.last) & FRAME_MASK);
            if (2..=HIGH / NOMINAL).contains(&gap) {
                c.missed = c.missed.wrapping_add(gap as u32 - 1);
                self.owe += NOMINAL * (gap - 1);
            }
        } else if fill < TARGET {
            return Next { skip: 0, send: 0 };
        } else {
            self.primed = true;
            self.avg = (TARGET as i32) << 4;
        }
        let mut skip = 0;
        if fill > HIGH {
            skip = fill - TARGET;
            c.resyncs = c.resyncs.wrapping_add(1);
            self.avg = (TARGET as i32) << 4;
            self.owe = 0;
        }
        let fill = fill - skip;
        self.owe = self.owe.min(fill);
        self.avg += ((((fill - self.owe) as i32) << 4) - self.avg) / 32;
        let err = (self.avg >> 4) - TARGET as i32;
        let trim = (err / 8).clamp(-8, (MAX_FRAMES - NOMINAL) as i32);
        let base = (NOMINAL as i32 + trim) as usize;
        let pay = self.owe.min(MAX_FRAMES - base);
        self.owe -= pay;
        let want = base + pay;
        self.last = frame;
        c.sent = c.sent.wrapping_add(1);
        if want > fill {
            c.short = c.short.wrapping_add(1);
            self.primed = false;
            self.owe = 0;
            return Next { skip, send: fill };
        }
        Next { skip, send: want }
    }
}

/// Full speed's frame numbers: 11 bits.
const FRAME_MASK: u16 = 0x7FF;

/// One frame's DAC words (each sample in bits 31..8, `DacSample`) as the
/// stream's six bytes, little-endian.
#[inline(always)]
pub fn pack([l, r]: [i32; 2], out: &mut [u8]) {
    let (l, r) = (l.to_le_bytes(), r.to_le_bytes());
    out[..6].copy_from_slice(&[l[1], l[2], l[3], r[1], r[2], r[3]]);
}

// DIEPCTL's bits (RM0433 § 57.14.43): the action bits a write sets, and
// the frame parity of an isochronous IN.
pub const EPENA: u32 = 1 << 31;
pub const EPDIS: u32 = 1 << 30;
pub const SODDFRM: u32 = 1 << 29;
pub const SEVNFRM: u32 = 1 << 28;
pub const SNAK: u32 = 1 << 27;
pub const CNAK: u32 = 1 << 26;

/// DIEPCTL holds a packet the core has not sent.
#[inline(always)]
pub fn armed(diepctl: u32) -> bool {
    diepctl & EPENA != 0
}

/// DSTS.FNSOF: the frame now.
#[inline(always)]
pub fn frame(dsts: u32) -> u16 {
    ((dsts >> 8) & 0x3FFF) as u16
}

/// The DIEPCTL write that aims an isochronous IN at the frame after
/// `now`: its parity bit, every other bit as read and no action bit, so it
/// neither arms nor disables. synopsys-usb-otg 0.4.0's `write` sets no
/// parity, and the core sends a packet only in a frame of the parity it
/// holds: every other packet is lost (kiffie/usbd-audio#4). Aimed by the
/// frame now, not by toggling the parity it held, so a late interrupt
/// catches up instead of chasing a frame already gone.
#[inline(always)]
pub fn aim(diepctl: u32, now: u16) -> u32 {
    let keep = diepctl & !(EPENA | EPDIS | SODDFRM | SEVNFRM | SNAK | CNAK);
    keep | if now & 1 == 0 { SODDFRM } else { SEVNFRM }
}
