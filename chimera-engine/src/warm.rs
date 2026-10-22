//! Bench only (`bench-warm`, the bench-algo set): each voice's block
//! rendered twice, the second from a copy of the voice as it was, both
//! timed. The second runs with the voice's code and data just used, so
//! the two together size what fetch and misses cost the first (#150).
//! Measurement only: the second render is the one that plays.

use crate::dsp::voice::Voice;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

/// Set by the bench around the rows that read `FIRST` and `SECOND`.
pub static ARMED: AtomicBool = AtomicBool::new(false);
/// Cycles of every voice's first render, and of its second, summed.
pub static FIRST: AtomicU32 = AtomicU32::new(0);
pub static SECOND: AtomicU32 = AtomicU32::new(0);

// SAFETY: ".uninit" is NOLOAD, so this holds garbage at boot; sound
// because it is `MaybeUninit` and `twice` writes it before any read. In
// AXI, out of D2 (full beside ALGO's kernel rows).
#[unsafe(link_section = ".uninit.bench_warm")]
static mut SAVED: MaybeUninit<Voice> = MaybeUninit::uninit();

/// `render` on `v`, twice while armed: the voice's bytes put back between.
#[inline(never)]
pub(crate) fn twice(v: &mut Voice, mut render: impl FnMut(&mut Voice)) {
    if !ARMED.load(Relaxed) {
        render(v);
        return;
    }
    let saved = (&raw mut SAVED).cast::<Voice>();
    // SAFETY: only the bench arms this, rendering ALGO rows on one thread
    // before the audio starts, so `SAVED` has no other user. The voice's
    // bytes go back where they were taken, the same object at the same
    // address, with nothing dropped or moved in between: an ALGO voice
    // owns no heap and shares no state its first render changes.
    unsafe { core::ptr::copy_nonoverlapping(v, saved, 1) };
    let t = crate::probe::now();
    render(v);
    FIRST.fetch_add(crate::probe::now().wrapping_sub(t), Relaxed);
    // SAFETY: as above; `saved` holds the bytes just copied.
    unsafe { core::ptr::copy_nonoverlapping(saved, v, 1) };
    let t = crate::probe::now();
    render(v);
    SECOND.fetch_add(crate::probe::now().wrapping_sub(t), Relaxed);
}
