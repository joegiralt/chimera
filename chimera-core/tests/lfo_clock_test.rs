//! A Part's FREE LFOs run on its phase clock (ADR 0135, #489): every voice
//! of the Part at one phase, whenever its note started; RETRIG restarts; a
//! RATE change keeps the phase; nothing on the audio path allocates.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::lfo::{LfoClock, LfoParams};
use chimera_core::dsp::modulator::LfoSlot;
use chimera_core::hw::{MAX_VOICES, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{PartId, Performance};
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

/// Counts this thread's allocations (the test harness's threads apart).
struct Counting;

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: every call forwards to `System` unchanged; the count is a
// thread-local `Cell` (const-initialised, so touching it never allocates).
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.with(|a| a.set(a.get() + 1));
        // SAFETY: the caller's contract, passed on.
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: the caller's contract, passed on.
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// One BLOCK's step of the clock at 48 kHz.
const STEP: f32 = BLOCK_SIZE as f32 / SAMPLE_RATE as f32;

/// An ALGO Sound whose LFO 1 runs at `rate` Hz, FREE (`sync` 0) or RETRIG
/// (1) at PHASE ¼.
fn sound(rate: f32, sync: u8) -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    let l = &mut s.params.lfos[0];
    (l.rate, l.sync, l.phase_offset) = (rate, sync, 0.25);
    s
}

struct Rig {
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
    shared: Box<AudioShared>,
}

impl Rig {
    fn new(snd: &Sound) -> Self {
        Self {
            inst: Box::new(Instrument::new(
                SAMPLE_RATE,
                SampleBudget::for_cpu(u32::MAX),
            )),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0),
            shared: shared(snd),
        }
    }

    fn note_on(&mut self, note: u8) {
        let ev = NoteEvent {
            channel: self.shared.parts[0].mix.channel,
            note: MidiNote::new(note).unwrap(),
            kind: NoteKind::On(Velocity::new(100).unwrap()),
        };
        self.inst.handle_with(ev, &self.shared, &GateView::empty());
    }

    fn blocks(&mut self, n: usize) {
        for _ in 0..n {
            let Rig {
                inst,
                fx,
                dac,
                scope,
                shared,
            } = self;
            inst.render_with(fx, dac, shared, scope, &GateView::empty());
        }
    }

    /// LFO 1's phase on each sounding voice.
    fn phases(&self) -> Vec<f32> {
        (0..MAX_VOICES)
            .filter(|&v| self.inst.voice(v).is_active())
            .map(|v| self.inst.voice(v).lfo_phase(LfoSlot::Lfo1))
            .collect()
    }
}

fn shared(snd: &Sound) -> Box<AudioShared> {
    let mut perf = Performance::new();
    **perf.edit(PartId::ALL[0]).sound = snd.clone();
    Box::new(AudioShared::from_performance(&perf))
}

/// #489: two notes started 37 blocks apart on a FREE LFO play it at one
/// phase, the Part's (each voice ran its own before).
#[test]
fn free_lfos_of_two_voices_share_the_part_phase() {
    let mut r = Rig::new(&sound(0.3, 0));
    r.blocks(5);
    r.note_on(60);
    r.blocks(37);
    r.note_on(64);
    r.blocks(13);
    let p = r.phases();
    assert_eq!(p.len(), 2, "two voices sound");
    assert_eq!(p[0].to_bits(), p[1].to_bits(), "{p:?}");
}

/// RETRIG still restarts each note at its PHASE, apart from the clock.
#[test]
fn retrig_lfos_still_restart_at_each_note() {
    let mut r = Rig::new(&sound(0.3, 1));
    r.note_on(60);
    r.blocks(37);
    r.note_on(64);
    r.blocks(1);
    let mut p = r.phases();
    p.sort_by(f32::total_cmp);
    let inc = 0.3 / SAMPLE_RATE as f32 * BLOCK_SIZE as f32;
    assert!((p[0] - (0.25 + inc)).abs() < 1e-6, "{p:?}");
    assert!((p[1] - (0.25 + 38.0 * inc)).abs() < 1e-5, "{p:?}");
}

/// A RATE change moves the clock's speed, never its phase.
#[test]
fn a_rate_change_keeps_the_phase() {
    let lfos = |rate: f32| {
        [LfoParams {
            rate,
            ..LfoParams::default()
        }; 3]
    };
    let mut c = LfoClock::ZERO;
    for _ in 0..40 {
        c.advance(&lfos(1.0), STEP);
    }
    let before = c.phases()[0];
    c.advance(&lfos(7.0), STEP);
    let step = (c.phases()[0] - before).rem_euclid(1.0);
    assert!(
        (step - 7.0 * STEP).abs() < 1e-6,
        "{before} → {}",
        c.phases()[0]
    );
    // and on a held voice: one block's step at the new rate
    let mut r = Rig::new(&sound(1.0, 0));
    r.note_on(60);
    r.blocks(40);
    let before = r.phases()[0];
    r.shared = shared(&sound(7.0, 0));
    r.blocks(1);
    let step = (r.phases()[0] - before).rem_euclid(1.0);
    assert!(
        (step - 7.0 * STEP).abs() < 1e-5,
        "{before} → {}",
        r.phases()[0]
    );
}

/// The clock and the voices reading it allocate nothing (the engine is
/// `no_std` without `alloc` on the chip; this pins it on the host).
#[test]
fn the_audio_path_never_allocates() {
    let mut r = Rig::new(&sound(0.3, 0));
    let mut c = LfoClock::ZERO;
    let lfos = [LfoParams::default(); 3];
    let before = ALLOCS.with(Cell::get);
    for k in 0..64u8 {
        if k % 8 == 0 {
            r.note_on(48 + k / 8);
        }
        r.blocks(1);
        c.advance(&lfos, STEP);
    }
    assert_eq!(ALLOCS.with(Cell::get), before);
}
