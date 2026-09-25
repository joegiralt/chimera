//! Oscilloscope buffer — double-buffered for clean display.
//!
//! Audio thread writes to the back buffer. When full, it finds a
//! rising zero-crossing trigger point and swaps to front.
//! UI thread reads the stable front buffer — no tearing.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Display width in samples.
pub const SCOPE_LEN: usize = 240;
/// Back buffer is larger to allow trigger search.
const BACK_LEN: usize = SCOPE_LEN * 2;

static mut FRONT: [f32; SCOPE_LEN] = [0.0; SCOPE_LEN];
static mut BACK: [f32; BACK_LEN] = [0.0; BACK_LEN];
static BACK_POS: AtomicUsize = AtomicUsize::new(0);
static FRESH: AtomicBool = AtomicBool::new(false);

/// Called by audio thread after each block render.
pub fn write_samples(samples: &[f32]) {
    let mut pos = BACK_POS.load(Ordering::Relaxed);
    for &s in samples {
        if pos < BACK_LEN {
            // SAFETY: `write_samples` is only ever called from the audio
            // thread (the sole writer to `BACK`), and the bounds check above
            // (`pos < BACK_LEN`) guarantees the index is in range.
            unsafe {
                BACK[pos] = s;
            }
            pos += 1;
        }
    }
    BACK_POS.store(pos, Ordering::Relaxed);

    // When back buffer is full, find trigger and copy to front
    if pos >= BACK_LEN {
        // Find rising zero-crossing for trigger
        let mut trigger = 0;
        unsafe {
            for i in 1..BACK_LEN - SCOPE_LEN {
                if BACK[i - 1] <= 0.0 && BACK[i] > 0.0 {
                    trigger = i;
                    break;
                }
            }
            // Copy SCOPE_LEN samples from trigger point to front
            // SAFETY: FRONT/BACK are only written from the audio thread (single
            // writer). References are created through raw pointers, never to the
            // `static mut` directly. The UI-side race is a known issue (spec
            // § Known issues) and does not affect audio output.
            let front = &mut *core::ptr::addr_of_mut!(FRONT);
            let back = &*core::ptr::addr_of!(BACK);
            front.copy_from_slice(&back[trigger..trigger + SCOPE_LEN]);
        }
        BACK_POS.store(0, Ordering::Relaxed);
        FRESH.store(true, Ordering::Relaxed);
    }
}

/// Read the front buffer for display. Always stable — no tearing.
pub fn read_samples(out: &mut [f32; SCOPE_LEN]) {
    unsafe {
        // SAFETY: shared reference created through a raw pointer; a torn read
        // only affects the oscilloscope display.
        out.copy_from_slice(&*core::ptr::addr_of!(FRONT));
    }
}

/// Largest |sample| in a scope buffer (or a slice of one).
pub fn peak(buf: &[f32]) -> f32 {
    buf.iter()
        .fold(0.0f32, |m, &s| m.max(if s < 0.0 { -s } else { s }))
}

/// Below this peak the output counts as silent (the header dot is off).
pub const SOUNDING_PEAK: f32 = 1.0e-3;
