use core::fmt::Write;
use core::hint::black_box;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use chimera_core::dsp::algo::kernel::{Kernel, KernelBlock, OpBlock, SAMPLE_SCALE};
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::plan::{EvalPlan, OPS};
use chimera_core::dsp::algo::tx::FEEDBACK_CYCLES;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::engines::{EngineSlot, SlotKind};
use chimera_core::dsp::filter::FilterMode;
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus};
use chimera_core::dsp::modal::{
    BankModes, CHORD_COUNT, ModalEngine, ModalParams, ResonatorMode, SymPool,
};
use chimera_core::dsp::modulator::{EnvForm, EnvSlot, EnvType, Func, Glide, LfoForm, LfoType};
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{
    BLOCK_SIZE, MAX_PARTS, MAX_VOICES, SAMPLE_RATE, SampleBudget, VOICE_RAM_BUDGET,
};
use chimera_core::instrument::{
    AudioShared, DacBlocks, Instrument, PanCache, PartAudio, mix_parts,
};
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{CUTOFF, MAX_MOD_SOURCES, ModSource, ModState, VCA};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{
    EngineType, EnvParams, FilterParams, FolderParams, OutParams, ParamSnapshot,
};
use chimera_core::preset::Performance;
use chimera_core::scope::{ScopeFrame, ScopeWriter, scope_buffer};
use chimera_core::sym_alloc::SymAlloc;
use chimera_core::triple::TripleBuffer;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::{draw, theme};
use chimera_core::voice_alloc::VoiceIdx;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::ChimeraDisplay;
use cortex_m::peripheral::DWT;

use crate::audio::engine;
use crate::clocks::Clocks;

const WARM_BLOCKS: u32 = 8;
const TIMED_BLOCKS: u32 = 64;
const HOLD_SECONDS: u32 = 30;
const ROWS: usize = 9;
/// The TAPE row only with `master-tape` (ADR 0055).
const FX_ROWS: usize = if cfg!(feature = "master-tape") { 7 } else { 6 };

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
/// noise through `mix_parts`, six Parts written, no voices. MIX is the mix
/// and the output limiter, the others their effect over it; against ADR
/// 0031's readings (MIX 148, DELAY 183, before the limiter and MECHANICS),
/// MIX − 148 is the limiter and DELAY − 183 is MECHANICS.
type FxRow = (&'static str, fn(&mut AudioShared, u32));
const FX: [FxRow; FX_ROWS] = [
    ("MIX", |_, _| {}),
    ("CHORUS", |s, _| worst_chorus(s)),
    ("DELAY", |s, _| worst_delay(s)),
    ("REVERB", worst_reverb),
    #[cfg(feature = "master-tape")]
    ("TAPE", |s, _| worst_tape(s)),
    ("COMP", |s, _| worst_comp(s)),
    ("BUS", |s, b| {
        worst_chorus(s);
        worst_delay(s);
        worst_reverb(s, b);
        #[cfg(feature = "master-tape")]
        worst_tape(s);
        worst_comp(s);
    }),
];

fn worst_chorus(s: &mut AudioShared) {
    let c = &mut s.fx.chorus;
    (c.mode, c.rate, c.depth, c.mix) = (3, 1.0, 1.0, 0.5);
}

/// REV SEND costs only with the reverb on: the BUS row pays it.
fn worst_delay(s: &mut AudioShared) {
    let d = &mut s.fx.delay;
    (d.time_ms, d.wow_flutter, d.saturation, d.mix, d.rev_send) = (500.0, 1.0, 1.0, 0.5, 1.0);
}

/// The ring at its costliest: full GRIT, longest TIME and SIZE, with a
/// SIZE crossfade always running (steps 31 and 30 in turn).
fn worst_reverb(s: &mut AudioShared, block: u32) {
    let r = &mut s.fx.reverb;
    let size = [1.0, 30.0 / 31.0][block as usize % 2];
    (r.grit, r.time, r.size, r.damping, r.mix) = (1.0, 1.0, size, 0.5, 0.5);
}

/// Full DRIVE and WOW (its interpolated, most expensive tap), full MIX:
/// fully engaged and steady once warm, never fading.
#[cfg(feature = "master-tape")]
fn worst_tape(s: &mut AudioShared) {
    let t = &mut s.fx.tape;
    (t.drive, t.tone, t.wow, t.mix) = (1.0, 0.5, 1.0, 1.0);
}

/// −40 dB threshold at 20:1 (the noise is always over it), the fastest
/// timing, 12 dB of makeup, full MIX: fully in and compressing, on all
/// three DAC pairs (`MasterComp::process` always runs every pair).
fn worst_comp(s: &mut AudioShared) {
    let c = &mut s.fx.comp;
    (c.thresh, c.ratio, c.attack, c.release, c.makeup, c.mix) = (0.0, 7, 0.0, 0.0, 0.5, 1.0);
}

const ROUTING_ROWS: usize = 36;
/// Rows per ROUTING screen: ten from y 46 at `ROW_H` 25 end at 283.
const ROUTING_PAGE: usize = 10;

/// A ROUTING row: its label, the Part it plays (params and matrix), set
/// before the notes, and what it changes before every block. Each VCA row
/// is 1 OP plus its routes, so each `ModRouting` term is one row less
/// another.
type RoutingRow = (&'static str, fn(&mut PartAudio), Each);
type Each = fn(&mut PartAudio, u32);
const STILL: Each = |_, _| {};
const ROUTING: [RoutingRow; ROUTING_ROWS] = [
    ("1 OP", |p| p.params = algo(AlgoId::A1, 0b1, 0), STILL),
    ("MODS", mods, STILL),
    (
        "SVF",
        |p| {
            p.params = algo(AlgoId::A1, 0b1, 0);
            // The SVF's costliest mode.
            let _ = p.params.filter.set_mode(FilterMode::Phaser);
        },
        STILL,
    ),
    ("A VCA", |p| on_vca(p, &[ModSource::Env2], None), STILL),
    ("B VCA", |p| b_on_vca(p, Func::Env(EnvForm::Ad), 0.5), STILL),
    // ENV with SHAPE off centre: a curve divide per sample.
    (
        "B CURVE",
        |p| b_on_vca(p, Func::Env(EnvForm::Ad), 0.8),
        STILL,
    ),
    // LFO FREE: a `tilt` divide and a wrap per sample.
    (
        "B LFO",
        |p| b_on_vca(p, Func::Lfo(LfoForm::Free), 0.8),
        STILL,
    ),
    (
        "B GLIDE",
        |p| b_on_vca(p, Func::Lfo(LfoForm::Free), 0.8),
        glide,
    ),
    // BURST with TILT off centre: `fast_sin`, square and two `tilt` divides.
    (
        "BURST AD",
        |p| b_on_vca(p, Func::Burst(EnvForm::Ad), 0.8),
        STILL,
    ),
    (
        "BURST CYC",
        |p| b_on_vca(p, Func::Burst(EnvForm::Cycle), 0.8),
        STILL,
    ),
    ("VEL VCA", |p| on_vca(p, &[ModSource::Vel], None), STILL),
    (
        "2 VCA",
        |p| on_vca(p, &[ModSource::Vel, ModSource::Note], None),
        STILL,
    ),
    ("LFO VCA", |p| on_vca(p, &[ModSource::Lfo1], None), STILL),
    ("A SLIDE", |p| slide(p, None), STILL),
    (
        "B SLIDE",
        |p| slide(p, Some((Func::Env(EnvForm::Ad), 0.8))),
        STILL,
    ),
    ("FOLD", |p| one_op(p).folder.fold = 1.0, STILL),
    ("DRIVE", |p| one_op(p).drive.drive = 1.0, STILL),
    ("1 DEST", |p| one_dest(p, LfoType::Classic), STILL),
    ("FUNC LFO", |p| one_dest(p, LfoType::Func), STILL),
    ("A LEVEL", a_level, STILL),
    // |x| ≤ 1.4·|dry|: every sample takes `fast_tanh`'s divide.
    ("DRIVE LO", |p| one_op(p).drive.drive = 0.05, STILL),
    // `apply_offset`, and the SVF's ramped `g` every block.
    (
        "1 CUTOFF",
        |p| {
            one_op(p);
            p.mod_state = matrix(&[(ModSource::Lfo1, CUTOFF, 127)]);
        },
        STILL,
    ),
    // Driven hard: `saturate` takes its divide on about 1 call in 6, which
    // the SVF row's level never reaches. HOT − 1 OP is the hot LP24 term.
    ("LP24 HOT", |p| hot(p, FilterMode::Lp24), STILL),
    ("SVF HOT", |p| hot(p, FilterMode::Phaser), STILL),
    // Each Modal model, the default Sound (BODY 0.3) otherwise:
    // `ModalEngine::cost`. STR − STR0 and SYM − SYM0 are `BODY`; BODY
    // costs the same at any amount above 0.
    ("MDL STR", |p| modal(p, ResonatorMode::String), STILL),
    ("MDL STR0", |p| bare(p, ResonatorMode::String), STILL),
    // STR E − STR0 is `ENSEMBLE`.
    ("MDL STR E", str_ens, STILL),
    // STR+ − STR is `ENSEMBLE` plus four LFO routes' `ModRouting` terms
    // (BODY 1 bills as 0.3); the re-split is in `COST_STRING`.
    ("MDL STR+", str_full, STILL),
    ("MDL BOW", |p| modal(p, ResonatorMode::Bowed), STILL),
    ("MDL SYM", |p| modal(p, ResonatorMode::Sympathetic), STILL),
    ("MDL SYM0", |p| bare(p, ResonatorMode::Sympathetic), STILL),
    // SYM+ − SYM is `ENSEMBLE` plus a chord glide always running,
    // unrouted: about `CHORD`'s work, which SYM LFO − SYM reads alone.
    (
        "MDL SYM+",
        |p| modal_full(p, ResonatorMode::Sympathetic),
        chord_storm,
    ),
    // SYM LFO − MDL SYM is `ModalEngine::CHORD`.
    ("SYM LFO", sym_lfo, STILL),
    ("MDL RES", |p| modal(p, ResonatorMode::Modal), STILL),
    // RES48 − RES is 16 × `COST_MODE`.
    (
        "MDL RES48",
        |p| {
            modal(p, ResonatorMode::Modal);
            p.params.modal.modes = BankModes::M48;
        },
        STILL,
    ),
    // MODE flipped every 4 blocks: restarts and rests every flip.
    ("SWITCH", |p| modal(p, ResonatorMode::String), switch_storm),
];

/// Sympathetic ended by ENV 1 on the VCA at RELEASE 0: a low note's loop
/// filters too seldom to fall silent within `IDLE_BLOCKS`, and a clear's
/// extent follows the loops' lengths, not how long they rang.
fn short_sym(p: &mut PartAudio) {
    modal(p, ResonatorMode::Sympathetic);
    p.params.envelopes[0].release = 0.0;
    p.mod_state = matrix(&[(ModSource::Env1, VCA, 127)]);
}

/// Sympathetic with LFO 1 (10 Hz sine) on STRUCTURE at 127: it crosses
/// chords faster than they glide, so the halo is always gliding.
fn sym_lfo(p: &mut PartAudio) {
    modal(p, ResonatorMode::Sympathetic);
    p.params.lfos[0].rate = 10.0;
    let structure = ParamAddr::new(BlockRef::Modal, ModalParams::STRUCTURE);
    p.mod_state = matrix(&[(ModSource::Lfo1, structure, 127)]);
}

/// `mode` at BODY 0: the model alone.
fn bare(p: &mut PartAudio, mode: ResonatorMode) {
    modal(p, mode);
    p.params.modal.body = 0.0;
}

/// `mode` with every extra on: BODY 1, the ensemble at full DEPTH and MIX
/// 0.5, STRUCTURE mid-range.
fn modal_full(p: &mut PartAudio, mode: ResonatorMode) {
    modal(p, mode);
    let m = &mut p.params.modal;
    (m.structure, m.body, m.ens_depth, m.ens_mix) = (0.5, 1.0, 1.0, 0.5);
}

/// STRING at BODY 0 with the ensemble at full DEPTH and MIX 0.5, no
/// routes.
fn str_ens(p: &mut PartAudio) {
    bare(p, ResonatorMode::String);
    (p.params.modal.ens_depth, p.params.modal.ens_mix) = (1.0, 0.5);
}

/// STRING in full, LFO 1 (10 Hz sine) into each macro at 64: the
/// dispersion re-splits every block.
fn str_full(p: &mut PartAudio) {
    modal_full(p, ResonatorMode::String);
    p.params.lfos[0].rate = 10.0;
    let at = |q| (ModSource::Lfo1, ParamAddr::new(BlockRef::Modal, q), 64);
    p.mod_state = matrix(&[
        at(ModalParams::STRUCTURE),
        at(ModalParams::BRIGHT),
        at(ModalParams::DAMP),
        at(ModalParams::POS),
    ]);
}

/// STRUCTURE one chord on every 8 blocks, faster than a glide ends.
fn chord_storm(p: &mut PartAudio, block: u32) {
    let chord = (block / 8) % CHORD_COUNT as u32;
    p.params.modal.structure = (chord as f32 + 0.5) / CHORD_COUNT as f32;
}

/// String and Sympathetic in turn, 4 blocks each.
fn switch_storm(p: &mut PartAudio, block: u32) {
    p.params.modal.mode = if (block / 4).is_multiple_of(2) {
        ResonatorMode::String
    } else {
        ResonatorMode::Sympathetic
    };
}

/// The Modal Sound with its model set to `mode`.
fn modal(p: &mut PartAudio, mode: ResonatorMode) {
    p.params = ParamSnapshot::for_engine(EngineType::Modal);
    p.params.modal.mode = mode;
}

/// 1 OP through the SVF in `mode` at DRIVE 1, RES 1 and CUTOFF 1000 Hz
/// (not the Sound's 20 kHz: the host sweep's worst for `saturate`).
fn hot(p: &mut PartAudio, mode: FilterMode) {
    let f = &mut one_op(p).filter;
    let _ = f.set_mode(mode);
    (f.drive, f.cutoff, f.resonance) = (1.0, 1000.0, 1.0);
}

/// 1 OP, no routes.
fn one_op(p: &mut PartAudio) -> &mut ParamSnapshot {
    p.params = algo(AlgoId::A1, 0b1, 0);
    &mut p.params
}

/// 1 OP with LFO 1, of type `t`, into MORPH at 127: one destination's
/// per-block offset.
fn one_dest(p: &mut PartAudio, t: LfoType) {
    one_op(p).lfos[0].lfo_type = t;
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    p.mod_state = matrix(&[(ModSource::Lfo1, morph, 127)]);
}

/// A VCA with LFO 1 (1 Hz sine, so moving) into ENV 2's LEVEL: the peak
/// ramps across every block.
fn a_level(p: &mut PartAudio) {
    one_op(p);
    let level = ParamAddr::new(BlockRef::Env(EnvSlot::Env2), EnvParams::LEVEL);
    p.mod_state = matrix(&[(ModSource::Env2, VCA, 127), (ModSource::Lfo1, level, 127)]);
}

/// `routes` as the Part's matrix, each destination primed.
fn matrix(routes: &[(ModSource, ParamAddr, i8)]) -> ModState {
    let mut reg = ModDestRegistry::new();
    for &(_, a, _) in routes {
        let _ = reg.add(a, *b"BENCH\0\0\0");
    }
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    for &(s, a, amount) in routes {
        if let Some(d) = ms.find(a) {
            ms.set_route(s.index(), d, amount);
        }
    }
    ms
}

/// 1 OP with `sources` routed to the VCA at 127; with `b`, ENV 2 is type
/// B running that `Func` at that SHAPE (0.5 linear, 0.8 curved or tilted).
fn on_vca(p: &mut PartAudio, sources: &[ModSource], b: Option<(Func, f32)>) {
    p.params = algo(AlgoId::A1, 0b1, 0);
    if let Some((f, shape)) = b {
        let e2 = &mut p.params.envelopes[1];
        e2.env_type = EnvType::B;
        e2.func.set_func(f);
        e2.func.shape = shape;
    }
    let mut routes = [(ModSource::Env1, VCA, 127); 2];
    for (r, &s) in routes.iter_mut().zip(sources) {
        r.0 = s;
    }
    p.mod_state = matrix(&routes[..sources.len()]);
}

/// ENV 2, type B running `f` at `shape`, alone on the VCA.
fn b_on_vca(p: &mut PartAudio, f: Func, shape: f32) {
    on_vca(p, &[ModSource::Env2], Some((f, shape)));
}

/// LFO FREE with a FORM change's glide always running: every
/// `Glide::SAMPLES` one block runs LFV, so FREE takes over again and a
/// fresh glide starts before the last one ends. Each change also rebuilds
/// the coefficients that block.
fn glide(p: &mut PartAudio, block: u32) {
    let every = u32::from(Glide::SAMPLES) / BLOCK_SIZE as u32;
    let form = if block.is_multiple_of(every) {
        LfoForm::Lfv
    } else {
        LfoForm::Free
    };
    p.params.envelopes[1].func.set_func(Func::Lfo(form));
}

/// ENV 2 alone on the VCA (type A, or B per `b`), LFO 1 (1 Hz sine, so
/// moving) into ENV 2's TIME, RISE, FALL and SHAPE: its coefficients
/// rebuild every block. At 32 the times move little, so no AD ends early.
fn slide(p: &mut PartAudio, b: Option<(Func, f32)>) {
    on_vca(p, &[ModSource::Env2], b);
    let env2 = |q| ParamAddr::new(BlockRef::Env(EnvSlot::Env2), q);
    p.mod_state = matrix(&[
        (ModSource::Env2, VCA, 127),
        (ModSource::Lfo1, env2(EnvParams::TIME), 32),
        (ModSource::Lfo1, env2(EnvParams::RISE), 32),
        (ModSource::Lfo1, env2(EnvParams::FALL), 32),
        (ModSource::Lfo1, env2(EnvParams::SHAPE), 32),
    ]);
}

/// Spec § Tests "Bench": 1 OP; ENV 2 type B, ENV mode, SHAPE off centre →
/// VCA; ENV 1 → CUTOFF; every source routed; all three LFOs FUNC. FOLD is
/// stored at 1, so the folder runs whatever LFO 3 does.
fn mods(p: &mut PartAudio) {
    b_on_vca(p, Func::Env(EnvForm::Ad), 0.8);
    p.params.folder.fold = 1.0;
    for l in p.params.lfos.iter_mut() {
        l.lfo_type = LfoType::Func;
    }
    let res = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
    let drive = ParamAddr::new(BlockRef::Filter, FilterParams::DRIVE);
    let fold = ParamAddr::new(BlockRef::Folder, FolderParams::FOLD);
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    let level = ParamAddr::new(BlockRef::Out, OutParams::VOLUME);
    p.mod_state = matrix(&[
        (ModSource::Env1, CUTOFF, 64),
        (ModSource::Env2, VCA, 64),
        (ModSource::Env3, res, 64),
        (ModSource::Lfo1, morph, 64),
        (ModSource::Lfo2, drive, 64),
        (ModSource::Lfo3, fold, 64),
        (ModSource::Vel, level, 64),
        (ModSource::Note, CUTOFF, 64),
    ]);
}

static mut SCOPE: TripleBuffer<ScopeFrame> = scope_buffer();
// A static, not a local: `AudioShared` is 3 KB (ADR 0020).
static mut SHARED: MaybeUninit<AudioShared> = MaybeUninit::uninit();
// SAFETY: ".ram_d2.voices" is NOLOAD, so this holds garbage at boot; sound
// because it is `MaybeUninit` and `time_rebuild` builds it in place before
// any read. In D2 with the `Instrument`, as a voice's slot is.
#[unsafe(link_section = ".ram_d2.voices")]
static mut SLOT: MaybeUninit<EngineSlot> = MaybeUninit::uninit();
static mut SYM: SymAlloc = SymAlloc::new();

/// Rebuilds and note-ons timed, each averaged.
const ROUNDS: u32 = 16;
/// Render-to-idle gives up after 10 s of blocks: a voice stuck on.
const IDLE_BLOCKS: u32 = 10 * SAMPLE_RATE / BLOCK_SIZE as u32;

struct Rig<'p> {
    inst_slot: &'static mut MaybeUninit<Instrument>,
    fx_slot: &'static mut MaybeUninit<FxBus>,
    shared_slot: &'static mut MaybeUninit<AudioShared>,
    perf: &'p Performance,
    scope: ScopeWriter,
    dac: DacBlocks,
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
        dac: DacBlocks::new(),
    };
    let mut rows = [Counts::default(); ROWS];
    for (row, &(_, patch, low, step)) in rows.iter_mut().zip(&PATCHES) {
        for n in 1..=MAX_VOICES {
            let setup = |s: &mut AudioShared| s.parts[0].params = black_box(patch());
            row.add(n, rig.time(setup, |_, _| {}, n, low, step));
        }
    }
    let fx: [u32; FX_ROWS] = core::array::from_fn(|i| rig.time_bus(FX[i].1));
    let kernel = time_kernel();
    show(display, clocks, &rows, kernel, &fx);
    hold(clocks);
    let mut routing = [Counts::default(); ROUTING_ROWS];
    for (row, &(_, part, each)) in routing.iter_mut().zip(&ROUTING) {
        for n in 1..=MAX_VOICES {
            let timed = rig.time(
                |s| {
                    part(&mut s.parts[0]);
                    black_box(&s.parts[0]);
                },
                |s, b| each(&mut s.parts[0], b),
                n,
                36,
                12,
            );
            row.add(n, timed);
        }
    }
    let pages = ROUTING
        .chunks(ROUTING_PAGE)
        .zip(routing.chunks(ROUTING_PAGE));
    for (page, (labels, counts)) in pages.enumerate() {
        show_routing(display, page, labels, counts);
        hold(clocks);
    }
    let rebuild = time_rebuild();
    // A4 after A4, the default Sound: the lines clear a loop each.
    let note_on = rig.time_sym_note_on(MidiNote::A4, |p| modal(p, ResonatorMode::Sympathetic));
    // The lowest note after itself: every ring whole, the worst case.
    let lowest = MidiNote::new(0).unwrap_or(MidiNote::A4);
    let lowest = rig.time_sym_note_on(lowest, short_sym);
    show_memory(display, rebuild, note_on, lowest);
    hold(clocks);
}

/// Cycles per `rebuild` into Sympathetic from Algo: the lend and the main
/// string, no set. Each is undone by an untimed rebuild into Algo, which
/// gives the lease back.
#[inline(never)]
fn time_rebuild() -> u32 {
    // SAFETY: as in `run`: the bench is the only user of its statics, and
    // these references end when this returns.
    let (slot, pool) = unsafe { (&mut *addr_of_mut!(SLOT), &mut *addr_of_mut!(SYM)) };
    let slot = EngineSlot::init_in_place(slot, SlotKind::Algo);
    let voice = VoiceIdx::ALL[0];
    let sym = black_box(SlotKind::Modal(ResonatorMode::Sympathetic));
    let mut cycles = 0u32;
    for _ in 0..ROUNDS {
        slot.rebuild(SlotKind::Algo, pool, voice);
        // Promised, as the `Instrument` places a note: it lends that slot.
        pool.place(voice);
        let start = DWT::cycle_count();
        slot.rebuild(sym, pool, voice);
        cycles = cycles.wrapping_add(DWT::cycle_count().wrapping_sub(start));
        black_box(&*slot);
        // The pool is free each round: a bare build would time no lend.
        assert!(slot.rings());
    }
    slot.rebuild(SlotKind::Algo, pool, voice);
    cycles / ROUNDS
}

/// A row's cycles per sample at 1..=`MAX_VOICES` notes, and how many
/// voices its per-voice figure spans: those that rang a Sympathetic set at
/// the most notes, or with none ringing, those that sounded.
#[derive(Clone, Copy)]
struct Counts {
    cycles: [u32; MAX_VOICES],
    spans: usize,
}

impl Default for Counts {
    fn default() -> Self {
        Self {
            cycles: [0; MAX_VOICES],
            spans: MAX_VOICES,
        }
    }
}

impl Counts {
    fn add(&mut self, notes: usize, (cycles, spans): (u32, usize)) {
        self.cycles[notes - 1] = cycles;
        self.spans = spans;
    }

    /// The cost of a voice that sounds, or for Sympathetic that rings:
    /// from one note to as many as that (the pool rings at most 4; the
    /// notes past play bare, cheaper, and would dilute the slope).
    fn per_voice(&self) -> u32 {
        let k = self.spans.clamp(2, MAX_VOICES);
        self.cycles[k - 1].saturating_sub(self.cycles[0]) / (k as u32 - 1)
    }
}

fn hold(clocks: Clocks) {
    for _ in 0..HOLD_SECONDS {
        crate::clocks::delay_us(clocks.cpu_hz, 1_000_000);
    }
}

impl Rig<'_> {
    /// Voice `v` plays note `low + step * v`; `each` runs before every
    /// block, its time counted (a few cycles a block). Returns the cycles
    /// per sample and the voices its per-voice figure spans (`Counts`).
    #[inline(never)]
    fn time(
        &mut self,
        setup: impl FnOnce(&mut AudioShared),
        mut each: impl FnMut(&mut AudioShared, u32),
        voices: usize,
        low: u8,
        step: u8,
    ) -> (u32, usize) {
        // The allocator must not refuse what the bench wants to measure.
        let budget = SampleBudget::for_cpu(u32::MAX);
        let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
        let fx = FxBus::init_in_place(self.fx_slot);
        let shared = AudioShared::init_in_place(self.shared_slot, self.perf);
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
        for b in 0..WARM_BLOCKS {
            each(shared, b);
            inst.render(fx, &mut self.dac, shared, &mut self.scope);
        }
        let start = DWT::cycle_count();
        for b in 0..TIMED_BLOCKS {
            each(shared, WARM_BLOCKS + b);
            inst.render(fx, &mut self.dac, shared, &mut self.scope);
        }
        let cycles = DWT::cycle_count().wrapping_sub(start);
        let spans = match inst.ringing() {
            0 => inst.sounding(),
            ringing => ringing,
        };
        (cycles / (TIMED_BLOCKS * BLOCK_SIZE as u32), spans)
    }

    /// Cycles of `Instrument::handle` for one Sympathetic note-on of
    /// `note`, Part 0 set by `part`, on an idle voice: `place`, `lend`, the
    /// rebuild, the clear and the excitation. Each round releases the note
    /// and renders until every voice is idle again; one untimed round
    /// warms first, so the slot's lines hold `note`'s last round.
    #[inline(never)]
    fn time_sym_note_on(&mut self, note: MidiNote, part: fn(&mut PartAudio)) -> u32 {
        let budget = SampleBudget::for_cpu(u32::MAX);
        let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
        let fx = FxBus::init_in_place(self.fx_slot);
        let shared = AudioShared::init_in_place(self.shared_slot, self.perf);
        part(&mut shared.parts[0]);
        let ev = |kind| NoteEvent {
            channel: MidiChannel::clamped(0),
            note,
            kind,
        };
        let mut cycles = 0u32;
        for round in 0..=ROUNDS {
            let start = DWT::cycle_count();
            inst.handle(black_box(ev(NoteKind::On(Velocity::DEFAULT))), shared);
            if round > 0 {
                cycles = cycles.wrapping_add(DWT::cycle_count().wrapping_sub(start));
            }
            inst.handle(ev(NoteKind::Off), shared);
            for _ in 0..IDLE_BLOCKS {
                inst.render(fx, &mut self.dac, shared, &mut self.scope);
                if inst.allocator().slots().iter().all(|s| s.is_free()) {
                    break;
                }
            }
        }
        cycles / ROUNDS
    }

    /// Six Parts written with one noise block, sends 0.5, no voices:
    /// `mix_parts` alone, the output limiter included (ADR 0050). `each`
    /// sets the FX before every block.
    #[inline(never)]
    fn time_bus(&mut self, each: fn(&mut AudioShared, u32)) -> u32 {
        let fx = FxBus::init_in_place(self.fx_slot);
        let shared = AudioShared::init_in_place(self.shared_slot, self.perf);
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
        let mut block = |shared: &mut AudioShared, fx: &mut FxBus, dac: &mut DacBlocks, b: u32| {
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
/// (the top mip clamps: about 21 KB of tables at six voices, all 64 KB at
/// eight, read from their uncached, zero-wait DTCM copy
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
/// One count per voice across the screen: 29 px holds five digits at eight.
const CELL_W: i32 = (theme::SCREEN_W - 8) / MAX_VOICES as i32;

fn show(
    display: &mut impl ChimeraDisplay,
    clocks: Clocks,
    rows: &[Counts; ROWS],
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
    line.clear();
    let _ = write!(line, "CYCLES/SAMPLE, 1..{MAX_VOICES} VOICES");
    draw::text(
        display,
        &theme::FONT_LABEL,
        line.as_str(),
        4,
        30,
        theme::MID,
    );
    let mut y = 46;
    for (&(label, ..), counts) in PATCHES.iter().zip(rows) {
        voice_row(display, &mut line, y, label, counts);
        y += ROW_H;
    }
    line.clear();
    let _ = write!(line, "KERNEL /VOICE {kernel} (350)");
    draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, y, theme::INK);
    // Seven rows, three to a line: with FX_ROWS at 7 the old 18/14 spacing
    // put the third line at y 317; 16/10 keeps it at 307, under 310.
    y += 16;
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
            y + row * 10,
            theme::INK2,
        );
    }
    display.flush();
}

/// ROUTING screen `page` (from 0): `rows`' labels and their counts.
fn show_routing(
    display: &mut impl ChimeraDisplay,
    page: usize,
    rows: &[RoutingRow],
    counts: &[Counts],
) {
    draw::fill_rect(display, 0, 0, theme::SCREEN_W, theme::SCREEN_H, theme::BG);
    let mut line = FmtBuf::new();
    let pages = ROUTING_ROWS.div_ceil(ROUTING_PAGE);
    let _ = write!(line, "ROUTING {}/{pages}", page + 1);
    draw::text(
        display,
        &theme::FONT_VALUE,
        line.as_str(),
        4,
        16,
        theme::INK,
    );
    for (i, (&(label, ..), c)) in rows.iter().zip(counts).enumerate() {
        voice_row(display, &mut line, 46 + i as i32 * ROW_H, label, c);
    }
    display.flush();
}

/// The sizes behind the D2 budget and the exclusive state's two timings.
fn show_memory(display: &mut impl ChimeraDisplay, rebuild: u32, note_on: u32, lowest: u32) {
    use core::mem::size_of;
    draw::fill_rect(display, 0, 0, theme::SCREEN_W, theme::SCREEN_H, theme::BG);
    draw::text(display, &theme::FONT_VALUE, "MEMORY", 4, 16, theme::INK);
    let lines = [
        format_args!("VOICE {}", size_of::<Voice>()),
        format_args!("SLOT {}", size_of::<EngineSlot>()),
        format_args!("MODAL {}", size_of::<ModalEngine>()),
        format_args!("SYM POOL {}", size_of::<SymPool>()),
        format_args!("INSTR {}/{VOICE_RAM_BUDGET}", size_of::<Instrument>()),
        format_args!("REBUILD {rebuild} CYC"),
        format_args!("SYM NOTE-ON {note_on} CYC"),
        format_args!("SYM NOTE-ON LOW {lowest} CYC"),
    ];
    let mut line = FmtBuf::new();
    for (i, args) in lines.into_iter().enumerate() {
        line.clear();
        let _ = line.write_fmt(args);
        let y = 46 + i as i32 * ROW_H;
        draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, y, theme::INK);
    }
    display.flush();
}

/// `label`'s per-voice cost (`Counts::per_voice`), with the voices that
/// sounded when fewer than all, and its count at each voice count.
fn voice_row(
    display: &mut impl ChimeraDisplay,
    line: &mut FmtBuf,
    y: i32,
    label: &str,
    counts: &Counts,
) {
    line.clear();
    let _ = write!(line, "{label} /VOICE {}", counts.per_voice());
    if counts.spans < MAX_VOICES {
        let _ = write!(line, " ({} V)", counts.spans);
    }
    draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, y, theme::INK);
    // One cell per count: the counts together overflow a `FmtBuf`.
    for (i, c) in counts.cycles.iter().enumerate() {
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
