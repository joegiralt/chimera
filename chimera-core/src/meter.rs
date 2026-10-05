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

/// The tempo the reverb follows and its synced time's fold: what a synced
/// cell shows. Global as `MASTER_GR`: only a test binary whose renders keep
/// 120 BPM may draw synced cells.
static REV_TEMPO: Meter = Meter::new();
static REV_FOLD: Meter = Meter::new();

pub fn publish_rev_clock(t: Tempo, f: FoldState) {
    REV_TEMPO.publish(t.0);
    REV_FOLD.publish(f.0 as f32);
}

/// The last published clock; 120 BPM, unfolded, before any.
pub fn rev_clock() -> (Tempo, FoldState) {
    let t = REV_TEMPO.read();
    let t = if t > 0.0 { Tempo(t) } else { Tempo::DEFAULT };
    (t, FoldState(REV_FOLD.read() as i8))
}
