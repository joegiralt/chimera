//! The notes played, as the audio thread shows the UI: the keys held, the
//! last note in and the pitch it sounds at. A few relaxed atomic ops per
//! note event (never per sample or block), read by the screensaver.
//! Global, as `meter`'s: a test binary that publishes owns them.

use core::sync::atomic::{AtomicU32, Ordering};

static HELD: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];
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
pub fn note_on(part: u8, key: u8, vel: u8, note: u8, cents: i16) {
    HELD[(key >> 5) as usize & 3].fetch_or(1 << (key & 31), Ordering::Relaxed);
    LAST_IN.store(
        VALID | (part as u32 & 15) << 14 | (vel as u32 & 127) << 7 | key as u32 & 127,
        Ordering::Relaxed,
    );
    LAST_OUT.store(
        VALID | (cents as u16 as u32) << 8 | note as u32 & 127,
        Ordering::Relaxed,
    );
}

/// Audio: a key came up.
pub fn note_off(key: u8) {
    HELD[(key >> 5) as usize & 3].fetch_and(!(1 << (key & 31)), Ordering::Relaxed);
}

pub fn last_in() -> Option<NoteIn> {
    let w = LAST_IN.load(Ordering::Relaxed);
    (w & VALID != 0).then_some(NoteIn {
        note: (w & 127) as u8,
        vel: (w >> 7 & 127) as u8,
        part: (w >> 14 & 15) as u8,
    })
}

pub fn last_out() -> Option<NoteOut> {
    let w = LAST_OUT.load(Ordering::Relaxed);
    (w & VALID != 0).then_some(NoteOut {
        note: (w & 127) as u8,
        cents: (w >> 8) as u16 as i16,
    })
}

/// The held keys, lowest first, up to `N`.
pub fn held<const N: usize>() -> ([u8; N], usize) {
    let mut out = [0u8; N];
    let mut n = 0;
    for key in 0..128u8 {
        if n < N && HELD[(key >> 5) as usize].load(Ordering::Relaxed) >> (key & 31) & 1 != 0 {
            out[n] = key;
            n += 1;
        }
    }
    (out, n)
}

/// How many keys are held.
pub fn held_count() -> u32 {
    HELD.iter()
        .map(|w| w.load(Ordering::Relaxed).count_ones())
        .sum()
}
