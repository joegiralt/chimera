//! Bench only (`bench-warm`, the bench-algo set): each voice's block
//! rendered twice, the second from a copy of the voice as it was, both
//! timed. The first starts with the voice's state out of the D-cache (the
//! bench's `FLUSH` cleans and invalidates it and the copy after copying)
//! and the voice path's code where the voices before left it; the second
//! runs with the voice's code and data just used. The two together size
//! what fetch and misses cost (#150). Measurement only: the second render
//! is the one that plays.

use crate::dsp::voice::Voice;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering::Relaxed};

/// Set by the bench around the rows that read `FIRST` and `SECOND`.
pub static ARMED: AtomicBool = AtomicBool::new(false);
/// Cycles of every voice's first render, and of its second, summed.
pub static FIRST: AtomicU32 = AtomicU32::new(0);
pub static SECOND: AtomicU32 = AtomicU32::new(0);
/// The bench's D-cache clean and invalidate over `(address, bytes)`; the
/// engine has no cache to name. Unset, nothing is flushed.
static FLUSH: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

pub fn set_flush(f: fn(usize, usize)) {
    FLUSH.store(f as *mut (), Relaxed);
}

fn flush(at: *const Voice) {
    let p = FLUSH.load(Relaxed);
    if !p.is_null() {
        // SAFETY: only `set_flush` stores a non-null pointer, a
        // `fn(usize, usize)`.
        let f: fn(usize, usize) = unsafe { core::mem::transmute(p) };
        f(at as usize, size_of::<Voice>());
    }
}

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
    // The copy read the voice in: out of the D-cache again, both, so the
    // first render reads its state from memory as a cold one would.
    flush(v);
    flush(saved);
    let t = crate::probe::now();
    render(v);
    FIRST.fetch_add(crate::probe::now().wrapping_sub(t), Relaxed);
    // SAFETY: as above; `saved` holds the bytes just copied.
    unsafe { core::ptr::copy_nonoverlapping(saved, v, 1) };
    let t = crate::probe::now();
    render(v);
    SECOND.fetch_add(crate::probe::now().wrapping_sub(t), Relaxed);
}
