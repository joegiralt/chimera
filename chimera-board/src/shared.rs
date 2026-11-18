use core::mem::MaybeUninit;
use core::ptr::{addr_of, addr_of_mut};
#[cfg(feature = "perf-probe")]
use core::sync::atomic::AtomicU8;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering, compiler_fence};

use chimera_core::dsp::algo::engine::{FormantView, FormantViews};
use chimera_core::dsp::algo::waves::{self, Waves};
use chimera_core::dsp::knobs::KnobPublisher;
use chimera_core::hw::{self, MAX_PARTS};
use chimera_core::instrument::AudioShared;
#[cfg(any(feature = "midi-din", feature = "midi-usb"))]
use chimera_core::midi_in::CtrlPorts;
#[cfg(any(feature = "midi-din", feature = "midi-usb"))]
pub use chimera_core::midi_in::Port;
use chimera_core::note_queue::NoteSources;
#[cfg(any(feature = "midi-din", feature = "midi-usb", feature = "bench"))]
use chimera_core::note_queue::SourceId;
#[cfg(feature = "orbit")]
use chimera_core::orbit::runner::OrbitView;
#[cfg(all(feature = "midi-din", feature = "midi-map"))]
use chimera_core::out_ring::{OutPorts, OutRing};
use chimera_core::preset::Performance;
use chimera_core::project::Kits;
use chimera_core::project::arena::{ARENA_BYTES, ArenaWriter};
use chimera_core::scope::{ScopeFrame, scope_buffer};
use chimera_core::source_table::SOURCE_TABLE;
use chimera_core::stream::registry::StreamRegistry;
use chimera_core::stream::ring::{AudioSide, CardSide, Rings, ThirdSlots};
use chimera_core::stream::stats::StreamStats;
use chimera_core::triple::{Reader, TripleBuffer, Writer};
use chimera_core::ui::UiState;

static mut SCOPE: TripleBuffer<ScopeFrame> = scope_buffer();
static SCOPE_TAKEN: AtomicBool = AtomicBool::new(false);

pub fn take_scope() -> Option<(Writer<ScopeFrame>, Reader<ScopeFrame>)> {
    if SCOPE_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `SCOPE` ever made.
    Some(unsafe { &mut *addr_of_mut!(SCOPE) }.split())
}

// D3 SRAM4 (sram4.x): the stream rings. NOLOAD, so nothing reads it
// before `cache::sram4_self_test`, then `zero_rings` builds them in place.
#[unsafe(link_section = ".sram4")]
static mut RINGS: MaybeUninit<Rings> = MaybeUninit::uninit();
// Their third slots, which SRAM4 can't hold: 32 KB of D2 beside the UI,
// NOLOAD, built in place by `zero_rings` too. Cacheable, as the rings:
// only the CPU touches either.
#[unsafe(link_section = ".ram_d2")]
static mut THIRD: MaybeUninit<ThirdSlots> = MaybeUninit::uninit();
// The stream registry (slice 1 § 5.5): NOLOAD in D2, zeroed by
// `zero_rings` too.
#[unsafe(link_section = ".ram_d2")]
static mut REGISTRY: MaybeUninit<StreamRegistry> = MaybeUninit::uninit();
static RINGS_TAKEN: AtomicBool = AtomicBool::new(false);
static RINGS_ZEROED: AtomicBool = AtomicBool::new(false);

const SRAM4_WORDS: usize = 8;
const SRAM4_STRIDE: usize = hw::SRAM4 / SRAM4_WORDS;
// The self-test's last word lies inside the rings.
const _: () = assert!((SRAM4_WORDS - 1) * SRAM4_STRIDE + 4 <= size_of::<Rings>());

/// One word every 8 KiB of SRAM4, for its self-test.
pub fn sram4_words() -> [*mut u32; SRAM4_WORDS] {
    let base = addr_of_mut!(RINGS).cast::<u8>();
    // SAFETY: each offset is inside `RINGS` (asserted above), 4-aligned as
    // `Rings` is.
    core::array::from_fn(|i| unsafe { base.add(i * SRAM4_STRIDE) }.cast::<u32>())
}

/// Every slot Empty and every registry row free: after
/// `cache::sram4_self_test`, once.
pub fn zero_rings() {
    if RINGS_TAKEN.swap(true, Ordering::AcqRel) {
        return;
    }
    // SAFETY: the flag lets one caller past, before `rings`, `sides` or
    // `registry` hands out any reference (they wait for `RINGS_ZEROED`);
    // all-zero bytes are a valid `Rings`, `ThirdSlots` (ring.rs) and
    // `StreamRegistry` (registry.rs).
    unsafe {
        addr_of_mut!(RINGS).write_bytes(0, 1);
        addr_of_mut!(THIRD).write_bytes(0, 1);
        addr_of_mut!(REGISTRY).write_bytes(0, 1);
    }
    RINGS_ZEROED.store(true, Ordering::Release);
}

/// The rings, once `zero_rings` has built them.
pub fn rings() -> Option<&'static Rings> {
    // SAFETY: zeroed (a valid `Rings`) before the flag's `Release`, and only
    // ever touched again through `Rings`' own atomics and sides.
    RINGS_ZEROED
        .load(Ordering::Acquire)
        .then(|| unsafe { (*addr_of!(RINGS)).assume_init_ref() })
}

/// The stream registry, once `zero_rings` has emptied it.
#[inline]
pub fn registry() -> Option<&'static StreamRegistry> {
    // SAFETY: zeroed (empty) before the flag's `Release`, then touched only
    // through `StreamRegistry`'s own tags.
    RINGS_ZEROED
        .load(Ordering::Acquire)
        .then(|| unsafe { (*addr_of!(REGISTRY)).assume_init_ref() })
}

/// The rings' audio and card sides, once.
pub fn ring_sides() -> Option<(AudioSide<'static>, CardSide<'static>)> {
    // SAFETY: as `rings`: zeroed before the flag's `Release`, then touched
    // only through `ThirdSlots`' own flag and the sides.
    let third = RINGS_ZEROED
        .load(Ordering::Acquire)
        .then(|| unsafe { (*addr_of!(THIRD)).assume_init_ref() })?;
    rings()?.sides(third)
}

// The wave tables' zero-wait working copy, at the top of DTCM (dtcm.x).
#[unsafe(link_section = ".dtcm_waves")]
static mut WAVES_DTCM: MaybeUninit<Waves> = MaybeUninit::uninit();
static WAVES_TAKEN: AtomicBool = AtomicBool::new(false);

/// Renders the wave tables into DTCM and reads them from there (ADR 0122).
pub fn build_waves() {
    if WAVES_TAKEN.swap(true, Ordering::AcqRel) {
        return;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `WAVES_DTCM` ever made.
    waves::render_into(unsafe { &mut *addr_of_mut!(WAVES_DTCM) });
}

// `UiState` is ~27 KB: it lives in D2, not in `main`'s frame on the
// ~63.75 KB DTCM stack (ADR 0025), nor in AXI, which the MIDI map's 96 B a
// Sound filled to the margin (ADR 0087, 0139). NOLOAD: built in place by
// `take_ui`. D2 is cacheable apart from the 16 KB DMA window (`cache.rs`).
#[unsafe(link_section = ".ram_d2")]
static mut UI: MaybeUninit<UiState> = MaybeUninit::uninit();
static UI_TAKEN: AtomicBool = AtomicBool::new(false);

/// Proof that ITCM's state (`__sitcm`..`__eitcm`) was zeroed in whole
/// 64-bit words (ADR 0286): each in-place build into `.itcm` takes one, so
/// none can run first. Only `zero_itcm` makes one.
#[derive(Clone, Copy)]
pub struct ItcmZeroed(());

/// Zeroes ITCM's state with aligned 64-bit stores, the ECC word (RM0433
/// § 2.4, AN5342): no write into it is ever a sub-word one onto a word
/// whose ECC is still power-on noise.
///
/// # Safety
/// Once, in `boot`, before any interrupt is enabled: a second call would wipe what the
/// builds wrote, and nothing else may touch ITCM while it runs.
pub(crate) unsafe fn zero_itcm() -> ItcmZeroed {
    unsafe extern "C" {
        static mut __sitcm: u64;
        static mut __eitcm: u64;
    }
    let end = addr_of_mut!(__eitcm);
    let mut p = addr_of_mut!(__sitcm);
    while p < end {
        // SAFETY: `itcm.x` aligns both ends to 8 and puts `.itcm` in ITCM,
        // CPU-only RAM; the caller's contract makes this the one pass, and
        // no `take_*` can have built anything yet (each needs the
        // `ItcmZeroed` made below).
        // `KIT_VIEW`'s atomics accept any bytes, zero among them.
        unsafe {
            p.write_volatile(0);
            p = p.add(1);
        }
    }
    ItcmZeroed(())
}

// The pool's Sounds (ADR 0087): 160 KiB of AXI `.bss`, lent to the UI's
// project once, with `UI`.
static mut POOL_ARENA: [u8; ARENA_BYTES] = [0; ARENA_BYTES];

// Each Part's kit (ADR 0265): ITCM (ADR 0266), lent to the UI's project
// with the arena; D2, where the Parts are, is full, and AXI keeps its
// margin (ADR 0087). CPU only: the card writes a copy. Built at `take_ui`.
#[unsafe(link_section = ".itcm")]
static mut KITS: MaybeUninit<Kits> = MaybeUninit::uninit();

/// Out of line for flash, not the stack: built in place, its frame is
/// 1 KB, but inlined into `synth` fat LTO reshuffles the UI loop's
/// inlining for 6.3 KB more `.text` (ADR 0089).
#[inline(never)]
pub fn take_ui(_: ItcmZeroed) -> Option<&'static mut UiState> {
    if UI_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so these are the only
    // references to `POOL_ARENA`, `KITS` and `UI` ever made.
    let arena = ArenaWriter::new(unsafe { &mut *addr_of_mut!(POOL_ARENA) }, &SOURCE_TABLE);
    // SAFETY: as above.
    let kits = Kits::init_in_place(unsafe { &mut *addr_of_mut!(KITS) });
    // SAFETY: as above.
    Some(UiState::init_in_place(
        unsafe { &mut *addr_of_mut!(UI) },
        arena,
        kits,
    ))
}

// `ed`'s state (the queue and `since`'s copy, ~1.4 KB): ITCM, as `KITS`, CPU
// only (the console's answer path). Not `main`'s stack (ADR 0025), where it
// was +1 KB; not AXI, which the margin (ADR 0087) holds; not D2, full in the
// bench builds. NOLOAD: built in place by `take_ed`.
#[unsafe(link_section = ".itcm")]
static mut ED: MaybeUninit<chimera_core::console::EdState> = MaybeUninit::uninit();
static ED_TAKEN: AtomicBool = AtomicBool::new(false);

pub fn take_ed(_: ItcmZeroed) -> Option<&'static mut chimera_core::console::EdState> {
    if ED_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `ED` ever made.
    Some(unsafe { &mut *addr_of_mut!(ED) }.write(chimera_core::console::EdState::new()))
}

// The audio's three Performance snapshots (ADR 0021, ~23 KB): ITCM (ADR
// 0286), as `KITS`, not AXI, which the per-Part settings need above its
// margin (ADR 0087). CPU only: the UI writes, the audio interrupt reads,
// the bench lends slot 0 first. NOLOAD: built in place by `take_audio`.
#[unsafe(link_section = ".itcm")]
static mut AUDIO: MaybeUninit<TripleBuffer<AudioShared>> = MaybeUninit::uninit();
static AUDIO_TAKEN: AtomicBool = AtomicBool::new(false);

/// `AUDIO`'s first slot, lent to the bench, which runs before
/// `take_audio`: its `AudioShared` needs no 7 KB of RAM of its own.
///
/// # Safety
/// At most once, and only before `take_audio`, which builds every slot;
/// the reference must be gone by then. Both orders are asserted: a second
/// call, or one after `take_audio`, panics.
#[cfg(feature = "bench")]
pub unsafe fn bench_audio(_: ItcmZeroed) -> &'static mut MaybeUninit<AudioShared> {
    static BENCH_TAKEN: AtomicBool = AtomicBool::new(false);
    assert!(
        !BENCH_TAKEN.swap(true, Ordering::AcqRel),
        "bench_audio twice"
    );
    assert!(
        !AUDIO_TAKEN.load(Ordering::Acquire),
        "bench_audio after take_audio"
    );
    // SAFETY: the asserts let one call past, before `take_audio`, so this
    // is the only reference to `AUDIO` yet; the caller drops it before
    // `take_audio` makes its own.
    TripleBuffer::first_slot(unsafe { &mut *addr_of_mut!(AUDIO) })
}

// The UI side of the knobs, beside the writer it publishes through (ADR
// 0115): 2.2 KB of the UI's, in D2, not AXI, which the bench builds hold
// within 8 KiB of full (ADR 0087). NOLOAD: written at `take_audio`.
#[unsafe(link_section = ".ram_d2")]
static mut KNOBS: MaybeUninit<KnobPublisher> = MaybeUninit::uninit();

/// The triple buffer's ends and the knobs' publisher, which every publish
/// of a Project takes. Seeded from the UI's `Performance`, not
/// `AudioShared::default`, whose temporary `Performance` puts an 8 KB
/// frame on the stack. Out of line: the publisher is built here, not on
/// `synth`'s frame (the stack check's 8 KB a step).
#[inline(never)]
pub fn take_audio(
    _: ItcmZeroed,
    perf: &Performance,
) -> Option<(
    Writer<AudioShared>,
    Reader<AudioShared>,
    &'static mut KnobPublisher,
)> {
    if AUDIO_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `KNOBS` ever made; `write` initialises it whole.
    let knobs = unsafe { &mut *addr_of_mut!(KNOBS) }.write(KnobPublisher::new());
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // live reference to `AUDIO`: the bench's (`bench_audio`) is gone.
    let slot = unsafe { &mut *addr_of_mut!(AUDIO) };
    // SAFETY: `AudioShared::init_in_place` writes every field of each slot,
    // one Part at a time, so no whole `AudioShared` is ever on the stack.
    let buf = unsafe {
        TripleBuffer::init_slots_in_place(slot, |s| {
            AudioShared::init_in_place(s, perf);
        })
    };
    let (w, r) = buf.split();
    Some((w, r, knobs))
}

// ORBIT's view, from the runner to the UI: AXI.
#[cfg(feature = "orbit")]
static mut ORBIT: MaybeUninit<TripleBuffer<OrbitView>> = MaybeUninit::uninit();
#[cfg(feature = "orbit")]
static ORBIT_TAKEN: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "orbit")]
pub fn take_orbit() -> Option<(Writer<OrbitView>, Reader<OrbitView>)> {
    if ORBIT_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `ORBIT` ever made.
    let slot = unsafe { &mut *addr_of_mut!(ORBIT) };
    Some(TripleBuffer::init_in_place(slot, OrbitView::default).split())
}

// The voices' formant views, from the audio to the OP pages: in D2, not
// AXI, which the ORBIT build fills to its margin (ADR 0087), as `KNOBS`.
// NOLOAD: built at `take_formant`.
#[unsafe(link_section = ".ram_d2")]
static mut FORMANT: MaybeUninit<TripleBuffer<FormantViews>> = MaybeUninit::uninit();
static FORMANT_TAKEN: AtomicBool = AtomicBool::new(false);

pub fn take_formant() -> Option<(Writer<FormantViews>, Reader<FormantViews>)> {
    if FORMANT_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `FORMANT` ever made.
    let slot = unsafe { &mut *addr_of_mut!(FORMANT) };
    Some(TripleBuffer::init_in_place(slot, || [FormantView::NONE; MAX_PARTS]).split())
}

/// MIDI in's control rings, one a port (MIDI map spec § 1): each port's
/// poll pushes, the UI drains. USB's holds a burst (4 KB) where the MIDI
/// map reads it; elsewhere both are dropped unread each lap, and DIN's 64
/// keep the bench's AXI margin (ADR 0087).
#[cfg(any(feature = "midi-din", feature = "midi-usb"))]
pub type Ctrl = CtrlPorts<{ CTRL_USB }>;
/// USB's control ring's slots.
#[cfg(feature = "midi-map")]
pub const CTRL_USB: usize = chimera_core::midi_in::CTRL_USB_LEN;
#[cfg(not(feature = "midi-map"))]
pub const CTRL_USB: usize = chimera_core::midi_in::CTRL_RING_LEN;

// All zero, so `.bss` in AXI with no code to build it.
#[cfg(any(feature = "midi-din", feature = "midi-usb"))]
static CTRL: Ctrl = CtrlPorts::new();
#[cfg(any(feature = "midi-din", feature = "midi-usb"))]
static CTRL_TAKEN: AtomicBool = AtomicBool::new(false);

/// The control rings, empty; once. Both ends share the reference.
#[cfg(any(feature = "midi-din", feature = "midi-usb"))]
pub fn take_ctrl() -> Option<&'static Ctrl> {
    (!CTRL_TAKEN.swap(true, Ordering::AcqRel)).then_some(&CTRL)
}

// MIDI out's rings, one a port (MIDI phase 1 spec § 9), the UI's to push;
// DIN's the USART1 ISR's to pop, USB's its poll's to pack into IN packets.
// All zero, so `.bss` in AXI with no code to build them.
#[cfg(all(feature = "midi-din", feature = "midi-map"))]
static OUT: [OutRing; Port::COUNT] = [const { OutRing::new() }; Port::COUNT];
#[cfg(all(feature = "midi-din", feature = "midi-map"))]
static OUT_TAKEN: AtomicBool = AtomicBool::new(false);

/// MIDI out's rings, empty; once. Both ends share them.
#[cfg(all(feature = "midi-din", feature = "midi-map"))]
pub fn take_out() -> Option<OutPorts<'static>> {
    let din = &OUT[Port::Din as usize];
    #[cfg(feature = "midi-usb")]
    let ports = OutPorts::new(din, &OUT[Port::Usb as usize]);
    #[cfg(not(feature = "midi-usb"))]
    let ports = OutPorts::din(din);
    (!OUT_TAKEN.swap(true, Ordering::AcqRel)).then_some(ports)
}

// What the audio interrupt (`chimera-stm32`) and this side both touch:
// the note sources, the DMA's and MIDI DIN's counters, the probe's voices.

/// One a MIDI port (DIN's, and USB's with `midi-usb`), and in a bench
/// build the console's `card`, which plays a chord.
pub const NOTE_SOURCES: usize =
    1 + cfg!(feature = "midi-usb") as usize + cfg!(feature = "bench") as usize;

/// Port `p`'s note source: its index. Call it in a `const` block: USB's
/// where USB MIDI isn't built is then a build error, not another
/// source's id.
#[cfg(any(feature = "midi-din", feature = "midi-usb"))]
pub const fn source(p: Port) -> SourceId<NOTE_SOURCES> {
    assert!(
        cfg!(feature = "midi-usb") || matches!(p, Port::Din),
        "no USB MIDI in this build"
    );
    SourceId::new(p as usize)
}
#[cfg(feature = "bench")]
pub const CARD: SourceId<NOTE_SOURCES> = SourceId::new(NOTE_SOURCES - 1);
pub static NOTES: NoteSources<NOTE_SOURCES> = NoteSources::new();

pub static OVERRUNS: AtomicU32 = AtomicU32::new(0);
/// Voices the load net shed (ADR 0273).
pub static LOAD_SHEDS: AtomicU32 = AtomicU32::new(0);
/// Trailing DMA streams drifting from stream 0.
pub static DESYNCS: AtomicU32 = AtomicU32::new(0);
/// Transfer errors on streams 1 and 2, each re-armed in phase.
pub static DMA_ERRORS: AtomicU32 = AtomicU32::new(0);
/// FIFO errors on streams 1 and 2.
pub static FIFO_ERRORS: AtomicU32 = AtomicU32::new(0);
/// Halves rendered, wrapping: the watchdog's audio heartbeat.
pub static BLOCKS: AtomicU32 = AtomicU32::new(0);
/// Cycles spent in the audio interrupt, wrapping: what the card IRQ takes
/// off its own entries' time.
pub static AUDIO_CYCLES: AtomicU32 = AtomicU32::new(0);
/// Cycles spent in the MIDI level's interrupts (DIN's, and USB's with
/// `midi-usb`) less the audio's inside them, wrapping: the card IRQ takes
/// them off its own entries' time too.
pub static MIDI_CYCLES: AtomicU32 = AtomicU32::new(0);
pub use chimera_hal::units::Cycles;

/// The cycle counter now.
#[inline(always)]
pub fn dwt() -> Cycles {
    Cycles::new(cortex_m::peripheral::DWT::cycle_count())
}

/// The cycle counter and the other interrupts' cycle total, read as a pair.
#[derive(Clone, Copy)]
pub struct Stamp {
    pub at: Cycles,
    pub over: Cycles,
}

/// `Stamp` of the counter and `over()` as one pair: a change of `over`
/// between the reads is caught and the pair read again (N1). The fences
/// keep the counter's read between the loads.
#[inline(always)]
pub fn stamp(over: impl Fn() -> u32) -> Stamp {
    loop {
        let before = over();
        compiler_fence(Ordering::SeqCst);
        let at = dwt();
        compiler_fence(Ordering::SeqCst);
        if over() == before {
            return Stamp {
                at,
                over: Cycles::new(before),
            };
        }
    }
}

/// The audio's cycle total: what a MIDI-level handler's `stamp` reads.
#[inline(always)]
pub fn audio_cycles() -> u32 {
    AUDIO_CYCLES.load(Ordering::Relaxed)
}

/// A handler's own cycles from `enter` to `leave`, the audio's inside
/// them taken off.
#[inline(always)]
pub fn own_cycles(enter: Stamp, leave: Stamp) -> Cycles {
    let audio = leave.over.since(enter.over);
    Cycles::new(leave.at.since(enter.at).get().saturating_sub(audio.get()))
}

/// The streams' counters: the voices', and the card IRQ's time.
pub static STREAM_STATS: StreamStats = StreamStats::new();
/// MIDI DIN's line errors: overrun, framing, noise.
pub static MIDI_ERRORS: AtomicU32 = AtomicU32::new(0);
/// MIDI DIN's pipe: the faults its parser counted, the events a full note
/// queue refused and those a full control ring refused (`PipeStats`), as
/// of the ISR's last feed.
pub static MIDI_FAULTS: AtomicU32 = AtomicU32::new(0);
pub static MIDI_DROPS: AtomicU32 = AtomicU32::new(0);
pub static MIDI_CTRL_DROPS: AtomicU32 = AtomicU32::new(0);
/// MIDI DIN's pipe owes the note queue (`PipeSlot::owes`) as of the ISR's
/// last entry: the main loop pends USART1 to retry, as no byte may come
/// (#884).
pub static MIDI_OWES: AtomicBool = AtomicBool::new(false);

/// A saturating count, as the audio and MIDI interrupts keep them.
#[inline]
pub fn bump(counter: &AtomicU32, n: u32) {
    let _ = counter.try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
        Some(v.saturating_add(n))
    });
}

/// After each block: voices sounding, and the voice budget booked in
/// percent. Plain stores; the probe reads them in the same interrupt.
#[cfg(feature = "perf-probe")]
pub static VOICES: AtomicU8 = AtomicU8::new(0);
#[cfg(feature = "perf-probe")]
pub static COST_PCT: AtomicU8 = AtomicU8::new(0);
