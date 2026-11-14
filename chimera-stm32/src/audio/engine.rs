use core::cell::Cell;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_board::controls::now_ms;
use chimera_board::sd::BUS;
use chimera_board::shared::{BLOCKS, LOAD_SHEDS, NOTE_SOURCES, STREAM_STATS, bump, registry};
#[cfg(feature = "perf-probe")]
use chimera_board::shared::{COST_PCT, VOICES};
use chimera_core::console::{LOAD_NET, LoadNetState};
#[cfg(feature = "bench")]
use chimera_engine::audio_out::DacSample;
use chimera_engine::audio_out::{Half, interleave};
use chimera_engine::dsp::algo::engine::FormantViews;
use chimera_engine::dsp::fx_bus::FxBus;
use chimera_engine::hw::{BLOCK_SIZE, DAC_PAIRS, SAMPLE_RATE, SampleBudget};
use chimera_engine::instrument::{AudioShared, DacBlocks, Instrument, NoteOrigin};
use chimera_engine::note_queue::{InEvent, NoteDrain};
use chimera_engine::orbit::door::OrbitPorts;
use chimera_engine::part::DacPair;
use chimera_engine::perf::load::LoadNet;
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
    /// Each DMA half's pairs holding only 0 words (`put`).
    dma_zero: [[bool; DAC_PAIRS]; 2],
    /// The host's USB playback's audio side (ADR 0260).
    #[cfg(all(feature = "usb-audio", not(feature = "usb-config-writer")))]
    usb_in: (chimera_midi::uac_out::Drain, chimera_midi::uac_out::Fade),
    /// Holds the note queues through a project load (ADR 0046).
    gate: LoadGate,
    orbit: chimera_board::orbit::Runner,
    /// The source table's one reader (ADR 0087).
    sources: AudioToken,
    /// The stream rings' audio side: the voices' ports, or the card
    /// bench's heads where it runs.
    streams: AudioSide<'static>,
    /// The load net (ADR 0273): none where the cycle counter won't count.
    net: Option<LoadNet>,
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
    net: Option<LoadNet>,
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
            dma_zero: [[false; DAC_PAIRS]; 2],
            #[cfg(all(feature = "usb-audio", not(feature = "usb-config-writer")))]
            usb_in: (
                chimera_midi::uac_out::Drain::new(),
                chimera_midi::uac_out::Fade::new(),
            ),
            gate: LoadGate::new(),
            orbit,
            sources: {
                // One block a half (ADR 0099's drain wait).
                sources.set_period_us(BLOCK_SIZE as u32 * 1_000_000 / SAMPLE_RATE);
                sources
            },
            streams,
            net,
        });
    }
    // The console's `status` says whether the net runs.
    LOAD_NET.store(match net {
        Some(_) => LoadNetState::On,
        None => LoadNetState::Off,
    });
    ENGINE_READY.store(true, Ordering::Release);
}

/// One half rendered before the audio interrupt runs (`prefill`).
pub fn render_half(half: Half) {
    // SAFETY: only `prefill` (before the audio interrupt is unmasked) calls
    // this, so no other reference to `ENGINE` is live.
    if let Some(e) = unsafe { engine() } {
        render(e, half);
    }
}

/// The audio interrupt's half: `render_half`, timed into the probe; one
/// past the load net's threshold sheds a voice before the next (ADR 0273,
/// `LoadNet::after`), counted in `LOAD_SHEDS`.
///
/// # Safety
/// Only from the audio interrupt, `probe::measure`'s one caller.
pub unsafe fn render_half_timed(half: Half) {
    // SAFETY: the caller is the audio interrupt (this fn's contract), which
    // does not re-enter and runs only after `prefill`: the one reference to
    // `ENGINE`, and `measure`'s one caller.
    unsafe {
        let Some(e) = engine() else {
            return;
        };
        let took = chimera_board::probe::measure(|| render(e, half));
        if e.net.is_some_and(|net| net.after(took, e.inst)) {
            bump(&LOAD_SHEDS, 1);
        }
    }
}

/// `ENGINE`, once `init` has written it.
///
/// # Safety
/// Only from the audio interrupt or `prefill` (before that interrupt is
/// unmasked), never concurrently or re-entrantly: no other reference to
/// `ENGINE` may be live.
unsafe fn engine() -> Option<&'static mut Engine> {
    if !ENGINE_READY.load(Ordering::Acquire) {
        return None;
    }
    // SAFETY: `ENGINE` is initialised (READY is set only after its write),
    // and the caller holds no other reference to it (this fn's contract).
    Some(unsafe { (*addr_of_mut!(ENGINE)).assume_init_mut() })
}

/// One half, as the desktop's `Engine::block` renders a block: the gate,
/// then only when it opens the drain (live notes to ORBIT's capture and
/// then the Parts, real-time to ORBIT's clock) and ORBIT, then the render
/// and ORBIT's view.
fn render(e: &mut Engine, half: Half) {
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
    // The capture taps the main out before the host's playback joins it
    // (ADR 0260): the host never records itself.
    #[cfg(feature = "usb-audio")]
    usb_out(&e.dac);
    #[cfg(all(feature = "usb-audio", not(feature = "usb-config-writer")))]
    usb_in(&mut e.dac, &mut e.usb_in);
    for pair in DacPair::ALL {
        put(
            &e.dac,
            pair,
            half,
            &mut e.dma_zero[half.index()][pair.index()],
        );
        #[cfg(feature = "bench")]
        if chimera_board::CARD_BENCH && pair == DacPair::P1 && BENCH.listening() {
            // SAFETY: as `put`'s; this reference ends with the turn.
            bench_listen(&mut e.streams, unsafe { dma::half_mut(pair, half) });
            // Its words are the bench's now, not the block's.
            e.dma_zero[half.index()][pair.index()] = false;
        }
    }
}

/// The main out (DAC pair 1) into USB audio's ring as its DAC plays it
/// (ADR 0248): the same `to_dac` words, from the cached block, not the
/// uncached DMA half. A ring the host isn't reading is full and refuses
/// the block whole.
#[cfg(feature = "usb-audio")]
#[inline(always)]
fn usb_out(dac: &DacBlocks) {
    use chimera_engine::audio_out::to_dac;
    let p = &dac.out()[DacPair::P1.index()];
    chimera_board::usb_audio::RING.push(BLOCK_SIZE, |i| {
        [to_dac(p[2 * i]).get(), to_dac(p[2 * i + 1]).get()]
    });
}

/// The host's USB playback into the main out (ADR 0260), after the
/// capture's tap: a block from its ring at USB IN's step, eased from the
/// last block's gain; silence while the host isn't playing, until the
/// ring holds its target, and whenever it runs dry (counted).
#[cfg(all(feature = "usb-audio", not(feature = "usb-config-writer")))]
#[inline(never)]
fn usb_in(
    dac: &mut DacBlocks,
    (drain, fade): &mut (chimera_midi::uac_out::Drain, chimera_midi::uac_out::Fade),
) {
    use chimera_board::usb_audio::{PLAY, PLAY_LEVEL, PLAY_ON, PLAY_SHORT};
    use chimera_midi::uac_out::{self, Counts, Fade};
    const _: () = assert!(BLOCK_SIZE == uac_out::BLOCK);
    if !PLAY_ON.load(Ordering::Acquire) {
        drain.stop();
        *fade = Fade::new();
        PLAY.skip(PLAY.len());
        return;
    }
    let level = uac_out::gain(PLAY_LEVEL.load(Ordering::Relaxed));
    let (pairs, zero) = dac.out_mut();
    let mut c = Counts::default();
    let p = DacPair::P1.index();
    if uac_out::play(&PLAY, drain, &mut c, fade, level, &mut pairs[p]) {
        zero[p] = false;
    }
    if c.short != 0 {
        PLAY_SHORT.fetch_add(1, Ordering::Relaxed);
    }
}

/// `pair`'s words of the block out into its DMA `half`: none to write
/// where the pair is +0 and the half already holds its 0 words (`held`,
/// #793). One copy of the conversion's loop for the three pairs.
#[inline(never)]
fn put(dac: &DacBlocks, pair: DacPair, half: Half, held: &mut bool) {
    let zero = dac.is_zero(pair);
    if zero && *held {
        return;
    }
    *held = zero;
    // SAFETY: `main` runs `dma::clear` before `prefill`; the caller
    // (`render_half`) is this half's only writer while the DMA reads the
    // other half, and the reference ends here.
    interleave(dac.out(), pair, unsafe { dma::half_mut(pair, half) });
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

/// The program-change hold (ADR 0224): AXI `.bss`, only where a program
/// change is heard.
#[cfg(feature = "midi-map")]
static HOLD: chimera_engine::note_queue::ProgramHold =
    chimera_engine::note_queue::ProgramHold::new();

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
            let play = |src: chimera_engine::note_queue::SourceId<NOTE_SOURCES>, ev| match ev {
                // Capture sees the raw key, before any snap.
                InEvent::Note(n) => {
                    orbit.live(n);
                    inst.handle_streamed(n, shared, NoteOrigin::Live, reads, ports);
                }
                InEvent::Bend(b) => inst.bend(b),
                InEvent::Sustain(p) => inst.sustain(p),
                InEvent::Mode(m) => inst.channel_mode(m),
                InEvent::Expr(e) => inst.expr(e),
                InEvent::Rt(rt) => orbit.realtime(src.index(), rt, now),
            };
            // Only the MIDI map hears a program change (ADR 0224): without
            // it, no hold.
            #[cfg(feature = "midi-map")]
            notes.drain_held(&HOLD, play);
            #[cfg(not(feature = "midi-map"))]
            notes.drain(play);
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
