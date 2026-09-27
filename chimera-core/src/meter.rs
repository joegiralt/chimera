//! What the audio thread shows the UI besides the scope: one atomic word
//! per meter, written once per block with a relaxed store (it never
//! blocks), read whenever the UI draws.

use core::sync::atomic::{AtomicU32, Ordering};

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
pub static MASTER_GR: Meter = Meter::new();
