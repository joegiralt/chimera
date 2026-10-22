use core::cell::Cell;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_board::controls::now_ms;
use chimera_board::sd::BUS;
use chimera_board::shared::{BLOCKS, NOTE_SOURCES, STREAM_STATS, registry};
#[cfg(feature = "perf-probe")]
use chimera_board::shared::{COST_PCT, VOICES};
#[cfg(feature = "bench")]
use chimera_engine::audio_out::DacSample;
use chimera_engine::audio_out::{Half, interleave};
use chimera_engine::dsp::algo::engine::FormantViews;
use chimera_engine::dsp::fx_bus::FxBus;
use chimera_engine::hw::{BLOCK_SIZE, SAMPLE_RATE, SampleBudget};
use chimera_engine::instrument::{AudioShared, DacBlocks, Instrument, NoteOrigin};
use chimera_engine::note_queue::{InEvent, NoteDrain};
use chimera_engine::orbit::door::OrbitPorts;
use chimera_engine::part::DacPair;
use chimera_engine::project::SourceId;
use chimera_engine::project::{LOAD_LINK, LoadGate};
use chimera_engine::scope::{ScopeFrame, ScopeWriter};
use chimera_engine::source_table::AudioToken;
use chimera_engine::stream::layout::StreamFile;
use chimera_engine::stream::port::{NoPorts, StreamCard, Streams};
use chimera_engine::stream::ring::AudioSide;
use chimera_engine::stream::units::AudioBlocks;
use chimera_engine::triple::{Reader, Writer};

#[cfg(feature = "bench")]
use chimera_engine::stream::bench::{Asked, BENCH};

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
    /// The stream rings' audio side: the voices' ports, or the card
    /// bench's heads where it runs.
    streams: AudioSide<'static>,
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

#[allow(clippy::too_many_arguments)]
pub fn init(
    budget: SampleBudget,
    shared: Reader<AudioShared>,
    notes: NoteDrain<'static, NOTE_SOURCES>,
    scope: Writer<ScopeFrame>,
    formant: Writer<FormantViews>,
    orbit: chimera_board::orbit::Runner,
    mut sources: AudioToken,
    streams: AudioSide<'static>,
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
            streams,
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
    let run = Run {
        inst: &mut *e.inst,
        sources: &mut e.sources,
        out: (&mut *e.fx, &mut e.dac, &mut e.scope),
        gate: &mut e.gate,
        notes: &mut e.notes,
        orbit: &mut e.orbit,
    };
    #[cfg(feature = "bench")]
    if chimera_board::CARD_BENCH {
        bench_block(&mut e.streams);
    }
    if chimera_board::CARD_BENCH || !chimera_engine::stream::STREAMS {
        // No TAPE, or the rings are the card bench's: no voice streams.
        block(run, shared, &mut NoPorts);
        // Nothing streams: a write's quiet holds at once.
        BUS.answer_quiet(now_ms());
    } else {
        let reg = registry();
        let frames = |id: SourceId| reg?.get(id).map(StreamFile::frames);
        let mut ports = Streams {
            audio: &mut e.streams,
            card: StreamCard {
                frames: &frames,
                now: AudioBlocks::new(BLOCKS.load(Ordering::Relaxed)),
                bus: &BUS,
                stats: &STREAM_STATS,
                // Every new want wakes the card IRQ, whoever has the bus (I1).
                pend: chimera_board::card_dma::pend,
                loud: Cell::new(false),
            },
        };
        block(run, shared, &mut ports);
        ports.answer_quiet(now_ms());
    }
    BUS.set_streams_active(e.inst.streaming());
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
        // reference ends with this pair's turn of the loop.
        let out = unsafe { dma::half_mut(pair, half) };
        interleave(e.dac.out(), pair, out);
        #[cfg(feature = "bench")]
        if chimera_board::CARD_BENCH && pair == DacPair::P1 && BENCH.listening() {
            bench_listen(&mut e.streams, out);
        }
    }
}

/// What a block runs on: the engine's parts but the rings and the shared
/// snapshot.
struct Run<'a> {
    inst: &'a mut Instrument,
    sources: &'a mut AudioToken,
    out: (&'a mut FxBus, &'a mut DacBlocks, &'a mut ScopeWriter),
    gate: &'a mut LoadGate,
    notes: &'a mut NoteDrain<'static, NOTE_SOURCES>,
    orbit: &'a mut chimera_board::orbit::Runner,
}

/// One block's notes and render, every voice's stream through `ports`:
/// the gate, then only when it opens the drain (live notes to ORBIT's
/// capture and then the Parts, real-time to ORBIT's clock) and ORBIT.
#[inline(never)]
fn block<P: OrbitPorts>(r: Run<'_>, shared: &AudioShared, ports: &mut P) {
    let Run {
        inst,
        sources,
        out,
        gate,
        notes,
        orbit,
    } = r;
    inst.run_block_ported(sources, out, shared, ports, |inst, reads, ports| {
        if gate.before_block(&LOAD_LINK, inst, shared) {
            let now = orbit.now();
            notes.drain(|src, ev| match ev {
                // Capture sees the raw key, before any snap.
                InEvent::Note(n) => {
                    orbit.live(n);
                    inst.handle_streamed(n, shared, NoteOrigin::Live, reads, ports);
                }
                InEvent::Bend(b) => inst.bend(b),
                InEvent::Sustain(p) => inst.sustain(p),
                InEvent::Mode(m) => inst.channel_mode(m),
                InEvent::Rt(rt) => orbit.realtime(src.index(), rt, now),
            });
            orbit.block(inst, shared, reads, ports);
        }
    });
}

/// The bench's heads over the registry's bed rows (disarmed till the beds
/// are set): a new want wakes the card IRQ (I1's rule 1). Out of line, as
/// `bench_listen`, so its frame isn't on the stack under the render.
#[cfg(feature = "bench")]
#[inline(never)]
fn bench_block(streams: &mut AudioSide<'static>) {
    let now = AudioBlocks::new(BLOCKS.load(Ordering::Relaxed));
    if let Some(reg) = registry()
        && BENCH.block(&mut streams.turn(), reg, now) == Asked::New
    {
        chimera_board::card_dma::pend();
    }
}

/// Bench stream 0 in place of the synth on the master.
#[cfg(feature = "bench")]
#[inline(never)]
fn bench_listen(streams: &mut AudioSide<'static>, out: &mut [DacSample; BLOCK_SIZE * 2]) {
    let mut words = [0; BLOCK_SIZE * 2];
    BENCH.fill_listen(&streams.turn(), &mut words);
    for (o, w) in out.iter_mut().zip(words) {
        // A full-scale word is 2^31: the DAC's own −1..1.
        *o = chimera_engine::audio_out::to_dac(w as f32 / 2_147_483_648.0);
    }
}
