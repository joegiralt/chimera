#![cfg_attr(
    not(all(
        feature = "bench-algo",
        feature = "bench-routing",
        feature = "bench-memory",
        feature = "bench-stress",
        feature = "bench-orbit",
        feature = "bench-tape"
    )),
    allow(dead_code, unused_imports)
)]
//! The bench on the unit, an area a build (`bench-algo`, `bench-routing`,
//! `bench-memory`, `bench-stress`, `bench-orbit`, `bench-tape`): all of it
//! no longer fits FLASH beside the synth. What an area leaves out is dead
//! here; clippy checks every area at once.

use core::hint::black_box;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_board::{Display, bench_screen};
use chimera_core::console::Report;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::{draw, theme};
use chimera_engine::addr::{BlockRef, ParamAddr};
use chimera_engine::audio_out::{DacSample, interleave};
use chimera_engine::dsp::algo::algorithms::AlgoId;
use chimera_engine::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use chimera_engine::dsp::algo::kernel::{Kernel, KernelBlock, KindBlock, OpBlock, SAMPLE_SCALE};
use chimera_engine::dsp::algo::kinds::{Kinds, OpKind};
use chimera_engine::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_engine::dsp::algo::plan::{EvalPlan, OPS};
use chimera_engine::dsp::algo::tx::FEEDBACK_CYCLES;
use chimera_engine::dsp::algo::waves::WaveId;
use chimera_engine::dsp::engines::{EngineSlot, SlotKind};
use chimera_engine::dsp::filter::{FilterKind, FilterMode, K2040Voice};
use chimera_engine::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_engine::dsp::modal::{
    BankModes, CHORD_COUNT, ModalEngine, ModalParams, ResonatorMode, SymPool,
};
use chimera_engine::dsp::modulator::{EnvForm, EnvSlot, EnvType, Func, Glide, LfoForm, LfoType};
use chimera_engine::dsp::rev::extra_cost;
use chimera_engine::dsp::rev::sync::Tempo;
use chimera_engine::dsp::reverb::ReverbType;
use chimera_engine::dsp::tape_engine::read::{DESC_LEN, Descriptor, MIN_SOURCE_FRAMES, Shape};
use chimera_engine::dsp::tape_engine::{PlayMode, StretchType, TapeMachine, TapeModel, wave};
use chimera_engine::dsp::va::VaMode;
use chimera_engine::dsp::voice::Voice;
use chimera_engine::dsp::voice_fx::params::FxType;
#[cfg(feature = "bench-algo")]
use chimera_engine::factory::{Demo, demo_sound};
use chimera_engine::factory::{FACTORY_LEN, factory_sound};
use chimera_engine::hw::{
    BLOCK_SIZE, BlockBudget, MAX_PARTS, MAX_VOICES, SAMPLE_RATE, SampleBudget, VOICE_RAM_BUDGET,
};
use chimera_engine::instrument::{
    AudioShared, DacBlocks, Instrument, MixState, NoteOrigin, PartAudio, mix_parts,
};
use chimera_engine::modulation::{CUTOFF, FixedCol, MAX_MOD_SOURCES, ModSource, ModState, VCA};
use chimera_engine::note_queue::{NoteEvent, NoteKind};
#[cfg(feature = "orbit")]
use chimera_engine::orbit::runner::OrbitRunner;
use chimera_engine::params::{
    EngineType, EnvParams, FilterParams, OutParams, ParamSnapshot, Steal,
};
use chimera_engine::part::{DacPair, PartParams};
use chimera_engine::preset::{Performance, Sound};
use chimera_engine::project::{NEW_PARTS, Project, SlotId, SourceId, SourceRef, project_crc};
use chimera_engine::scope::{ScopeFrame, ScopeWriter, scope_buffer};
use chimera_engine::source_table::{AudioToken, GateView};
use chimera_engine::stream::port::{StreamCard, Streams};
use chimera_engine::stream::ring::{AudioSide, CardSide};
use chimera_engine::stream::stats::StreamStats;
use chimera_engine::stream::units::{AudioBlocks, Frames, SlotIdx, SourceRate};
use chimera_engine::sym_alloc::SymAlloc;
use chimera_engine::taps::TapOp;
use chimera_engine::triple::{TripleBuffer, Writer};
use chimera_engine::voice_alloc::VoiceIdx;
use chimera_engine::{MidiChannel, MidiNote, Velocity};
use chimera_engine::{
    dsp::knobs::{KnobPublisher, Knobs},
    dsp::lfo::LfoClock,
    project::Loads,
    tracked::Tracked,
};
use chimera_hal::card_bus::CardBus;
use cortex_m::peripheral::DWT;
use embedded_graphics_core::pixelcolor::Rgb565;

use crate::audio::engine;
use chimera_board::clocks::{self, Clocks};
#[cfg(feature = "orbit")]
use chimera_board::orbit::SAMPLE_RATE_NZ;

/// The bench's screens as text: under 4 KB measured with every page in
/// one build (3.9 KB), an area's less; `# TRUNCATED` past this.
pub const BENCH_TEXT_LEN: usize = 6144;
type Text = Report<BENCH_TEXT_LEN>;

// A `static mut`, not a `singleton!`: that builds its value on the caller's
// stack before moving it in, 6 KB on `synth`'s frame. In D2, out of AXI's
// margin: nothing times it.
#[unsafe(link_section = ".ram_d2.voices")]
static mut REPORT: MaybeUninit<Text> = MaybeUninit::uninit();
static REPORT_TAKEN: AtomicBool = AtomicBool::new(false);

/// The report, once: `run` writes it, the console reads it from then on.
pub fn take_report() -> Option<&'static mut Text> {
    if REPORT_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `REPORT` ever made. ".ram_d2.voices" is NOLOAD, so it
    // is zeroed here, in place: a `Report` of all zero bytes is an empty
    // one (its bytes, a zero length, not cut).
    unsafe {
        let r = &mut *addr_of_mut!(REPORT);
        r.as_mut_ptr().write_bytes(0, 1);
        Some(r.assume_init_mut())
    }
}

/// The bench plays no pool source: its TAPE rows are refused, as before.
const NO_POOL: GateView<'static> = GateView::empty();
const WARM_BLOCKS: u32 = 8;
/// A row that sets another reverb TYPE than the fresh bus's QUAD warms
/// through the switch to it (15 + 4 + 15 blocks) before it is timed.
const SWITCH_WARM: u32 = 34;
const TIMED_BLOCKS: u32 = 64;
const HOLD_SECONDS: u32 = 30;
const ROWS: usize = 10;
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
/// `ParamSnapshot::for_engine`, once: inlined, each bench row carried
/// its own copy.
#[inline(never)]
fn snapshot(e: EngineType) -> ParamSnapshot {
    ParamSnapshot::for_engine(e)
}

const PATCHES: [Row; ROWS] = [
    ("MODAL", || snapshot(EngineType::Modal), 48, 5),
    ("FLOOR", || algo(AlgoId::A1, 0, 0), 36, 12),
    ("1 OP", || algo(AlgoId::A1, 0b1, 0), 36, 12),
    // Operators 1, 3 and 5: none can pair, so each runs alone.
    ("ALT", || algo(AlgoId::A1, 0b1_0101, 0), 36, 12),
    ("6 OP", || algo(AlgoId::A1, ALL, 0), 36, 12),
    ("CHAIN", || algo(AlgoId::A17, ALL, 0), 36, 12),
    ("CHN FB", || algo(AlgoId::A17, ALL, 7), 36, 12),
    ("WC", || algo_pair(AlgoId::A14, AlgoId::A22), 36, 12),
    ("A16+17", a16_a17, 36, 12),
    // LADDER − 1 OP: LADDER's steady bill over CLEAN LP4 (ADR 0104).
    ("LADDER", ladder_op, 36, 12),
];

/// The VA screen's rows, a row a MODE: each at BLEND 1 (the operator all in),
/// SYNC's and SOFT SYNC's slave a fifth up, FM's and PHASE's modulator at
/// unison, LOOPFM's WAVE at its default (2/3). A new MODE appends its row.
const VA_ROWS: usize = 11;
const VA: [Row; VA_ROWS] = [
    ("MIX", || va(VaMode::Mix), 36, 12),
    ("SYNC", || va(VaMode::Sync), 36, 12),
    ("RING", || va(VaMode::Ring), 36, 12),
    ("XOR", || va(VaMode::Xor), 36, 12),
    ("BITS", || va(VaMode::Bits), 36, 12),
    ("FM", || va(VaMode::Fm), 36, 12),
    ("AM", || va(VaMode::Am), 36, 12),
    ("MAX", || va(VaMode::Max), 36, 12),
    ("LOOPFM", || va(VaMode::LoopFm), 36, 12),
    ("SOFT", || va(VaMode::SoftSync), 36, 12),
    ("PHASE", || va(VaMode::Phase), 36, 12),
];

#[inline(never)]
fn va(mode: VaMode) -> ParamSnapshot {
    let mut p = snapshot(EngineType::Va);
    p.va.mode = mode;
    p.va.amount = 1.0;
    match mode {
        VaMode::Sync | VaMode::SoftSync => p.va.coarse[1] = 7,
        VaMode::Fm | VaMode::Phase => p.va.coarse[1] = 0,
        _ => {}
    }
    p
}

/// 1 OP through LADDER LP4.
fn ladder_op() -> ParamSnapshot {
    let mut p = algo(AlgoId::A1, 0b1, 0);
    p.filter.set_kind(FilterKind::Ladder);
    p
}

/// The FORMANT screen's rows (ADR 0131): each term's case at 1..8 voices,
/// notes C2 up in tenths (to A♯7: the terms cover grain clocks to C8).
const FORMANT_ROWS: usize = 6;
const FORMANT: [RoutingRow; FORMANT_ROWS] = [
    // One FRMT on A1, BW 0: the spans at SKIRT 0 and at SKIRT 3.
    ("FRMT0", |p| *p.params = frmt(0), STILL),
    ("FRMT3", |p| *p.params = frmt(3), STILL),
    // FRMT3 under a full ratio-1 sine (T1's 2 → 1): the moving loop.
    ("FRMT PM", |p| *p.params = frmt_pm(), STILL),
    (
        "NOIS",
        |p| *p.params = kinds(AlgoId::A1, AlgoId::A1, [N, T, T, T, T, T], 0b1, 0),
        STILL,
    ),
    // The costliest patch the budget admits (`frmt_max` in cost_test):
    // T1 ∪ A16, a linked FRMT on 2, the unlinked one on 6, two NOIS.
    (
        "FRMT MAX",
        |p| *p.params = kinds(AlgoId::T1, AlgoId::A16, [N, F, T, N, T, F], ALL, 1),
        STILL,
    ),
    ("CHOIR", choir, STILL),
];
const F: OpKind = OpKind::Formant;
const N: OpKind = OpKind::Noise;
const T: OpKind = OpKind::Table(WaveId::W1);

/// `k` on `a` ∪ `b` at MORPH 64, as `algo` lights and feeds them; formant
/// and noise bands at ratio 1, so each voice's grain clock is its note.
#[inline(never)]
fn kinds(a: AlgoId, b: AlgoId, k: [OpKind; OPS], lit: u8, feedback: u8) -> ParamSnapshot {
    let mut p = algo(a, lit, feedback);
    for (o, k) in p.algo.ops.iter_mut().zip(k) {
        if k.is_formant() {
            o.coarse = AlgoOpParams::DEFAULT.coarse;
        }
    }
    p.algo = AlgoParams::new(a, b, 64, Kinds::new(k), p.algo.ops);
    p
}

/// One FRMT on A1 at SKIRT `skirt`, BW 0.
fn frmt(skirt: u8) -> ParamSnapshot {
    let mut p = kinds(AlgoId::A1, AlgoId::A1, [F, T, T, T, T, T], 0b1, 0);
    (p.algo.ops[0].skirt, p.algo.ops[0].bw) = (skirt, 0);
    p
}

/// FRMT3 on T1, operator 2 a sine at ratio 1 and LEVEL 99 into it.
fn frmt_pm() -> ParamSnapshot {
    let mut p = kinds(AlgoId::T1, AlgoId::T1, [F, T, T, T, T, T], 0b11, 0);
    (p.algo.ops[0].skirt, p.algo.ops[0].bw) = (3, 0);
    p.algo.ops[1].coarse = AlgoOpParams::DEFAULT.coarse;
    p
}

/// The CHOIR demo Sound, its VOWEL route in.
#[cfg(feature = "bench-algo")]
fn choir(p: &mut PartAudio) {
    let s = demo_sound(Demo::Choir);
    (*p.params, *p.mod_state) = (s.params, s.mod_state);
}
#[cfg(not(feature = "bench-algo"))]
fn choir(_: &mut PartAudio) {}

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
    ("REV QUAD", worst_reverb),
    #[cfg(feature = "master-tape")]
    ("REEL", |s, _| worst_tape(s)),
    ("COMP", |s, _| worst_comp(s)),
    ("BUS", worst_bus),
];

/// The REVERB screen's rows, each timed as an FX row is, less MIX.
const REV_ROWS: usize = 7;
const REV: [FxRow; REV_ROWS] = [
    ("REV QUAD", worst_reverb),
    ("REV QUAD S", smear_reverb),
    ("REV GATE", |s, b| worst_burst(s, b, ReverbType::Gate)),
    ("REV RVRS", |s, b| worst_burst(s, b, ReverbType::Reverse)),
    ("REV ROOM", |s, b| worst_loop(s, b, ReverbType::Room)),
    ("REV HALL", |s, b| worst_loop(s, b, ReverbType::Hall)),
    ("REV BLOOM", |s, b| worst_loop(s, b, ReverbType::Bloom)),
];

/// The STRESS rows' blocks: 10 s, past NEW's four bars at its BPM.
const STRESS_BLOCKS: u32 = 7_500;

/// REV SWITCH MAX's blocks, and a switch every `SWITCH_EVERY` of them:
/// FADE OUT, CLEAR, the refill after it and FADE IN all land inside.
const SWITCH_BLOCKS: u32 = 2_000;
const SWITCH_EVERY: u32 = 40;

/// Every effect at its worst: the BUS row's.
fn worst_bus(s: &mut AudioShared, b: u32) {
    worst_chorus(s);
    worst_delay(s);
    worst_reverb(s, b);
    #[cfg(feature = "master-tape")]
    worst_tape(s);
    worst_comp(s);
}

fn worst_chorus(s: &mut AudioShared) {
    let c = &mut s.fx.chorus;
    (c.mode, c.rate, c.depth, c.mix) = (3, 1.0, 1.0, 0.5);
}

/// REV SEND costs only with the reverb on: the BUS row pays it.
fn worst_delay(s: &mut AudioShared) {
    let d = &mut s.fx.delay;
    (d.time_ms, d.wow_flutter, d.saturation, d.mix, d.rev_send) = (500.0, 1.0, 1.0, 0.5, 1.0);
}

/// The ring at its costliest: longest TIME and SIZE, with a SIZE
/// crossfade always running (steps 31 and 30 in turn), SMEAR off.
fn worst_reverb(s: &mut AudioShared, block: u32) {
    let r = &mut s.fx.reverb;
    let size = [1.0, 30.0 / 31.0][block as usize % 2];
    (r.time, r.size, r.damping, r.mix, r.smear) = (1.0, size, 0.5, 0.5, 0.0);
}

/// QUAD's SMEAR path: SMEAR 1 and RATE 1 with SIZE still, so the four
/// stage delays read moving pairs every block (reverb TYPEs spec § Bench
/// rows: gate ≤ REV QUAD).
fn smear_reverb(s: &mut AudioShared, _block: u32) {
    let r = &mut s.fx.reverb;
    (r.time, r.size, r.damping, r.mix, r.smear, r.rate) = (1.0, 1.0, 0.5, 0.5, 1.0, 1.0);
}

/// GATE or REVERSE at its worst (reverb TYPEs spec § Bench rows): LENGTH
/// and PRE-DLY (ONSET) between their ends every block, so every read
/// always crossfades (a PRE-DLY change is one window of all); DENSITY,
/// TAIL or REGEN, and SMEAR at 1. Both TYPEs run both paths.
fn worst_burst(s: &mut AudioShared, block: u32, kind: ReverbType) {
    let r = &mut s.fx.reverb;
    let size = [0.0, 1.0][block as usize % 2];
    (
        r.kind, r.size, r.predelay, r.time, r.diffuse, r.smear, r.rate, r.mix,
    ) = (kind, size, size, 1.0, 1.0, 1.0, 1.0, 0.5);
}

/// ROOM, HALL or BLOOM at its worst (reverb TYPEs spec § Bench rows): SIZE
/// (BLOOM's SWELL) and PRE-DLY between their ends every block, so a
/// crossfade always runs (HALL's SIZE 0 is its plate corner, whose zone
/// moves PRE-DLY too); DECAY, DIFFUSE, SMEAR and RATE at 1.
fn worst_loop(s: &mut AudioShared, block: u32, kind: ReverbType) {
    let r = &mut s.fx.reverb;
    let size = [0.0, 1.0][block as usize % 2];
    (
        r.kind, r.size, r.predelay, r.time, r.diffuse, r.smear, r.rate, r.mix,
    ) = (kind, size, size, 1.0, 1.0, 1.0, 1.0, 0.5);
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

const ROUTING_ROWS: usize = 52;
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
    ("1 OP", |p| *p.params = algo(AlgoId::A1, 0b1, 0), STILL),
    ("MODS", mods, STILL),
    // Each filter type steady, and CLEAN's moving path: SVF − 1 OP is
    // CLEAN LP4's term (billed 0), NOTCH2 − 1 OP NOTCH2's, 2040 − 1 OP and
    // DIRTY − 1 OP their steady bills, CLEAN MOVE − SVF `CLEAN_MOVE_COST`.
    ("SVF", |p| filter_mode(p, FilterMode::Lp24), STILL),
    ("NOTCH2", |p| filter_mode(p, FilterMode::Phaser), STILL),
    ("CLEAN MOVE", |p| filter_mode(p, FilterMode::Lp24), sweep),
    (
        "2040",
        |p| {
            // EXP: the 2040's costlier VOICE.
            let f = &mut one_op(p).filter;
            f.set_kind(FilterKind::K2040);
            f.voice = K2040Voice::Exp;
        },
        STILL,
    ),
    (
        "DIRTY",
        |p| one_op(p).filter.set_kind(FilterKind::Dirty),
        STILL,
    ),
    // OP5 tapped: TAP FREQ − 1 OP is a capture, `TapShape::apply` and
    // CLEAN's moving path; TAP VCA − 1 OP a capture and the tap-VCA.
    ("TAP FREQ", |p| tap(p, FixedCol::Freq), STILL),
    ("TAP VCA", |p| tap(p, FixedCol::Vca), STILL),
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
    ("FOLD", |p| fx(p, FxType::Fold, 1.0), STILL),
    ("SOFT", |p| fx(p, FxType::Soft, 1.0), STILL),
    ("1 DEST", |p| one_dest(p, LfoType::Classic), STILL),
    ("FUNC LFO", |p| one_dest(p, LfoType::Func), STILL),
    ("A LEVEL", a_level, STILL),
    // |x| ≤ 1.4·|dry|: every sample takes `fast_tanh`'s divide.
    ("SOFT LO", |p| fx(p, FxType::Soft, 0.05), STILL),
    // `apply_offset`, and the SVF's ramped `g` every block.
    (
        "1 CUTOFF",
        |p| {
            one_op(p);
            *p.mod_state = matrix(&[(ModSource::Lfo1, CUTOFF, 127)]);
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
    // BOW+ − BOW is a moving bow point's per-sample split, BRIGHT's output
    // low-pass and three routes.
    ("MDL BOW+", bow_full, STILL),
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
    // The FX slot's bills (FX plan Task 1): SOFT RT − SOFT and FOLD RT −
    // FOLD are each type's ramp plus a route (1 DEST − 1 OP); FOLD TAP −
    // FOLD is the ramp, `TAP_FOLD_COST` and a capture. 1 DEST, not 1 OP,
    // is their base: `matrix` drops the Part's default routes.
    ("FOLD HALF", |p| fx(p, FxType::Fold, 0.5), STILL),
    ("SOFT RT", |p| staged(p, FxType::Soft), STILL),
    ("FOLD RT", |p| staged(p, FxType::Fold), STILL),
    (
        "FOLD TAP",
        |p| {
            tap(p, FixedCol::Fx);
            p.params.fx.set_type(FxType::Fold);
            p.params.fx.pots[0] = 0.5;
        },
        STILL,
    ),
    // FUZZ (FX plan Task 13; ADR 0156): FUZZ − 1 OP is its steady bill
    // (`FxType::Fuzz.cost`, 175), FUZZ MOVE − FUZZ its ramp (SUSTAIN from
    // LFO 1; `ramp_cost`) less the route, and FUZZ 8V, at full SUSTAIN (the
    // ADAA guard's slow side), is read at its 8-voice cell.
    ("FUZZ", |p| fx(p, FxType::Fuzz, 0.5), STILL),
    ("FUZZ MOVE", |p| staged(p, FxType::Fuzz), STILL),
    ("FUZZ 8V", |p| fx(p, FxType::Fuzz, 1.0), STILL),
    (
        "2040 R2",
        |p| {
            let f = &mut one_op(p).filter;
            f.set_kind(FilterKind::K2040);
            f.voice = K2040Voice::Rev2;
        },
        STILL,
    ),
    (
        "LADDER",
        |p| one_op(p).filter.set_kind(FilterKind::Ladder),
        STILL,
    ),
];

/// 1 OP with the FX slot at `ty`, P1 ½, LFO 1 (1 Hz sine, so moving) into
/// the FX column at 127: the type ramps every block. At 1 the route's rise
/// clamps, and the slot reads steady.
fn staged(p: &mut PartAudio, ty: FxType) {
    fx(p, ty, 0.5);
    *p.mod_state = matrix(&[(ModSource::Lfo1, FixedCol::Fx.addr(), 127)]);
}

/// 1 OP with the FX slot at `ty`, in its default place, P1 at `p1`.
fn fx(p: &mut PartAudio, ty: FxType, p1: f32) {
    let s = one_op(p);
    s.fx.set_type(ty);
    s.fx.pots[0] = p1;
}

/// When `Rig::time_note_on`'s round may end.
type Idle = fn(&Instrument) -> bool;

/// Every pool slot free: a Sympathetic note-on's timed `place` and `lend`
/// start from a free slot, and a released halo keeps its lease until
/// silent (ADR 0054).
fn slots_free(inst: &Instrument) -> bool {
    inst.allocator().slots().iter().all(|s| s.is_free())
}

/// No voice sounding: a STRING note-on lands on an idle voice, not a
/// steal. STRING holds no slot, so `slots_free` is true at once.
fn silent(inst: &Instrument) -> bool {
    inst.sounding() == 0
}

/// Sympathetic ended by ENV 1 on the VCA at RELEASE 0: a low note's loop
/// filters too seldom to fall silent within `IDLE_BLOCKS`, and a clear's
/// extent follows the loops' lengths, not how long they rang.
fn short_sym(p: &mut PartAudio) {
    modal(p, ResonatorMode::Sympathetic);
    p.params.envelopes[0].release = 0.0;
    *p.mod_state = matrix(&[(ModSource::Env1, VCA, 127)]);
}

/// Sympathetic with LFO 1 (10 Hz sine) on STRUCTURE at 127: it crosses
/// chords faster than they glide, so the halo is always gliding.
fn sym_lfo(p: &mut PartAudio) {
    modal(p, ResonatorMode::Sympathetic);
    p.params.lfos[0].rate = 10.0;
    let structure = ParamAddr::new(BlockRef::Modal, ModalParams::STRUCTURE);
    *p.mod_state = matrix(&[(ModSource::Lfo1, structure, 127)]);
}

/// STRING ended as `short_sym` is, at COLOR 0: the pluck's seven
/// smoothing passes.
fn dark_pluck(p: &mut PartAudio) {
    short_string(p);
    p.params.modal.color = 0.0;
}

/// STRING ended as `short_sym` is, at the default COLOR: one pass.
fn short_string(p: &mut PartAudio) {
    modal(p, ResonatorMode::String);
    p.params.envelopes[0].release = 0.0;
    *p.mod_state = matrix(&[(ModSource::Env1, VCA, 127)]);
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
    *p.mod_state = matrix(&[
        at(ModalParams::STRUCTURE),
        at(ModalParams::BRIGHT),
        at(ModalParams::DAMP),
        at(ModalParams::POS),
    ]);
}

/// BOWED at FORCE 1, SPEED 1, POS 0.5 and BRIGHT 0, LFO 1 (10 Hz sine)
/// into BRIGHT, DAMP and POS at 64: the 10 Hz POS route moves the bow
/// point every block, so it glides across each block, split per sample
/// (`render_bowed`), the bow's worst case.
fn bow_full(p: &mut PartAudio) {
    modal(p, ResonatorMode::Bowed);
    let m = &mut p.params.modal;
    (m.force, m.speed, m.pos, m.bright) = (1.0, 1.0, 0.5, 0.0);
    p.params.lfos[0].rate = 10.0;
    let at = |q| (ModSource::Lfo1, ParamAddr::new(BlockRef::Modal, q), 64);
    *p.mod_state = matrix(&[
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
// Setup, never timed: one copy, not one a row.
#[inline(never)]
fn modal(p: &mut PartAudio, mode: ResonatorMode) {
    *p.params = snapshot(EngineType::Modal);
    p.params.modal.mode = mode;
}

/// 1 OP through CLEAN in `mode`.
fn filter_mode(p: &mut PartAudio, mode: FilterMode) {
    let _ = one_op(p).filter.set_mode(mode);
}

/// FREQ stepped 100 Hz a block, 500 Hz to 3.7 kHz and round: every block
/// a new cutoff, so every block takes the moving path.
fn sweep(p: &mut PartAudio, block: u32) {
    p.params.filter.cutoff = 500.0 + 100.0 * (block % 32) as f32;
}

/// 1 OP with OP5 tapped into `col` at 64.
fn tap(p: &mut PartAudio, col: FixedCol) {
    one_op(p).taps.set(TapOp::Op5, col, 64);
}

/// 1 OP through the SVF in `mode` at LEVEL 1 (×4), RES 1 and CUTOFF 1000 Hz
/// (not the Sound's 20 kHz: the host sweep's worst for `saturate`).
fn hot(p: &mut PartAudio, mode: FilterMode) {
    let f = &mut one_op(p).filter;
    let _ = f.set_mode(mode);
    (f.level, f.cutoff, f.resonance) = (1.0, 1000.0, 1.0);
}

/// 1 OP, no routes. Out of line: inlined, every row that builds on it
/// carried its own copy of the snapshot (about 700 B each).
#[inline(never)]
fn one_op(p: &mut PartAudio) -> &mut ParamSnapshot {
    *p.params = algo(AlgoId::A1, 0b1, 0);
    &mut p.params
}

/// 1 OP with LFO 1, of type `t`, into MORPH at 127: one destination's
/// per-block offset.
fn one_dest(p: &mut PartAudio, t: LfoType) {
    one_op(p).lfos[0].lfo_type = t;
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    *p.mod_state = matrix(&[(ModSource::Lfo1, morph, 127)]);
}

/// A VCA with LFO 1 (1 Hz sine, so moving) into ENV 2's LEVEL: the peak
/// ramps across every block.
fn a_level(p: &mut PartAudio) {
    one_op(p);
    let level = ParamAddr::new(BlockRef::Env(EnvSlot::Env2), EnvParams::LEVEL);
    *p.mod_state = matrix(&[(ModSource::Env2, VCA, 127), (ModSource::Lfo1, level, 127)]);
}

/// `routes` as the Part's matrix: fixed columns, else registry columns.
fn matrix(routes: &[(ModSource, ParamAddr, i8)]) -> ModState {
    let mut ms = ModState::with_sources(MAX_MOD_SOURCES);
    for &(s, a, amount) in routes {
        ms.set_route_addr(s.index(), a, amount);
    }
    ms
}

/// 1 OP with `sources` routed to the VCA at 127; with `b`, ENV 2 is type
/// B running that `Func` at that SHAPE (0.5 linear, 0.8 curved or tilted).
// Setup, never timed: one copy, not one a row.
#[inline(never)]
fn on_vca(p: &mut PartAudio, sources: &[ModSource], b: Option<(Func, f32)>) {
    *p.params = algo(AlgoId::A1, 0b1, 0);
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
    *p.mod_state = matrix(&routes[..sources.len()]);
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
    *p.mod_state = matrix(&[
        (ModSource::Env2, VCA, 127),
        (ModSource::Lfo1, env2(EnvParams::TIME), 32),
        (ModSource::Lfo1, env2(EnvParams::RISE), 32),
        (ModSource::Lfo1, env2(EnvParams::FALL), 32),
        (ModSource::Lfo1, env2(EnvParams::SHAPE), 32),
    ]);
}

/// Spec § Tests "Bench": 1 OP; ENV 2 type B, ENV mode, SHAPE off centre →
/// VCA; ENV 1 → CUTOFF; every source routed; all three LFOs FUNC. FOLD is
/// stored at 1, so the fold runs whatever LFO 3 does.
fn mods(p: &mut PartAudio) {
    b_on_vca(p, Func::Env(EnvForm::Ad), 0.8);
    p.params.fx.set_type(FxType::Fold);
    p.params.fx.pots[0] = 1.0;
    for l in p.params.lfos.iter_mut() {
        l.lfo_type = LfoType::Func;
    }
    let res = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
    let flt_level = ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL);
    let fold = FixedCol::Fx.addr();
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    let level = ParamAddr::new(BlockRef::Out, OutParams::VOLUME);
    *p.mod_state = matrix(&[
        (ModSource::Env1, CUTOFF, 64),
        (ModSource::Env2, VCA, 64),
        (ModSource::Env3, res, 64),
        (ModSource::Lfo1, morph, 64),
        (ModSource::Lfo2, flt_level, 64),
        (ModSource::Lfo3, fold, 64),
        (ModSource::Vel, level, 64),
        (ModSource::Note, CUTOFF, 64),
    ]);
}

// SAFETY: ".ram_d2" is NOLOAD, so this holds garbage at boot; sound
// because it is `MaybeUninit` and `run` writes it before any read. In D2,
// cacheable, out of AXI's margin: the bench's scope is never timed.
#[unsafe(link_section = ".ram_d2")]
static mut SCOPE: MaybeUninit<TripleBuffer<ScopeFrame>> = MaybeUninit::uninit();
// SAFETY: ".ram_d2.voices" is NOLOAD, so this holds garbage at boot; sound
// because it is `MaybeUninit` and `time_rebuild` builds it in place before
// any read. In D2 with the `Instrument`, as a voice's slot is.
#[unsafe(link_section = ".ram_d2.voices")]
static mut SLOT: MaybeUninit<EngineSlot> = MaybeUninit::uninit();
// The STRESS LOAD rows' Sound and the UI's side of the knobs, as a load
// publishes them (ADR 0115). In D2, out of AXI's margin; NOLOAD, built in
// place by `stress` before any read.
#[unsafe(link_section = ".ram_d2.voices")]
static mut LOADING: MaybeUninit<(Tracked<Sound>, KnobPublisher)> = MaybeUninit::uninit();

/// `LOADING` built afresh, in place: out of line, so the publisher's 2 KB
/// is never on `stress`'s frame (the stack check's 8 KB a step).
#[inline(never)]
fn loading() -> &'static mut (Tracked<Sound>, KnobPublisher) {
    // SAFETY: the bench is the only user of `LOADING` (see `run`), one
    // `stress` at a time, and `write` builds it whole before any read.
    unsafe { &mut *addr_of_mut!(LOADING) }.write((
        Tracked::fresh(Sound::init(EngineType::Algo)),
        KnobPublisher::new(),
    ))
}

/// The STRESS LOAD rows: a Sound loaded every UI frame, preset browsing.
const LOAD_EVERY: u32 = 12;
static mut SYM: SymAlloc = SymAlloc::new();

/// Rebuilds and note-ons timed, each averaged.
const ROUNDS: u32 = 16;
/// Render-to-idle gives up after 10 s of blocks: a voice stuck on.
const IDLE_BLOCKS: u32 = 10 * SAMPLE_RATE / BLOCK_SIZE as u32;

struct Rig<'p> {
    cpu_hz: u32,
    inst_slot: &'static mut MaybeUninit<Instrument>,
    fx_slot: &'static mut MaybeUninit<FxBus>,
    shared_slot: &'static mut MaybeUninit<AudioShared>,
    perf: &'p Performance,
    scope: &'static mut ScopeWriter,
    dac: &'static mut DacBlocks,
}

// The rig's scope writer and DAC blocks: in AXI, as the engine keeps its
// own, not 3.5 KB on the bench's frames under `main`. Zeroed `.bss`;
// `rig_io` builds them before any read.
static mut RIG_IO: MaybeUninit<(ScopeWriter, DacBlocks)> = MaybeUninit::uninit();

/// `RIG_IO` built in place: out of line, so no temporary of it is ever on
/// the deep path's frames.
#[inline(never)]
fn rig_io(w: Writer<ScopeFrame>) -> (&'static mut ScopeWriter, &'static mut DacBlocks) {
    // SAFETY: the bench is the only user of `RIG_IO` (see `run`), which
    // calls this once, and `write` builds it whole before any read.
    let io = unsafe { &mut *addr_of_mut!(RIG_IO) };
    let (scope, dac) = io.write((ScopeWriter::new(w), DacBlocks::new()));
    (scope, dac)
}

// Not inlined, so its frame never adds to `main`'s.
#[inline(never)]
pub fn run(
    display: &mut Display,
    clocks: Clocks,
    (project, sources): (&mut Project, &mut AudioToken),
    rings: chimera_board::BenchRings<'_>,
    report: &mut Text,
) {
    #[cfg(feature = "bench-memory")]
    let proj_crc = time_project_crc(project);
    #[cfg(not(feature = "bench-memory"))]
    let proj_crc = 0;
    let rec = &mut Recorder { display, report };
    title(rec, clocks);
    // SAFETY: the bench runs once from `main`, before `engine::init` and
    // before any interrupt is unmasked, so it is the only user of the
    // engine's slots, the audio buffer's and its own statics; its references
    // are gone when it returns, before `take_audio` and `engine::init`.
    let (inst_slot, fx_slot, shared_slot, scope_w) = unsafe {
        let (i, f) = engine::slots();
        let (w, _unread) = (&mut *addr_of_mut!(SCOPE)).write(scope_buffer()).split();
        (i, f, chimera_board::shared::bench_audio(), w)
    };
    let (scope, dac) = rig_io(scope_w);
    #[cfg(feature = "bench-tape")]
    {
        tape_page(
            (&mut *inst_slot, &mut *fx_slot, &mut *shared_slot),
            (&mut *scope, sources),
            project,
            rec,
        );
        hold(clocks);
        stream_page(
            (&mut *inst_slot, &mut *fx_slot, &mut *shared_slot),
            (&mut *scope, sources),
            project,
            rings,
            rec,
        );
        hold(clocks);
    }
    #[cfg(not(feature = "bench-tape"))]
    let _ = (sources, rings);
    algo_pages(
        clocks,
        (inst_slot, fx_slot, shared_slot, scope, dac),
        project,
        rec,
        proj_crc,
    );
}

/// The areas a build may bench, each with its feature's state.
const AREAS: [(bool, &str); 6] = [
    (cfg!(feature = "bench-algo"), "ALGO"),
    (cfg!(feature = "bench-routing"), "ROUTING"),
    (cfg!(feature = "bench-memory"), "MEMORY"),
    (cfg!(feature = "bench-stress"), "STRESS"),
    (cfg!(feature = "bench-orbit"), "ORBIT"),
    (cfg!(feature = "bench-tape"), "TAPE"),
];

/// The build's areas, its silicon and clock: the report's first line.
fn title(rec: &mut Recorder<'_>, clocks: Clocks) {
    rec.clear();
    let mut line = FmtBuf::new();
    let _ = write!(line, "BENCH");
    for (_, area) in AREAS.iter().filter(|a| a.0) {
        let _ = write!(line, " {area}");
    }
    let _ = write!(
        line,
        " REV {} {} MHZ",
        clocks.rev.label(),
        clocks.cpu_hz / 1_000_000
    );
    rec.heading(line.as_str());
    rec.flush();
}

/// The ALGO, FX, ROUTING, MEMORY, STRESS and ORBIT pages this build has: out of `run`, so
/// its frame stays under the stack check's 8 KB with the TAPE page's scope
/// in it.
#[inline(never)]
fn algo_pages(
    clocks: Clocks,
    (inst_slot, fx_slot, shared_slot, scope, dac): (
        &'static mut MaybeUninit<Instrument>,
        &'static mut MaybeUninit<FxBus>,
        &'static mut MaybeUninit<AudioShared>,
        &'static mut ScopeWriter,
        &'static mut DacBlocks,
    ),
    project: &Project,
    rec: &mut Recorder<'_>,
    proj_crc: u32,
) {
    let mut rig = Rig {
        cpu_hz: clocks.cpu_hz,
        inst_slot,
        fx_slot,
        shared_slot,
        perf: project.perf(),
        scope,
        dac,
    };
    #[cfg(feature = "bench-algo")]
    {
        algo_page(&mut rig, rec, clocks);
        hold(clocks);
    }
    #[cfg(feature = "bench-routing")]
    routing_pages(&mut rig, rec, clocks);
    #[cfg(feature = "bench-memory")]
    memory_page(&mut rig, rec, clocks, proj_crc);
    #[cfg(feature = "bench-stress")]
    stress_pages(&mut rig, rec, clocks);
    #[cfg(feature = "bench-stress")]
    reverb_pages(&mut rig, rec, clocks);
    #[cfg(feature = "bench-orbit")]
    orbit_pages(&mut rig, rec, clocks);
    let _ = (&mut rig, rec, proj_crc);
}

/// The ALGO screen: each patch at 1..8 voices, the kernel and the FX bus.
#[cfg(feature = "bench-algo")]
fn algo_page(rig: &mut Rig<'_>, rec: &mut Recorder<'_>, clocks: Clocks) {
    let mut rows = [Counts::default(); ROWS];
    for (row, &(_, patch, low, step)) in rows.iter_mut().zip(&PATCHES) {
        for n in 1..=MAX_VOICES {
            let setup = |s: &mut AudioShared| *s.parts[0].params = black_box(patch());
            row.add(n, rig.time(&setup, &|_, _| {}, n, low, step));
        }
    }
    let fx: [u32; FX_ROWS] = core::array::from_fn(|i| rig.time_bus(FX[i].1));
    let kernel = time_kernel();
    show(rec, clocks, &rows, kernel, &fx);
    hold(clocks);
    let mut formant = [Counts::default(); FORMANT_ROWS];
    time_rows(rig, &FORMANT, &mut formant, (36, 10));
    show_rows(rec, "FORMANT", &FORMANT, &formant);
    hold(clocks);
    let mut va = [Counts::default(); VA_ROWS];
    for (row, &(_, patch, low, step)) in va.iter_mut().zip(&VA) {
        for n in 1..=MAX_VOICES {
            let setup = |s: &mut AudioShared| *s.parts[0].params = black_box(patch());
            row.add(n, rig.time(&setup, &|_, _| {}, n, low, step));
        }
    }
    rec.clear();
    let mut line = FmtBuf::new();
    rec.heading("VA");
    for (i, (&(label, ..), c)) in VA.iter().zip(&va).enumerate() {
        voice_row(rec, &mut line, 46 + i as i32 * ROW_H, label, c);
    }
    rec.flush();
}

/// Each of `rows` at 1..8 voices, notes `low` up by `step`.
fn time_rows(rig: &mut Rig<'_>, rows: &[RoutingRow], counts: &mut [Counts], (low, step): (u8, u8)) {
    for (row, &(_, part, each)) in counts.iter_mut().zip(rows) {
        for n in 1..=MAX_VOICES {
            let timed = rig.time(
                &|s| {
                    part(&mut s.parts[0]);
                    black_box(&s.parts[0]);
                },
                &|s, b| each(&mut s.parts[0], b),
                n,
                low,
                step,
            );
            row.add(n, timed);
        }
    }
}

/// The ROUTING screens, `ROUTING_PAGE` rows each.
#[cfg(feature = "bench-routing")]
fn routing_pages(rig: &mut Rig<'_>, rec: &mut Recorder<'_>, clocks: Clocks) {
    let mut routing = [Counts::default(); ROUTING_ROWS];
    time_rows(rig, &ROUTING, &mut routing, (36, 12));
    let pages = ROUTING
        .chunks(ROUTING_PAGE)
        .zip(routing.chunks(ROUTING_PAGE));
    let total = ROUTING_ROWS.div_ceil(ROUTING_PAGE);
    for (page, (labels, counts)) in pages.enumerate() {
        let mut heading = FmtBuf::new();
        let _ = write!(heading, "ROUTING {}/{total}", page + 1);
        show_rows(rec, heading.as_str(), labels, counts);
        hold(clocks);
    }
}

/// TAPE's synthetic sources (spec § 5): eight of a kind, xorshift noise
/// seeded apart, written straight into the pool's arena. A model's cycles
/// follow a source's frames, rate and shape, not what it holds.
#[derive(Clone, Copy)]
enum Tapes {
    /// 12 000 frames at 24 kHz, 18 KB: the LOOP and STRETCH rows.
    Long,
    /// Four transwave frames, top 512, at 48 kHz.
    Wave,
    /// 64 frames at 24 kHz, the fewest a source holds: a loop of all of it
    /// at SIZE 0; ONESHOT's 16 slices of 4 frames.
    Least,
    /// 1 024 frames at 8 kHz: room for the shortest legal loop, 256 frames
    /// at `inc` 16.
    Short,
}

/// Whether a block's notes go down again first: held, struck again, or
/// struck and released (TRON running out and let go in one block).
#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Hold,
    Again,
    AgainOff,
}

/// A TAPE row: its label, sources, the Sound's set-up over LOOP with TAPE
/// and AGE at 0.5, and what changes before each block. Notes 120–127
/// (`inc` at the clamp, ROOT 0 on 8 kHz sources).
type TapeRow = (
    &'static str,
    Tapes,
    fn(&mut ParamSnapshot),
    fn(&mut ParamSnapshot, u32) -> Hit,
);

const HOLD: fn(&mut ParamSnapshot, u32) -> Hit = |_, _| Hit::Hold;

/// TINY's cases, each in its own model's bill: SIZE 0 on the fewest
/// frames, then each in its shortest legal loop at XFADE ½.
const TAPE_ROWS: [TapeRow; 13] = [
    ("LOOP", Tapes::Long, |p| p.tape.xfade = 0.5, HOLD),
    (
        "STRETCH",
        Tapes::Long,
        |p| {
            (p.tape.machine, p.tape.size) = (TapeMachine::Stretch, 0.0);
            (p.tape.kind, p.tape.period) = (StretchType::Mono, 109.0);
        },
        HOLD,
    ),
    // C7, ±12 st every block: a level change every block, billed as at
    // STEAL GLIDE.
    (
        "WAVE C7",
        Tapes::Wave,
        |p| {
            p.tape.machine = TapeMachine::Wave;
            p.pitch.steal = Steal::Glide;
        },
        |p, b| {
            p.pitch.pitch = if b % 2 == 0 { 12.0 } else { -12.0 };
            Hit::Hold
        },
    ),
    (
        "LOOP 64",
        Tapes::Least,
        |p| tiny(p, TapeModel::Play, PlayMode::Loop),
        HOLD,
    ),
    (
        "REV 64",
        Tapes::Least,
        |p| {
            tiny(p, TapeModel::Play, PlayMode::Loop);
            (p.tape.start, p.tape.end) = (1.0, 0.0);
        },
        HOLD,
    ),
    (
        "PING 64",
        Tapes::Least,
        |p| tiny(p, TapeModel::Play, PlayMode::Ping),
        HOLD,
    ),
    (
        "SCRUB 64",
        Tapes::Least,
        |p| tiny(p, TapeModel::Scrub, PlayMode::Loop),
        HOLD,
    ),
    (
        "LOOP 256",
        Tapes::Short,
        |p| tiny(p, TapeModel::Play, PlayMode::Loop),
        HOLD,
    ),
    (
        "REV 256",
        Tapes::Short,
        |p| {
            tiny(p, TapeModel::Play, PlayMode::Loop);
            (p.tape.start, p.tape.end) = (1.0, 0.0);
        },
        HOLD,
    ),
    (
        "PING 256",
        Tapes::Short,
        |p| tiny(p, TapeModel::Play, PlayMode::Ping),
        square,
    ),
    (
        "SCRUB 256",
        Tapes::Short,
        |p| tiny(p, TapeModel::Scrub, PlayMode::Loop),
        square,
    ),
    (
        "1SHOT",
        Tapes::Least,
        |p| {
            tiny(p, TapeModel::Play, PlayMode::One);
            p.tape.slice = chimera_engine::dsp::tape_engine::SliceCode::new(17);
        },
        |_, _| Hit::Again,
    ),
    (
        "TRON",
        Tapes::Least,
        |p| tiny(p, TapeModel::Tron, PlayMode::Loop),
        |_, _| Hit::AgainOff,
    ),
];

/// SIZE 0, XFADE ½, ROOT 0: `inc` clamped on any source.
fn tiny(p: &mut ParamSnapshot, model: TapeModel, mode: PlayMode) {
    p.tape.machine = TapeMachine::of(model, mode);
    (p.tape.size, p.tape.xfade, p.tape.root) = (0.0, 0.5, 0.0);
}

/// POSITION a square wave, flipping every block: the bounds jump, and
/// PING-PONG clamps.
fn square(p: &mut ParamSnapshot, b: u32) -> Hit {
    p.tape.position = (b % 2) as f32;
    Hit::Hold
}

/// The TAPE page's slots: the engine's, as `Rig`'s, and
/// the audio's source reader, which answers the pool's gate.
struct TapeRig<'t> {
    inst_slot: &'t mut MaybeUninit<Instrument>,
    fx_slot: &'t mut MaybeUninit<FxBus>,
    shared_slot: &'t mut MaybeUninit<AudioShared>,
    scope: &'t mut ScopeWriter,
    dac: DacBlocks,
    sources: &'t mut AudioToken,
}

/// The engine's slots and the bench's `AudioShared`, lent.
type Slots<'t> = (
    &'t mut MaybeUninit<Instrument>,
    &'t mut MaybeUninit<FxBus>,
    &'t mut MaybeUninit<AudioShared>,
);

/// Cold runs a row takes, its worst block kept.
const COLD_RUNS: u32 = 4;

impl TapeRig<'_> {
    /// The worst block of `COLD_RUNS` runs of `TIMED_BLOCKS`, cycles a
    /// sample, `voices` notes of `row` down, each on its own source of
    /// `ids`. With `cold` the D-cache is cleaned and invalidated before
    /// every timed block, so a block starts with nothing cached, as one
    /// after the UI's would.
    #[inline(never)]
    fn time_cold(
        &mut self,
        perf: &Performance,
        &(_, _, setup, each): &TapeRow,
        ids: &[SourceId; MAX_VOICES],
        (voices, cold): (usize, bool),
    ) -> u32 {
        // SAFETY: the bench runs alone, before interrupts are unmasked;
        // `main`'s SCB and CPUID aren't in use until it returns, and these
        // only clean and invalidate the D-cache.
        let mut cp = unsafe { cortex_m::Peripherals::steal() };
        let mut worst = 0u32;
        for _ in 0..COLD_RUNS {
            let budget = SampleBudget::for_cpu(u32::MAX);
            let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
            let fx = FxBus::init_in_place(self.fx_slot);
            let shared = AudioShared::init_in_place(self.shared_slot, perf);
            let p = &mut shared.parts[0];
            (*p.params, *p.mod_state) = (snapshot(EngineType::Tape), ModState::new());
            (p.params.tape.tape, p.params.tape.age) = (0.5, 0.5);
            setup(&mut p.params);
            hit(self.sources, inst, shared, (ids, voices), Hit::Again);
            for b in 0..WARM_BLOCKS + TIMED_BLOCKS {
                let h = each(&mut shared.parts[0].params, b);
                hit(self.sources, inst, shared, (ids, voices), h);
                if cold {
                    cp.SCB.clean_invalidate_dcache(&mut cp.CPUID);
                }
                let start = DWT::cycle_count();
                let out = (&mut *fx, &mut self.dac, &mut *self.scope);
                inst.run_block(self.sources, out, shared, |_, _| {});
                let c = DWT::cycle_count().wrapping_sub(start);
                if b >= WARM_BLOCKS {
                    worst = worst.max(c);
                }
            }
        }
        // The row's notes die with its `Instrument`: nothing reads now.
        self.sources.store_sounding(0);
        worst / BLOCK_SIZE as u32
    }
}

/// Voice `v` struck at note 120 + `v` on `ids[v]` (`Hit::Again`), and let
/// go too (`AgainOff`).
fn hit(
    sources: &mut AudioToken,
    inst: &mut Instrument,
    shared: &mut AudioShared,
    (ids, voices): (&[SourceId; MAX_VOICES], usize),
    h: Hit,
) {
    if h == Hit::Hold {
        return;
    }
    let view = sources.begin_block();
    for (v, &id) in ids.iter().enumerate().take(voices) {
        shared.parts[0].source = Some(id);
        let note = MidiNote::new(120 + v as u8).unwrap_or(MidiNote::A4);
        let kinds = [NoteKind::On(Velocity::DEFAULT), NoteKind::Off];
        for kind in kinds
            .into_iter()
            .take(if h == Hit::AgainOff { 2 } else { 1 })
        {
            let ev = NoteEvent {
                channel: MidiChannel::clamped(0),
                note,
                kind,
            };
            inst.handle_with(ev, shared, &view);
        }
    }
}

/// Eight sources of `t`; `None` if the arena can't take them (a booted
/// project's own sources and Sounds in it: boot NEW).
fn tape_sources(project: &mut Project, t: Tapes) -> Option<[SourceId; MAX_VOICES]> {
    let (shape, count, frames, rate) = match t {
        Tapes::Long => (Shape::Plain, 1, 12_000, 24_000),
        Tapes::Wave => (Shape::Transwave, 4, 4 * wave::frame_len(512), 48_000),
        Tapes::Least => (Shape::Plain, 1, MIN_SOURCE_FRAMES, 24_000),
        Tapes::Short => (Shape::Plain, 1, 1_024, 8_000),
    };
    let len = DESC_LEN + 3 * frames.div_ceil(2) as usize;
    let desc = Descriptor::whole(shape, count, rate)?;
    let mut ids = [None; MAX_VOICES];
    for (i, id) in ids.iter_mut().enumerate() {
        let crc = 0xBE7C_0000 | (t as u32) << 8 | i as u32;
        let r = SourceRef { crc, frames, rate };
        *id = project.source_write(r, len, |b| {
            b[..DESC_LEN].copy_from_slice(&desc.to_bytes());
            let mut x = crc;
            for y in &mut b[DESC_LEN..] {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                *y = x as u8;
            }
        });
    }
    ids.iter()
        .all(Option::is_some)
        .then(|| ids.map(|id| id.unwrap_or(SourceId::ALL[0])))
}

/// Every source the bench wrote out of the pool and the arena again:
/// purged, and passed with the gate answered between looks (a block
/// begun, nothing sounding), till every hole closes and every slot drains.
fn free_sources(project: &mut Project, sources: &mut AudioToken) {
    project.purge_unused_sources(0);
    let mut answer = || {
        let _ = sources.begin_block();
        true
    };
    while project.sources_pass_with(&mut answer) {}
    answer();
    answer();
    project.sources_pass_with(&mut answer);
}

/// The TAPE rows (spec § 5), each on its own sources, freed before the
/// next: `/VOICE` is the worst cold block with eight voices less with
/// none, over 8; LOOP's warm reading beside it shows the cold one is
/// cold; `8 VOICES` is LOOP's whole render, the bus in. Each line ends in
/// its bill, the chain's 30 in.
#[inline(never)]
fn tape_page(
    (inst_slot, fx_slot, shared_slot): Slots<'_>,
    (scope, sources): (&mut ScopeWriter, &mut AudioToken),
    project: &mut Project,
    rec: &mut Recorder<'_>,
) {
    let rig = &mut TapeRig {
        inst_slot,
        fx_slot,
        shared_slot,
        scope,
        dac: DacBlocks::new(),
        sources,
    };
    rec.clear();
    rec.heading("TAPE COLD /VOICE");
    let mut line = FmtBuf::new();
    for (i, row) in TAPE_ROWS.iter().enumerate() {
        line.clear();
        let mut bill = snapshot(EngineType::Tape);
        (bill.tape.tape, bill.tape.age) = (0.5, 0.5);
        (row.2)(&mut bill);
        let bill = Voice::CHAIN_COST.0 + EngineSlot::cost(&bill, &ModState::new()).0;
        match tape_sources(project, row.1) {
            Some(ids) => {
                let perf = project.perf();
                let none = rig.time_cold(perf, row, &ids, (0, true));
                let all = rig.time_cold(perf, row, &ids, (MAX_VOICES, true));
                let per = all.saturating_sub(none) / MAX_VOICES as u32;
                let _ = write!(line, "{} {per}/{bill}", row.0);
                if i == 0 {
                    let warm = rig.time_cold(perf, row, &ids, (MAX_VOICES, false));
                    let warm0 = rig.time_cold(perf, row, &ids, (0, false));
                    let warm = warm.saturating_sub(warm0) / MAX_VOICES as u32;
                    let _ = write!(line, " WARM {warm} 8V {all}");
                }
            }
            None => {
                let _ = write!(line, "{} FULL", row.0);
            }
        }
        free_sources(project, rig.sources);
        rec.line(Face::Value, line.as_str(), (4, 46 + i as i32 * 18));
    }
    rec.flush();
}

/// TAPE STREAM's file: `Tapes::Long`'s frames and rate, so its RAM twin is
/// the LOOP row's sources.
const STREAM_FRAMES: u32 = 12_000;
const STREAM_RATE: u16 = 24_000;
/// ROOT C4; the eight notes C4 − 7 to 0 st, `inc` 0.33 to 0.50: with the
/// bend's 2 st and wow, 310 KB/s billed, the most eight distinct notes
/// admit under `STREAM_BUDGET_BPS` (316 KB/s), a semitone higher being 329
/// KB/s. A held streamed voice that fast spans a chunk edge every 80
/// blocks or so, never every block: HIT strikes every block instead, each
/// a new start on its head and its ring asked again. (COST_STREAM's +100
/// was measured at C4 − 4 to + 3, under the old 466 KB/s placeholder.)
const STREAM_LOW: u8 = 53;

/// A TAPE STREAM row: its label, the Sound over PLAY with TAPE and AGE at
/// 0.5, and what changes before each block.
type StreamRow = (
    &'static str,
    fn(&mut ParamSnapshot),
    fn(&mut ParamSnapshot, u32) -> Hit,
);

const STREAM_ROWS: [StreamRow; 2] = [
    // The whole file looped, XFADE ½: the ring read through, each pass
    // onto the loop head.
    (
        "LOOP",
        |p| (p.tape.machine, p.tape.size, p.tape.xfade) = (TapeMachine::Loop, 1.0, 0.5),
        HOLD,
    ),
    (
        "HIT",
        |p| p.tape.machine = TapeMachine::One,
        |_, _| Hit::Again,
    ),
];

/// The Sound a row sets: PLAY at ROOT C4, TAPE and AGE at 0.5.
fn stream_sound(row: &StreamRow) -> ParamSnapshot {
    let mut p = snapshot(EngineType::Tape);
    (p.tape.tape, p.tape.age) = (0.5, 0.5);
    (p.tape.machine, p.tape.root) = (TapeMachine::Loop, 6_000.0);
    (row.1)(&mut p);
    p
}

/// Eight streamed rows of a `STREAM_FRAMES` file for `p`: each its heads
/// (`heads_for`, as a load stages them) of seeded noise; `None` if the
/// arena can't take them.
fn stream_sources(project: &mut Project, p: &ParamSnapshot) -> Option<[SourceId; MAX_VOICES]> {
    use chimera_engine::dsp::tape_engine::read::{streamed_held_len, write_head_table};
    let mut ids = [None; MAX_VOICES];
    for (i, id) in ids.iter_mut().enumerate() {
        let crc = 0x57E4_0000 | i as u32;
        let r = SourceRef {
            crc,
            frames: STREAM_FRAMES,
            rate: STREAM_RATE,
        };
        let heads = chimera_engine::stream::stage::heads_for(r, &p.tape).ok()?;
        let rate = SourceRate::new(STREAM_RATE)?;
        let reach = Frames::new(chimera_engine::stream::admit::head_frames(rate));
        *id = project.source_write(r, heads.held_len(), |b| {
            let at = write_head_table(rate, reach, heads.as_slice(), b).unwrap_or(b.len());
            debug_assert_eq!(b.len(), streamed_held_len(heads.as_slice()));
            noise(crc, &mut b[at..]);
        });
    }
    ids.iter()
        .all(Option::is_some)
        .then(|| ids.map(|id| id.unwrap_or(SourceId::ALL[0])))
}

/// `b` filled with xorshift noise from `seed`.
fn noise(seed: u32, b: &mut [u8]) {
    let mut x = seed | 1;
    for y in b {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *y = x as u8;
    }
}

/// Every Wanted slot read at once, as an on-time card: noise in, the
/// guard, Ready. Outside the timing.
fn serve_all(card: &mut CardSide<'_>) {
    for v in VoiceIdx::ALL {
        for s in SlotIdx::ALL {
            let Some(c) = card.claim(v, s) else {
                continue;
            };
            if let Some(b) = card.fill_buf(&c) {
                noise(0xC4D0 ^ c.chunk().get(), b);
            }
            card.write_guard(&c);
            card.fill_done(c);
        }
    }
}

/// The rings and the card's side of the voices' ports, a row's.
struct StreamRig<'s, 'r> {
    audio: &'s mut AudioSide<'r>,
    card: &'s mut CardSide<'r>,
    bus: CardBus,
    stats: StreamStats,
}

impl StreamRig<'_, '_> {
    /// `f` with the block's ports at block `b`: a streamed source is one
    /// of `streamed`.
    fn ports<R>(
        &mut self,
        streamed: &[SourceId],
        b: u32,
        f: impl FnOnce(&mut Streams<'_, '_>) -> R,
    ) -> R {
        let frames = |id: SourceId| {
            streamed
                .contains(&id)
                .then_some(chimera_engine::stream::units::Frames::new(STREAM_FRAMES))
        };
        let mut ports = Streams {
            audio: self.audio,
            card: StreamCard {
                frames: &frames,
                now: AudioBlocks::new(b),
                bus: &self.bus,
                stats: &self.stats,
                pend: || {},
                loud: core::cell::Cell::new(false),
            },
        };
        f(&mut ports)
    }
}

impl TapeRig<'_> {
    /// `time_cold`'s worst block, its voices through the rings' ports: the
    /// eight notes of `row` on `ids`, every Wanted slot served between
    /// blocks, the D-cache cleaned and invalidated before each timed one.
    /// Streamed when `streamed`, else `ids` are RAM sources. Ends with every
    /// ring emptied (`drop_ring`). With the voices streaming at the end.
    #[inline(never)]
    fn time_streamed(
        &mut self,
        perf: &Performance,
        row: &StreamRow,
        ids: &[SourceId; MAX_VOICES],
        (st, streamed): (&mut StreamRig<'_, '_>, bool),
    ) -> (u32, u8) {
        // SAFETY: as `time_cold`'s: the bench runs alone, before interrupts.
        let mut cp = unsafe { cortex_m::Peripherals::steal() };
        let on: &[SourceId] = if streamed { ids } else { &[] };
        let (mut worst, mut streaming) = (0u32, 0);
        for _ in 0..COLD_RUNS {
            let budget = SampleBudget::for_cpu(u32::MAX);
            let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
            let fx = FxBus::init_in_place(self.fx_slot);
            let shared = AudioShared::init_in_place(self.shared_slot, perf);
            let p = &mut shared.parts[0];
            (*p.params, *p.mod_state) = (stream_sound(row), ModState::new());
            let mut b = 0;
            strike(self.sources, inst, shared, (st, on, b), ids);
            for k in 0..WARM_BLOCKS + TIMED_BLOCKS {
                if (row.2)(&mut shared.parts[0].params, k) == Hit::Again {
                    strike(self.sources, inst, shared, (st, on, b), ids);
                }
                serve_all(st.card);
                cp.SCB.clean_invalidate_dcache(&mut cp.CPUID);
                let out = (&mut *fx, &mut self.dac, &mut *self.scope);
                let c = st.ports(on, b, |ports| {
                    let start = DWT::cycle_count();
                    inst.run_block_ported(self.sources, out, shared, ports, |_, _, _| {});
                    DWT::cycle_count().wrapping_sub(start)
                });
                if k >= WARM_BLOCKS {
                    worst = worst.max(c);
                }
                b += 1;
            }
            streaming = inst.streaming().map_or(0, |n| n.get());
            // Every voice let go and freed: each ring dropped as it frees.
            let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
            let out = (&mut *fx, &mut self.dac, &mut *self.scope);
            st.ports(on, b, |ports| {
                inst.run_block_ported(self.sources, out, shared, ports, |_, _, _| {})
            });
        }
        self.sources.store_sounding(0);
        (worst / BLOCK_SIZE as u32, streaming)
    }
}

/// Voice `v` struck at `STREAM_LOW + v` on `ids[v]`, through the ports:
/// admitted and started as a live note is.
fn strike(
    sources: &mut AudioToken,
    inst: &mut Instrument,
    shared: &mut AudioShared,
    (st, on, b): (&mut StreamRig<'_, '_>, &[SourceId], u32),
    ids: &[SourceId; MAX_VOICES],
) {
    st.ports(on, b, |ports| {
        let view = sources.begin_block();
        for (v, &id) in ids.iter().enumerate() {
            shared.parts[0].source = Some(id);
            let ev = NoteEvent {
                channel: MidiChannel::clamped(0),
                note: MidiNote::new(STREAM_LOW + v as u8).unwrap_or(MidiNote::A4),
                kind: NoteKind::On(Velocity::DEFAULT),
            };
            inst.handle_streamed(ev, shared, NoteOrigin::Live, &view, ports);
        }
    });
}

/// TAPE STREAM (slice 1 § 10.4, #596): each row's eight voices streamed
/// less the same eight on RAM sources (the LOOP row's), cold, over 8:
/// what streaming adds a voice, `refresh_bills` and the ports in:
/// `COST_STREAM`'s measure. `None` rings: not lent, nothing timed.
#[inline(never)]
fn stream_page(
    (inst_slot, fx_slot, shared_slot): Slots<'_>,
    (scope, sources): (&mut ScopeWriter, &mut AudioToken),
    project: &mut Project,
    rings: chimera_board::BenchRings<'_>,
    rec: &mut Recorder<'_>,
) {
    let rig = &mut TapeRig {
        inst_slot,
        fx_slot,
        shared_slot,
        scope,
        dac: DacBlocks::new(),
        sources,
    };
    rec.clear();
    rec.heading("TAPE STREAM /VOICE");
    let Some((audio, card)) = rings else {
        rec.line(Face::Value, "NO RINGS", (4, 46));
        rec.flush();
        return;
    };
    let st = &mut StreamRig {
        audio,
        card,
        bus: CardBus::new(),
        stats: StreamStats::new(),
    };
    let mut line = FmtBuf::new();
    for (i, row) in STREAM_ROWS.iter().enumerate() {
        line.clear();
        let p = stream_sound(row);
        let ram = match tape_sources(project, Tapes::Long) {
            Some(ids) => {
                let (c, _) = rig.time_streamed(project.perf(), row, &ids, (st, false));
                free_sources(project, rig.sources);
                Some(c)
            }
            None => None,
        };
        let streamed = match stream_sources(project, &p) {
            Some(ids) => {
                let c = rig.time_streamed(project.perf(), row, &ids, (st, true));
                free_sources(project, rig.sources);
                Some(c)
            }
            None => None,
        };
        match (streamed, ram) {
            (Some((s, n)), Some(r)) => {
                let per = s.saturating_sub(r) / MAX_VOICES as u32;
                // Within `FmtBuf`'s 32: +PER over the 8 voices' STREAMED/RAM
                // cycles, S streams, U underruns.
                let _ = write!(
                    line,
                    "{} +{per} {s}/{r} S{n} U{}",
                    row.0,
                    st.stats.underruns.load(Ordering::Relaxed)
                );
            }
            _ => {
                let _ = write!(line, "{} FULL", row.0);
            }
        }
        rec.line(Face::Value, line.as_str(), (4, 46 + i as i32 * 18));
    }
    rec.flush();
}

/// The MEMORY screen: sizes, a rebuild, MODAL's note-ons, the project
/// CRC. Out of `run`, so its frame stays under the stack check's 8 KB.
#[cfg(feature = "bench-memory")]
#[inline(never)]
fn memory_page(rig: &mut Rig<'_>, rec: &mut Recorder<'_>, clocks: Clocks, proj_crc: u32) {
    let rebuild = time_rebuild();
    // A4 after A4, the default Sound: the lines clear a loop each.
    let sym = |p: &mut PartAudio| modal(p, ResonatorMode::Sympathetic);
    let note_on = rig.time_note_on(MidiNote::A4, sym, (false, slots_free));
    // The lowest note after itself: every ring whole, the worst case.
    let lowest = MidiNote::new(0).unwrap_or(MidiNote::A4);
    let lowest = rig.time_note_on(lowest, short_sym, (false, slots_free));
    // G1, the longest line the passes walk: its first block at COLOR 0
    // less at the default's one pass, the six more COLOR 0 takes.
    let g1 = MidiNote::new(31).unwrap_or(MidiNote::A4);
    let dark = rig.time_note_on(g1, dark_pluck, (true, silent));
    let light = rig.time_note_on(g1, short_string, (true, silent));
    show_memory(rec, (rebuild, proj_crc), note_on, (lowest, dark, light));
    hold(clocks);
}

/// The STRESS screen and its runs' phases: NEW's Parts with every key
/// down, ORBIT playing with `orbit`. Out of `run`, so its frame stays
/// under the stack check's 8 KB.
#[cfg(feature = "bench-stress")]
#[inline(never)]
fn stress_pages(rig: &mut Rig<'_>, rec: &mut Recorder<'_>, clocks: Clocks) {
    chimera_engine::probe::set_clock(DWT::cycle_count);
    let ((s1, sp1), (s2, sp2)) = (
        rig.stress(true, worst_reverb, &[], false),
        rig.stress(false, worst_reverb, &[], false),
    );
    // Loads published as the UI does, every frame: into Part 6, which
    // plays its six keys, then into Parts 1-5 at once (ORBIT's).
    let (l1, _) = rig.stress(true, worst_reverb, &[MAX_PARTS - 1], false);
    let (l5, _) = rig.stress(true, worst_reverb, &[0, 1, 2, 3, 4], false);
    // A knob turned on every Part every block, as moves (ADR 0116).
    let (mv, _) = rig.stress(true, worst_reverb, &[], true);
    let budget = BlockBudget::for_cpu(clocks.cpu_hz).budget_cycles();
    show_stress(rec, [s1, s2, l1, l5, mv], budget);
    hold(clocks);
    show_phases(rec, &[("ST1", sp1), ("ST2", sp2)]);
    hold(clocks);
}

/// The REVERB screen: each TYPE's worst block, and its STRESS and the
/// TYPE switch's against the budget. Out of `run`, so its frame stays
/// under the stack check's 8 KB.
#[cfg(feature = "bench-stress")]
#[inline(never)]
fn reverb_pages(rig: &mut Rig<'_>, rec: &mut Recorder<'_>, clocks: Clocks) {
    chimera_engine::probe::set_clock(DWT::cycle_count);
    let mix = rig.time_bus(FX[0].1);
    let rev: [u32; REV_ROWS] = core::array::from_fn(|i| rig.time_bus(REV[i].1));
    let bus_v = rig.time_bus_with_voices(worst_bus);
    // Each TYPE's STRESS, keys before ORBIT starts, then after.
    let mut st = |rev| {
        [
            rig.stress(true, rev, &[], false).0.worst,
            rig.stress(false, rev, &[], false).0.worst,
        ]
    };
    let stress = (
        [
            st(worst_reverb),
            st(REV[2].1),
            st(REV[4].1),
            st(REV[5].1),
            st(REV[6].1),
        ],
        BlockBudget::for_cpu(clocks.cpu_hz).budget_cycles(),
    );
    let switch = rig.switch_max();
    show_reverb(rec, &rev, mix, bus_v, stress, switch);
    hold(clocks);
}

/// The ORBIT screen and its block's phases. Out of `run`, so its frame
/// stays under the stack check's 8 KB.
#[cfg(feature = "bench-orbit")]
#[inline(never)]
fn orbit_pages(rig: &mut Rig<'_>, rec: &mut Recorder<'_>, clocks: Clocks) {
    chimera_engine::probe::set_clock(DWT::cycle_count);
    let ((mean, worst), [ow, ow1, omean]) = rig.time_orbit();
    rec.clear();
    rec.heading("ORBIT");
    let mut line = FmtBuf::new();
    // A block's ORBIT, mean and worst (a step on every ring, MUTATE at
    // the loop), against `SampleBudget`'s cycles per block.
    let _ = write!(line, "ORBIT {mean}/{worst} CYC/BLK");
    rec.line(Face::Value, line.as_str(), (4, 46));
    rec.flush();
    hold(clocks);
    show_phases(rec, &[("WORST", ow), ("WORST>0", ow1), ("MEAN", omean)]);
    hold(clocks);
}

/// Cycles per `project_crc` over `project` with every slot filled, the
/// test fixture `full()`'s shape: its free slots take copies (as `full()`
/// fills them) and are cleared after, so its content and status are as
/// before. A slot a Part names is left as it is.
#[inline(never)]
fn time_project_crc(project: &mut Project) -> u32 {
    let mut filled = 0u128;
    for s in SlotId::ALL {
        if project.pool().get(s).is_some() || !project.users(s).is_empty() {
            continue;
        }
        let i = s.index();
        let sound = match i % 3 {
            0 => None,
            _ => factory_sound(i % FACTORY_LEN),
        };
        let sound = sound.unwrap_or_else(|| Sound::init(EngineType::Modal));
        project
            .pool_store(s, &sound)
            .expect("NEW and sparse copies fit the arena");
        filled |= 1 << i;
    }
    let mut cycles = 0u32;
    for _ in 0..ROUNDS {
        let start = DWT::cycle_count();
        black_box(project_crc(black_box(&*project)));
        cycles = cycles.wrapping_add(DWT::cycle_count().wrapping_sub(start));
    }
    for s in SlotId::ALL {
        if filled & 1 << s.index() != 0 {
            project
                .pool_clear(s)
                .expect("no users: checked when filled");
        }
    }
    cycles / ROUNDS
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
        clocks::delay_us(clocks.cpu_hz, 1_000_000);
    }
}

impl Rig<'_> {
    /// Voice `v` plays note `low + step * v`; `each` runs before every
    /// block, its time counted (a few cycles a block). Returns the cycles
    /// per sample and the voices its per-voice figure spans (`Counts`).
    #[inline(never)]
    fn time(
        &mut self,
        setup: &dyn Fn(&mut AudioShared),
        each: &dyn Fn(&mut AudioShared, u32),
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
            inst.handle_with(ev, shared, &NO_POOL);
        }
        for b in 0..WARM_BLOCKS {
            each(shared, b);
            inst.render_with(fx, self.dac, shared, self.scope, &NO_POOL);
        }
        let start = DWT::cycle_count();
        for b in 0..TIMED_BLOCKS {
            each(shared, WARM_BLOCKS + b);
            inst.render_with(fx, self.dac, shared, self.scope, &NO_POOL);
        }
        let cycles = DWT::cycle_count().wrapping_sub(start);
        let spans = match inst.ringing() {
            0 => inst.sounding(),
            ringing => ringing,
        };
        (cycles / (TIMED_BLOCKS * BLOCK_SIZE as u32), spans)
    }

    /// Cycles of `Instrument::handle` for one note-on of `note`, Part 0
    /// set by `part`, on an idle voice, and with `first_block` its first
    /// render, where a pluck is shaped. Each round releases the note and
    /// renders until `idle`; one untimed round warms first, so the lines
    /// hold `note`'s last round.
    #[inline(never)]
    fn time_note_on(
        &mut self,
        note: MidiNote,
        part: fn(&mut PartAudio),
        (first_block, idle): (bool, Idle),
    ) -> u32 {
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
            inst.handle_with(
                black_box(ev(NoteKind::On(Velocity::DEFAULT))),
                shared,
                &NO_POOL,
            );
            if first_block {
                inst.render_with(fx, self.dac, shared, self.scope, &NO_POOL);
            }
            if round > 0 {
                cycles = cycles.wrapping_add(DWT::cycle_count().wrapping_sub(start));
            }
            inst.handle_with(ev(NoteKind::Off), shared, &NO_POOL);
            for _ in 0..IDLE_BLOCKS {
                inst.render_with(fx, self.dac, shared, self.scope, &NO_POOL);
                if idle(inst) {
                    break;
                }
            }
        }
        cycles / ROUNDS
    }

    /// Cycles of a half's ORBIT, `OrbitRunner::block` and the view built
    /// and copied out as `render_half` publishes it, its mean and its worst
    /// over `ORBIT_LOOPS` loops: five rings of 32 steps and 31 hits (a rest
    /// for MUTATE to move), MUTATE and GATE 100 % (each step's off lands
    /// with its on), at the top BPM so the loops come quickly, into a quiet
    /// `Instrument` that renders nothing. The clock's division and the
    /// notes' `handle` are inside the timing.
    #[cfg(feature = "orbit")]
    #[inline(never)]
    fn time_orbit(&mut self) -> ((u32, u32), [Phases; 3]) {
        use chimera_engine::block::Block;
        use chimera_engine::orbit::clock::Bpm;
        use chimera_engine::orbit::scene::SceneRing;
        const ORBIT_LOOPS: u32 = 2;
        let budget = SampleBudget::for_cpu(u32::MAX);
        let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
        let shared = AudioShared::init_in_place(self.shared_slot, self.perf);
        let o = &mut shared.orbit;
        for r in o.settings.rings.iter_mut() {
            r.set(SceneRing::STEPS, 31.0);
            r.set(SceneRing::HITS, 31.0);
            r.set(SceneRing::MUTATE, 100.0);
            r.set(SceneRing::GATE, 100.0);
            r.set(SceneRing::MUTE, 0.0);
        }
        o.bpm = Bpm::clamped(Bpm::MAX);
        o.push(chimera_engine::orbit::channel::Cmd::TogglePlay);
        let mut runner = OrbitRunner::new(SAMPLE_RATE_NZ);
        let mut view = runner.view();
        let (mut total, mut worst, mut blocks) = (0u32, 0u32, 0u32);
        let (mut w, mut w1, mut sum) = (Phases::default(), Phases::default(), Phases::default());
        while view.loops < ORBIT_LOOPS && blocks < IDLE_BLOCKS {
            let start = DWT::cycle_count();
            let ev = black_box(runner.block_with(inst, shared, &NO_POOL));
            runner.fill_view(black_box(&mut view));
            chimera_engine::probe::stamp(10);
            let c = DWT::cycle_count().wrapping_sub(start);
            let p = Phases::read(blocks, c, ev.boundary());
            sum.add(&p);
            if c > w.total {
                w = p;
            }
            if blocks > 0 && c > w1.total {
                w1 = p;
            }
            (total, worst, blocks) = (total.wrapping_add(c), worst.max(c), blocks + 1);
        }
        sum.div(blocks.max(1));
        ((total / blocks.max(1), worst), [w, w1, sum])
    }

    /// Six Parts written with one noise block, sends 0.5, no voices:
    /// `mix_parts` alone, the output limiter included (ADR 0050). `each`
    /// sets the FX before every block.
    #[inline(never)]
    fn time_bus(&mut self, each: fn(&mut AudioShared, u32)) -> u32 {
        self.bus(each, false)
    }

    /// `time_bus` with six `a16_a17` voices on Part 1 rendered, untimed,
    /// by `Instrument::render` before every block: their cache traffic
    /// sits between the FX blocks, as in the running synth.
    #[inline(never)]
    fn time_bus_with_voices(&mut self, each: fn(&mut AudioShared, u32)) -> u32 {
        self.bus(each, true)
    }

    fn bus(&mut self, each: fn(&mut AudioShared, u32), voices: bool) -> u32 {
        let fx = FxBus::init_in_place(self.fx_slot);
        let shared = AudioShared::init_in_place(self.shared_slot, self.perf);
        // Every effect off but the row's: the boot project's FX would sit
        // in MIX and every row less it.
        shared.fx = FxParams::OFF;
        for (i, part) in shared.parts.iter_mut().enumerate() {
            part.params.out.sends = [0.5; FX_SENDS];
            // One Part to a channel: only Part 1 plays the notes.
            part.mix.channel = MidiChannel::clamped(i as u8);
        }
        let mut inst = voices.then(|| {
            // The allocator must not refuse what the bench wants to measure.
            let budget = SampleBudget::for_cpu(u32::MAX);
            let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
            *shared.parts[0].params = a16_a17();
            hold_notes(inst, shared, 0);
            inst
        });
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
        let mut pans = MixState::default();
        let scope = &mut *self.scope;
        // Its cycles: the voices' render before it is not counted.
        let mut block = |shared: &mut AudioShared, fx: &mut FxBus, dac: &mut DacBlocks, b: u32| {
            if let Some(inst) = inst.as_deref_mut() {
                inst.render_with(fx, dac, shared, scope, &GateView::empty());
            }
            let start = DWT::cycle_count();
            each(shared, b);
            black_box(mix_parts(
                black_box(&buses),
                &written,
                &mut sends,
                &mut pans,
                fx,
                shared,
                Tempo::DEFAULT,
                SAMPLE_RATE,
                dac,
            ));
            DWT::cycle_count().wrapping_sub(start)
        };
        for b in 0..WARM_BLOCKS + SWITCH_WARM {
            block(shared, fx, self.dac, b);
        }
        let mut cycles = 0u32;
        for b in 0..TIMED_BLOCKS {
            let b = WARM_BLOCKS + SWITCH_WARM + b;
            cycles = cycles.wrapping_add(block(shared, fx, self.dac, b));
        }
        cycles / (TIMED_BLOCKS * BLOCK_SIZE as u32)
    }

    /// The costliest whole callback block over `STRESS_BLOCKS` and the most
    /// voices sounding: six `a16_a17` keys on Part 6, every effect at its
    /// worst (the reverb as `rev` sets it: QUAD's, or GATE's, ROOM's,
    /// HALL's or BLOOM's for those STRESS readings), and (with `orbit`)
    /// ORBIT playing NEW's groove on Parts 1–5 with NEW's Sounds (ADR 0073).
    /// ORBIT starts at the first timed block, whose cycles come back on
    /// their own too (T0); the keys go down before the warm blocks if
    /// `keys_first`, else a block after ORBIT starts. The allocator keeps
    /// the unit's real budget, so it refuses what the unit would. Timed as
    /// `render_half` runs: ORBIT's block and view, the render and the three
    /// pairs' interleave; the note queues are empty. Every `LOAD_EVERY`
    /// timed blocks, each of `loads` takes its Sound again as a load,
    /// through the knobs' publisher (ADR 0115), before the block's timing
    /// starts, as the UI publishes.
    #[inline(never)]
    fn stress(
        &mut self,
        keys_first: bool,
        rev: fn(&mut AudioShared, u32),
        loads: &[usize],
        turn: bool,
    ) -> (Stress, Phases) {
        let budget = SampleBudget::for_cpu(self.cpu_hz);
        let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
        let fx = FxBus::init_in_place(self.fx_slot);
        let shared = AudioShared::init_in_place(self.shared_slot, self.perf);
        shared.fx = FxParams::OFF;
        let (sound, knobs) = loading();
        let mut count = [Loads::NONE; MAX_PARTS];
        publish_new(shared, sound, knobs, &mut count);
        if keys_first {
            hold_notes(inst, shared, MAX_PARTS - 1);
        }
        #[cfg(feature = "orbit")]
        let (mut runner, mut view) = {
            let runner = OrbitRunner::new(SAMPLE_RATE_NZ);
            let view = runner.view();
            (runner, view)
        };
        let mut out = [[DacSample::ZERO; BLOCK_SIZE * 2]; DacPair::ALL.len()];
        // A TYPE other than QUAD warms through the switch to it.
        let warm = WARM_BLOCKS + SWITCH_WARM;
        let (mut worst, mut most, mut orbit, mut t0) = (0u32, 0usize, 0u32, 0u32);
        // The knobs' ease (probe slot 9): the most in a block, and in the
        // costliest block.
        let (mut knob_max, mut knob_peak, mut billed) = (0u32, 0u32, 0u32);
        let knob_cycles = || chimera_engine::probe::ACC[9].load(Ordering::Relaxed);
        let mut ph = Phases::default();
        for b in 0..warm + STRESS_BLOCKS {
            #[cfg(feature = "orbit")]
            if b == warm {
                shared
                    .orbit
                    .push(chimera_engine::orbit::channel::Cmd::TogglePlay);
            }
            if b == warm + 1 && !keys_first {
                hold_notes(inst, shared, MAX_PARTS - 1);
            }
            if b >= warm && (b - warm).is_multiple_of(LOAD_EVERY) {
                for &p in loads {
                    let part = &shared.parts[p];
                    (sound.params, sound.mod_state) =
                        ((*part.params).clone(), (*part.mod_state).clone());
                    count[p] = count[p].next();
                    shared.publish_sound(p, sound, count[p], knobs);
                }
            }
            if turn && b >= warm {
                for (p, &count) in count.iter().enumerate() {
                    let part = &shared.parts[p];
                    (sound.params, sound.mod_state) =
                        ((*part.params).clone(), (*part.mod_state).clone());
                    sound.params.filter.resonance = [0.3, 0.6][b as usize % 2];
                    shared.publish_sound(p, sound, count, knobs);
                }
            }
            worst_bus(shared, b);
            rev(shared, b);
            let start = DWT::cycle_count();
            #[cfg(feature = "orbit")]
            let boundary = {
                let ev = black_box(runner.block_with(inst, shared, &NO_POOL));
                runner.fill_view(black_box(&mut view));
                chimera_engine::probe::stamp(10);
                ev.boundary()
            };
            #[cfg(not(feature = "orbit"))]
            let boundary = false;
            let ran = DWT::cycle_count().wrapping_sub(start);
            if b >= warm && ran > ph.total {
                ph = Phases::read(b, ran, boundary);
            }
            let k0 = knob_cycles();
            inst.render_with(fx, self.dac, shared, self.scope, &NO_POOL);
            let k = knob_cycles().wrapping_sub(k0);
            for (pair, o) in DacPair::ALL.into_iter().zip(out.iter_mut()) {
                interleave(self.dac.out(), pair, o);
            }
            let c = DWT::cycle_count().wrapping_sub(start);
            black_box(&out);
            if b == warm {
                t0 = c;
            }
            if b >= warm {
                knob_max = knob_max.max(k);
                if c > worst {
                    knob_peak = k;
                    // What the bill counts while this block runs: the
                    // voices, the bus, the reverb TYPE's extra, the knobs
                    // and ORBIT's own work.
                    #[cfg(feature = "orbit")]
                    let orbit = OrbitRunner::COST.0;
                    #[cfg(not(feature = "orbit"))]
                    let orbit = 0;
                    let rev = extra_cost(shared.fx.reverb.kind).0;
                    let reserved =
                        FxBus::COST.0 + rev + Knobs::RESERVE.0 + LfoClock::RESERVE.0 + orbit;
                    billed = (inst.allocator().sounding_cost().0 + reserved) * BLOCK_SIZE as u32;
                }
                worst = worst.max(c);
                most = most.max(inst.sounding());
                orbit = orbit.max(ran);
            }
        }
        let knobs = (knob_max, knob_peak);
        (
            Stress {
                worst,
                most,
                orbit,
                t0,
                knobs,
                billed,
            },
            ph,
        )
    }

    /// REV SWITCH MAX (reverb TYPEs spec § Bench rows): the costliest whole
    /// callback block over `SWITCH_BLOCKS`, and the block budget it must
    /// stay under: six `a16_a17` voices asked of Part 1 (the unit's real
    /// budget admits what the TYPE's bill leaves, none forced), every effect at its
    /// worst, the reverb's TYPE stepping QUAD → GATE → ROOM → HALL → BLOOM
    /// every `SWITCH_EVERY` blocks; TAPE too in the master-tape build,
    /// by `worst_bus`. The four CLEAR blocks,
    /// 11.6 KB zeroed each, and the voices' refill after them are what a
    /// mean would hide.
    #[inline(never)]
    fn switch_max(&mut self) -> (u32, u32) {
        let budget = SampleBudget::for_cpu(self.cpu_hz);
        let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
        let fx = FxBus::init_in_place(self.fx_slot);
        let shared = AudioShared::init_in_place(self.shared_slot, self.perf);
        shared.fx = FxParams::OFF;
        for (i, part) in shared.parts.iter_mut().enumerate() {
            part.params.out.sends = [0.5; FX_SENDS];
            part.mix.channel = MidiChannel::clamped(i as u8);
        }
        *shared.parts[0].params = a16_a17();
        hold_notes(inst, shared, 0);
        let mut out = [[DacSample::ZERO; BLOCK_SIZE * 2]; DacPair::ALL.len()];
        let mut worst = 0u32;
        for b in 0..WARM_BLOCKS + SWITCH_BLOCKS {
            worst_bus(shared, b);
            shared.fx.reverb.kind = [
                ReverbType::Quad,
                ReverbType::Gate,
                ReverbType::Room,
                ReverbType::Hall,
                ReverbType::Bloom,
            ][(b / SWITCH_EVERY) as usize % 5];
            let start = DWT::cycle_count();
            inst.render_with(fx, self.dac, shared, self.scope, &GateView::empty());
            for (pair, o) in DacPair::ALL.into_iter().zip(out.iter_mut()) {
                interleave(self.dac.out(), pair, o);
            }
            let c = DWT::cycle_count().wrapping_sub(start);
            black_box(&out);
            if b >= WARM_BLOCKS {
                worst = worst.max(c);
            }
        }
        (worst, BlockBudget::for_cpu(self.cpu_hz).budget_cycles())
    }
}

/// A STRESS run's figures: its costliest block, the most voices it
/// sounded, its costliest ORBIT block, the block ORBIT starts in, and the
/// knobs' ease (the most in a block, and in the costliest block).
#[derive(Clone, Copy)]
struct Stress {
    worst: u32,
    most: usize,
    orbit: u32,
    t0: u32,
    knobs: (u32, u32),
    /// What the bill counted at its costliest block, cycles.
    billed: u32,
}

/// One ORBIT block's phases from `chimera_engine::probe`'s stamps: 1 intake,
/// 2 transport and adopt, 3 clock, 4 offs due, 5 loop boundary (MUTATE),
/// 6 the rings' steps, 7 harmony, 8 the notes' `handle`, 9 resnap, 10 view.
/// `acc`: 0 recost, 1 pick, 2 note-on, 3 ons, 5 offs, 6 costliest event.
#[derive(Clone, Copy, Default)]
struct Phases {
    block: u32,
    total: u32,
    boundary: u32,
    d: [u32; 11],
    acc: [u32; chimera_engine::probe::ACCS],
}

impl Phases {
    fn read(block: u32, total: u32, boundary: bool) -> Self {
        use chimera_engine::probe::{ACC, AT, SEEN};
        use core::sync::atomic::Ordering::Relaxed;
        let seen = SEEN.load(Relaxed);
        let mut p = Phases {
            block,
            total,
            boundary: boundary as u32,
            ..Default::default()
        };
        let mut last = AT[0].load(Relaxed);
        for i in 1..p.d.len() {
            if seen & 1 << i != 0 {
                let t = AT[i].load(Relaxed);
                p.d[i] = t.wrapping_sub(last);
                last = t;
            }
        }
        p.acc = core::array::from_fn(|i| ACC[i].load(Relaxed));
        p
    }

    #[cfg(feature = "orbit")]
    fn add(&mut self, o: &Phases) {
        self.total += o.total;
        self.boundary += o.boundary;
        for (a, b) in self.d.iter_mut().zip(&o.d) {
            *a += b;
        }
        for (a, b) in self.acc.iter_mut().zip(&o.acc) {
            *a += b;
        }
    }

    /// The mean of `n` added; `block` is the count, `acc[6]` stays a sum.
    #[cfg(feature = "orbit")]
    fn div(&mut self, n: u32) {
        self.block = n;
        self.total /= n;
        for a in self.d.iter_mut().chain(self.acc.iter_mut()) {
            *a /= n;
        }
    }
}

fn show_phases(rec: &mut Recorder<'_>, rows: &[(&str, Phases)]) {
    rec.clear();
    rec.heading("ORBIT PHASES");
    let mut line = FmtBuf::new();
    let mut y = 30;
    for (tag, p) in rows {
        let d = &p.d;
        let a = &p.acc;
        let lines = [
            format_args!("{tag} #{} {} B{}", p.block, p.total, p.boundary),
            format_args!("IN{} TR{} CK{} PO{}", d[1], d[2], d[3], d[4]),
            format_args!("MU{} PL{} HA{}", d[5], d[6], d[7]),
            format_args!("SN{} RS{} VW{}", d[8], d[9], d[10]),
            format_args!("ON{} OF{} MX{}", a[3], a[5], a[6]),
            format_args!("RC{} PK{} NO{}", a[0], a[1], a[2]),
            format_args!(
                "C6{} SL{} TL{} PS{}",
                a[8],
                a[4],
                a[7],
                core::mem::size_of::<ParamSnapshot>()
            ),
        ];
        for args in lines {
            line.clear();
            let _ = line.write_fmt(args);
            rec.line(Face::Label(theme::INK2), line.as_str(), (4, y));
            y += 8;
        }
    }
    rec.flush();
}

// `time_kernel`'s plan, voices and blocks: in D2, as the audio's voices
// are, and off its frame (the stack check's 8 KB). NOLOAD, so garbage at
// boot: sound because each is `MaybeUninit` and `time_kernel` writes every
// element (`fill`) before any read.
#[unsafe(link_section = ".ram_d2.voices")]
static mut KERNEL_PLAN: MaybeUninit<EvalPlan> = MaybeUninit::uninit();
#[unsafe(link_section = ".ram_d2.voices")]
static mut KERNELS: MaybeUninit<[Kernel; MAX_VOICES]> = MaybeUninit::uninit();
#[unsafe(link_section = ".ram_d2.voices")]
static mut KERNEL_ENVS: MaybeUninit<[[OpEnv; OPS]; MAX_VOICES]> = MaybeUninit::uninit();
#[unsafe(link_section = ".ram_d2.voices")]
static mut KERNEL_BLOCKS: MaybeUninit<[KernelBlock<'static>; MAX_VOICES]> = MaybeUninit::uninit();

/// Every element of `*slot` from `f`, written in place, one at a time: the
/// array is never built on the caller's stack.
///
/// # Safety
///
/// `slot` is valid and aligned, and nothing else references it until the
/// returned reference is dropped.
unsafe fn fill<T, const N: usize>(
    slot: *mut MaybeUninit<[T; N]>,
    mut f: impl FnMut(usize) -> T,
) -> &'static mut [T; N] {
    let p = slot.cast::<T>();
    for i in 0..N {
        // SAFETY: `i < N`, inside the array `slot` holds; per the contract
        // no one else is looking.
        unsafe { p.add(i).write(f(i)) };
    }
    // SAFETY: every element was written above.
    unsafe { &mut *slot.cast::<[T; N]>() }
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
    // SAFETY: the bench is the only user of these statics, `time_kernel`
    // their only writer, one run at a time and before the audio starts;
    // each reference below is the only one, and dies with this call.
    let plan: &'static EvalPlan =
        unsafe { &mut *addr_of_mut!(KERNEL_PLAN) }.write(black_box(EvalPlan::build(
            black_box(&A14),
            black_box(0b11),
            black_box(&A22),
            black_box(0b1),
        )));
    let rates = EnvRates {
        ar: 31,
        d1r: 0,
        d1l: 15,
        d2r: 0,
        rr: 8,
        rs: 0,
    };
    // SAFETY: as `plan`'s.
    let kernels = unsafe { fill(addr_of_mut!(KERNELS), |_| Kernel::new()) };
    // SAFETY: as `plan`'s.
    let envs = unsafe {
        fill(addr_of_mut!(KERNEL_ENVS), |v| {
            let note = MidiNote::new(48 + 5 * v as u8).unwrap_or(MidiNote::A4);
            let mut env = [OpEnv::IDLE; OPS];
            for e in env.iter_mut() {
                e.note_on(EnvCoefs::new(rates, note, SAMPLE_RATE as f32));
            }
            env
        })
    };
    // SAFETY: as `plan`'s.
    let blocks = black_box(unsafe {
        fill(addr_of_mut!(KERNEL_BLOCKS), |v| KernelBlock {
            plan,
            ops: core::array::from_fn(|i| {
                let wave = WAVES[(i + v) % OPS];
                OpBlock {
                    inc: (i as u32 + 1) * (11_600_000 + 2_000_000 * v as u32),
                    gain_from: 0.9 * SAMPLE_SCALE,
                    gain_to: 0.8 * SAMPLE_SCALE,
                    feedback: FEEDBACK_CYCLES[7],
                    kind: KindBlock::Table {
                        lo: wave.table(v),
                        hi: wave.table(v + 1),
                        xfade_from: 0.4,
                        xfade_to: 0.6,
                    },
                }
            }),
            morph_from: 0.45,
            morph_to: 0.55,
            norm_from: 0.7,
            norm_to: 0.7,
        })
    });
    let mut out = [0.0f32; BLOCK_SIZE];
    // No tap wanted: the rows go untouched, as on a tap-free Sound.
    let mut taps = chimera_engine::taps::TapRows::new([false; 2]);
    let mut run = |kernels: &mut [Kernel; MAX_VOICES], envs: &mut [[OpEnv; OPS]; MAX_VOICES]| {
        for ((k, env), blk) in kernels.iter_mut().zip(envs.iter_mut()).zip(blocks.iter()) {
            k.render(black_box(blk), env, &mut out, &mut taps);
        }
    };
    for _ in 0..WARM_BLOCKS {
        run(kernels, envs);
    }
    let start = DWT::cycle_count();
    for _ in 0..TIMED_BLOCKS {
        run(kernels, envs);
    }
    let cycles = DWT::cycle_count().wrapping_sub(start);
    black_box(&out);
    cycles / (TIMED_BLOCKS * BLOCK_SIZE as u32 * MAX_VOICES as u32)
}

const ALL: u8 = 0b11_1111;

/// `alg` alone, the operators in mask `lit` at LEVEL 99 and the rest at 0,
/// every operator with `feedback`. Out of line: inlined, each row's
/// closure carried its own copy.
#[inline(never)]
fn algo(alg: AlgoId, lit: u8, feedback: u8) -> ParamSnapshot {
    let mut p = snapshot(EngineType::Algo);
    for i in 0..OPS {
        let level = if lit & (1 << i) != 0 { 99 } else { 0 };
        let op = &mut p.algo.ops[i];
        (op.coarse, op.level, op.feedback) = (COARSE[i], level, feedback);
    }
    p.algo = AlgoParams::new(alg, alg, 0, Kinds::tables(WAVES), p.algo.ops);
    p
}

/// ADR 0026's costliest pair, the bench's heaviest voice.
fn a16_a17() -> ParamSnapshot {
    algo_pair(AlgoId::A16, AlgoId::A17)
}

/// Six notes, C2 up in octaves, on Part `part`'s channel.
fn hold_notes(inst: &mut Instrument, shared: &AudioShared, part: usize) {
    for v in 0..6u8 {
        let ev = NoteEvent {
            channel: shared.parts[part].mix.channel,
            note: MidiNote::new(36 + 12 * v).unwrap_or(MidiNote::A4),
            kind: NoteKind::On(Velocity::DEFAULT),
        };
        inst.handle_with(ev, shared, &NO_POOL);
    }
}

/// `a` ∪ `b` at MORPH 64, six audible operators, all with feedback: A14 ∪
/// A22 is spec § Budget's worst case, A16 ∪ A17 ADR 0026's costliest (842).
#[inline(never)]
fn algo_pair(a: AlgoId, b: AlgoId) -> ParamSnapshot {
    let mut p = algo(a, ALL, 7);
    p.algo = AlgoParams::new(a, b, 64, p.algo.kinds(), p.algo.ops);
    p
}

const ROW_H: i32 = 25;
/// The ALGO screen's row pitch: ten rows, the kernel line and three FX
/// lines end at y 302, under 310 (`ROW_H` would push the FX lines off it).
const ALGO_ROW_H: i32 = 22;
const _: () = assert!(46 + ROWS as i32 * ALGO_ROW_H + 16 + 2 * 10 <= 310);
/// One count per voice across the screen: 29 px holds five digits at eight.
const CELL_W: i32 = (theme::SCREEN_W - 8) / MAX_VOICES as i32;

/// A bench line's font: a value in INK, or a label in its colour.
#[derive(Clone, Copy)]
enum Face {
    Value,
    Label(Rgb565),
}

/// The bench's screen and its report together: text can only be drawn by
/// recording it, so `bench` reads exactly what the screens showed.
struct Recorder<'a> {
    display: &'a mut Display,
    report: &'a mut Text,
}

impl Recorder<'_> {
    /// A new screen.
    fn clear(&mut self) {
        draw::fill_rect(
            self.display,
            0,
            0,
            theme::SCREEN_W,
            theme::SCREEN_H,
            theme::BG,
        );
    }

    /// The screen's title, recorded as the report's heading.
    fn heading(&mut self, text: &str) {
        self.report.heading(text);
        bench_screen::text(self.display, None, text, (4, 16));
    }

    fn line(&mut self, face: Face, text: &str, at: (i32, i32)) {
        let display = &mut *self.display;
        self.report.drawn(text, |t| match face {
            Face::Value => bench_screen::text(display, None, t, at),
            Face::Label(c) => bench_screen::text(display, Some(c), t, at),
        });
    }

    fn flush(&mut self) {
        bench_screen::flush(self.display);
    }
}

fn show(
    rec: &mut Recorder<'_>,
    clocks: Clocks,
    rows: &[Counts; ROWS],
    kernel: u32,
    fx: &[u32; FX_ROWS],
) {
    rec.clear();
    let mut line = FmtBuf::new();
    let _ = write!(
        line,
        "BENCH REV {} {} MHZ",
        clocks.rev.label(),
        clocks.cpu_hz / 1_000_000
    );
    rec.heading(line.as_str());
    line.clear();
    let _ = write!(line, "CYCLES/SAMPLE, 1..{MAX_VOICES} VOICES");
    rec.line(Face::Label(theme::MID), line.as_str(), (4, 30));
    let mut y = 46;
    for (&(label, ..), counts) in PATCHES.iter().zip(rows) {
        voice_row(rec, &mut line, y, label, counts);
        y += ALGO_ROW_H;
    }
    line.clear();
    let _ = write!(line, "KERNEL /VOICE {kernel} (350)");
    rec.line(Face::Value, line.as_str(), (4, y));
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
        rec.line(
            Face::Label(theme::INK2),
            line.as_str(),
            (4 + col * 78, y + row * 10),
        );
    }
    rec.flush();
}

/// The REVERB screen: each `REV` row less MIX and BUS+V as read, three to
/// a line as the FX screen's, then STRESS's and REV SWITCH MAX's costliest
/// blocks against their budget.
fn show_reverb(
    rec: &mut Recorder<'_>,
    rev: &[u32; REV_ROWS],
    mix: u32,
    bus_v: u32,
    (
        [
            [first, after],
            [gate1, gate2],
            [room1, room2],
            [hall1, hall2],
            [bloom1, bloom2],
        ],
        budget,
    ): ([[u32; 2]; 5], u32),
    (switch, switch_budget): (u32, u32),
) {
    rec.clear();
    rec.heading("REVERB");
    let mut line = FmtBuf::new();
    let rows = REV
        .iter()
        .map(|&(label, _)| label)
        .zip(rev.iter().map(|&c| c.saturating_sub(mix)));
    let cells = rows.chain([("BUS+V", bus_v)]);
    let mut y = 46;
    for (i, (label, c)) in cells.enumerate() {
        line.clear();
        let _ = write!(line, "{label} {c}");
        let (col, row) = ((i % 3) as i32, (i / 3) as i32);
        y = 46 + row * 10;
        rec.line(Face::Label(theme::INK2), line.as_str(), (4 + col * 78, y));
    }
    // Keys before ORBIT starts, then after, as MEMORY's STRESS: a TYPE a
    // line, as `FmtBuf` holds 32 bytes.
    let rows = [
        ("QUAD", first, after),
        ("GATE", gate1, gate2),
        ("ROOM", room1, room2),
        ("HALL", hall1, hall2),
        ("BLOOM", bloom1, bloom2),
    ];
    for (label, a, b) in rows {
        y += 10;
        line.clear();
        let _ = write!(line, "STRESS {label} {a}/{b}");
        rec.line(Face::Label(theme::INK2), line.as_str(), (4, y));
    }
    y += 10;
    line.clear();
    let _ = write!(line, "STRESS BUDGET {budget}");
    rec.line(Face::Label(theme::INK2), line.as_str(), (4, y));
    line.clear();
    let _ = write!(line, "REV SWITCH MAX {switch}/{switch_budget}");
    rec.line(Face::Label(theme::INK2), line.as_str(), (4, y + 10));
    rec.flush();
}

/// A screen of `rows`' labels and their counts under `heading`: a ROUTING
/// page or FORMANT.
fn show_rows(rec: &mut Recorder<'_>, heading: &str, rows: &[RoutingRow], counts: &[Counts]) {
    rec.clear();
    let mut line = FmtBuf::new();
    rec.heading(heading);
    for (i, (&(label, ..), c)) in rows.iter().zip(counts).enumerate() {
        voice_row(rec, &mut line, 46 + i as i32 * ROW_H, label, c);
    }
    rec.flush();
}

/// The sizes behind the D2 budget, the exclusive state's two timings and
/// the project CRC's.
fn show_memory(
    rec: &mut Recorder<'_>,
    (rebuild, proj_crc): (u32, u32),
    note_on: u32,
    (lowest, dark, light): (u32, u32, u32),
) {
    use core::mem::size_of;
    rec.clear();
    rec.heading("MEMORY");
    let lines = [
        format_args!("VOICE {}", size_of::<Voice>()),
        format_args!("SLOT {}", size_of::<EngineSlot>()),
        format_args!("MODAL {}", size_of::<ModalEngine>()),
        format_args!("SYM POOL {}", size_of::<SymPool>()),
        format_args!("INSTR {}/{VOICE_RAM_BUDGET}", size_of::<Instrument>()),
        format_args!("REBUILD {rebuild} CYC"),
        format_args!("SYM NOTE-ON {note_on} CYC"),
        format_args!("SYM NOTE-ON LOW {lowest} CYC"),
        // The whole note-on and block, against the audio budget; then
        // what COLOR 0 adds, its six passes over the line.
        format_args!("DARK NOTE+BLOCK {dark} CYC"),
        format_args!("DARK +6 PASSES {} CYC", dark.saturating_sub(light)),
        // One project's CRC, every slot filled: the project status's cost.
        format_args!("PROJ CRC {proj_crc} CYC"),
    ];
    show_lines(rec, lines.into_iter());
}

/// The STRESS screen: the costliest block of each run (keys before ORBIT,
/// then after) against the block budget, and the most voices it sounded;
/// with ORBIT, each run's costliest ORBIT block and the block ORBIT
/// starts in, whole. Then the LOAD runs' (a Sound loaded into Part 6 every
/// UI frame, then into Parts 1-5 at once, keys first) and the MOVE run's
/// (a knob turned on every Part every block); each run's knobs' ease, the
/// most in any block and in its costliest block; and what the bill
/// counted at its costliest block (first, after, LOAD 6, LOAD 1-5, MOVE).
fn show_stress(rec: &mut Recorder<'_>, runs: [Stress; 5], budget: u32) {
    let [s1, s2, l1, l5, mv] = runs;
    rec.clear();
    rec.heading("STRESS");
    let (k, p, b) = (
        runs.map(|r| r.knobs.0),
        runs.map(|r| r.knobs.1),
        runs.map(|r| r.billed),
    );
    let lines = [
        format_args!(
            "STRESS {}/{}/{budget} {}/{}V",
            s1.worst, s2.worst, s1.most, s2.most
        ),
        format_args!("STRESS ORBIT {}/{} CYC", s1.orbit, s2.orbit),
        format_args!("STRESS T0 {}/{}", s1.t0, s2.t0),
    ];
    let shown = if cfg!(feature = "orbit") { 3 } else { 1 };
    let more = [
        format_args!("STRESS LOAD {}/{}/{budget}", l1.worst, l5.worst),
        format_args!("STRESS MOVE {}/{budget}", mv.worst),
        format_args!("KNOBS MAX {}/{}/{}/{}/{}", k[0], k[1], k[2], k[3], k[4]),
        format_args!("KNOBS PEAK {}/{}/{}/{}/{}", p[0], p[1], p[2], p[3], p[4]),
        format_args!("BILLED {}/{}/{}/{}/{}", b[0], b[1], b[2], b[3], b[4]),
    ];
    show_lines(rec, lines.into_iter().take(shown).chain(more));
}

/// `lines` down the screen, 22 px apart, and flushed.
fn show_lines<'a>(rec: &mut Recorder<'_>, lines: impl Iterator<Item = core::fmt::Arguments<'a>>) {
    let mut line = FmtBuf::new();
    for (i, args) in lines.enumerate() {
        line.clear();
        let _ = line.write_fmt(args);
        rec.line(Face::Value, line.as_str(), (4, 46 + i as i32 * 22));
    }
    rec.flush();
}

/// `label`'s per-voice cost (`Counts::per_voice`), with the voices that
/// sounded when fewer than all, and its count at each voice count.
fn voice_row(rec: &mut Recorder<'_>, line: &mut FmtBuf, y: i32, label: &str, counts: &Counts) {
    line.clear();
    let _ = write!(line, "{label} /VOICE {}", counts.per_voice());
    if counts.spans < MAX_VOICES {
        let _ = write!(line, " ({} V)", counts.spans);
    }
    rec.line(Face::Value, line.as_str(), (4, y));
    // One cell per count: the counts together overflow a `FmtBuf`.
    for (i, c) in counts.cycles.iter().enumerate() {
        line.clear();
        let _ = write!(line, "{c}");
        rec.line(
            Face::Label(theme::INK2),
            line.as_str(),
            (4 + i as i32 * CELL_W, y + 12),
        );
    }
}

/// Each Part's Sound published as a load is, through the knobs' publisher
/// (ADR 0115): NEW's, and A16 ∪ A17 on Part 6. Out of `Rig::stress`: its
/// Sounds stay off that frame, under the stack check's 8 KB.
#[inline(never)]
fn publish_new(
    shared: &mut AudioShared,
    sound: &mut Tracked<Sound>,
    knobs: &mut KnobPublisher,
    count: &mut [Loads; MAX_PARTS],
) {
    for (i, part) in shared.parts.iter_mut().enumerate() {
        part.mix = PartParams::for_part(i);
    }
    for (p, count) in count.iter_mut().enumerate() {
        let new = NEW_PARTS.get(p).and_then(|s| factory_sound(s.index()));
        let s = new.unwrap_or_else(|| Sound::init(EngineType::Algo));
        (sound.params, sound.mod_state) = (s.params, s.mod_state);
        if p == MAX_PARTS - 1 {
            sound.params = a16_a17();
        }
        sound.params.out.sends = [0.5; FX_SENDS];
        *count = count.next();
        shared.publish_sound(p, sound, *count, knobs);
    }
}
