//! The notes played, as the audio thread shows the UI: the keys held, the
//! last note in and the pitch it sounds at. A few relaxed atomic ops per
//! note event (never per sample or block), read by the screensaver.
//! Global, as `meter`'s: a test binary that publishes owns them.

use core::sync::atomic::{AtomicU32, Ordering};

/// Held keys, a 128-bit set a MIDI channel: word `channel * 4 + key / 32`.
static HELD: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];
static LAST_IN: AtomicU32 = AtomicU32::new(0);
static LAST_OUT: AtomicU32 = AtomicU32::new(0);
const VALID: u32 = 1 << 31;

/// The last note received: key, velocity and the Part (0-based) it played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteIn {
    pub note: u8,
    pub vel: u8,
    pub part: u8,
}

/// The pitch the last note sounds at: the note after snap, and the Part's
/// PITCH + FINE offset in cents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteOut {
    pub note: u8,
    pub cents: i16,
}

impl NoteOut {
    /// Cents above MIDI note 0, 100 per semitone.
    pub fn total_cents(self) -> i32 {
        self.note as i32 * 100 + self.cents as i32
    }
}

/// Audio: a key went down on `part`, sounding `note` at `cents` offset.
pub fn note_on(channel: u8, part: u8, key: u8, vel: u8, note: u8, cents: i16) {
    HELD[word(channel, key)].fetch_or(1 << (key & 31), Ordering::Relaxed);
    LAST_IN.store(
        VALID | (part as u32 & 15) << 14 | (vel as u32 & 127) << 7 | key as u32 & 127,
        Ordering::Relaxed,
    );
    LAST_OUT.store(
        VALID | (cents as u16 as u32) << 8 | note as u32 & 127,
        Ordering::Relaxed,
    );
}

/// Audio: a key came up on `channel`.
pub fn note_off(channel: u8, key: u8) {
    HELD[word(channel, key)].fetch_and(!(1 << (key & 31)), Ordering::Relaxed);
}

fn word(channel: u8, key: u8) -> usize {
    (channel as usize & 15) * 4 + (key as usize >> 5 & 3)
}

/// Nothing is held on `channel`.
pub fn clear_channel(channel: u8) {
    for w in &HELD[(channel as usize & 15) * 4..][..4] {
        w.store(0, Ordering::Relaxed);
    }
}

/// Nothing is held: the voices were killed, or no voice holds a key (a
/// lost note-off).
pub fn clear_held() {
    for w in &HELD {
        w.store(0, Ordering::Relaxed);
    }
}

/// Any key held on any channel.
pub fn any_held() -> bool {
    HELD.iter().any(|w| w.load(Ordering::Relaxed) != 0)
}

/// Inlined: the screensaver and PIT's tuner (bank 2's) each take their own.
#[inline(always)]
pub fn last_in() -> Option<NoteIn> {
    let w = LAST_IN.load(Ordering::Relaxed);
    (w & VALID != 0).then_some(NoteIn {
        note: (w & 127) as u8,
        vel: (w >> 7 & 127) as u8,
        part: (w >> 14 & 15) as u8,
    })
}

#[inline(always)]
pub fn last_out() -> Option<NoteOut> {
    let w = LAST_OUT.load(Ordering::Relaxed);
    (w & VALID != 0).then_some(NoteOut {
        note: (w & 127) as u8,
        cents: (w >> 8) as u16 as i16,
    })
}

/// The held keys across every channel, each key number once, lowest
/// first: the first `N` of them, and how many there are in all.
pub fn held<const N: usize>() -> ([u8; N], usize) {
    let mut out = [0u8; N];
    let mut total = 0;
    for key in 0..128u8 {
        let down =
            (0..16).any(|ch| HELD[word(ch, key)].load(Ordering::Relaxed) >> (key & 31) & 1 != 0);
        if down {
            if total < N {
                out[total] = key;
            }
            total += 1;
        }
    }
    (out, total)
}
