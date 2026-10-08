//! `fault dma <1|2>`: provoke a transfer error on a trailing audio DMA stream
//! as a debugger would. Test builds only (`fault-inject`).

use chimera_core::console::StreamSnap;
use stm32h7xx_hal::pac;

/// ITCM: DMA1 cannot reach it, so the stream's next read bus-errors.
const UNREACHABLE: u32 = 0;
/// Far longer than a transfer takes to drain once EN is cleared.
const SPINS: u32 = 100_000;

/// Stream `n` (1 or 2) disabled, aimed at nowhere and enabled again. Never
/// waits unboundedly: `None` if the stream would not stop, or `n` is not 1 or 2.
/// The audio interrupt keeps running; it counts and re-arms the error.
pub fn dma_transfer_error(n: u8) -> Option<()> {
    if !(1..=2).contains(&n) {
        return None;
    }
    // SAFETY: main-loop code touching one trailing stream's CR and M0AR; the
    // audio interrupt reads only its flags and NDTR and re-arms it only after a
    // TEIF, which this raises last.
    let st = unsafe { &(*pac::DMA1::ptr()).st[usize::from(n)] };
    st.cr.modify(|_, w| w.en().disabled());
    let mut spins = SPINS;
    while st.cr.read().en().is_enabled() {
        spins = spins.checked_sub(1)?;
    }
    // SAFETY: EN is clear, so M0AR is writable; the address is deliberately
    // one DMA1 cannot read, which is the fault under test.
    unsafe { st.m0ar.write(|w| w.m0a().bits(UNREACHABLE)) };
    st.cr.modify(|_, w| w.en().enabled());
    Some(())
}

/// Reads per stream: well over a ring period (2.7 ms) at a register read's cost.
const SAMPLES: u32 = 200_000;

/// The three streams' CR, NDTR and M0AR, with the NDTR range seen over
/// `SAMPLES` rounds of reads. Read-only; the audio interrupt runs through it.
pub fn dump() -> [StreamSnap; 3] {
    // SAFETY: only reads DMA1's stream registers, which have no read side
    // effects.
    let dma1 = unsafe { &*pac::DMA1::ptr() };
    let mut out = [StreamSnap {
        cr: 0,
        ndtr: 0,
        m0ar: 0,
        ndtr_min: u16::MAX,
        ndtr_max: 0,
    }; 3];
    for _ in 0..SAMPLES {
        for (s, st) in out.iter_mut().zip(&dma1.st) {
            let n = st.ndtr.read().ndt().bits();
            s.ndtr_min = s.ndtr_min.min(n);
            s.ndtr_max = s.ndtr_max.max(n);
        }
    }
    for (s, st) in out.iter_mut().zip(&dma1.st) {
        (s.cr, s.ndtr, s.m0ar) = (
            st.cr.read().bits(),
            st.ndtr.read().ndt().bits(),
            st.m0ar.read().bits(),
        );
    }
    out
}
