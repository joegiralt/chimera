use core::mem::MaybeUninit;
use core::ptr::{addr_of, addr_of_mut};
use core::sync::atomic::{AtomicU8, Ordering};

use chimera_engine::audio_out::{
    DacSample, Half, WARMUP_BLOCKS, desynced, plan_halves, rearm_due, trail_verdict,
};
use chimera_engine::hw::{BLOCK_SIZE, DAC_PAIRS};
use chimera_engine::part::DacPair;
use cortex_m::peripheral::NVIC;
use stm32h7xx_hal::pac::{self, interrupt};

use chimera_board::priority::{self, Priority};
use chimera_board::shared::{BLOCKS, DESYNCS, DMA_ERRORS, FIFO_ERRORS, OVERRUNS, bump};

pub const RING_WORDS: usize = 2 * BLOCK_SIZE * 2;
// DMAMUX1 requests for SAI1_A, SAI1_B, SAI2_A (RM0433; stock PreenFM3 uses the same).
const REQUEST_ID: [u8; DAC_PAIRS] = [87, 88, 89];

#[repr(C, align(32))]
struct Rings([[[DacSample; BLOCK_SIZE * 2]; 2]; DAC_PAIRS]);
const _: () = assert!(core::mem::size_of::<Rings>() == DAC_PAIRS * RING_WORDS * 4);
// The card's staging shares the non-cacheable DMA region (ram_d2.x).
const _: () = assert!(
    core::mem::size_of::<Rings>() + chimera_board::card_dma::STAGING_BYTES
        <= chimera_engine::hw::D2_DMA_RESERVE
);

// SAFETY: ".ram_d2.dma" is real RAM_D2 SRAM (linker-defined) and NOLOAD, so
// this static holds garbage at boot; sound because the type is `MaybeUninit`
// and `clear` zero-fills it in place before any read.
#[unsafe(link_section = ".ram_d2.dma")]
static mut RINGS: MaybeUninit<Rings> = MaybeUninit::uninit();

// Streams 1 and 2 may trail stream 0 by the SAI FIFO (8 words) plus the DMA's.
// Not measurable from the code or RM: it depends on arbitration at start, so
// 16 stays; `stats` desyncs on a healthy unit shows if it is too tight.
const DESYNC_TOLERANCE: u16 = 16;

pub fn clear() {
    // SAFETY: before any DMA runs; D2 is NOLOAD, and zero bytes are valid
    // `DacSample`s, so later references point at initialised memory.
    unsafe { addr_of_mut!(RINGS).cast::<Rings>().write_bytes(0, 1) };
}

/// # Safety
/// `clear` must have run, and the caller must be the only writer of this
/// half: the audio interrupt, or the pre-fill before it is unmasked, while the
/// DMA reads the other half. The reference must be dropped before the next call.
pub(super) unsafe fn half_mut(
    pair: DacPair,
    half: Half,
) -> &'static mut [DacSample; BLOCK_SIZE * 2] {
    // SAFETY: upheld by the caller, per this function's contract.
    unsafe { &mut (*addr_of_mut!(RINGS).cast::<Rings>()).0[pair.index()][half.index()] }
}

fn ring_addr(pair: DacPair) -> u32 {
    addr_of!(RINGS) as u32 + (pair.index() * RING_WORDS * 4) as u32
}

pub fn init(nvic: &mut NVIC) {
    // SAFETY: single-threaded init before the stream-0 interrupt is unmasked;
    // DMA1 and DMAMUX1 are used only here and in the handler below.
    let (rcc, dma1, dmamux) =
        unsafe { (&*pac::RCC::ptr(), &*pac::DMA1::ptr(), &*pac::DMAMUX1::ptr()) };
    rcc.ahb1enr.modify(|_, w| w.dma1en().set_bit());
    let _ = rcc.ahb1enr.read();
    clear_flags(dma1);
    for pair in DacPair::ALL {
        configure_stream(dma1, dmamux, pair, pair == DacPair::P1);
    }
    priority::set_irq(nvic, pac::Interrupt::DMA1_STR0, Priority::AUDIO);
    // SAFETY: the rings are pre-filled; the handler touches only the rings,
    // the render path, DMA1's LISR/LIFCR and the NDTR of streams 0–2.
    unsafe { NVIC::unmask(pac::Interrupt::DMA1_STR0) };
}

pub fn start() {
    // SAFETY: called once from `main` after `init`; only EN is set.
    let dma1 = unsafe { &*pac::DMA1::ptr() };
    for pair in DacPair::ALL {
        dma1.st[pair.index()].cr.modify(|_, w| w.en().enabled());
    }
}

fn configure_stream(
    dma1: &pac::dma1::RegisterBlock,
    dmamux: &pac::dmamux1::RegisterBlock,
    pair: DacPair,
    interrupts: bool,
) {
    let st = &dma1.st[pair.index()];
    st.cr.modify(|_, w| w.en().disabled());
    while st.cr.read().en().is_enabled() {}
    // SAFETY: the request ID is this pair's SAI block; PAR is that block's
    // data register and M0AR a word-aligned ring of RING_WORDS words in D2,
    // which DMA1 can reach.
    unsafe {
        dmamux.ccr[pair.index()].modify(|_, w| w.dmareq_id().bits(REQUEST_ID[pair.index()]));
        st.par
            .write(|w| w.pa().bits(super::sai::data_register(pair)));
        st.m0ar.write(|w| w.m0a().bits(ring_addr(pair)));
    }
    st.ndtr.write(|w| w.ndt().bits(RING_WORDS as u16));
    st.cr.write(|w| {
        let w = w
            .dir()
            .memory_to_peripheral()
            .circ()
            .enabled()
            .minc()
            .incremented()
            .pinc()
            .fixed()
            .msize()
            .bits32()
            .psize()
            .bits32()
            .pl()
            .very_high();
        if interrupts {
            w.htie().enabled().tcie().enabled().teie().enabled()
        } else {
            w
        }
    });
}

fn clear_flags(dma1: &pac::dma1::RegisterBlock) {
    dma1.lifcr.write(|w| {
        w.ctcif0()
            .clear()
            .chtif0()
            .clear()
            .cteif0()
            .clear()
            .cdmeif0()
            .clear()
            .cfeif0()
            .clear()
            .ctcif1()
            .clear()
            .chtif1()
            .clear()
            .cteif1()
            .clear()
            .cdmeif1()
            .clear()
            .cfeif1()
            .clear()
            .ctcif2()
            .clear()
            .chtif2()
            .clear()
            .cteif2()
            .clear()
            .cdmeif2()
            .clear()
            .cfeif2()
            .clear()
    });
}

fn clear_trailing_errors(dma1: &pac::dma1::RegisterBlock, pair: DacPair) {
    dma1.lifcr.write(|w| match pair {
        DacPair::P2 => w.cteif1().clear().cfeif1().clear().cdmeif1().clear(),
        _ => w.cteif2().clear().cfeif2().clear().cdmeif2().clear(),
    });
}

/// Trailing streams halted by a transfer error, one bit per pair, waiting
/// for stream 0's next wrap.
static PENDING: AtomicU8 = AtomicU8::new(0);

/// Restart a stream the hardware halted on a transfer error, at its ring's
/// base: the call is made as stream 0 wraps, so the two are in phase to
/// within the interrupt's latency. Never waits.
fn rearm(dma1: &pac::dma1::RegisterBlock, pair: DacPair) {
    let st = &dma1.st[pair.index()];
    if st.cr.read().en().is_enabled() {
        return;
    }
    // SAFETY: EN is clear, so M0AR and NDTR are writable; the address is this
    // pair's ring base.
    unsafe { st.m0ar.write(|w| w.m0a().bits(ring_addr(pair))) };
    st.ndtr.write(|w| w.ndt().bits(RING_WORDS as u16));
    clear_trailing_errors(dma1, pair);
    st.cr.modify(|_, w| w.en().enabled());
}

#[interrupt]
fn DMA1_STR0() {
    // SAFETY: once `init` has run, this handler is the only user of DMA1's
    // LISR and LIFCR (it clears stream 0's HT/TC and streams 1–2's error flags), reads
    // their NDTR and re-arms a halted stream 1 or 2.
    let dma1 = unsafe { &*pac::DMA1::ptr() };
    let lisr = dma1.lisr.read();
    // A transfer error clears stream 0's EN (RM0433, DMA error management):
    // no more HT or TC, so nothing renders while streams 1 and 2 loop their
    // last ring.
    if lisr.teif0().is_error() {
        crate::panic::silence_and_halt();
    }
    let (half_done, full_done) = (lisr.htif0().is_half(), lisr.tcif0().is_complete());
    dma1.lifcr.write(|w| {
        if half_done {
            w.chtif0().clear();
        }
        if full_done {
            w.ctcif0().clear();
        }
        w
    });
    let ndtr = |p: DacPair| dma1.st[p.index()].ndtr.read().ndt().bits();
    let lead = ndtr(DacPair::P1);
    let warm = BLOCKS.load(Ordering::Relaxed) >= WARMUP_BLOCKS;
    for (pair, te, fe) in [
        (
            DacPair::P2,
            lisr.teif1().is_error(),
            lisr.feif1().is_error(),
        ),
        (
            DacPair::P3,
            lisr.teif2().is_error(),
            lisr.feif2().is_error(),
        ),
    ] {
        let bit = 1 << pair.index();
        let pending = PENDING.load(Ordering::Relaxed) & bit != 0;
        let apart = desynced(lead, ndtr(pair), RING_WORDS as u16, DESYNC_TOLERANCE);
        let v = trail_verdict(te, fe, apart, pending, warm);
        bump(&DESYNCS, v.drift as u32);
        bump(&DMA_ERRORS, v.transfer_error as u32);
        bump(&FIFO_ERRORS, v.fifo_error as u32);
        if te || fe {
            clear_trailing_errors(dma1, pair);
        }
        if rearm_due(pending || te, full_done) {
            rearm(dma1, pair);
            PENDING.fetch_and(!bit, Ordering::Relaxed);
        } else if te {
            PENDING.fetch_or(bit, Ordering::Relaxed);
        }
    }
    let plan = plan_halves(half_done, full_done);
    for half in plan.halves.into_iter().flatten() {
        // SAFETY: this is the audio interrupt, `measure`'s one caller.
        unsafe { chimera_board::probe::measure(|| super::render_half(half)) };
        BLOCKS.fetch_add(1, Ordering::Relaxed);
    }
    let after = dma1.lisr.read();
    let late = after.htif0().is_half() || after.tcif0().is_complete();
    let overruns = plan.overrun as u32 + late as u32;
    if overruns > 0 {
        bump(&OVERRUNS, overruns);
    }
}
