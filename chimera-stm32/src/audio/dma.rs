use core::mem::MaybeUninit;
use core::ptr::{addr_of, addr_of_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use chimera_core::audio_out::{DacSample, Half, plan_halves};
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS};
use chimera_core::part::DacPair;
use cortex_m::peripheral::NVIC;
use stm32h7xx_hal::pac::{self, interrupt};

use crate::priority::{self, Priority};

pub const RING_WORDS: usize = 2 * BLOCK_SIZE * 2;
// DMAMUX1 requests for SAI1_A, SAI1_B, SAI2_A (RM0433; stock PreenFM3 uses the same).
const REQUEST_ID: [u8; DAC_PAIRS] = [87, 88, 89];

#[repr(C, align(32))]
struct Rings([[[DacSample; BLOCK_SIZE * 2]; 2]; DAC_PAIRS]);
const _: () = assert!(core::mem::size_of::<Rings>() == DAC_PAIRS * RING_WORDS * 4);

#[unsafe(link_section = ".ram_d2.dma")]
static mut RINGS: MaybeUninit<Rings> = MaybeUninit::uninit();

pub static OVERRUNS: AtomicU32 = AtomicU32::new(0);

pub fn clear() {
    // SAFETY: before any DMA runs; D2 is NOLOAD, and zero bytes are valid
    // `DacSample`s, so later references point at initialised memory.
    unsafe { addr_of_mut!(RINGS).cast::<Rings>().write_bytes(0, 1) };
}

pub fn half_mut(pair: DacPair, half: Half) -> &'static mut [DacSample; BLOCK_SIZE * 2] {
    // SAFETY: `clear` ran first; only the audio interrupt, and the pre-fill
    // before it is unmasked, write the rings, one half at a time, while the
    // DMA reads the other half.
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
    configure_stream(dma1, dmamux, DacPair::P1, true);
    priority::set_irq(nvic, pac::Interrupt::DMA1_STR0, Priority::AUDIO);
    // SAFETY: the rings are pre-filled; the handler touches only the rings,
    // the render path and DMA1's stream 0–2 flags.
    unsafe { NVIC::unmask(pac::Interrupt::DMA1_STR0) };
}

pub fn start() {
    // SAFETY: called once from `main` after `init`; only EN is set.
    let dma1 = unsafe { &*pac::DMA1::ptr() };
    dma1.st[DacPair::P1.index()]
        .cr
        .modify(|_, w| w.en().enabled());
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
            w.htie().enabled().tcie().enabled()
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

#[interrupt]
fn DMA1_STR0() {
    // SAFETY: once `start` has run, this handler is the only reader and
    // clearer of DMA1's stream 0–2 flags.
    let dma1 = unsafe { &*pac::DMA1::ptr() };
    let lisr = dma1.lisr.read();
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
    let plan = plan_halves(half_done, full_done);
    for half in plan.halves.into_iter().flatten() {
        super::render_half(half);
    }
    let after = dma1.lisr.read();
    let late = after.htif0().is_half() || after.tcif0().is_complete();
    let overruns = plan.overrun as u32 + late as u32;
    if overruns > 0 {
        OVERRUNS.fetch_add(overruns, Ordering::Relaxed);
    }
}
