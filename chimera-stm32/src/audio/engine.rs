use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_core::audio_out::{Half, interleave};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS, SAMPLE_RATE, SampleBudget};
use chimera_core::instrument::{AudioShared, DacOut, Instrument};
use chimera_core::note_queue::NoteSources;
#[cfg(feature = "midi-din")]
use chimera_core::note_queue::SourceId;
use chimera_core::part::DacPair;
use chimera_core::scope::{ScopeFrame, ScopeWriter};
use chimera_core::triple::{Reader, Writer};

use super::dma;

pub const NOTE_SOURCES: usize = 1;
#[cfg(feature = "midi-din")]
pub const DIN: SourceId<NOTE_SOURCES> = SourceId::new(0);
pub static NOTES: NoteSources<NOTE_SOURCES> = NoteSources::new();

#[unsafe(link_section = ".ram_d2.voices")]
static mut INSTRUMENT: MaybeUninit<Instrument> = MaybeUninit::uninit();
static mut FX: MaybeUninit<FxBus> = MaybeUninit::uninit();

struct Engine {
    inst: &'static mut Instrument,
    fx: &'static mut FxBus,
    shared: Reader<AudioShared>,
    scope: ScopeWriter,
    dac: DacOut,
}

static mut ENGINE: MaybeUninit<Engine> = MaybeUninit::uninit();
static ENGINE_TAKEN: AtomicBool = AtomicBool::new(false);
static ENGINE_READY: AtomicBool = AtomicBool::new(false);

/// # Safety
/// Only before `init`, and from one context at a time.
pub unsafe fn slots() -> (
    &'static mut MaybeUninit<Instrument>,
    &'static mut MaybeUninit<FxBus>,
) {
    // SAFETY: the caller guarantees no other reference to these statics is live.
    unsafe { (&mut *addr_of_mut!(INSTRUMENT), &mut *addr_of_mut!(FX)) }
}

pub fn init(budget: SampleBudget, shared: Reader<AudioShared>, scope: Writer<ScopeFrame>) {
    if ENGINE_TAKEN.swap(true, Ordering::AcqRel) {
        return;
    }
    // SAFETY: the flag lets exactly one caller past, and `render_half` does
    // not touch these statics until `ENGINE_READY` is set below, so these are
    // the only references. The Instrument (D2) and FX bus (AXI) are built in
    // place; `Engine` (3.5 KB) is written by value.
    unsafe {
        let (inst_slot, fx_slot) = slots();
        let inst = Instrument::init_in_place(inst_slot, SAMPLE_RATE, budget);
        let fx = FxBus::init_in_place(fx_slot);
        (*addr_of_mut!(ENGINE)).write(Engine {
            inst,
            fx,
            shared,
            scope: ScopeWriter::new(scope),
            dac: [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS],
        });
    }
    ENGINE_READY.store(true, Ordering::Release);
}

pub fn render_half(half: Half) {
    if !ENGINE_READY.load(Ordering::Acquire) {
        return;
    }
    // SAFETY: `ENGINE` is initialised (READY is set only after its write).
    // Only the DMA1 stream 0 interrupt and `prefill` (before that interrupt
    // is unmasked) call this, never concurrently and never re-entrantly, so
    // this is the only live reference to `ENGINE`.
    let e = unsafe { (*addr_of_mut!(ENGINE)).assume_init_mut() };
    let shared = e.shared.read();
    NOTES.drain(|ev| e.inst.handle(ev, shared));
    e.inst.render(e.fx, &mut e.dac, shared, &mut e.scope);
    for pair in DacPair::ALL {
        // SAFETY: `main` runs `dma::clear` before `prefill`; the caller is
        // this half's only writer while the DMA reads the other half, and the
        // reference ends with this `interleave` call.
        interleave(&e.dac, pair, unsafe { dma::half_mut(pair, half) });
    }
}
