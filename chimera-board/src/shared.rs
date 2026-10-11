use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
#[cfg(feature = "perf-probe")]
use core::sync::atomic::AtomicU8;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use chimera_core::dsp::algo::waves::{self, Waves};
use chimera_core::dsp::knobs::KnobPublisher;
use chimera_core::instrument::AudioShared;
use chimera_core::note_queue::NoteSources;
#[cfg(any(feature = "midi-din", feature = "bench"))]
use chimera_core::note_queue::SourceId;
#[cfg(feature = "orbit")]
use chimera_core::orbit::runner::OrbitView;
use chimera_core::preset::Performance;
use chimera_core::project::arena::{ARENA_BYTES, ArenaWriter};
use chimera_core::scope::{ScopeFrame, scope_buffer};
use chimera_core::source_table::SOURCE_TABLE;
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

// `UiState` is ~27 KB: it lives in AXI, not in `main`'s frame on the
// ~63.75 KB DTCM stack (ADR 0025).
static mut UI: MaybeUninit<UiState> = MaybeUninit::uninit();
static UI_TAKEN: AtomicBool = AtomicBool::new(false);

// The pool's Sounds (ADR 0087): 160 KiB of AXI `.bss`, lent to the UI's
// project once, with `UI`.
static mut POOL_ARENA: [u8; ARENA_BYTES] = [0; ARENA_BYTES];

/// Out of line for flash, not the stack: built in place, its frame is
/// 1 KB, but inlined into `synth` fat LTO reshuffles the UI loop's
/// inlining for 6.3 KB more `.text` (ADR 0089).
#[inline(never)]
pub fn take_ui() -> Option<&'static mut UiState> {
    if UI_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so these are the only
    // references to `POOL_ARENA` and `UI` ever made.
    let arena = ArenaWriter::new(unsafe { &mut *addr_of_mut!(POOL_ARENA) }, &SOURCE_TABLE);
    // SAFETY: as above.
    Some(UiState::init_in_place(
        unsafe { &mut *addr_of_mut!(UI) },
        arena,
    ))
}

static mut AUDIO: MaybeUninit<TripleBuffer<AudioShared>> = MaybeUninit::uninit();
static AUDIO_TAKEN: AtomicBool = AtomicBool::new(false);

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
    // reference to `AUDIO` ever made.
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

// ORBIT's view, from the runner to the UI: AXI, beside `AUDIO`.
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

// What the audio interrupt (`chimera-stm32`) and this side both touch:
// the note sources, the DMA's and MIDI DIN's counters, the probe's voices.

/// MIDI DIN, and in a bench build the console's `card`, which plays a chord.
pub const NOTE_SOURCES: usize = if cfg!(feature = "bench") { 2 } else { 1 };
#[cfg(feature = "midi-din")]
pub const DIN: SourceId<NOTE_SOURCES> = SourceId::new(0);
#[cfg(feature = "bench")]
pub const CARD: SourceId<NOTE_SOURCES> = SourceId::new(1);
pub static NOTES: NoteSources<NOTE_SOURCES> = NoteSources::new();

pub static OVERRUNS: AtomicU32 = AtomicU32::new(0);
/// Trailing DMA streams drifting from stream 0.
pub static DESYNCS: AtomicU32 = AtomicU32::new(0);
/// Transfer errors on streams 1 and 2, each re-armed in phase.
pub static DMA_ERRORS: AtomicU32 = AtomicU32::new(0);
/// FIFO errors on streams 1 and 2.
pub static FIFO_ERRORS: AtomicU32 = AtomicU32::new(0);
/// Halves rendered, wrapping: the watchdog's audio heartbeat.
pub static BLOCKS: AtomicU32 = AtomicU32::new(0);
/// MIDI DIN's line errors: overrun, framing, noise.
pub static MIDI_ERRORS: AtomicU32 = AtomicU32::new(0);

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
