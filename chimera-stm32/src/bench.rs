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
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus};
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS, MAX_PARTS, MAX_VOICES, SAMPLE_RATE, SampleBudget};
use chimera_core::instrument::{AudioShared, DacOut, Instrument, PanCache, mix_parts};
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
const HOLD_SECONDS: u32 = 30;
const ROWS: usize = 9;
const FX_ROWS: usize = 5;

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
    ("WC", || algo_pair(AlgoId::A14, AlgoId::A22), 36, 12),
    ("A16+17", || algo_pair(AlgoId::A16, AlgoId::A17), 36, 12),
];

/// Label and per-block FX setup of each FX row. Every row runs the same
/// noise through `mix_parts`, six Parts written, no voices.
type FxRow = (&'static str, fn(&mut AudioShared, u32));
const FX: [FxRow; FX_ROWS] = [
    ("MIX", |_, _| {}),
    ("CHORUS", |s, _| worst_chorus(s)),
    ("DELAY", |s, _| worst_delay(s)),
    ("REVERB", worst_reverb),
    ("BUS", |s, b| {
        worst_chorus(s);
        worst_delay(s);
        worst_reverb(s, b);
    }),
];

fn worst_chorus(s: &mut AudioShared) {
    let c = &mut s.fx.chorus;
    (c.mode, c.rate, c.depth, c.mix) = (3, 1.0, 1.0, 0.5);
}

fn worst_delay(s: &mut AudioShared) {
    let d = &mut s.fx.delay;
    (d.time_ms, d.wow_flutter, d.saturation, d.mix) = (500.0, 1.0, 1.0, 0.5);
}

/// The ring at its costliest: longest TIME and SIZE, with a SIZE crossfade
/// always running (steps 31 and 30 in turn).
fn worst_reverb(s: &mut AudioShared, block: u32) {
    let r = &mut s.fx.reverb;
    let size = [1.0, 30.0 / 31.0][block as usize % 2];
    (r.time, r.size, r.damping, r.mix) = (1.0, size, 0.5, 0.5);
}

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
    let fx: [u32; FX_ROWS] = core::array::from_fn(|i| rig.time_bus(FX[i].1));
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

    /// Six Parts written with one noise block, sends 0.5, no voices:
    /// `mix_parts` alone. `each` sets the FX before every block.
    #[inline(never)]
    fn time_bus(&mut self, each: fn(&mut AudioShared, u32)) -> u32 {
        let fx = FxBus::init_in_place(self.fx_slot);
        let shared = self
            .shared_slot
            .write(AudioShared::from_performance(self.perf));
        for part in shared.parts.iter_mut() {
            part.mix.sends = [0.5; FX_SENDS];
        }
        let mut x = 0x1234_5678u32;
        let noise: [f32; BLOCK_SIZE] = core::array::from_fn(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as f32 / u32::MAX as f32 - 0.5
        });
        let buses = [noise; MAX_PARTS];
        let written = [true; MAX_PARTS];
        let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
        let mut pans = PanCache::default();
        let mut block = |shared: &mut AudioShared, fx: &mut FxBus, dac: &mut DacOut, b: u32| {
            each(shared, b);
            black_box(mix_parts(
                black_box(&buses),
                &written,
                &mut sends,
                &mut pans,
                fx,
                shared,
                SAMPLE_RATE,
                dac,
            ));
        };
        for b in 0..WARM_BLOCKS {
            block(shared, fx, &mut self.dac, b);
        }
        let start = DWT::cycle_count();
        for b in 0..TIMED_BLOCKS {
            block(shared, fx, &mut self.dac, WARM_BLOCKS + b);
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

/// `a` ∪ `b` at MORPH 64, six audible operators, all with feedback: A14 ∪
/// A22 is spec § Budget's worst case, A16 ∪ A17 ADR 0026's costliest (842).
fn algo_pair(a: AlgoId, b: AlgoId) -> ParamSnapshot {
    let mut p = algo(a, ALL, 7);
    (p.algo.alg_b, p.algo.morph) = (b.get(), 64);
    p
}

const ROW_H: i32 = 25;
const CELL_W: i32 = 38;

fn show(
    display: &mut impl ChimeraDisplay,
    clocks: Clocks,
    rows: &[[u32; MAX_VOICES]; ROWS],
    kernel: u32,
    fx: &[u32; FX_ROWS],
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
        16,
        theme::INK,
    );
    draw::text(
        display,
        &theme::FONT_LABEL,
        "CYCLES/SAMPLE, 1..6 VOICES",
        4,
        30,
        theme::MID,
    );
    let mut y = 46;
    for (&(label, ..), cycles) in PATCHES.iter().zip(rows) {
        voice_row(display, &mut line, y, label, cycles);
        y += ROW_H;
    }
    line.clear();
    let _ = write!(line, "KERNEL /VOICE {kernel} (350)");
    draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, y, theme::INK);
    y += 18;
    for (i, (&(label, _), &c)) in FX.iter().zip(fx).enumerate() {
        // Each effect less MIX; MIX and BUS as read.
        let c = if i == 0 || i == FX_ROWS - 1 {
            c
        } else {
            c.saturating_sub(fx[0])
        };
        line.clear();
        let _ = write!(line, "{label} {c}");
        let (col, row) = ((i % 3) as i32, (i / 3) as i32);
        draw::text(
            display,
            &theme::FONT_LABEL,
            line.as_str(),
            4 + col * 78,
            y + row * 14,
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
