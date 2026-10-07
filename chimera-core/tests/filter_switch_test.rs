//! TYPE, MODE and VOICE changes: warm-started and crossfaded over two
//! blocks, never snapped (spec § 1 B5, test 5).

mod common;

use chimera_core::block::Block;
use chimera_core::dsp::filter::ladder::ladder_bound;
use chimera_core::dsp::filter::{Filter, FilterKind, FilterMode, FilterMods, K2040Voice};
use chimera_core::modulation::ModSource;
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::preset::Sound;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;
const FS: f32 = SR as f32;
/// The change's block: the note has settled (`click`'s steady window
/// starts 40 before it).
const AT: usize = 60;
const BLOCKS: usize = AT + 70;
/// The crossfade's length, in blocks.
const FADE: usize = 2;

/// Every (TYPE, MODE, VOICE) a Sound can hold: CLEAN's eight modes, the
/// 2040's two voices, DIRTY's four modes.
fn settings() -> Vec<(FilterKind, FilterMode, K2040Voice)> {
    let mut v = Vec::new();
    for k in FilterKind::BUILT {
        for &m in k.modes() {
            if k == FilterKind::K2040 {
                v.extend(K2040Voice::ALL.map(|x| (k, m, x)));
            } else {
                v.push((k, m, K2040Voice::Rev2));
            }
        }
    }
    v
}

fn params((k, m, v): (FilterKind, FilterMode, K2040Voice), res: f32) -> FilterParams {
    at(1000.0, (k, m, v), res)
}

fn at(fc: f32, (k, m, v): (FilterKind, FilterMode, K2040Voice), res: f32) -> FilterParams {
    let mut p = FilterParams::default();
    p.set_kind(k);
    assert!(p.set_mode(m));
    (p.cutoff, p.resonance, p.voice) = (fc, res, v);
    p
}

/// A 110 Hz saw at `amp`.
fn saw(amp: f32) -> impl Fn(usize) -> f32 {
    move |i| amp * (2.0 * ((i as f32 * 110.0 / FS) % 1.0) - 1.0)
}

/// `BLOCKS` blocks of `x` through one Filter, block `b`'s params `p(b)`.
fn run(p: impl Fn(usize) -> FilterParams, x: &impl Fn(usize) -> f32) -> Vec<f32> {
    let mut f = Filter::new();
    let mut out = Vec::with_capacity(BLOCKS * BLOCK_SIZE);
    for b in 0..BLOCKS {
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| x(b * BLOCK_SIZE + i));
        f.process(&mut buf, &p(b), SR, &FilterMods::NONE);
        out.extend_from_slice(&buf);
    }
    out
}

/// The largest second difference over blocks `a..b`.
fn d2(y: &[f32], a: usize, b: usize) -> f32 {
    y[(a * BLOCK_SIZE).saturating_sub(1)..b * BLOCK_SIZE]
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .fold(0.0, f32::max)
}

/// `Tape::click`'s test on the filter's own output: the transient over the
/// three blocks from `e` against the steady state before it (on `y`) and
/// after it (on `steady`, the destination run alone).
fn click(y: &[f32], steady: &[f32], e: usize) -> Option<f32> {
    use common::sweep::{CLICK_FLOOR, CLICK_RATIO};
    let trans = d2(y, e, e + 3);
    let calm = d2(y, e - 40, e - 1).max(d2(steady, e + 20, e + 60));
    (trans > CLICK_FLOOR && trans > CLICK_RATIO * calm).then_some(trans / calm.max(1e-9))
}

fn peak(y: &[f32]) -> f32 {
    y.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

fn rms(y: &[f32]) -> f32 {
    (y.iter().map(|v| v * v).sum::<f32>() / y.len() as f32).sqrt()
}

/// A slow saw's warm start under a low FREQ, pinned: its measured worst,
/// 16.0 % (into NOTCH2), × 1.1. Each
/// state sits near its DC steady state, which the warm start seeds; from
/// rest the same changes are up to 55 % off when the fade ends.
const SLOW_WARM: f32 = 0.176;

/// Test 5: every change mid-note on a held 110 Hz saw at FREQ 1 kHz lands
/// within 10 % of the destination's own steady output from the
/// crossfade's end, and clicks nowhere. At RES 1 the ring runs free, so its
/// phase is history's: the level is held instead, once the ring has built.
/// A 27.5 Hz saw under FREQ 250 Hz, where a start from rest is still
/// settling when the fade ends, holds the warm start to `SLOW_WARM`.
#[test]
fn every_type_and_mode_change_lands_near_steady_without_a_click() {
    let all = settings();
    let mut worst = [0.0f32; 3];
    let mut fails = Vec::new();
    for (case, (fc, hz, res)) in [
        (1000.0, 110.0, 0.5),
        (1000.0, 110.0, 1.0),
        (250.0, 27.5, 0.5),
    ]
    .into_iter()
    .enumerate()
    {
        let x = move |i: usize| 0.8 * (2.0 * ((i as f32 * hz / FS) % 1.0) - 1.0);
        for &from in &all {
            for &to in &all {
                if from == to {
                    continue;
                }
                let (a, b) = (at(fc, from, res), at(fc, to, res));
                let y = run(|blk| if blk < AT { a } else { b }, &x);
                let dest = run(|_| b, &x);
                let tail = (AT + FADE) * BLOCK_SIZE;
                let what = format!("{fc} Hz, {hz} Hz saw, RES {res}: {from:?} → {to:?}");
                if let Some(r) = click(&y, &dest, AT) {
                    fails.push(format!("{what}: click {r:.1}×"));
                }
                let (off, limit) = if res < 1.0 {
                    let err = y[tail..]
                        .iter()
                        .zip(&dest[tail..])
                        .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
                    (
                        err / peak(&dest[tail..]),
                        if hz > 100.0 { 0.10 } else { SLOW_WARM },
                    )
                } else {
                    // EXP's 3080 takes about five blocks to build its ring
                    // from a warm start.
                    let held = tail + 10 * BLOCK_SIZE..tail + 20 * BLOCK_SIZE;
                    ((rms(&y[held.clone()]) / rms(&dest[held]) - 1.0).abs(), 0.10)
                };
                worst[case] = worst[case].max(off);
                if off > limit {
                    fails.push(format!("{what}: off by {:.1} %", off * 100.0));
                }
            }
        }
    }
    println!(
        "worst: {:.2} % at RES ½, level {:.2} % at RES 1, {:.2} % slow",
        worst[0] * 100.0,
        worst[1] * 100.0,
        worst[2] * 100.0
    );
    assert!(
        fails.is_empty(),
        "{} fail:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

/// TYPE scrolled a detent a block: CLEAN → 2040 → DIRTY → CLEAN. DIRTY is
/// held while CLEAN → 2040 fades and then dropped for CLEAN, which starts
/// when that fade ends; nothing clicks, and four blocks later CLEAN is
/// within 10 % of its own steady output.
#[test]
fn type_scroll_inside_a_crossfade_lands_on_the_last_without_a_click() {
    let x = saw(0.8);
    let mut p = params((FilterKind::Svf, FilterMode::Lp24, K2040Voice::Rev2), 0.5);
    let mut seq = vec![p];
    for k in [FilterKind::K2040, FilterKind::Dirty, FilterKind::Svf] {
        p.set_kind(k);
        seq.push(p);
    }
    let last = *seq.last().unwrap();
    let y = run(|b| seq[b.saturating_sub(AT - 1).min(3)], &x);
    let dest = run(|_| last, &x);
    assert!(common::clicks(&y).is_empty(), "{:?}", common::clicks(&y));
    for e in AT..AT + 4 {
        assert_eq!(click(&y, &dest, e), None, "block {e}");
    }
    // The last change starts at AT + 2 (the first fade's end).
    let from = (AT + 2 + 4) * BLOCK_SIZE;
    let err = y[from..]
        .iter()
        .zip(&dest[from..])
        .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()))
        / peak(&dest[from..]);
    println!("off CLEAN's steady by {:.2} %", err * 100.0);
    assert!(err <= 0.10, "{err}");
}

/// A TYPE change is the filter's alone: the matrix and its routes stay
/// bit for bit (spec § 1: routes are never touched).
#[test]
fn a_type_change_leaves_mod_state_byte_identical() {
    let mut s = Sound::init(EngineType::Algo);
    s.mod_state.set_route(ModSource::Env1 as usize, 0, 40);
    s.mod_state.set_route(ModSource::Lfo1 as usize, 0, -25);
    let before = s.mod_state.clone();
    for k in 0..FilterKind::BUILT.len() {
        s.params.filter.set(FilterParams::KIND, k as f32);
        assert_eq!(s.params.filter.kind(), FilterKind::BUILT[k]);
        assert!(s.mod_state.bits_eq(&before));
    }
}

/// 10 s of per-block random FREQ, RES, MODE, TYPE and VOICE on white noise
/// ±1: every sample finite and within the pinned bound of each type that
/// may sound in that block (a fade's two, held up to two blocks more).
#[test]
fn random_changes_stay_bounded() {
    // Each type's pinned output bound: CLEAN's grid bound, the 2040's
    // `R/α·OUT` on REV2, DIRTY's clamp.
    let bound = |k: FilterKind| match k {
        FilterKind::Svf => 11.43,
        FilterKind::K2040 => 2.0 + 1e-6,
        FilterKind::Dirty => 1.0,
        FilterKind::Ladder => FilterKind::Ladder
            .modes()
            .iter()
            .map(|&m| ladder_bound(m, 1.0))
            .fold(0.0, f32::max),
    };
    let mut rng = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 40) as f32 / (1u32 << 24) as f32
    };
    let mut f = Filter::new();
    let mut p = FilterParams::default();
    let mut kinds = [FilterKind::Svf; 5];
    let mut i = 0usize;
    for b in 0..(10 * SR as usize / BLOCK_SIZE) {
        let n = FilterKind::BUILT.len();
        let k = FilterKind::BUILT[(next() * n as f32) as usize % n];
        p.set_kind(k);
        let modes = k.modes();
        assert!(p.set_mode(modes[(next() * modes.len() as f32) as usize % modes.len()]));
        p.cutoff = 20.0 * 1000f32.powf(next());
        p.resonance = next();
        p.voice = K2040Voice::ALL[(next() * 2.0) as usize % 2];
        kinds.rotate_left(1);
        kinds[4] = k;
        let lim = kinds.iter().map(|&k| bound(k)).fold(0.0f32, f32::max);
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|_| {
            i += 1;
            2.0 * next() - 1.0
        });
        f.process(&mut buf, &p, SR, &FilterMods::NONE);
        for &v in &buf {
            assert!(v.is_finite() && v.abs() <= lim, "block {b}: {v} past {lim}");
        }
    }
}

/// A VOICE flip at LEVEL 1 on a full-scale saw, where the voices' ceilings
/// differ (`R/α`: 2 against 0.94 after POST), both ways, at RES 0 and
/// 0.9: no click (Task 8 review F3).
#[test]
fn voice_flip_at_level_1_has_no_click() {
    let x = saw(1.0);
    for res in [0.0, 0.9] {
        for (a, b) in [
            (K2040Voice::Rev2, K2040Voice::Exp),
            (K2040Voice::Exp, K2040Voice::Rev2),
        ] {
            let mut p = params((FilterKind::K2040, FilterMode::Lp24, a), res);
            p.level = 1.0;
            let mut q = p;
            q.voice = b;
            let y = run(|blk| if blk < AT { p } else { q }, &x);
            let c = common::clicks(&y);
            assert!(c.is_empty(), "RES {res}: {a:?} → {b:?}: {c:?}");
        }
    }
}
