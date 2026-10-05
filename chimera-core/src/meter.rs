//! What the audio thread shows the UI besides the scope: one atomic word
//! per meter, written once per block with a relaxed store (it never
//! blocks), read whenever the UI draws.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::dsp::rev::sync::{FoldState, Tempo};

/// One `f32`, held as its bits.
pub struct Meter(AtomicU32);

impl Meter {
    pub const fn new() -> Self {
        Self(AtomicU32::new(0))
    }

    pub fn publish(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn read(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

impl Default for Meter {
    fn default() -> Self {
        Self::new()
    }
}

/// The master compressor's gain reduction, dB (0 while it is bypassed).
///
/// Global: tests run in parallel threads, so at most one test per test
/// binary may publish a nonzero value or render MST expecting one.
pub static MASTER_GR: Meter = Meter::new();

/// The tempo the reverb follows and its synced time's fold, in one word:
/// tempo × 4 (its 0.25 BPM grid) in the low 16 bits, the fold above. What
/// a synced cell shows. Global as `MASTER_GR`: only a test binary whose
/// renders keep 120 BPM may draw synced cells.
static REV_CLOCK: AtomicU32 = AtomicU32::new(0);

pub fn publish_rev_clock(t: Tempo, f: FoldState) {
    REV_CLOCK.store(clock_word(t, f), Ordering::Relaxed);
}

/// `t` × 4 in bits 0–15, `f` in bits 16–23: the published word, and what
/// a synced cell's redraw is keyed on.
pub fn clock_word(t: Tempo, f: FoldState) -> u32 {
    (libm::roundf(t.0 * 4.0) as u32 & 0xFFFF) | (f.0 as u8 as u32) << 16
}

/// The last published clock; 120 BPM, unfolded, before any.
pub fn rev_clock() -> (Tempo, FoldState) {
    let w = REV_CLOCK.load(Ordering::Relaxed);
    let q = w & 0xFFFF;
    let t = if q == 0 {
        Tempo::DEFAULT
    } else {
        Tempo(q as f32 / 4.0)
    };
    (t, FoldState((w >> 16) as u8 as i8))
}
