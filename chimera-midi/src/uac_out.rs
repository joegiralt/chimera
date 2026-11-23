// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! USB Audio 1.0 from the host (#885, design 2026-11-10): the stereo
//! stream's room, the explicit feedback the device sends, and when the
//! audio interrupt starts taking blocks. Not globbed into bank 2 (bank 2
//! is full): bank 1's, under ADR 0251.
//!
//! The device is an asynchronous sink (USB 2.0 § 5.12.4.2): the SAI's
//! clock is the master, so the host sends what the device asks for. The
//! ask is the ring's smoothed fill against `TARGET`, a PI servo:
//! the host's packets drift with the SAI's clock and the ring holds.

pub use crate::uac::{FRAME_BYTES, NOMINAL};
use crate::uac_link::Ring;

/// Frames one packet may carry: the nominal and four to spare. Linux caps
/// its packets, and the feedback it takes, at wMaxPacketSize's frames
/// (sound/usb/endpoint.c `snd_usb_endpoint_set_params`); a servo inside
/// ±a frame never asks for more than 49.
pub const MAX_FRAMES: usize = NOMINAL + 4;
/// The OUT endpoint's wMaxPacketSize.
pub const PACKET: u16 = (MAX_FRAMES * FRAME_BYTES) as u16;

/// The ring's frames: the target, a block and two packets of slack.
pub const RING: usize = 512;
/// The fill the servo holds, about 4 ms: a block and two lost packets over
/// the block the audio interrupt takes.
pub const TARGET: usize = 192;
/// The audio interrupt's block.
pub const BLOCK: usize = 64;

/// The feedback endpoint's bRefresh: a value every 2^2 = 4 frames.
pub const REFRESH: u8 = 2;
/// The feedback's 10.14 one frame.
const ONE: i32 = 1 << 14;
/// 48 frames a bus frame, 10.14.
pub const NOMINAL_FB: u32 = (NOMINAL as u32) << 14;
/// The servo's proportional gain: 10.14 per 1/16 frame of fill error,
/// 1/256 frame a bus frame per frame of error.
const GAIN: i32 = 4;
/// The integral's: its sum over 256, critically damped against `GAIN`
/// (time constant about half a second), so a steady loss (a host losing
/// packets) or the crystal's drift settles at the target, not off it.
const SUM: i32 = 256;

/// The receive side's counters, wrapping: `bench`'s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Packets taken from the host.
    pub received: u32,
    /// Blocks the ring ran dry for: silence, and the stream primes again.
    pub short: u32,
    /// Packets the ring had no room for, dropped whole.
    pub overruns: u32,
}

/// The feedback servo, run at each SOF while the host streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feedback {
    /// The fill, in 1/16 frame, one pole at 1/32.
    avg: i32,
    /// The error summed, 1/16 frame a bus frame, held to a frame's trim.
    sum: i32,
}

impl Default for Feedback {
    fn default() -> Self {
        Self::new()
    }
}

impl Feedback {
    pub const fn new() -> Feedback {
        Feedback {
            avg: (TARGET as i32) << 4,
            sum: 0,
        }
    }

    /// The ring holds `fill` frames: the 10.14 rate the next feedback
    /// packet asks for, within a frame of nominal (Linux takes 7/8 of
    /// nominal to the packet's room).
    pub fn tick(&mut self, fill: usize) -> u32 {
        let fill = fill.min(RING) as i32;
        self.avg += ((fill << 4) - self.avg) / 32;
        let err = ((TARGET as i32) << 4) - self.avg;
        self.sum = (self.sum + err).clamp(-ONE * SUM, ONE * SUM);
        let trim = (err * GAIN + self.sum / SUM).clamp(-ONE, ONE);
        (NOMINAL_FB as i32 + trim) as u32
    }
}

/// A 10.14 value as the feedback packet's three bytes, little-endian.
#[inline(always)]
pub fn bytes(v: u32) -> [u8; 3] {
    let [a, b, c, _] = v.to_le_bytes();
    [a, b, c]
}

/// The audio interrupt's side: silence until the ring holds `TARGET`,
/// then a block each render; a ring short of a block plays silence and
/// primes again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drain {
    primed: bool,
}

impl Drain {
    pub const fn new() -> Drain {
        Drain { primed: false }
    }

    /// The stream stopped: the next starts by priming.
    pub fn stop(&mut self) {
        self.primed = false;
    }

    /// This render takes a `BLOCK` from a ring holding `fill`.
    pub fn take(&mut self, fill: usize, c: &mut Counts) -> bool {
        if !self.primed {
            self.primed = fill >= TARGET;
        } else if fill < BLOCK {
            self.primed = false;
            c.short = c.short.wrapping_add(1);
        }
        self.primed
    }
}

/// A packet's 3-byte subframes into `ring` as DAC words (bits 31..8),
/// whole frames only. A ring without room for them all drops the packet
/// whole (`overruns`) and keeps what it held.
pub fn receive<const N: usize>(packet: &[u8], ring: &Ring<N>, c: &mut Counts) {
    c.received = c.received.wrapping_add(1);
    let word = |b: &[u8]| i32::from_le_bytes([0, b[0], b[1], b[2]]);
    let frames = packet.len() / FRAME_BYTES;
    let ok = ring.push(frames, |i| {
        let f = &packet[i * FRAME_BYTES..];
        [word(f), word(&f[3..])]
    });
    if !ok {
        c.overruns = c.overruns.wrapping_add(1);
    }
}

/// USB IN's steps (SETTINGS › AUDIO ROUTING › USB AUDIO): OFF, −24 to
/// −3 dB, and 0 dB, the default.
pub const LEVELS: u8 = 8;
static GAINS: [f32; LEVELS as usize] = [0.0, 0.063, 0.126, 0.251, 0.355, 0.501, 0.708, 1.0];

/// Step `s`'s gain; past the last, 0 dB's.
pub fn gain(s: u8) -> f32 {
    GAINS[usize::from(s.min(LEVELS - 1))]
}

/// The gain the host's sound came in at, last block: each block eases
/// from it to the level, and a stream starting fades in from 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fade {
    now: f32,
}

impl Fade {
    pub const fn new() -> Fade {
        Fade { now: 0.0 }
    }
}

/// A DAC word to the block's scale: `to_dac`'s inverse (its full scale is
/// 8,388,607 steps of 256), so the host's full scale is the synth's,
/// word for word.
const WORD: f32 = 1.0 / (8_388_607.0 * 256.0);

/// The render's block of the host's sound, added into `out` (interleaved
/// L R, `BLOCK` frames) at `level`, eased from the last block's gain. A
/// ring not primed or short of a block adds nothing (silence, never a
/// partial or stale block) and the gain falls to 0, so the next start
/// fades in. Whether a block was added.
pub fn play<const N: usize>(
    ring: &Ring<N>,
    drain: &mut Drain,
    c: &mut Counts,
    fade: &mut Fade,
    level: f32,
    out: &mut [f32; 2 * BLOCK],
) -> bool {
    if !drain.take(ring.len(), c) {
        fade.now = 0.0;
        return false;
    }
    let step = (level - fade.now) / BLOCK as f32;
    let mut k = fade.now;
    ring.pop(BLOCK, |i, [l, r]| {
        out[2 * i] += k * l as f32 * WORD;
        out[2 * i + 1] += k * r as f32 * WORD;
        k += step;
    });
    fade.now = level;
    true
}
