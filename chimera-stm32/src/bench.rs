use core::fmt::Write;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS, MAX_VOICES, SAMPLE_RATE, SampleBudget};
use chimera_core::instrument::{AudioShared, DacOut, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Performance;
use chimera_core::scope::{ScopeFrame, ScopeWriter, scope_buffer};
use chimera_core::triple::TripleBuffer;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::{draw, theme};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::ChimeraDisplay;
use cortex_m::peripheral::DWT;

use crate::audio::engine;
use crate::clocks::Clocks;

const WARM_BLOCKS: u32 = 8;
const TIMED_BLOCKS: u32 = 64;
const REVERB_TYPES: usize = 3;
const HOLD_SECONDS: u32 = 30;
const ENGINES: usize = EngineType::ALL.len();

static mut SCOPE: TripleBuffer<ScopeFrame> = scope_buffer();
// A static, not a local: `AudioShared` is 3 KB (ADR 0020).
static mut SHARED: MaybeUninit<AudioShared> = MaybeUninit::uninit();

struct Rig<'p> {
    inst_slot: &'static mut MaybeUninit<Instrument>,
    fx_slot: &'static mut MaybeUninit<FxBus>,
    shared_slot: &'static mut MaybeUninit<AudioShared>,
    perf: &'p Performance,
    scope: ScopeWriter,
    dac: DacOut,
}

// Not inlined, so its frame never adds to `main`'s.
#[inline(never)]
pub fn run(display: &mut impl ChimeraDisplay, clocks: Clocks, perf: &Performance) {
    // SAFETY: the bench runs once from `main`, before `engine::init` and
    // before any interrupt is unmasked, so it is the only user of the
    // engine's slots and of its own statics; its references are gone when it
    // returns, before `engine::init` takes the slots.
    let (inst_slot, fx_slot, shared_slot, scope_w) = unsafe {
        let (i, f) = engine::slots();
        let (w, _unread) = (&mut *addr_of_mut!(SCOPE)).split();
        (i, f, &mut *addr_of_mut!(SHARED), w)
    };
    let mut rig = Rig {
        inst_slot,
        fx_slot,
        shared_slot,
        perf,
        scope: ScopeWriter::new(scope_w),
        dac: [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS],
    };
    let mut voices = [[0u32; MAX_VOICES]; ENGINES];
    for (row, &engine) in EngineType::ALL.iter().enumerate() {
        for n in 1..=MAX_VOICES {
            voices[row][n - 1] =
                rig.time(|s| s.parts[0].params = ParamSnapshot::for_engine(engine), n);
        }
    }
    let fx: [u32; REVERB_TYPES] = core::array::from_fn(|t| {
        rig.time(
            |s| {
                s.fx.chorus.mode = 1;
                s.fx.chorus.mix = 0.5;
                s.fx.delay.mix = 0.5;
                s.fx.reverb.mix = 0.5;
                s.fx.reverb.reverb_type = t as u8;
            },
            0,
        )
    });
    show(display, clocks, &voices, &fx);
    for _ in 0..HOLD_SECONDS {
        crate::clocks::delay_us(clocks.cpu_hz, 1_000_000);
    }
}

impl Rig<'_> {
    #[inline(never)]
    fn time(&mut self, setup: impl FnOnce(&mut AudioShared), voices: usize) -> u32 {
        // The allocator must not refuse what the bench wants to measure.
        let budget = SampleBudget::for_cpu(u32::MAX);
        let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
        let fx = FxBus::init_in_place(self.fx_slot);
        let shared = self
            .shared_slot
            .write(AudioShared::from_performance(self.perf));
        setup(shared);
        for v in 0..voices {
            let note = MidiNote::new(48 + 5 * v as u8).unwrap_or(MidiNote::A4);
            let ev = NoteEvent {
                channel: MidiChannel::clamped(0),
                note,
                kind: NoteKind::On(Velocity::DEFAULT),
            };
            inst.handle(ev, shared);
        }
        for _ in 0..WARM_BLOCKS {
            inst.render(fx, &mut self.dac, shared, &mut self.scope);
        }
        let start = DWT::cycle_count();
        for _ in 0..TIMED_BLOCKS {
            inst.render(fx, &mut self.dac, shared, &mut self.scope);
        }
        DWT::cycle_count().wrapping_sub(start) / (TIMED_BLOCKS * BLOCK_SIZE as u32)
    }
}

fn name(e: EngineType) -> &'static str {
    match e {
        EngineType::Pizza => "PIZZA",
        EngineType::Fm => "FM",
        EngineType::Modal => "MODAL",
        EngineType::Va => "VA",
    }
}

fn show(
    display: &mut impl ChimeraDisplay,
    clocks: Clocks,
    voices: &[[u32; MAX_VOICES]; ENGINES],
    fx: &[u32; REVERB_TYPES],
) {
    const CELL_W: i32 = 38;
    draw::fill_rect(display, 0, 0, theme::SCREEN_W, theme::SCREEN_H, theme::BG);
    let mut line = FmtBuf::new();
    let _ = write!(
        line,
        "BENCH REV {} {} MHZ",
        clocks.rev.label(),
        clocks.cpu_hz / 1_000_000
    );
    draw::text(
        display,
        &theme::FONT_VALUE,
        line.as_str(),
        4,
        20,
        theme::INK,
    );
    draw::text(
        display,
        &theme::FONT_LABEL,
        "CYCLES/SAMPLE, 1..6 VOICES",
        4,
        36,
        theme::MID,
    );
    for (row, (&engine, cycles)) in EngineType::ALL.iter().zip(voices).enumerate() {
        let y = 58 + row as i32 * 30;
        let per_voice = cycles[MAX_VOICES - 1].saturating_sub(cycles[0]) / (MAX_VOICES as u32 - 1);
        line.clear();
        let _ = write!(line, "{} /VOICE {}", name(engine), per_voice);
        draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, y, theme::INK);
        // One cell per count: six five-digit counts overflow a `FmtBuf`.
        for (i, c) in cycles.iter().enumerate() {
            line.clear();
            let _ = write!(line, "{c}");
            let x = 4 + i as i32 * CELL_W;
            draw::text(
                display,
                &theme::FONT_LABEL,
                line.as_str(),
                x,
                y + 13,
                theme::INK2,
            );
        }
    }
    let y = 58 + ENGINES as i32 * 30;
    draw::text(display, &theme::FONT_VALUE, "FX", 4, y, theme::INK);
    for (i, (label, c)) in ["PLATE", "FDN", "MV"].iter().zip(fx).enumerate() {
        line.clear();
        let _ = write!(line, "{label} {c}");
        let x = 4 + i as i32 * 2 * CELL_W;
        draw::text(
            display,
            &theme::FONT_LABEL,
            line.as_str(),
            x,
            y + 13,
            theme::INK2,
        );
    }
    display.flush();
}
