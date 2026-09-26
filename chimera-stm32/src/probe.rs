#[cfg(feature = "perf-probe")]
pub use imp::*;
#[cfg(not(feature = "perf-probe"))]
pub use stub::*;

#[cfg(feature = "perf-probe")]
mod imp {
    use core::ptr::{addr_of, addr_of_mut};
    use core::sync::atomic::{AtomicBool, Ordering};

    use chimera_core::clock_plan::SiliconRev;
    use chimera_core::hw::BlockBudget;
    use chimera_core::perf::load::AudioStats;
    use chimera_core::perf::stack::{STACK_PAINT, untouched_words};
    use chimera_core::triple::{Reader, TripleBuffer, Writer};
    use cortex_m::peripheral::{DCB, DWT};

    use crate::audio::{dma, engine};
    use crate::clocks::Clocks;

    const BLANK: AudioStats = AudioStats::new(SiliconRev::Unknown(0), 0);
    const PAINT_MARGIN: usize = 256;

    static mut STATS_BUF: TripleBuffer<AudioStats> = TripleBuffer::new(BLANK, BLANK, BLANK);
    static mut STATS: AudioStats = BLANK;
    static mut WRITER: Option<Writer<AudioStats>> = None;
    static mut BUDGET: BlockBudget = BlockBudget::for_cpu(chimera_core::hw::CPU_HZ_REV_Y);
    static READY: AtomicBool = AtomicBool::new(false);
    static TAKEN: AtomicBool = AtomicBool::new(false);

    unsafe extern "C" {
        static _stack_start: u32;
        static _stack_end: u32;
    }

    pub fn paint_stack() {
        let bottom = (&raw const _stack_end) as *mut u32;
        let limit = cortex_m::register::msp::read() as usize - PAINT_MARGIN;
        let mut p = bottom;
        while (p as usize) < limit {
            // SAFETY: [_stack_end, SP − margin) is DTCM stack below every live
            // frame (main's included), and nothing else runs yet.
            unsafe {
                p.write_volatile(STACK_PAINT);
                p = p.add(1);
            }
        }
    }

    pub fn stack_used() -> u32 {
        let bottom = &raw const _stack_end;
        let words = ((&raw const _stack_start) as usize - bottom as usize) / 4;
        // SAFETY: volatile reads inside the linker's stack region; a read that
        // races an interrupt's frame only moves the mark by that word.
        let untouched =
            untouched_words((0..words).map(|i| unsafe { bottom.add(i).read_volatile() }));
        ((words - untouched) * 4) as u32
    }

    pub fn enable_cycle_counter(dcb: &mut DCB, dwt: &mut DWT) -> bool {
        dcb.enable_trace();
        dwt.enable_cycle_counter();
        if counting() {
            return true;
        }
        // Without a debugger the M7's DWT can come up software-locked.
        DWT::unlock();
        dwt.enable_cycle_counter();
        counting()
    }

    fn counting() -> bool {
        let start = DWT::cycle_count();
        cortex_m::asm::delay(1_000);
        DWT::cycle_count() != start
    }

    pub fn init(dcb: &mut DCB, dwt: &mut DWT, clocks: Clocks) -> Option<Reader<AudioStats>> {
        if !enable_cycle_counter(dcb, dwt) || TAKEN.swap(true, Ordering::AcqRel) {
            return None;
        }
        // SAFETY: the flag lets one caller past, before the audio interrupt is
        // unmasked, so nothing else touches these statics yet.
        unsafe {
            *addr_of_mut!(STATS) = AudioStats::new(clocks.rev, clocks.cpu_hz);
            *addr_of_mut!(BUDGET) = BlockBudget::for_cpu(clocks.cpu_hz);
            let (w, r) = (&mut *addr_of_mut!(STATS_BUF)).split();
            *addr_of_mut!(WRITER) = Some(w);
            READY.store(true, Ordering::Release);
            Some(r)
        }
    }

    pub fn measure(render: impl FnOnce()) {
        if !READY.load(Ordering::Acquire) {
            return render();
        }
        let start = DWT::cycle_count();
        render();
        let cycles = DWT::cycle_count().wrapping_sub(start);
        // SAFETY: after `init`, only the audio interrupt calls `measure`, and
        // it does not re-enter.
        let (stats, budget, writer) = unsafe {
            (
                &mut *addr_of_mut!(STATS),
                *addr_of!(BUDGET),
                (*addr_of_mut!(WRITER)).as_mut(),
            )
        };
        stats.record(cycles, budget);
        stats.overruns = dma::OVERRUNS.load(Ordering::Relaxed);
        stats.desyncs = dma::DESYNCS.load(Ordering::Relaxed);
        let drops = engine::NOTES.drops();
        stats.drops[..drops.len()].copy_from_slice(&drops);
        #[cfg(feature = "midi-din")]
        {
            let din = &mut stats.drops[engine::DIN.index()];
            *din = din.saturating_add(crate::midi_din::ERRORS.load(Ordering::Relaxed));
        }
        stats.sources = drops.len() as u8;
        if let Some(w) = writer {
            let s = *stats;
            w.publish(|out| *out = s);
        }
    }
}

#[cfg(not(feature = "perf-probe"))]
mod stub {
    use chimera_core::perf::load::AudioStats;
    use chimera_core::triple::Reader;
    use cortex_m::peripheral::{DCB, DWT};

    use crate::clocks::Clocks;

    pub fn paint_stack() {}

    pub fn stack_used() -> u32 {
        0
    }

    pub fn init(_: &mut DCB, _: &mut DWT, _: Clocks) -> Option<Reader<AudioStats>> {
        None
    }

    pub fn measure(render: impl FnOnce()) {
        render()
    }
}
