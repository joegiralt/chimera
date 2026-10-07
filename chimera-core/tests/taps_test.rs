//! ALGO's taps as stored routes and matrix rows (filter-types spec § 6, § 7):
//! the `TapRoutes` record, the OP5 and OP6 rows, X cells, and the jacks'
//! tap arms.

mod common;

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::DiskCode;
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::modulation::{FixedCol, MAX_MOD_SOURCES, ModSource};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::preset::Sound;
use chimera_core::storage::{FileError, RecordTag};
use chimera_core::taps::{Jack, TapOp, TapRoutes, to_tap_amount};
use chimera_core::ui::filter_panel::{JackSlot, jack_amount, repatch, set_jack_amount};
use chimera_core::ui::mod_grid::{FIXED_COLS, MatrixState, RowSource, X_OP5, X_OP6};
use common::codec_util::{decode, encode, record_offsets, sound_file};

fn code(a: ParamAddr) -> [u8; 2] {
    [a.block.disk_code().unwrap(), a.param.0]
}

fn entry(jack: u8, a: ParamAddr, amount: i8) -> [u8; 4] {
    let [b, id] = code(a);
    [jack, b, id, amount as u8]
}

/// A matrix showing `s`, as the UI loads it.
fn matrix(s: &Sound) -> MatrixState {
    let mut m = MatrixState::new();
    m.load_sound(s);
    m
}

/// A Sound with a registry column, so the matrix has X cells to try.
fn algo_with_registry() -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    let a = ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::LEVEL);
    s.dest_registry.add(a, *b"O1 LEVEL").unwrap();
    s.mod_state.push(a);
    s
}

#[test]
fn tap_routes_round_trip_and_bad_entries_drop() {
    for op in TapOp::ALL {
        for c in FixedCol::ALL {
            for a in [127, -127, 0, 50] {
                let mut s = Sound::init(EngineType::Algo);
                s.params.taps.set(op, c, a);
                let d = decode(&encode(&s)).unwrap();
                assert_eq!(d.params.taps.get(op, c), Some(a), "{op:?} {c:?} {a}");
                assert_eq!(d.params.taps, s.params.taps);
                assert!(d.bits_eq(&s));
            }
        }
    }
    // Every entry at once.
    let mut s = Sound::init(EngineType::Algo);
    for (i, (op, c)) in TapOp::ALL
        .into_iter()
        .flat_map(|op| FixedCol::ALL.map(|c| (op, c)))
        .enumerate()
    {
        s.params.taps.set(op, c, (i as i16 * 30 - 100) as i8);
    }
    assert_eq!(decode(&encode(&s)).unwrap().params.taps, s.params.taps);

    let engine = (RecordTag::Engine, vec![EngineType::Algo.disk_code()]);
    let op5_level = ParamAddr::new(BlockRef::AlgoOp(Op::E), AlgoOpParams::LEVEL);
    let flt_level = ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL);
    let q = FixedCol::Q.addr();
    let entries = [
        entry(8, q, 30),
        entry(3, q, 11),
        entry(10, q, 12),
        entry(8, op5_level, 13),
        entry(9, flt_level, 14),
        entry(9, FixedCol::Fold.addr(), -40),
    ];
    let payload: Vec<u8> = entries.concat();
    let d = decode(&sound_file(&[
        engine.clone(),
        (RecordTag::TapRoutes, payload.clone()),
    ]))
    .unwrap();
    let mut want = TapRoutes::default();
    want.set(TapOp::Op5, FixedCol::Q, 30);
    want.set(TapOp::Op6, FixedCol::Fold, -40);
    assert_eq!(d.params.taps, want);
    // A 3-byte tail is not an entry.
    let mut tail = payload;
    tail.extend_from_slice(&entry(8, q, 1)[..3]);
    assert_eq!(
        decode(&sound_file(&[engine, (RecordTag::TapRoutes, tail)])).err(),
        Some(FileError::Bounds)
    );
    // An empty `TapRoutes` writes no record.
    let s = Sound::init(EngineType::Algo);
    assert_eq!(encode(&s), encode(&decode(&encode(&s)).unwrap()));
    let f = encode(&s);
    let tags: Vec<u16> = record_offsets(&f)
        .iter()
        .filter(|&&o| o + 4 <= f.len() - 4)
        .map(|&o| u16::from_le_bytes([f[o], f[o + 1]]))
        .collect();
    assert!(tags.contains(&RecordTag::Routes.code()));
    assert!(!tags.contains(&RecordTag::TapRoutes.code()));
}

#[test]
fn x_cells_refuse_every_edit() {
    let mut s = algo_with_registry();
    s.params.taps.set(TapOp::Op6, FixedCol::Vca, 20);
    let mut m = matrix(&s);
    assert!(m.num_cols() > FIXED_COLS);
    for col in FIXED_COLS..m.num_cols() {
        assert!(m.is_x(9, col) && m.is_x(8, col), "col {col}");
        let (taps, rev) = (m.taps, m.rev);
        let src = RowSource::Tap(TapOp::Op6).index();
        m.set(src, col, 40);
        (m.sel_row, m.sel_col) = (9, col);
        m.adjust_amount(10);
        m.adjust_amount(-10);
        m.delete_selected();
        assert_eq!((m.taps, m.rev), (taps, rev), "col {col}");
        assert_eq!(m.amount(src, col), 0);
        assert!(!m.is_present(src, col));
        assert_eq!(m.hint(), X_OP6);
        m.sel_row = 8;
        assert_eq!(m.hint(), X_OP5);
    }
    // The fixed columns are not X cells: they take an amount.
    for c in FixedCol::ALL {
        assert!(!m.is_x(9, c.index()));
    }
    (m.sel_row, m.sel_col) = (9, FixedCol::Freq.index());
    m.adjust_amount(10);
    assert_eq!(m.taps.get(TapOp::Op6, FixedCol::Freq), Some(10));
    let mut t = TapRoutes::default();
    m.sync_taps(&mut t);
    assert_eq!(t, m.taps);
}

#[test]
fn tap_rows_only_on_algo() {
    let m = matrix(&Sound::init(EngineType::Algo));
    assert_eq!(m.rows(), 10);
    assert_eq!(m.row_source(8), RowSource::Tap(TapOp::Op5));
    assert_eq!(m.row_source(9), RowSource::Tap(TapOp::Op6));
    assert_eq!(m.row_source(0), RowSource::Mod(ModSource::Env1.index()));
    let mut modal = Sound::init(EngineType::Modal);
    modal.params.taps.set(TapOp::Op5, FixedCol::Q, 50);
    let mut m = matrix(&modal);
    assert_eq!(m.rows(), 8);
    assert!(!m.is_x(8, FIXED_COLS));
    // A stored tap is inert off ALGO: no row reaches it.
    m.move_row(20);
    assert_eq!(m.sel_row, 7);
    m.set(RowSource::Tap(TapOp::Op5).index(), FixedCol::Q.index(), 9);
    assert_eq!(m.taps.get(TapOp::Op5, FixedCol::Q), Some(50));
}

#[test]
fn mod_tap_mod_keeps_the_depth() {
    for a in -50..=50 {
        let mut s = Sound::init(EngineType::Algo);
        assert_eq!(
            JackSlot::Fm3.effective(&s),
            Jack::Mod(ModSource::Lfo1),
            "FM3 IN starts on LFO 1"
        );
        set_jack_amount(&mut s, JackSlot::Fm3, a);
        assert!(repatch(&mut s, JackSlot::Fm3, Jack::Tap(TapOp::Op6)));
        assert_eq!(
            s.params.taps.get(TapOp::Op6, FixedCol::Freq),
            Some(to_tap_amount(a)),
            "{a}"
        );
        assert_eq!(
            s.mod_state.fixed_present(FixedCol::Freq) & 1 << ModSource::Lfo1.index(),
            0
        );
        assert_eq!(jack_amount(&s, JackSlot::Fm3), to_tap_amount(a));
        assert!(repatch(&mut s, JackSlot::Fm3, Jack::Mod(ModSource::Lfo1)));
        let back = jack_amount(&s, JackSlot::Fm3);
        assert!((back as i16 - a as i16).abs() <= 1, "{a} came back {back}");
        assert_eq!(s.params.taps.get(TapOp::Op6, FixedCol::Freq), None);
    }
    // Tap → Tap moves the entry; an occupied target refuses.
    let mut s = Sound::init(EngineType::Algo);
    assert!(repatch(&mut s, JackSlot::Qm, Jack::Tap(TapOp::Op5)));
    set_jack_amount(&mut s, JackSlot::Qm, 70);
    assert!(repatch(&mut s, JackSlot::Qm, Jack::Tap(TapOp::Op6)));
    assert_eq!(s.params.taps.get(TapOp::Op5, FixedCol::Q), None);
    assert_eq!(s.params.taps.get(TapOp::Op6, FixedCol::Q), Some(70));
    s.params.taps.set(TapOp::Op5, FixedCol::Q, 5);
    assert!(!repatch(&mut s, JackSlot::Qm, Jack::Tap(TapOp::Op5)));
    s.mod_state
        .set_fixed(ModSource::Lfo2.index(), FixedCol::Q, 1);
    assert!(!repatch(&mut s, JackSlot::Qm, Jack::Mod(ModSource::Lfo2)));
    assert_eq!(s.params.filter.qm_src(), Jack::Tap(TapOp::Op6));
    // Off ALGO no jack takes a tap.
    let mut s = Sound::init(EngineType::Modal);
    assert!(!repatch(&mut s, JackSlot::Fm3, Jack::Tap(TapOp::Op5)));
}

#[test]
fn no_mod_source_caller_sees_a_tap() {
    assert_eq!(ModSource::ALL.len(), MAX_MOD_SOURCES);
    assert_eq!(MAX_MOD_SOURCES, 8);
    let mut s = Sound::init(EngineType::Algo);
    for op in TapOp::ALL {
        for c in FixedCol::ALL {
            s.params.taps.set(op, c, 64);
        }
    }
    let d = decode(&encode(&s)).unwrap();
    assert!(d.mod_state.num_sources() <= MAX_MOD_SOURCES);
    assert!(d.mod_state.bits_eq(&s.mod_state));
    assert_eq!(d.params.taps.wanted(), [true, true]);
}

#[test]
fn qm_on_a_tap_views_the_tap_cell() {
    let mut s = Sound::init(EngineType::Algo);
    assert!(repatch(&mut s, JackSlot::Qm, Jack::Tap(TapOp::Op5)));
    set_jack_amount(&mut s, JackSlot::Qm, 30);
    assert_eq!(s.params.taps.get(TapOp::Op5, FixedCol::Q), Some(30));
    let m = matrix(&s);
    assert_eq!(
        m.amount(RowSource::Tap(TapOp::Op5).index(), FixedCol::Q.index()),
        30
    );
    assert_eq!(m.amount(m.row_source(8).index(), FixedCol::Q.index()), 30);
    assert!(m.is_present(m.row_source(8).index(), FixedCol::Q.index()));
}

#[test]
fn tap_routes_moving_and_wanted() {
    let mut t = TapRoutes::default();
    assert!(t.is_empty() && !t.moving(FixedCol::Freq) && t.wanted() == [false, false]);
    t.set(TapOp::Op6, FixedCol::Fold, 0);
    assert!(!t.is_empty() && !t.moving(FixedCol::Fold) && t.wanted() == [false, false]);
    t.set(TapOp::Op6, FixedCol::Fold, -3);
    assert!(t.moving(FixedCol::Fold) && !t.moving(FixedCol::Q));
    assert_eq!(t.wanted(), [false, true]);
    t.clear(TapOp::Op6, FixedCol::Fold);
    assert!(t.is_empty());
    // The depth conversion: 10 octaves a route, 4 a tap, held to ±127.
    assert_eq!(to_tap_amount(40), 100);
    assert_eq!(to_tap_amount(127), 127);
    assert_eq!(to_tap_amount(-127), -127);
    assert_eq!(to_tap_amount(1), 3);
    assert_eq!(chimera_core::taps::from_tap_amount(100), 40);
}

// ---- Task 14: capture and the four per-sample destinations (spec § 6) ----

use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use chimera_core::dsp::algo::kernel::{Kernel, KernelBlock, OpBlock, SAMPLE_SCALE};
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::plan::{EvalPlan, OPS};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::coef::exp2_poly;
use chimera_core::dsp::filter::k2040::{OUT_2040, R_2040};
use chimera_core::dsp::filter::{Filter, FilterKind, FilterMode, FilterMods, K2040Voice};
use chimera_core::dsp::voice::Voice;
use chimera_core::dsp::wavefolder::Wavefolder;
use chimera_core::modulation::ModState;
use chimera_core::params::{FolderParams, ParamSnapshot};
use chimera_core::taps::{TapLane, TapRows, tap_vca};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::{Rig, clicks, goertzel};

const SR: u32 = 48_000;
/// 0.1 s: every test tone sits on a 10 Hz bin.
const WIN: usize = 4800;
/// The DC blocker has settled (0.1 s at 5 Hz) many times over by then.
const DC_FROM: usize = 750;
/// The off-bin energy (products past 3rd order, `off_bin_db`) a 100 % FREQ
/// tap may add over the tap-free filter: measured at most +15.9 dB (the
/// 2040; CLEAN BP2 +15.4), pinned with 4 dB to spare.
const TAP_ALIAS_DB: f32 = 20.0;
/// A 100 % Q tap swings RES through the onset, so the resonant modes ring
/// off the input's bins for part of each tap cycle: measured at most +46.4
/// dB (CLEAN LP4; LP2 and NOTCH1 +28), pinned with 3.6 dB to spare.
const TAP_Q_ALIAS_DB: f32 = 50.0;
/// CLEAN's output bound on the grid (filter_types_test's `CLEAN_BOUND`).
const CLEAN_BOUND: f32 = 11.43;

const DIRTY_MODES: [FilterMode; 4] = [
    FilterMode::Lp12,
    FilterMode::Bp12,
    FilterMode::Hp12,
    FilterMode::Notch,
];

const A7: u8 = 14; // 4 → 1, 5 → 2, 6 → 3; carriers 1, 2, 3
const A1: u8 = 8; // every operator a carrier

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-30).log10()
}

fn sine(hz: f32, n: usize) -> f32 {
    libm::sinf(2.0 * core::f32::consts::PI * hz * n as f32 / SR as f32)
}

/// OP1 a sine at the note on `alg`, with OP `i + 1` at COARSE `c`, LEVEL `l`
/// for each `(i, c, l)`; the filter wide open.
fn algo(alg: u8, ops: &[(usize, u8, u8)]) -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo = AlgoParams::single(WaveId::W1);
    (s.params.algo.alg_a, s.params.algo.alg_b) = (alg, alg);
    for &(i, c, l) in ops {
        (s.params.algo.ops[i].coarse, s.params.algo.ops[i].level) = (c, l);
    }
    (s.params.filter.cutoff, s.params.filter.resonance) = (20_000.0, 0.0);
    s.mod_state = ModState::new();
    s
}

/// OP1 at A4 (440 Hz) heard alone; OP6 at ratio ½ (220 Hz) into the silent
/// OP3, so it is heard only through its taps.
fn am_patch() -> Sound {
    algo(A7, &[(5, 0, 99)])
}

fn play(p: &ParamSnapshot, mods: &ModState, blocks: usize) -> Vec<f32> {
    let mut rig = Rig::new(SR);
    rig.note_on(MidiNote::A4, Velocity::DEFAULT, p);
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..blocks {
        rig.render(&mut b, p, mods);
        out.extend_from_slice(&b);
    }
    out
}

/// `x` under a Hann window: no leakage from a tone a hair off its bin.
fn hann(x: &[f32]) -> Vec<f32> {
    let n = x.len() as f32;
    x.iter()
        .enumerate()
        .map(|(i, &v)| v * 0.5 * (1.0 - libm::cosf(2.0 * core::f32::consts::PI * i as f32 / n)))
        .collect()
}

fn tail(v: &[f32]) -> &[f32] {
    &v[v.len() - WIN..]
}

#[test]
fn vca_tap_sidebands_follow_am_and_ring_theory() {
    let at = |a: Option<i8>| {
        let mut s = am_patch();
        if let Some(a) = a {
            s.params.taps.set(TapOp::Op6, FixedCol::Vca, a);
        }
        let y = play(&s.params, &s.mod_state, 300);
        let t = hann(tail(&y));
        [440.0, 220.0, 660.0].map(|hz| goertzel(&t, hz, SR))
    };
    let [c0, ..] = at(None);
    assert!(c0 > 0.01, "the carrier sounds: {c0}");
    // Ring (100 %): x·A·sin, sidebands A/2 each, the carrier gone.
    let [c1, lo1, hi1] = at(Some(127));
    let amp = (lo1 + hi1) / c0;
    assert!((0.5..=1.5).contains(&amp), "OP6's amplitude {amp}");
    assert!(db(c1 / c0) <= -40.0, "ring carrier {} dB", db(c1 / c0));
    // AM (50 %): (1 − a) carrier, a·A/2 sidebands.
    let a = 64.0 / 127.0;
    let [c, lo, hi] = at(Some(64));
    let near = |got: f32, want: f32, what: &str| {
        let e = db(got / want);
        assert!(e.abs() <= 1.0, "{what}: {e:.2} dB off theory");
    };
    near(c / c0, 1.0 - a, "AM carrier");
    near(lo / c0, a * amp / 2.0, "AM lower sideband");
    near(hi / c0, a * amp / 2.0, "AM upper sideband");
    // 0 %: no sidebands.
    let [_, lo0, hi0] = at(Some(0));
    assert!(
        db(lo0.max(hi0) / c0) < -90.0,
        "0 %: {} dB",
        db(lo0.max(hi0) / c0)
    );
}

/// `amp`·saw at 200 Hz (240 samples a period: exact on `WIN`).
fn saw(n: usize) -> f32 {
    0.5 * (2.0 * (n % 240) as f32 / 240.0 - 1.0)
}

fn rows_with(op6_hz: f32, b: usize) -> TapRows {
    let mut r = TapRows::new([false, true]);
    let [_, row] = r.rows_mut();
    for (i, v) in row.unwrap().iter_mut().enumerate() {
        *v = sine(op6_hz, b * BLOCK_SIZE + i);
    }
    r
}

fn lane(rows: &TapRows, a: f32) -> TapLane<'_> {
    TapLane {
        rows: rows.rows().unwrap(),
        from: [0.0, a],
        to: [0.0, a],
    }
}

#[test]
fn freq_tap_makes_sidebands_on_lp2() {
    let run = |a: f32| {
        let mut p = FilterParams::default();
        assert!(p.set_mode(FilterMode::Lp12));
        (p.cutoff, p.resonance) = (1000.0, 0.3);
        let mut f = Filter::new();
        let mut out = Vec::new();
        for b in 0..150 {
            let rows = rows_with(290.0, b);
            let mods = FilterMods {
                freq: Some(lane(&rows, a)),
                ..FilterMods::NONE
            };
            let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| saw(b * BLOCK_SIZE + i));
            f.process(&mut buf, &p, SR, &mods);
            out.extend_from_slice(&buf);
        }
        let t = tail(&out);
        (goertzel(t, 200.0, SR), goertzel(t, 1290.0, SR))
    };
    let (f50, s50) = run(0.5);
    assert!(db(s50 / f50) > -60.0, "50 %: {} dB", db(s50 / f50));
    let (f0, s0) = run(0.0);
    assert!(db(s0 / f0) < -90.0, "0 %: {} dB", db(s0 / f0));
}

#[test]
fn fold_tap_moves_the_fold_at_the_op_rate() {
    let p = FolderParams {
        fold: 0.2,
        symmetry: 0.5,
        mix: 1.0,
    };
    let run = |tap: bool| {
        let mut w = Wavefolder::new();
        let mut out = Vec::new();
        for b in 0..150 {
            let rows = rows_with(290.0, b);
            let l = lane(&rows, 1.0);
            let mut buf: [f32; BLOCK_SIZE] =
                core::array::from_fn(|i| 0.8 * sine(200.0, b * BLOCK_SIZE + i));
            w.process(&mut buf, &p, true, tap.then_some(&l));
            out.extend_from_slice(&buf);
        }
        let t = tail(&out);
        (goertzel(t, 200.0, SR), goertzel(t, 490.0, SR))
    };
    let (f, s) = run(true);
    assert!(db(s / f) > -60.0, "tapped: {} dB", db(s / f));
    let (f, s) = run(false);
    assert!(db(s / f) < -90.0, "untapped: {} dB", db(s / f));
    // FOLD off, a FOLD tap: the folder runs (and is billed).
    let mut q = algo(A7, &[(5, 0, 99)]).params;
    q.folder.fold = 0.0;
    assert!(!Voice::stage_runs(&q, &ModState::new()).0);
    q.taps.set(TapOp::Op6, FixedCol::Fold, 127);
    assert!(Voice::stage_runs(&q, &ModState::new()).0);
    let mut m = ParamSnapshot::for_engine(EngineType::Modal);
    (m.folder.fold, m.taps) = (0.0, q.taps);
    assert!(!Voice::stage_runs(&m, &ModState::new()).0, "inert off ALGO");
}

#[test]
fn tap_vca_leaves_no_dc() {
    // OP6 alone, a 1:1 carrier, times itself: a DC of half its square.
    let mut s = algo(A1, &[(0, 4, 0), (5, 4, 99)]);
    s.params.taps.set(TapOp::Op6, FixedCol::Vca, 127);
    let y = play(&s.params, &s.mod_state, DC_FROM + 375);
    let t = &y[DC_FROM * BLOCK_SIZE..];
    let peak = t.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.05, "it sounds: {peak}");
    let dc = t.iter().map(|&v| f64::from(v)).sum::<f64>() / t.len() as f64;
    assert!(db(dc.abs() as f32) < -80.0, "DC {} dB", db(dc.abs() as f32));
}

/// Every (type, mode or voice), as `FilterParams`.
fn every_setting() -> Vec<FilterParams> {
    let mut v = Vec::new();
    for &m in FilterKind::Svf.modes() {
        let mut p = FilterParams::default();
        assert!(p.set_mode(m));
        v.push(p);
    }
    for voice in K2040Voice::ALL {
        let mut p = FilterParams::default();
        p.set_kind(FilterKind::K2040);
        p.voice = voice;
        v.push(p);
    }
    for m in DIRTY_MODES {
        let mut p = FilterParams::default();
        p.set_kind(FilterKind::Dirty);
        assert!(p.set_mode(m));
        v.push(p);
    }
    v
}

fn bound(p: &FilterParams) -> f32 {
    match p.kind() {
        FilterKind::Svf => CLEAN_BOUND,
        FilterKind::K2040 => R_2040 / p.voice.alpha() * OUT_2040 + 1e-6,
        FilterKind::Dirty => 1.0,
    }
}

/// The share of `x`'s energy (dB) off every product `m·a ± n·b` with
/// `|m|, |n| ≤ 3`, folded about Nyquist; on 50 Hz bins.
fn off_bin_db(x: &[f32], a: f32, b: f32) -> f32 {
    let bin = SR as f32 / x.len() as f32;
    let fold = |f: f32| {
        let f = f.rem_euclid(SR as f32);
        if f > SR as f32 / 2.0 {
            SR as f32 - f
        } else {
            f
        }
    };
    let mut on = std::collections::BTreeSet::new();
    for m in -3i32..=3 {
        for n in -3i32..=3 {
            on.insert((fold(m as f32 * a + n as f32 * b) / bin).round() as usize);
        }
    }
    let (mut total, mut off) = (0.0f64, 0.0f64);
    for k in 0..=x.len() / 2 {
        let e = f64::from(goertzel(x, k as f32 * bin, SR)).powi(2);
        total += e;
        if !on.contains(&k) {
            off += e;
        }
    }
    (10.0 * (off.max(1e-30) / total).log10()) as f32
}

#[test]
fn taps_never_blow_up_or_alias_past_the_margin() {
    // OP6 at 8 kHz on a note at 8 kHz/√2, both on 50 Hz bins.
    let (note, op6) = (5650.0f32, 8000.0f32);
    for p0 in every_setting() {
        let mut p = p0;
        (p.cutoff, p.resonance) = (10_000.0, 0.5);
        let run = |which: Option<bool>| {
            let mut f = Filter::new();
            let mut out = Vec::new();
            for b in 0..60 {
                let rows = rows_with(op6, b);
                let l = lane(&rows, 1.0);
                let mods = match which {
                    None => FilterMods::NONE,
                    Some(true) => FilterMods {
                        freq: Some(l),
                        ..FilterMods::NONE
                    },
                    Some(false) => FilterMods {
                        q: Some(l),
                        ..FilterMods::NONE
                    },
                };
                let mut buf: [f32; BLOCK_SIZE] =
                    core::array::from_fn(|i| 0.5 * sine(note, b * BLOCK_SIZE + i));
                f.process(&mut buf, &p, SR, &mods);
                for v in buf {
                    assert!(
                        v.is_finite() && v.abs() <= bound(&p),
                        "{p:?} {which:?}: {v}"
                    );
                }
                out.extend_from_slice(&buf);
            }
            off_bin_db(&out[out.len() - 960..], note, op6)
        };
        let free = run(None).max(-120.0);
        for which in [true, false] {
            let tapped = run(Some(which));
            println!(
                "{:?} {:?} {}: free {free:.1} dB, tapped {tapped:.1} dB",
                p.kind(),
                p.mode(),
                if which { "FREQ" } else { "Q" }
            );
            let margin = if which { TAP_ALIAS_DB } else { TAP_Q_ALIAS_DB };
            assert!(
                tapped <= free + margin,
                "{p:?} FREQ {which}: {tapped} vs {free}"
            );
        }
    }
}

/// Carried from Task 9: a Q tap drives `q` above both of the block's ends
/// (RES down to 0) at FREQ near DIRTY's ceiling; `f` must be bounded at
/// the `q` the tap reaches, not the ends', or the loop leaves Jury's
/// region and rails.
#[test]
fn dirty_bounds_f_at_the_q_a_tap_reaches() {
    let mut p = FilterParams::default();
    p.set_kind(FilterKind::Dirty);
    (p.cutoff, p.resonance) = (20_000.0, 0.45);
    let mut f = Filter::new();
    let mut x = 0x1234_5678u32;
    let mut peak = 0.0f32;
    for b in 0..750 {
        let rows = rows_with(300.0, b);
        let mods = FilterMods {
            q: Some(lane(&rows, -0.45)),
            ..FilterMods::NONE
        };
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            1e-3 * (x as f32 / u32::MAX as f32 - 0.5)
        });
        f.process(&mut buf, &p, SR, &mods);
        if b > 100 {
            peak = buf.iter().fold(peak, |m, v| m.max(v.abs()));
        }
    }
    assert!(peak < 0.01, "a small signal stays small: {peak}");
}

#[test]
fn tap_enable_disable_and_repatch_are_click_free() {
    let mut s = am_patch();
    (s.params.filter.cutoff, s.params.out.volume) = (2000.0, 1.0);
    s.mod_state = Sound::init(EngineType::Algo).mod_state;
    set_jack_amount(&mut s, JackSlot::Fm3, 20);
    assert_eq!(jack_amount(&s, JackSlot::Fm3), 20);
    let mut rig = Rig::new(SR);
    rig.note_on(MidiNote::A4, Velocity::DEFAULT, &s.params);
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    let mut block = |rig: &mut Rig, s: &Sound, n: usize, out: &mut Vec<f32>| {
        for _ in 0..n {
            rig.render(&mut b, &s.params, &s.mod_state);
            out.extend_from_slice(&b);
        }
    };
    block(&mut rig, &s, 40, &mut out);
    // VCA: 0 → 100 % → 0 in encoder steps, then 100 % deleted at once.
    for a in (0..=127).step_by(8).chain((0..=127).rev().step_by(8)) {
        s.params.taps.set(TapOp::Op6, FixedCol::Vca, a as i8);
        block(&mut rig, &s, 1, &mut out);
    }
    // A 100 % tap patched and deleted at once, at many phases.
    for _ in 0..24 {
        s.params.taps.set(TapOp::Op6, FixedCol::Vca, 127);
        block(&mut rig, &s, 3, &mut out);
        s.params.taps.clear(TapOp::Op6, FixedCol::Vca);
        block(&mut rig, &s, 2, &mut out);
    }
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.5, "loud enough to hear a step: {peak}");
    // FM3 IN: LFO 1 → OP6 mid-note, and back.
    assert!(repatch(&mut s, JackSlot::Fm3, Jack::Tap(TapOp::Op6)));
    assert!(s.params.taps.get(TapOp::Op6, FixedCol::Freq).is_some());
    block(&mut rig, &s, 20, &mut out);
    assert!(repatch(&mut s, JackSlot::Fm3, Jack::Mod(ModSource::Lfo1)));
    block(&mut rig, &s, 20, &mut out);
    let c = clicks(&out);
    assert!(c.is_empty(), "{:?}", &c[..c.len().min(8)]);
}

#[test]
fn taps_never_hold_a_voice() {
    let ends = |tap: bool| {
        let mut s = am_patch();
        if tap {
            s.params.taps.set(TapOp::Op6, FixedCol::Vca, 127);
        }
        let mut rig = Rig::new(SR);
        rig.note_on(MidiNote::A4, Velocity::DEFAULT, &s.params);
        let mut b = [0.0f32; BLOCK_SIZE];
        for _ in 0..20 {
            rig.render(&mut b, &s.params, &s.mod_state);
        }
        rig.note_off();
        (0..20_000)
            .position(|_| {
                rig.render(&mut b, &s.params, &s.mod_state);
                !rig.is_active()
            })
            .expect("the voice ends")
    };
    assert_eq!(ends(true), ends(false));
}

#[test]
fn taps_survive_an_engine_round_trip_and_are_inert_between() {
    let mut s = am_patch();
    assert!(s.params.filter.set_fm3_src(Jack::Tap(TapOp::Op6)));
    for c in FixedCol::ALL {
        s.params.taps.set(TapOp::Op6, c, 100);
        s.params.taps.set(TapOp::Op5, c, -60);
    }
    let before = s.params.taps;
    // The Part on MODAL: the same filter, jacks and taps.
    let mut modal = ParamSnapshot::for_engine(EngineType::Modal);
    (modal.filter, modal.taps) = (s.params.filter, s.params.taps);
    let mut bare = modal.clone();
    bare.taps = TapRoutes::default();
    assert!(bare.filter.set_fm3_src(Jack::Mod(ModSource::Lfo1)));
    let mods = ModState::new();
    let (a, b) = (play(&modal, &mods, 100), play(&bare, &mods, 100));
    assert!(a.iter().all(|v| v.is_finite()));
    assert!(a.iter().any(|&v| v != 0.0), "MODAL sounds");
    assert!(
        a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()),
        "taps inert on MODAL"
    );
    // Back on ALGO: the taps as they were, and heard.
    let mut back = s.params.clone();
    back.taps = modal.taps;
    assert_eq!(back.taps, before);
    let mut none = back.clone();
    none.taps = TapRoutes::default();
    let (t, u) = (play(&back, &mods, 50), play(&none, &mods, 50));
    assert!(t.iter().zip(&u).any(|(x, y)| x != y), "taps heard on ALGO");
}

#[test]
fn exp2_poly_is_within_a_tenth_of_a_percent() {
    assert_eq!(exp2_poly(0.0), 1.0);
    for i in -256..=256 {
        let x = i as f32 / 64.0;
        let e = (f64::from(exp2_poly(x)) / 2f64.powf(f64::from(x)) - 1.0).abs();
        assert!(e <= 1e-3, "{x}: {e}");
    }
}

const HOLD: EnvRates = EnvRates {
    ar: 31,
    d1r: 0,
    d1l: 15,
    d2r: 0,
    rr: 8,
    rs: 0,
};

fn op(hz: f32, gain: f32) -> OpBlock {
    OpBlock {
        inc: (hz / SR as f32 * 4_294_967_296.0) as u32,
        gain_from: gain * SAMPLE_SCALE,
        gain_to: gain * SAMPLE_SCALE,
        feedback: 0.0,
        lo: WaveId::W1.table(0),
        hi: WaveId::W1.table(0),
        xfade_from: 0.0,
        xfade_to: 0.0,
    }
}

/// Four blocks of `ops` on `plan`: the output, and the rows taken.
fn kernel(plan: &EvalPlan, ops: [OpBlock; OPS], want: [bool; 2]) -> (Vec<f32>, Vec<[f32; 2]>) {
    let mut k = Kernel::new();
    let mut env: [OpEnv; OPS] = core::array::from_fn(|_| {
        let mut e = OpEnv::IDLE;
        e.note_on(EnvCoefs::new(HOLD, MidiNote::A4, SR as f32));
        e
    });
    let blk = KernelBlock {
        plan,
        ops,
        morph_from: 0.0,
        morph_to: 0.0,
        norm_from: 1.0,
        norm_to: 1.0,
    };
    let (mut out, mut rows) = (Vec::new(), Vec::new());
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..4 {
        let mut t = TapRows::new(want);
        for r in t.rows_mut().into_iter().flatten() {
            *r = [9.0; BLOCK_SIZE];
        }
        k.render(&blk, &mut env, &mut b, &mut t);
        out.extend_from_slice(&b);
        // No op wanted: no rows at all, so nothing was written.
        let r = t.rows().copied().unwrap_or([[9.0; BLOCK_SIZE]; 2]);
        rows.extend((0..BLOCK_SIZE).map(|i| [r[0][i], r[1][i]]));
    }
    (out, rows)
}

#[test]
fn capture_matches_the_op_output() {
    // OP5 and OP6 into OP1; OP5 loud enough to clamp. Then a cycle, 6 → 5
    // on A and 5 → 6 on B: a delayed plan, rendered per sample.
    let mut fwd = [0u8; OPS];
    (fwd[4], fwd[5]) = (1, 1);
    let mut back = fwd;
    back[4] |= 1 << 5;
    let mut fwd6 = fwd;
    fwd6[5] |= 1 << 4;
    for (a_mods, b_mods, delayed) in [(fwd, fwd, false), (fwd6, back, true)] {
        for op5_gain in [1.6, 0.0] {
            let ops: [OpBlock; OPS] = core::array::from_fn(|i| match i {
                0 => op(440.0, 1.0),
                4 => op(660.0, op5_gain),
                5 => op(220.0, 0.7),
                _ => op(0.0, 0.0),
            });
            let plan = EvalPlan::build(&a_mods, 1, &b_mods, 1);
            assert_eq!(plan.delayed != 0, delayed);
            let (_, rows) = kernel(&plan, ops, [true, true]);
            for (r, i) in [4usize, 5].into_iter().enumerate() {
                // Op `i` alone as the carrier: its own output.
                let solo = EvalPlan::build(&a_mods, 1 << i, &b_mods, 1 << i);
                let (own, _) = kernel(&solo, ops, [false; 2]);
                for (n, (row, y)) in rows.iter().zip(&own).enumerate() {
                    #[allow(clippy::manual_clamp)] // the kernel's own max/min
                    let want = y.max(-1.0).min(1.0);
                    // Equal, −0 and +0 alike: the kernel quiets the row.
                    assert!(
                        row[r] == want,
                        "op {i} sample {n} delayed {delayed}: {} vs {want}",
                        row[r]
                    );
                }
                if op5_gain > 1.0 && i == 4 {
                    assert!(own.iter().any(|y| y.abs() > 1.0), "OP5 clamps");
                }
            }
            // Unwanted rows are left alone.
            let (_, untouched) = kernel(&plan, ops, [false; 2]);
            assert!(untouched.iter().all(|r| r == &[9.0, 9.0]));
        }
    }
}

/// `tap_vca` is the product of each op's `1 − |a| + a·row`.
#[test]
fn tap_vca_is_the_product_of_each_ops_factor() {
    let mut t = TapRows::new([true, true]);
    if let [Some(a), Some(b)] = t.rows_mut() {
        for i in 0..BLOCK_SIZE {
            (a[i], b[i]) = (sine(1000.0, i), sine(300.0, i));
        }
    }
    let r = t.rows().unwrap();
    let l = TapLane {
        rows: r,
        from: [0.5, -0.25],
        to: [0.5, -0.25],
    };
    let mut buf = [0.5f32; BLOCK_SIZE];
    tap_vca(&mut buf, &l);
    for (i, &y) in buf.iter().enumerate() {
        let want = 0.5 * (0.5 + 0.5 * r[0][i]) * (0.75 - 0.25 * r[1][i]);
        assert!((y - want).abs() < 1e-6, "{i}: {y} vs {want}");
    }
}

/// An untapped block has no rows (no 512 B fill); a tapped one starts
/// every row at zeros, and hands out only the wanted rows to write.
#[test]
fn rows_exist_only_while_an_op_is_wanted() {
    let mut none = TapRows::new([false; 2]);
    assert!(none.rows().is_none());
    assert!(none.rows_mut().iter().all(Option::is_none));
    let mut one = TapRows::new([false, true]);
    let [a, b] = one.rows_mut();
    assert!(a.is_none() && b.is_some());
    assert_eq!(one.rows(), Some(&[[0.0; BLOCK_SIZE]; 2]));
}
