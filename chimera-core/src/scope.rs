//! Oscilloscope buffer — captures end-of-chain audio for UI display.
//!
//! The audio thread writes samples after the full signal chain.
//! The UI thread reads them to draw the waveform strip.

use core::sync::atomic::{AtomicU32, Ordering};

/// Number of samples in the scope display buffer.
/// 240 samples = 1 sample per screen pixel width = clean mapping.
pub const SCOPE_LEN: usize = 240;

/// Shared scope buffer. Written by audio, read by UI.
/// Stored as u32 (f32 bits) for atomic-free simplicity — single writer, single reader.
static mut SCOPE_BUF: [f32; SCOPE_LEN] = [0.0; SCOPE_LEN];
static SCOPE_WRITE_POS: AtomicU32 = AtomicU32::new(0);

/// Called by the audio thread after each block render.
/// Copies the block's output samples into the ring buffer.
///
/// # Safety
/// Single writer (audio thread only). Reader tolerates torn reads.
pub fn write_samples(samples: &[f32]) {
    let mut pos = SCOPE_WRITE_POS.load(Ordering::Relaxed) as usize;
    for &s in samples {
        unsafe {
            SCOPE_BUF[pos % SCOPE_LEN] = s;
        }
        pos += 1;
    }
    SCOPE_WRITE_POS.store(pos as u32, Ordering::Relaxed);
}

/// Read the scope buffer for display. Returns a snapshot.
/// The waveform may have minor tearing but that's fine for a visual scope.
pub fn read_samples(out: &mut [f32; SCOPE_LEN]) {
    let pos = SCOPE_WRITE_POS.load(Ordering::Relaxed) as usize;
    for i in 0..SCOPE_LEN {
        unsafe {
            out[i] = SCOPE_BUF[(pos + i) % SCOPE_LEN];
        }
    }
}
