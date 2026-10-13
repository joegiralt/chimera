use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_board::shared::NOTE_SOURCES;
#[cfg(feature = "perf-probe")]
use chimera_board::shared::{COST_PCT, VOICES};
use chimera_engine::audio_out::{Half, interleave};
use chimera_engine::dsp::algo::engine::FormantViews;
use chimera_engine::dsp::fx_bus::FxBus;
use chimera_engine::hw::{BLOCK_SIZE, SAMPLE_RATE, SampleBudget};
use chimera_engine::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_engine::note_queue::{InEvent, NoteDrain};
use chimera_engine::part::DacPair;
use chimera_engine::project::{LOAD_LINK, LoadGate};
use chimera_engine::scope::{ScopeFrame, ScopeWriter};
use chimera_engine::source_table::AudioToken;
use chimera_engine::triple::{Reader, Writer};

use super::dma;

// SAFETY: ".ram_d2.voices" is real RAM_D2 SRAM (linker-defined) and NOLOAD,
// so this static holds garbage at boot; sound because the type is
// `MaybeUninit` and `init` writes it in place via `Instrument::init_in_place`
// before any read.
#[unsafe(link_section = ".ram_d2.voices")]
static mut INSTRUMENT: MaybeUninit<Instrument> = MaybeUninit::uninit();
static mut FX: MaybeUninit<FxBus> = MaybeUninit::uninit();

struct Engine {
    inst: &'static mut Instrument,
    fx: &'static mut FxBus,
    shared: Reader<AudioShared>,
    notes: NoteDrain<'static, NOTE_SOURCES>,
    scope: ScopeWriter,
    /// The voices' formant views, once a block, for the OP pages.
    formant: Writer<FormantViews>,
    dac: DacBlocks,
    /// Holds the note queues through a project load (ADR 0046).
    gate: LoadGate,
    orbit: chimera_board::orbit::Runner,
    /// The source table's one reader (ADR 0087).
    sources: AudioToken,
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

pub fn init(
    budget: SampleBudget,
    shared: Reader<AudioShared>,
    notes: NoteDrain<'static, NOTE_SOURCES>,
    scope: Writer<ScopeFrame>,
    formant: Writer<FormantViews>,
    orbit: chimera_board::orbit::Runner,
    mut sources: AudioToken,
) {
    if ENGINE_TAKEN.swap(true, Ordering::AcqRel) {
        return;
    }
    // SAFETY: the flag lets exactly one caller past, and `render_half` does
    // not touch these statics until `ENGINE_READY` is set below, so these are
    // the only references. The Instrument (D2) and FX bus (AXI) are built in
    // place; `Engine` (5 KB, 6 KB with ORBIT's runner) is written by value.
    unsafe {
        let (inst_slot, fx_slot) = slots();
        let inst = Instrument::init_in_place(inst_slot, SAMPLE_RATE, budget);
        let fx = FxBus::init_in_place(fx_slot);
        (*addr_of_mut!(ENGINE)).write(Engine {
            inst,
            fx,
            shared,
            notes,
            scope: ScopeWriter::new(scope),
            formant,
            dac: DacBlocks::new(),
            gate: LoadGate::new(),
            orbit,
            sources: {
                // One block a half (ADR 0099's drain wait).
                sources.set_period_us(BLOCK_SIZE as u32 * 1_000_000 / SAMPLE_RATE);
                sources
            },
        });
    }
    ENGINE_READY.store(true, Ordering::Release);
}

/// One half, as the desktop's `Engine::block` renders a block: the gate,
/// then only when it opens the drain (live notes to ORBIT's capture and
/// then the Parts, real-time to ORBIT's clock) and ORBIT, then the render
/// and ORBIT's view.
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
    // TAPE's voices read the pool through the block's view.
    let (gate, notes, orbit) = (&mut e.gate, &mut e.notes, &mut e.orbit);
    let out = (&mut *e.fx, &mut e.dac, &mut e.scope);
    e.inst
        .run_block(&mut e.sources, out, shared, |inst, reads| {
            if gate.before_block(&LOAD_LINK, inst, shared) {
                let now = orbit.now();
                notes.drain(|src, ev| match ev {
                    // Capture sees the raw key, before any snap.
                    InEvent::Note(n) => {
                        orbit.live(n);
                        inst.handle_with(n, shared, reads);
                    }
                    InEvent::Bend(b) => inst.bend(b),
                    InEvent::Sustain(p) => inst.sustain(p),
                    InEvent::Rt(rt) => orbit.realtime(src.index(), rt, now),
                });
                orbit.block(inst, shared, reads);
            }
        });
    e.orbit.publish();
    let inst = &*e.inst;
    e.formant.publish(|v| inst.formant_views_into(v));
    #[cfg(feature = "perf-probe")]
    {
        VOICES.store(e.inst.sounding() as u8, Ordering::Relaxed);
        COST_PCT.store(e.inst.cost_pct(), Ordering::Relaxed);
    }
    for pair in DacPair::ALL {
        // SAFETY: `main` runs `dma::clear` before `prefill`; the caller is
        // this half's only writer while the DMA reads the other half, and the
        // reference ends with this `interleave` call.
        interleave(e.dac.out(), pair, unsafe { dma::half_mut(pair, half) });
    }
}
