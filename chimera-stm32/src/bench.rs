use core::fmt::Write;
use core::hint::black_box;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use chimera_core::dsp::algo::kernel::{Kernel, KernelBlock, OpBlock, SAMPLE_SCALE};
use chimera_core::dsp::algo::plan::{EvalPlan, OPS};
use chimera_core::dsp::algo::tx::FEEDBACK_CYCLES;
use chimera_core::dsp::algo::waves::WaveId;
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
const ROWS: usize = 8;

/// Each Algo row plays six distinct waves; voices sit an octave apart so
/// each reads its own mips (a D-cache worst case).
const WAVES: [WaveId; OPS] = [
    WaveId::W2,
    WaveId::SAW,
    WaveId::SQR,
    WaveId::P25,
    WaveId::TRI,
    WaveId::W7,
];
const COARSE: [u8; OPS] = [4, 8, 10, 13, 16, 19];

/// Label, patch, lowest note and note spacing of each row; the report solves
/// `AlgoEngine::cost`'s terms from them.
type Row = (&'static str, fn() -> ParamSnapshot, u8, u8);
const PATCHES: [Row; ROWS] = [
    (
        "MODAL",
        || ParamSnapshot::for_engine(EngineType::Modal),
        48,
        5,
    ),
    ("FLOOR", || algo(AlgoId::A1, 0, 0), 36, 12),
    ("1 OP", || algo(AlgoId::A1, 0b1, 0), 36, 12),
    // Operators 1, 3 and 5: none can pair, so each runs alone.
    ("ALT", || algo(AlgoId::A1, 0b1_0101, 0), 36, 12),
    ("6 OP", || algo(AlgoId::A1, ALL, 0), 36, 12),
    ("CHAIN", || algo(AlgoId::A17, ALL, 0), 36, 12),
    ("CHN FB", || algo(AlgoId::A17, ALL, 7), 36, 12),
    ("WC", algo_worst_case, 36, 12),
];

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
    let mut rows = [[0u32; MAX_VOICES]; ROWS];
    for (row, &(_, patch, low, step)) in rows.iter_mut().zip(&PATCHES) {
        for n in 1..=MAX_VOICES {
            row[n - 1] = rig.time(|s| s.parts[0].params = black_box(patch()), n, low, step);
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
            0,
            0,
        )
    });
    let kernel = time_kernel();
    show(display, clocks, &rows, kernel, &fx);
    for _ in 0..HOLD_SECONDS {
        crate::clocks::delay_us(clocks.cpu_hz, 1_000_000);
    }
}

impl Rig<'_> {
    /// Voice `v` plays note `low + step * v`.
    #[inline(never)]
    fn time(
        &mut self,
        setup: impl FnOnce(&mut AudioShared),
        voices: usize,
        low: u8,
        step: u8,
    ) -> u32 {
        // The allocator must not refuse what the bench wants to measure.
        let budget = SampleBudget::for_cpu(u32::MAX);
        let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
        let fx = FxBus::init_in_place(self.fx_slot);
        let shared = self
            .shared_slot
            .write(AudioShared::from_performance(self.perf));
        setup(shared);
        for v in 0..voices {
            let note = MidiNote::new(low + step * v as u8).unwrap_or(MidiNote::A4);
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

/// Spec § Budget worst case, one kernel per voice: six operators, all with
/// feedback, six distinct waves crossfading mips `v` and `v + 1` in voice `v`
/// (about 21 KB of tables, read from their uncached, zero-wait DTCM copy
/// as in the running synth), MORPH moving through 0.5
/// between A14 and A22 (the masks are written here, before the algorithm
/// tables exist). The inputs pass through `black_box` so nothing folds.
#[inline(never)]
fn time_kernel() -> u32 {
    const A14: [u8; OPS] = [0, 0, 0b11, 0b11, 0b100, 0b1000];
    const A22: [u8; OPS] = [0, 0b1, 0b1, 0b10, 0b100, 0b1_1000];
    let plan = black_box(EvalPlan::build(
        black_box(&A14),
        black_box(0b11),
        black_box(&A22),
        black_box(0b1),
    ));
    let rates = EnvRates {
        ar: 31,
        d1r: 0,
        d1l: 15,
        d2r: 0,
        rr: 8,
        rs: 0,
    };
    let mut kernels = [Kernel::new(); MAX_VOICES];
    let mut envs = [[OpEnv::IDLE; OPS]; MAX_VOICES];
    for (v, env) in envs.iter_mut().enumerate() {
        let note = MidiNote::new(48 + 5 * v as u8).unwrap_or(MidiNote::A4);
        for e in env.iter_mut() {
            e.note_on(EnvCoefs::new(rates, note, SAMPLE_RATE as f32));
        }
    }
    let blocks: [KernelBlock; MAX_VOICES] = black_box(core::array::from_fn(|v| KernelBlock {
        plan: &plan,
        ops: core::array::from_fn(|i| {
            let wave = WAVES[(i + v) % OPS];
            OpBlock {
                inc: (i as u32 + 1) * (11_600_000 + 2_000_000 * v as u32),
                gain_from: 0.9 * SAMPLE_SCALE,
                gain_to: 0.8 * SAMPLE_SCALE,
                feedback: FEEDBACK_CYCLES[7],
                lo: wave.table(v),
                hi: wave.table(v + 1),
                xfade_from: 0.4,
                xfade_to: 0.6,
            }
        }),
        morph_from: 0.45,
        morph_to: 0.55,
        norm_from: 0.7,
        norm_to: 0.7,
    }));
    let mut out = [0.0f32; BLOCK_SIZE];
    let mut run = |kernels: &mut [Kernel; MAX_VOICES], envs: &mut [[OpEnv; OPS]; MAX_VOICES]| {
        for ((k, env), blk) in kernels.iter_mut().zip(envs.iter_mut()).zip(&blocks) {
            k.render(black_box(blk), env, &mut out);
        }
    };
    for _ in 0..WARM_BLOCKS {
        run(&mut kernels, &mut envs);
    }
    let start = DWT::cycle_count();
    for _ in 0..TIMED_BLOCKS {
        run(&mut kernels, &mut envs);
    }
    let cycles = DWT::cycle_count().wrapping_sub(start);
    black_box(&out);
    cycles / (TIMED_BLOCKS * BLOCK_SIZE as u32 * MAX_VOICES as u32)
}

const ALL: u8 = 0b11_1111;

/// `alg` alone, the operators in mask `lit` at LEVEL 99 and the rest at 0,
/// every operator with `feedback`.
fn algo(alg: AlgoId, lit: u8, feedback: u8) -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    (p.algo.alg_a, p.algo.alg_b, p.algo.morph) = (alg.get(), alg.get(), 0);
    for (i, op) in p.algo.ops.iter_mut().enumerate() {
        let level = if lit & (1 << i) != 0 { 99 } else { 0 };
        (op.wave, op.coarse, op.level, op.feedback) = (WAVES[i].get(), COARSE[i], level, feedback);
    }
    p
}

/// Spec § Budget's worst case: six audible operators, all with feedback,
/// six distinct waves, MORPH 0.5 between A14 and A22.
fn algo_worst_case() -> ParamSnapshot {
    let mut p = algo(AlgoId::A14, ALL, 7);
    (p.algo.alg_b, p.algo.morph) = (AlgoId::A22.get(), 64);
    p
}

const ROW_H: i32 = 26;
const CELL_W: i32 = 38;

fn show(
    display: &mut impl ChimeraDisplay,
    clocks: Clocks,
    rows: &[[u32; MAX_VOICES]; ROWS],
    kernel: u32,
    fx: &[u32; REVERB_TYPES],
) {
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
    let mut y = 56;
    for (&(label, ..), cycles) in PATCHES.iter().zip(rows) {
        voice_row(display, &mut line, y, label, cycles);
        y += ROW_H;
    }
    line.clear();
    let _ = write!(line, "KERNEL /VOICE {kernel} (350)");
    draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, y, theme::INK);
    y += ROW_H;
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
            y + 12,
            theme::INK2,
        );
    }
    display.flush();
}

/// `label`'s per-voice cost (six voices minus one, over five) and its six counts.
fn voice_row(
    display: &mut impl ChimeraDisplay,
    line: &mut FmtBuf,
    y: i32,
    label: &str,
    cycles: &[u32; MAX_VOICES],
) {
    let per_voice = cycles[MAX_VOICES - 1].saturating_sub(cycles[0]) / (MAX_VOICES as u32 - 1);
    line.clear();
    let _ = write!(line, "{label} /VOICE {per_voice}");
    draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, y, theme::INK);
    // One cell per count: six five-digit counts overflow a `FmtBuf`.
    for (i, c) in cycles.iter().enumerate() {
        line.clear();
        let _ = write!(line, "{c}");
        draw::text(
            display,
            &theme::FONT_LABEL,
            line.as_str(),
            4 + i as i32 * CELL_W,
            y + 12,
            theme::INK2,
        );
    }
}
