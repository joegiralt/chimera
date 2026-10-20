//! The filter types' DSP (spec § Tests).

mod common;

use chimera_core::block::{Block, DiskCode};
use chimera_core::dsp::filter::clean::{clean_k, clean_k2, trim};
use chimera_core::dsp::filter::coef::{g_of, tan_accurate};
use chimera_core::dsp::filter::dirty::{
    DIRTY_STATE, DIRTY_TRIM_BP2, DIRTY_TRIM_HP2, DIRTY_TRIM_LP2, dirty_f, dirty_f_bound,
    dirty_f_raw, dirty_q, dirty_trim,
};
use chimera_core::dsp::filter::{
    Clean, Dirty, Filter, FilterKind, FilterMode, FilterMods, K2040Voice, routed_cutoff,
};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, FILTER_SPECS, FilterParams, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::Rig;
use common::codec_util::{decode, encode, filter_payload, sound_file};
use common::filter_ref::{
    OSC_BAND_DB, clean_ref, fft, g_track, grid_input, mag_db, ramp_track, rms_db, trim_ref,
};

const SR: u32 = 48_000;
const FS: f32 = SR as f32;

/// The filter's `tan` before CLEAN (#57), the baseline `tan_accurate`
/// must beat: a 5th-order Taylor series.
fn fast_tan(x: f32) -> f32 {
    let x2 = x * x;
    x * (1.0 + x2 * (1.0 / 3.0 + x2 * (2.0 / 15.0)))
}

#[test]
fn tan_accurate_beats_fast_tan_everywhere_and_is_tight_to_10k() {
    let mut f = 1000.0f32;
    while f <= 20_000.0 {
        let x = f / FS;
        let want = (core::f64::consts::PI * f64::from(x)).tan();
        let acc = (f64::from(tan_accurate(x)) - want).abs();
        let fast = (f64::from(fast_tan(core::f32::consts::PI * x)) - want).abs();
        // Both round to f32: a tie at low f is a few ulps either way.
        let ulps = 4.0 * f64::from(f32::EPSILON) * want;
        assert!(acc <= fast + ulps, "{f} Hz: {acc} vs fast_tan's {fast}");
        if f <= 10_000.0 {
            assert!(acc / want <= 1e-3, "{f} Hz: relative {}", acc / want);
        }
        if f == 15_000.0 || f == 20_000.0 {
            println!(
                "{f} Hz: tan_accurate {:.2e}, fast_tan {:.2e} relative",
                acc / want,
                fast / want
            );
        }
        f += 500.0;
    }
    // stmlib's ACCURATE itself, evaluated in f64 from `filter.h`, where its
    // highest terms weigh most.
    let stmlib = (20_000.0 / FS, 3.137_498f32);
    assert!((tan_accurate(stmlib.0) - stmlib.1).abs() < 1e-5);
}

#[test]
fn g_of_clamps_at_0_497() {
    assert_eq!(g_of(30_000.0, FS), tan_accurate(0.497));
    assert_eq!(g_of(24_000.0, FS), tan_accurate(0.497));
    let mut prev = 0.0f32;
    for hz in 20..=24_000 {
        let g = g_of(hz as f32, FS);
        assert!(g.is_finite() && g >= prev, "{hz} Hz: {g} after {prev}");
        prev = g;
    }
}

/// A saw at 0.8 of full scale, 1/218 a sample.
fn saw(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.8 * ((i % 218) as f32 / 109.0 - 1.0))
        .collect()
}

/// `(cutoff, RES)` for block b through `Clean`, and through `clean_ref`
/// with the ramps' `g`, `k` and trim per sample: max |y − y_ref| / max |y_ref|.
fn jump_error(mode: FilterMode, blocks: &[(f32, f32)]) -> f64 {
    let mut p = FilterParams::default();
    assert!(p.set_mode(mode));
    let x = saw(blocks.len() * BLOCK_SIZE);
    let mut f = Clean::new();
    let (mut y, mut g, mut k, mut t) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (b, &(fc, res)) in blocks.iter().enumerate() {
        (p.cutoff, p.resonance) = (fc, res);
        let mut buf: [f32; BLOCK_SIZE] = x[b * BLOCK_SIZE..][..BLOCK_SIZE].try_into().unwrap();
        f.process(&mut buf, &p, SR, &FilterMods::NONE);
        y.extend_from_slice(&buf);
        let (from, from_res) = blocks[b.saturating_sub(1)];
        g.extend(g_track(from.into(), fc.into(), FS.into()));
        let (k0, k1) = (clean_k(from_res), clean_k(res));
        k.extend(ramp_track(k0, k1));
        // The trim ramps across the block as `k` does, end to end.
        t.extend(ramp_track(trim_ref(mode, k0), trim_ref(mode, k1)));
    }
    let r = clean_ref(&x, &g, &k, &t, mode);
    let peak = r.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let err = y
        .iter()
        .zip(&r)
        .fold(0.0f64, |m, (&a, b)| m.max((f64::from(a) - b).abs()));
    err / peak
}

const JUMP_MODES: [FilterMode; 7] = [
    FilterMode::Lp6,
    FilterMode::Lp12,
    FilterMode::Lp24,
    FilterMode::Bp24,
    FilterMode::Hp24,
    FilterMode::Notch,
    FilterMode::Phaser,
];

#[test]
fn clean_one_block_jumps_match_the_exact_reference_at_res_0() {
    let held = |a: f32, b: f32| -> Vec<(f32, f32)> { [[(a, 0.0); 6], [(b, 0.0); 6]].concat() };
    // ENV 1 → CUTOFF at +100 % from 100 Hz, its 1 ms attack starting at
    // block 4, read per block as the voice does.
    let env: Vec<(f32, f32)> = (0..12usize)
        .map(|b| {
            let t = (b.saturating_sub(4) * BLOCK_SIZE) as f32 / (0.001 * FS);
            (routed_cutoff(100.0, t.min(1.0)), 0.0)
        })
        .collect();
    for mode in JUMP_MODES {
        for (name, cutoff) in [
            ("100 → 20k", held(100.0, 20_000.0)),
            ("20k → 100", held(20_000.0, 100.0)),
            ("ENV", env.clone()),
        ] {
            let e = jump_error(mode, &cutoff);
            assert!(e <= 1e-3, "{mode:?} {name}: {e:.2e}");
        }
    }
}

/// RES 0 → 0.9 → 0.2 in single blocks at a held 2 kHz: `k` ramps across
/// each, as `g` does.
#[test]
fn clean_res_jumps_match_the_exact_reference() {
    let blocks: Vec<(f32, f32)> = [0.0f32, 0.9, 0.2]
        .iter()
        .flat_map(|&res| [(2000.0, res); 4])
        .collect();
    for mode in JUMP_MODES {
        let e = jump_error(mode, &blocks);
        assert!(e <= 1e-3, "{mode:?}: {e:.2e}");
    }
}

#[test]
fn clean_k_law_has_its_onset_at_0_95() {
    assert_eq!(clean_k(0.0), 2.0);
    assert!(clean_k(0.95).abs() < 1e-6, "{}", clean_k(0.95));
    assert!((clean_k(1.0) + 0.10526).abs() < 1e-4, "{}", clean_k(1.0));
    // No divide nears zero, even at RES 1 (spec § CLEAN's DSP).
    let k = clean_k(1.0);
    for hz in 20..=24_000 {
        let g = g_of(hz as f32, FS);
        assert!(1.0 + k * g + g * g >= 0.997, "{hz} Hz");
    }
    // The second stage never oscillates on its own.
    assert!((clean_k2(clean_k(1.0)) - 0.17368).abs() < 1e-4);
    assert!((clean_k2(2.0) - 2.7).abs() < 1e-6);
    assert_eq!(clean_k2(-1.0), 0.1);
}

/// 1 down to `k = 0.5`, then linear to the mode's own trim at `k = 0`, held
/// below.
#[test]
fn clean_trim_ramps_with_k() {
    for mode in OSC_MODES {
        let osc = trim(mode, 0.0);
        // NOTCH1's ¼ HP keeps its ring the quietest: 0.63.
        assert!(osc > 0.1 && osc < 0.7, "{mode:?}: {osc}");
        assert_eq!(trim(mode, 0.5), 1.0);
        assert_eq!(trim(mode, 2.0), 1.0);
        assert_eq!(trim(mode, clean_k(1.0)), osc);
        assert!((trim(mode, 0.25) - (1.0 + osc) / 2.0).abs() < 1e-6);
    }
    // NOTCH2's is its output scale, `√(1 − 0.7·RES)·½`.
    assert_eq!(trim(FilterMode::Phaser, clean_k(0.0)), 0.5);
    assert!((trim(FilterMode::Phaser, clean_k(1.0)) - 0.5 * 0.3f32.sqrt()).abs() < 1e-6);
}

/// The modes that self-oscillate: all but NOTCH2 (PHASER), which never
/// will (spec § CLEAN's DSP).
const OSC_MODES: [FilterMode; 7] = [
    FilterMode::Lp6,
    FilterMode::Lp12,
    FilterMode::Lp24,
    FilterMode::Bp12,
    FilterMode::Bp24,
    FilterMode::Hp24,
    FilterMode::Notch,
];

/// `n` samples of `mode` at a held `fc` and RES, fed `x(i)`.
fn render(mode: FilterMode, fc: f32, res: f32, n: usize, x: impl Fn(usize) -> f32) -> Vec<f32> {
    let mut p = FilterParams::default();
    assert!(p.set_mode(mode));
    (p.cutoff, p.resonance) = (fc, res);
    let mut f = Clean::new();
    let mut out = Vec::with_capacity(n);
    for b in 0..n.div_ceil(BLOCK_SIZE) {
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| x(b * BLOCK_SIZE + i));
        f.process(&mut buf, &p, SR, &FilterMods::NONE);
        out.extend_from_slice(&buf);
    }
    out.truncate(n);
    out
}

fn impulse(i: usize) -> f32 {
    if i == 0 { 1.0 } else { 0.0 }
}

/// RES 1 from one impulse, read over 0.5 s after 0.25 s.
fn oscillation(mode: FilterMode, fc: f32) -> Vec<f32> {
    let n = (0.75 * FS) as usize;
    render(mode, fc, 1.0, n, impulse).split_off((0.25 * FS) as usize)
}

/// The pitch of `x`, Hz, from its upward zero crossings, interpolated.
fn pitch(x: &[f32]) -> f64 {
    let ups: Vec<f64> = x
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
        .map(|(i, w)| i as f64 + f64::from(w[0] / (w[0] - w[1])))
        .collect();
    assert!(ups.len() > 10, "no oscillation: {} crossings", ups.len());
    f64::from(FS) * (ups.len() - 1) as f64 / (ups[ups.len() - 1] - ups[0])
}

fn cents(f: f64, want: f64) -> f64 {
    1200.0 * (f / want).log2()
}

#[test]
fn clean_self_oscillates_in_band_and_tracks_1v_oct() {
    let (lo, hi) = OSC_BAND_DB;
    for mode in OSC_MODES {
        // 100 Hz to 6.4 kHz in octaves, and 8 kHz.
        let freqs = (0..7).map(|o| 100.0 * (1 << o) as f32).chain([8000.0]);
        let levels: Vec<(f32, f32, f32)> = freqs
            .map(|fc| {
                let y = oscillation(mode, fc);
                let level = rms_db(&y);
                let wobble = y
                    .chunks(4800)
                    .map(|w| (rms_db(w) - level).abs())
                    .fold(0.0f32, f32::max);
                (fc, level, wobble)
            })
            .collect();
        println!("{mode:?} RES 1 (Hz, dBFS, ±dB): {levels:.2?}");
        for (fc, level, wobble) in levels {
            assert!(wobble <= 1.0, "{mode:?} {fc} Hz: ±{wobble:.2} dB");
            assert!(
                level > lo && level < hi && level < 0.0,
                "{mode:?} {fc} Hz: {level:.2} dBFS"
            );
        }
        // FM2 (NOTE → CUTOFF) at +100 %: C2..C6 from FREQ at C4, two bases.
        for base in [261.626f32, 1250.0] {
            let mut last: Option<f64> = None;
            for note in (36..=84).step_by(12) {
                let fc = routed_cutoff(base, (note as f32 - 60.0) / 120.0);
                let want = f64::from(base) * 2f64.powf((note as f64 - 60.0) / 12.0);
                if want > 5000.0 {
                    continue;
                }
                let f = pitch(&oscillation(mode, fc));
                let c = cents(f, want);
                assert!(
                    c.abs() <= 25.0,
                    "{mode:?} base {base} note {note}: {c:.1} cents"
                );
                if let Some(l) = last {
                    let o = cents(f, l) - 1200.0;
                    assert!(
                        o.abs() <= 10.0,
                        "{mode:?} base {base} note {note}: octave {o:.1} cents"
                    );
                }
                last = Some(f);
            }
        }
    }
}

#[test]
fn clean_onset_is_res_0_95() {
    let n = (1.1 * FS) as usize;
    let tail = (1.0 * FS) as usize;
    for mode in OSC_MODES {
        for fc in [250.0, 1000.0, 4000.0] {
            let at = |res: f32| {
                let y = render(mode, fc, res, n, impulse);
                let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                let late = y[tail..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
                20.0 * (late / peak).log10()
            };
            let (below, above) = (at(0.94), at(0.96));
            assert!(below < -60.0, "{mode:?} {fc} Hz RES 0.94: {below:.1} dB");
            assert!(above > -20.0, "{mode:?} {fc} Hz RES 0.96: {above:.1} dB");
        }
    }
}

/// CLEAN's output bound on every input, at every setting (spec test 8):
/// the grid's max, 10.39 (LP1 on the ±10 impulses), × 1.1. NOTCH2, ADR
/// 0096: was 13.8, from the old PHASER's 12.55 (the impulses, all-passed).
const CLEAN_BOUND: f32 = 11.43;
/// Its SVF states' bound, the same way: 3.50 × 1.1. Each is read through
/// `sat_c1`, so the loop never sees more than 1.5. NOTCH2, ADR 0096: was
/// 4.22, from the old PHASER's 3.84.
const CLEAN_STATE_BOUND: f32 = 3.85;
/// NOTCH2's all-pass and feedback states' bound on the static grid (held
/// FREQ and RES): 5.03 × 1.1. They are linear (unit-gain all-passes,
/// `fb ≤ 0.7`), so they scale with the ±10 impulses. Not a bound under
/// per-block FREQ toggles at RES 1, which reach 6.49 (Task 5's review).
const NOTCH2_STATE_BOUND: f32 = 5.54;

/// The RES 1 jumps' bound on the 0.8 saw: their max, 0.833, × 1.1. It was
/// 0.733 before LP1 and NOTCH1 joined (NOTCH1 passes the saw's low end).
const CLEAN_JUMP_BOUND: f32 = 0.92;

/// Task 2's one-block jumps at RES 1, where self-oscillation makes the
/// reference's relative error meaningless: bounded instead (spec test 4).
/// NOTCH2 never oscillates, so it is held to the reference.
#[test]
fn clean_res_1_one_block_jumps_stay_bounded() {
    let held = |a: f32, b: f32| -> Vec<f32> { [[a; 6], [b; 6]].concat() };
    let env: Vec<f32> = (0..12usize)
        .map(|b| {
            let t = (b.saturating_sub(4) * BLOCK_SIZE) as f32 / (0.001 * FS);
            routed_cutoff(100.0, t.min(1.0))
        })
        .collect();
    let mut max = 0.0f32;
    for mode in JUMP_MODES {
        for cutoffs in [held(100.0, 20_000.0), held(20_000.0, 100.0), env.clone()] {
            if mode == FilterMode::Phaser {
                let blocks: Vec<(f32, f32)> = cutoffs.iter().map(|&fc| (fc, 1.0)).collect();
                let e = jump_error(mode, &blocks);
                assert!(e <= 1e-3, "{mode:?}: {e:.2e}");
                continue;
            }
            let mut p = FilterParams::default();
            assert!(p.set_mode(mode));
            p.resonance = 1.0;
            let x = saw(cutoffs.len() * BLOCK_SIZE);
            let mut f = Clean::new();
            for (b, &fc) in cutoffs.iter().enumerate() {
                p.cutoff = fc;
                let mut buf: [f32; BLOCK_SIZE] =
                    x[b * BLOCK_SIZE..][..BLOCK_SIZE].try_into().unwrap();
                f.process(&mut buf, &p, SR, &FilterMods::NONE);
                for v in buf {
                    assert!(
                        v.is_finite() && v.abs() <= CLEAN_JUMP_BOUND,
                        "{mode:?}: {v}"
                    );
                    max = max.max(v.abs());
                }
            }
        }
    }
    println!("RES 1 jumps: max |y| {max}");
}

#[test]
fn clean_is_bounded_on_the_grid() {
    let (mut max, mut max_state, mut max_ap) = (0.0f32, 0.0f32, 0.0f32);
    for &mode in FilterKind::Svf.modes() {
        let mut mode_max = 0.0f32;
        for fi in 0..24 {
            let fc = 20.0 * 1000f32.powf(fi as f32 / 23.0);
            for ri in 0..=10 {
                let mut p = FilterParams::default();
                assert!(p.set_mode(mode));
                (p.cutoff, p.resonance) = (fc, ri as f32 / 10.0);
                for which in 0..5 {
                    let mut f = Clean::new();
                    for b in 0..20 {
                        let mut buf: [f32; BLOCK_SIZE] =
                            core::array::from_fn(|i| grid_input(which, b * BLOCK_SIZE + i));
                        f.process(&mut buf, &p, SR, &FilterMods::NONE);
                        for v in buf {
                            assert!(v.is_finite(), "{mode:?} {fc} RES {ri}: {v}");
                            mode_max = mode_max.max(v.abs());
                        }
                        let states = f.states();
                        assert!(states.iter().all(|s| s.is_finite()));
                        let m = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                        max_state = max_state.max(m(&states[..4]));
                        max_ap = max_ap.max(m(&states[4..]));
                    }
                }
            }
        }
        println!("grid {mode:?}: max |y| {mode_max}");
        max = max.max(mode_max);
    }
    println!("grid: max |y| {max}, max |state| {max_state}, NOTCH2's {max_ap}");
    assert!(max <= CLEAN_BOUND, "{max}");
    assert!(max_state <= CLEAN_STATE_BOUND, "{max_state}");
    assert!(max_ap <= NOTCH2_STATE_BOUND, "{max_ap}");
}

#[test]
fn clean_recovers_from_a_nan_block() {
    for &mode in FilterKind::Svf.modes() {
        let mut p = FilterParams::default();
        assert!(p.set_mode(mode));
        (p.cutoff, p.resonance) = (1000.0, 0.5);
        let mut f = Clean::new();
        f.process(&mut [f32::NAN; BLOCK_SIZE], &p, SR, &FilterMods::NONE);
        // Cleared: silence in, silence out.
        let mut quiet = [0.0f32; BLOCK_SIZE];
        f.process(&mut quiet, &p, SR, &FilterMods::NONE);
        assert!(quiet.iter().all(|&v| v == 0.0), "{mode:?}: {quiet:?}");
        let x = saw(2 * BLOCK_SIZE);
        let mut b = [0.0f32; BLOCK_SIZE];
        for c in x.chunks(BLOCK_SIZE) {
            b.copy_from_slice(c);
            f.process(&mut b, &p, SR, &FilterMods::NONE);
        }
        assert!(b.iter().all(|v| v.is_finite()), "{mode:?}: {b:?}");
        assert!(b.iter().any(|&v| v != 0.0), "{mode:?}");
    }
}

/// A voice at `res` and 500 Hz, C4 held 0.5 s, run until its engine ends:
/// every sample, and the voice, idle again.
fn ring_out(engine: EngineType, res: f32) -> (Vec<f32>, Rig, ParamSnapshot) {
    let mut p = ParamSnapshot::for_engine(engine);
    (p.filter.cutoff, p.filter.resonance) = (500.0, res);
    let mut r = Rig::new(SR);
    r.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &p);
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    for blk in 0..20_000 {
        if blk == 375 {
            r.note_off();
        }
        r.render(&mut b, &p, &ModState::new());
        out.extend_from_slice(&b);
        if !r.is_active() {
            // One more block: the idle voice's silence.
            r.render(&mut b, &p, &ModState::new());
            out.extend_from_slice(&b);
            return (out, r, p);
        }
    }
    panic!("{engine:?}: never ended");
}

/// ALGO and MODAL INIT at RES 1: the amp gate (ADR 0110) has taken the ring
/// down when the engine ends (#374), and what is left fades over one block,
/// so the end steps no more than the ring itself does.
#[test]
fn a_voice_ending_mid_oscillation_fades_without_a_click() {
    for engine in [EngineType::Algo, EngineType::Modal] {
        let (y, ..) = ring_out(engine, 1.0);
        let step = |w: &[f32]| {
            w.windows(2)
                .map(|p| (p[1] - p[0]).abs())
                .fold(0.0f32, f32::max)
        };
        let n = y.len();
        // The ring before the end, and the end: the fade block and silence.
        let (ring, end) = (
            step(&y[n - 40 * BLOCK_SIZE..n - 2 * BLOCK_SIZE]),
            step(&y[n - 2 * BLOCK_SIZE - 1..]),
        );
        println!("{engine:?}: ring step {ring:.4}, end step {end:.4}");
        assert!(
            end <= ring * 1.1 + 1e-5,
            "{engine:?}: the end steps {end}, the ring {ring}"
        );
    }
}

/// The same, with the voice stolen (`kill`) in the block its engine ends
/// in: the steal fade stops with the engine, half way down, and the end
/// fade takes it the rest of the way.
#[test]
fn an_engine_ending_inside_a_steal_fade_fades_too() {
    for engine in [EngineType::Algo, EngineType::Modal] {
        // The block the engine ends in: the last but the idle one.
        let (y, ..) = ring_out(engine, 1.0);
        let end = y.len() / BLOCK_SIZE - 2;
        let mut p = ParamSnapshot::for_engine(engine);
        (p.filter.cutoff, p.filter.resonance) = (500.0, 1.0);
        let mut r = Rig::new(SR);
        r.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &p);
        let (mut out, mut b) = (Vec::new(), [0.0f32; BLOCK_SIZE]);
        for blk in 0..=end + 1 {
            if blk == 375 {
                r.note_off();
            }
            if blk == end {
                r.kill();
            }
            r.render(&mut b, &p, &ModState::new());
            out.extend_from_slice(&b);
        }
        assert!(!r.is_active(), "{engine:?}: still active");
        let step = |w: &[f32]| {
            w.windows(2)
                .map(|p| (p[1] - p[0]).abs())
                .fold(0.0f32, f32::max)
        };
        let n = out.len();
        let (ring, end) = (
            step(&out[n - 40 * BLOCK_SIZE..n - 2 * BLOCK_SIZE]),
            step(&out[n - 2 * BLOCK_SIZE - 1..]),
        );
        println!("{engine:?} stolen: ring step {ring:.4}, end step {end:.4}");
        assert!(
            end <= ring + 1e-5,
            "{engine:?}: the end steps {end}, the ring {ring}"
        );
    }
}

/// A voice that ended self-oscillating starts its next note from rest: bit
/// for bit as a voice that ended at RES 0 does (the engine before the
/// filter ran the same either way), and on ALGO as a fresh voice does.
/// MODAL's engine itself doesn't restart bit for bit as a fresh one.
#[test]
fn a_voice_reused_after_oscillation_starts_from_rest() {
    let first_block = |r: &mut Rig, p: &ParamSnapshot| {
        let mut b = [0.0f32; BLOCK_SIZE];
        r.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), p);
        r.render(&mut b, p, &ModState::new());
        b.map(f32::to_bits)
    };
    for engine in [EngineType::Algo, EngineType::Modal] {
        for next_res in [1.0, 0.0] {
            let (_, mut rang, mut p) = ring_out(engine, 1.0);
            let (_, mut quiet, _) = ring_out(engine, 0.0);
            p.filter.resonance = next_res;
            let got = first_block(&mut rang, &p);
            assert_eq!(
                got,
                first_block(&mut quiet, &p),
                "{engine:?} next RES {next_res}"
            );
            if engine == EngineType::Algo {
                assert_eq!(
                    got,
                    first_block(&mut Rig::new(SR), &p),
                    "fresh, next RES {next_res}"
                );
            }
        }
    }
}

#[test]
fn default_mode_is_in_modes_and_mode_stays_in_modes() {
    assert_eq!(FilterKind::Svf.default_mode(), FilterMode::Lp24);
    assert_eq!(
        FilterParams::default().mode(),
        FilterKind::Svf.default_mode()
    );
    // MODE's static default is the index of CLEAN's.
    let spec = FILTER_SPECS
        .iter()
        .find(|s| s.id == FilterParams::MODE)
        .unwrap();
    assert_eq!(
        FilterKind::Svf.modes()[spec.default as usize],
        FilterKind::Svf.default_mode()
    );
    for k in FilterKind::BUILT {
        let modes = k.modes();
        assert!(modes.contains(&k.default_mode()), "{k:?}");
        assert_eq!(k.mode_labels().len(), modes.len(), "{k:?}");
        let mut p = FilterParams::default();
        p.set_kind(k);
        for v in 0..=7 {
            p.write(FilterParams::MODE, v as f32);
            assert!(modes.contains(&p.mode()), "{k:?} write {v}");
        }
        for code in 0..=255u8 {
            let mut q = p;
            q.set_enum_code(FilterParams::MODE, code);
            assert!(modes.contains(&q.mode()), "{k:?} code {code}");
        }
    }
}

#[test]
fn clean_labels_name_what_they_do() {
    let k = FilterKind::Svf;
    assert_eq!(
        k.mode_labels(),
        ["LP1", "LP2", "LP4", "BP2", "BP4", "HP4", "NOTCH1", "NOTCH2"]
    );
    for (&m, &l) in k.modes().iter().zip(k.mode_labels()) {
        assert_eq!(k.mode_label(m), l);
    }
    assert_eq!(k.mode_label(FilterMode::Lp6), "LP1");
    assert_eq!(k.mode_label(FilterMode::Notch), "NOTCH1");
    assert_eq!(k.mode_label(FilterMode::Phaser), "NOTCH2");
    // Labels only: the disk keeps its idents.
    assert_eq!(FilterMode::Lp6.disk_ident(), "LP6");
    assert_eq!(FilterMode::Notch.disk_ident(), "NOTCH");
    assert_eq!(FilterMode::Phaser.disk_ident(), "PHASER");
}

/// `mode`'s gain at `f`, dB, at a held `fc` and RES: a unit sine, read over
/// 2 s after 0.25 s (a whole number of periods at any half-hertz `f`).
fn gain_db(mode: FilterMode, fc: f32, res: f32, f: f32) -> f32 {
    let (settle, n) = ((0.25 * FS) as usize, (2.0 * FS) as usize);
    let w = core::f64::consts::TAU * f64::from(f) / f64::from(FS);
    let y = render(mode, fc, res, settle + n, |i| (w * i as f64).sin() as f32);
    mag_db(&y[settle..], f, FS)
}

/// The digital frequency of the analog prototype's `w` × FREQ: CLEAN's `g`
/// prewarps FREQ, so `tan(π f/fs) = w·g`.
fn proto_hz(fc: f32, w: f32) -> f32 {
    FS / core::f32::consts::PI * (w * g_of(fc, FS)).atan()
}

/// RES 0 (spec § Modes against the Cascadia). Slopes are read over 4..8 ×
/// FREQ on the prototype's axis: over 2..4 × the cascade's second stage
/// (`k₂` = 2.7) is not yet at its asymptote, and LP4 reads −20.1 dB/oct.
#[test]
fn clean_responses_match_the_prototype() {
    for fc in [250.0f32, 1000.0, 4000.0] {
        let g = |mode, f| gain_db(mode, fc, 0.0, f);
        for (mode, want, tol) in [
            (FilterMode::Lp6, -6.0, 1.5),
            (FilterMode::Lp12, -12.0, 2.0),
            (FilterMode::Lp24, -24.0, 3.0),
        ] {
            let slope = g(mode, proto_hz(fc, 8.0)) - g(mode, proto_hz(fc, 4.0));
            println!("{fc} Hz {mode:?}: {slope:.2} dB/oct");
            assert!((slope - want).abs() <= tol, "{fc} Hz {mode:?}: {slope:.2}");
        }
        // HP4 the mirror image, over 1/8..1/4 × FREQ.
        let hp = g(FilterMode::Hp24, proto_hz(fc, 0.25)) - g(FilterMode::Hp24, proto_hz(fc, 0.125));
        println!("{fc} Hz HP24: {hp:.2} dB/oct");
        assert!((hp - 24.0).abs() <= 3.0, "{fc} Hz HP24: {hp:.2}");
        // LP1 crosses −3 dB within FREQ ± 3 %.
        let cut = -3.0103;
        let (lo, hi) = (g(FilterMode::Lp6, fc * 0.97), g(FilterMode::Lp6, fc * 1.03));
        assert!(lo > cut && hi < cut, "{fc} Hz LP1: {lo:.3}, {hi:.3}");
        // BP2 and BP4 peak within FREQ ± 3 %.
        for mode in [FilterMode::Bp12, FilterMode::Bp24] {
            let (lo, at, hi) = (g(mode, fc * 0.97), g(mode, fc), g(mode, fc * 1.03));
            assert!(at > lo && at > hi, "{fc} Hz {mode:?}: {lo}, {at}, {hi}");
        }
        // NOTCH1: its zero at 2 × FREQ (digitally `tan(π f/fs) = 2g`),
        // found within ± 3 % and ≥ 30 dB deep against DC's 0 dB.
        let fz = proto_hz(fc, 2.0);
        let sweep: Vec<(f32, f32)> = (-5..=5)
            .map(|i| {
                let f = fz * (1.0 + 0.01 * i as f32);
                (f, g(FilterMode::Notch, f))
            })
            .collect();
        let (f_min, _) = sweep
            .iter()
            .copied()
            .fold((0.0, f32::INFINITY), |m, x| if x.1 < m.1 { x } else { m });
        let depth = g(FilterMode::Notch, fz);
        println!("{fc} Hz NOTCH1: zero {fz:.1} Hz, {depth:.1} dB; min at {f_min:.1}");
        assert!(
            (f_min / fz - 1.0).abs() <= 0.03,
            "{fc} Hz NOTCH1: min at {f_min}"
        );
        assert!(depth <= -30.0, "{fc} Hz NOTCH1: {depth:.1} dB at the zero");
        // FREQ itself an 8.6 dB dip at Q 0.
        let at = g(FilterMode::Notch, fc);
        assert!((at + 8.6).abs() <= 0.5, "{fc} Hz NOTCH1 at FREQ: {at:.2}");
    }
}

/// LP1 is `lp + bp`: the one-pole at Q 0, and Q peaks it at FREQ by `√2/k`.
#[test]
fn lp1_at_q0_is_the_one_pole_and_q_peaks_it() {
    let fc = 1000.0;
    let at0 = gain_db(FilterMode::Lp6, fc, 0.0, fc);
    assert!((at0 + 3.01).abs() <= 0.2, "RES 0: {at0:.3} dB");
    let k = clean_k(0.5);
    let want = 20.0 * (2f32.sqrt() / k).log10();
    let at = gain_db(FilterMode::Lp6, fc, 0.5, fc);
    assert!(
        (at - want).abs() <= 0.5,
        "RES 0.5: {at:.2} dB, want {want:.2}"
    );
}

/// Broadband loudness, dB: the level of pink noise through a power
/// response, the mean of `|H|²` over log frequency, 20 Hz – 20 kHz. White
/// noise's would weight the top octave as all the others together, where
/// the feedback's one-sample delay sets the level, not the notches.
fn pink_db(p: &[f64]) -> f64 {
    let n = 2000;
    let mean = (0..n)
        .map(|i| {
            let f = 20.0 * 1000f64.powf(i as f64 / (n - 1) as f64);
            p[(f * WELCH_N as f64 / f64::from(FS)).round() as usize]
        })
        .sum::<f64>()
        / n as f64;
    10.0 * mean.log10()
}

/// NOTCH2's segment length: a 2^15-point Welch average.
const WELCH_N: usize = 1 << 15;

/// White noise, ±1: sample `i` is splitmix64(i)'s top 24 bits. (The bound
/// grid's hash noise is 7 dB from flat across the band.)
fn white(i: usize) -> f32 {
    let mut z = (i as u64)
        .wrapping_add(1)
        .wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((z ^ (z >> 31)) >> 40) as f32 / (1u32 << 23) as f32 - 1.0
}

/// NOTCH2's power response `|H|²` per bin (`k·fs/WELCH_N`, 0..=N/2) at
/// FREQ `fc` and RES `res`: white noise in, the H1 estimate `|Pxy|/Pxx` of
/// a Welch average (Hann, half overlap, 8 segments) after 0.1 s.
fn notch2_power(fc: f32, res: f32) -> Vec<f64> {
    let (settle, segs) = ((0.1 * FS) as usize, 8);
    let n = settle + WELCH_N / 2 * (segs + 1);
    let noise = white;
    let y = render(FilterMode::Phaser, fc, res, n, noise);
    let hann = |i: usize| 0.5 - 0.5 * (core::f64::consts::TAU * i as f64 / WELCH_N as f64).cos();
    let bins = WELCH_N / 2 + 1;
    let (mut pxx, mut pxy) = (vec![0.0f64; bins], vec![(0.0f64, 0.0f64); bins]);
    for s in 0..segs {
        let at = settle + s * WELCH_N / 2;
        let spec = |f: &dyn Fn(usize) -> f64| {
            let mut re: Vec<f64> = (0..WELCH_N).map(|i| f(at + i) * hann(i)).collect();
            let mut im = vec![0.0; WELCH_N];
            fft(&mut re, &mut im);
            (re, im)
        };
        let (xr, xi) = spec(&|i| f64::from(noise(i)));
        let (yr, yi) = spec(&|i| f64::from(y[i]));
        for k in 0..bins {
            pxx[k] += xr[k] * xr[k] + xi[k] * xi[k];
            // conj(X)·Y
            pxy[k].0 += xr[k] * yr[k] + xi[k] * yi[k];
            pxy[k].1 += xr[k] * yi[k] - xi[k] * yr[k];
        }
    }
    (0..bins)
        .map(|k| (pxy[k].0 * pxy[k].0 + pxy[k].1 * pxy[k].1) / (pxx[k] * pxx[k]))
        .collect()
}

fn bin_hz(k: f64) -> f64 {
    k * f64::from(FS) / WELCH_N as f64
}

/// The bin's vertex: a parabola through `p[m − 1..=m + 1]`, in bins.
fn vertex(p: &[f64], m: usize) -> f64 {
    let (a, b, c) = (p[m - 1], p[m], p[m + 1]);
    m as f64 + 0.5 * (a - c) / (a - 2.0 * b + c)
}

/// The minima ≥ 25 dB under the response's median (10 Hz – 22 kHz), Hz:
/// one per run of bins below that, each at its parabola's vertex on `|H|²`,
/// which is quadratic about an exact zero.
fn notch2_notches(p: &[f64]) -> Vec<f64> {
    let span = (10.0 * WELCH_N as f32 / FS) as usize..(22_000.0 * WELCH_N as f32 / FS) as usize;
    let mut db: Vec<f64> = p[span.clone()].iter().map(|v| 10.0 * v.log10()).collect();
    db.sort_by(f64::total_cmp);
    let floor = db[db.len() / 2] - 25.0;
    let deep = |k: usize| 10.0 * p[k].log10() < floor;
    let mut found = Vec::new();
    let mut k = span.start;
    while k < span.end {
        if !deep(k) {
            k += 1;
            continue;
        }
        let start = k;
        while k < span.end && deep(k) {
            k += 1;
        }
        let m = (start..k).min_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap();
        found.push(bin_hz(vertex(p, m)));
    }
    found
}

/// The response's peak between the two notches, Hz: the vertex of a
/// parabola on dB.
fn notch2_peak(p: &[f64]) -> f64 {
    let n = notch2_notches(p);
    assert_eq!(n.len(), 2, "{n:?}");
    let (lo, hi) = (
        (n[0] * WELCH_N as f64 / f64::from(FS)) as usize + 2,
        (n[1] * WELCH_N as f64 / f64::from(FS)) as usize - 1,
    );
    let m = (lo..hi).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap();
    let db: Vec<f64> = p[m - 1..=m + 1].iter().map(|v| 10.0 * v.log10()).collect();
    bin_hz(m as f64 + vertex(&db, 1) - 1.0)
}

/// The digital notches at FREQ `fc`: `(fs/π)·atan(g·tan(π/8))` and
/// `(fs/π)·atan(g·tan(3π/8))`, the analog 0.414 × and 2.414 × FREQ.
fn notch2_want(fc: f32) -> [f64; 2] {
    let g = f64::from(g_of(fc, FS));
    let pi = core::f64::consts::PI;
    [pi / 8.0, 3.0 * pi / 8.0].map(|a| f64::from(FS) / pi * (g * a.tan()).atan())
}

/// Two exact notches at every Q (spec § CLEAN's DSP, test 6): the output is
/// `(1 + A)·u`, zero where the four all-passes turn −180° and −540°.
#[test]
fn notch2_has_two_exact_notches_that_sweep_in_octaves() {
    for res in [0.0f32, 0.5, 1.0] {
        let mut last: Option<[f64; 2]> = None;
        for fc in [200.0f32, 400.0, 800.0, 1600.0] {
            let got = notch2_notches(&notch2_power(fc, res));
            let want = notch2_want(fc);
            println!("NOTCH2 {fc} Hz Q {res}: notches {got:.2?}, want {want:.2?}");
            assert_eq!(got.len(), 2, "{fc} Hz Q {res}: {got:?}");
            for (g, w) in got.iter().zip(want) {
                assert!((g / w - 1.0).abs() <= 0.03, "{fc} Hz Q {res}: {g} vs {w}");
            }
            if let Some(l) = last {
                for (g, l) in got.iter().zip(l) {
                    let oct = (g / l).log2();
                    assert!((oct - 1.0).abs() <= 0.05, "{fc} Hz Q {res}: {oct:.3} oct");
                }
            }
            last = Some([got[0], got[1]]);
        }
    }
}

/// The `√(1 − fb)` law's ceiling on the peak's rise: `10·log10(1/0.3)` =
/// 5.229 dB at `fb` 0.7, which the spec rounds to 5.2.
const NOTCH2_RISE_DB: f32 = 5.23;

/// Q is feedback, `fb = 0.7·RES`, scaled by `√(1 − fb)`: the peak between
/// the notches rises with Q, but the loudness (`pink_db`) holds to 2 kHz.
#[test]
fn notch2_q_raises_the_peaks_but_not_the_loudness() {
    for fc in [200.0f32, 400.0, 800.0, 1600.0, 2000.0, 8000.0] {
        // Found on the Welch spectrum, read by a sine at the nearest half
        // hertz: the peak is broad, and the bound is within 1e-3 dB of it.
        let peaks: Vec<f32> = [0.0f32, 0.5, 1.0]
            .iter()
            .map(|&res| {
                let hz = (notch2_peak(&notch2_power(fc, res)) * 2.0).round() / 2.0;
                gain_db(FilterMode::Phaser, fc, res, hz as f32)
            })
            .collect();
        println!("NOTCH2 {fc} Hz peak at Q 0, 0.5, 1: {peaks:.4?} dB");
        assert!(
            peaks[0] < peaks[1] && peaks[1] < peaks[2],
            "{fc} Hz: {peaks:?}"
        );
        let rise = peaks[2] - peaks[0];
        assert!(rise <= NOTCH2_RISE_DB, "{fc} Hz: +{rise:.4} dB");
    }
    for fc in [200.0f32, 400.0, 800.0, 1600.0, 2000.0] {
        let level = |res: f32| pink_db(&notch2_power(fc, res));
        let at0 = level(0.0);
        let levels: Vec<f64> = (0..=4).map(|q| level(q as f32 / 4.0) - at0).collect();
        println!("NOTCH2 {fc} Hz pink level re Q 0, Q 0..1: {levels:.2?} dB");
        for (q, l) in levels.iter().enumerate() {
            assert!(l.abs() <= 1.5, "{fc} Hz Q {}: {l:+.2} dB", q as f32 / 4.0);
        }
    }
}

/// NOTCH2 never self-oscillates (owner: as the Cascadia's PHZR): at RES 1
/// an impulse dies 80 dB within 0.5 s at every FREQ of the bound grid. A
/// loop that runs away overflows, and the NaN reset would silence it, so
/// every sample is held finite and the peak to the impulse's own 1. RES 1.5
/// and −3, which no knob reaches but a route or tap past the clamp could,
/// hold `fb` to ±0.7 as well.
#[test]
fn notch2_never_self_oscillates() {
    let mut worst = f32::NEG_INFINITY;
    for fi in 0..24 {
        for res in [1.0, 1.5, -3.0] {
            let fc = 20.0 * 1000f32.powf(fi as f32 / 23.0);
            worst = worst.max(notch2_impulse_decay(fc, res));
        }
    }
    println!("NOTCH2 RES 1, 1.5, −3: the impulse's level after 0.5 s, at worst {worst:.1} dB");
}

/// NOTCH2's impulse at `fc` and `res`: finite, bounded, and
/// 80 dB down after 0.5 s; that level, dB.
fn notch2_impulse_decay(fc: f32, res: f32) -> f32 {
    let y = render(FilterMode::Phaser, fc, res, (0.6 * FS) as usize, impulse);
    assert!(y.iter().all(|v| v.is_finite()), "{fc} Hz");
    let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    // `|1 + A| ≤ 2` times the trim `½√(1 − fb)`: 1 at fb ≥ 0, √1.7 at −0.7.
    let most = if res >= 0.0 { 1.0 } else { 1.7f32.sqrt() };
    assert!(peak <= most, "{fc} Hz RES {res}: peak {peak}");
    let late = y[(0.5 * FS) as usize..]
        .iter()
        .fold(0.0f32, |m, v| m.max(v.abs()));
    let db = 20.0 * (late / peak).log10();
    assert!(db < -80.0, "{fc} Hz RES {res}: {db:.1} dB after 0.5 s");
    db
}

/// NOTCH2's tail counts as ringing (`rings`), so a voice ending on it fades
/// rather than cuts: an impulse at 20 Hz, RES 1, still rings 0.25 s on,
/// held only in the all-passes and the feedback; `reset` silences it.
#[test]
fn notch2_rings_until_reset() {
    let mut p = FilterParams::default();
    assert!(p.set_mode(FilterMode::Phaser));
    (p.cutoff, p.resonance) = (20.0, 1.0);
    let mut f = Clean::new();
    for b in 0..(0.25 * FS) as usize / BLOCK_SIZE {
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| impulse(b * BLOCK_SIZE + i));
        f.process(&mut buf, &p, SR, &FilterMods::NONE);
    }
    let s = f.states();
    assert!(s[..4].iter().all(|&v| v == 0.0), "{s:?}");
    assert!(f.rings(), "{s:?}");
    f.reset();
    assert!(!f.rings());
}

/// The one-sample feedback delay slides the peak below FREQ at high FREQ
/// (spec: accepted, and pinned here): at 8 kHz, Q 1.
const NOTCH2_PEAK_AT_8K: f64 = 0.834;

#[test]
fn notch2_peak_slides_below_freq_as_pinned() {
    let hz = notch2_peak(&notch2_power(8000.0, 1.0));
    let ratio = hz / 8000.0;
    println!("NOTCH2 8 kHz Q 1: peak {hz:.1} Hz, {ratio:.4} × FREQ");
    assert!(
        (ratio / NOTCH2_PEAK_AT_8K - 1.0).abs() <= 0.02,
        "{ratio:.4} × FREQ"
    );
}

// DIRTY, the Chamberlin SVF at ×2 (spec § 4, plan Task 9).

/// DIRTY in `mode` at a held setting.
fn dirty(mode: FilterMode, fc: f32, res: f32) -> FilterParams {
    let mut p = FilterParams::default();
    p.set_kind(FilterKind::Dirty);
    assert!(p.set_mode(mode), "{mode:?}");
    (p.cutoff, p.resonance) = (fc, res);
    p
}

/// `n` samples through a fresh `Filter` at `p`, fed `x(i)`: any TYPE.
fn render_filter(p: &FilterParams, n: usize, x: impl Fn(usize) -> f32) -> Vec<f32> {
    let mut f = Filter::new();
    let mut out = Vec::with_capacity(n);
    for b in 0..n.div_ceil(BLOCK_SIZE) {
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| x(b * BLOCK_SIZE + i));
        f.process(&mut buf, p, SR, &FilterMods::NONE);
        out.extend_from_slice(&buf);
    }
    out.truncate(n);
    out
}

const DIRTY_MODES: [FilterMode; 4] = [
    FilterMode::Lp12,
    FilterMode::Bp12,
    FilterMode::Hp12,
    FilterMode::Notch,
];

/// DIRTY's gain at `f`, dB re its input: a 0.5 sine, read over 1 s after
/// 0.25 s (whole periods at any whole-hertz `f`).
fn dirty_gain_db(mode: FilterMode, fc: f32, res: f32, f: f32) -> f32 {
    let (settle, n) = ((0.25 * FS) as usize, (1.0 * FS) as usize);
    let w = core::f64::consts::TAU * f64::from(f) / f64::from(FS);
    let y = render_filter(&dirty(mode, fc, res), settle + n, |i| {
        0.5 * (w * i as f64).sin() as f32
    });
    mag_db(&y[settle..], f, FS) + 6.0206
}

/// Where BP2 peaks at RES 0, × FREQ, at 250 Hz, 1 kHz and 4 kHz: the
/// Chamberlin's own. Its resonance sits at `2·asin(f/(2√(1 − fq/2)))` at
/// ×2, so at RES 0 (`q = 2`) it rides up with `f`: +0.8 and +3.3 % on the
/// transfer function, and +15 % at 4 kHz as measured (the function says 13;
/// a Q ½ peak is flat to 0.01 dB across ±3 %). On FREQ from RES 0.95, where
/// `q = 0` (the pitch test).
const DIRTY_BP_PEAK: [(f32, f32); 3] = [(250.0, 1.0082), (1000.0, 1.0332), (4000.0, 1.153)];

/// RES 0 (spec test 1): LP2 and HP2 ±12 ± 2 dB/oct, BP2's peak where the
/// Chamberlin puts it (`DIRTY_BP_PEAK`) ± 3 %, NOTCH1's zero at FREQ ± 5 %
/// up to fs/8. The slopes are read over 4..8 × FREQ (HP2 mirrored, ⅛..¼),
/// as CLEAN's are (Task 4's ruling): at RES 0 the two-pole has Q ½, which
/// reads −10.6 dB/oct over 2..4 × on the analog prototype too. LP2 is
/// read to 2.5 kHz, where 8 × is still under Nyquist.
#[test]
fn dirty_responses() {
    for fc in [250.0f32, 1000.0, 2500.0, 4000.0] {
        let g = |mode, f: f32| dirty_gain_db(mode, fc, 0.0, f.round());
        if 8.0 * fc < FS / 2.0 {
            let lp = g(FilterMode::Lp12, 8.0 * fc) - g(FilterMode::Lp12, 4.0 * fc);
            println!("DIRTY {fc} Hz: LP2 {lp:.2} dB/oct");
            assert!((lp + 12.0).abs() <= 2.0, "{fc} Hz LP2: {lp:.2}");
        }
        let hp = g(FilterMode::Hp12, 0.25 * fc) - g(FilterMode::Hp12, 0.125 * fc);
        println!("DIRTY {fc} Hz: HP2 {hp:.2} dB/oct");
        assert!((hp - 12.0).abs() <= 2.0, "{fc} Hz HP2: {hp:.2}");
    }
    for (fc, at) in DIRTY_BP_PEAK {
        let g = |f: f32| dirty_gain_db(FilterMode::Bp12, fc, 0.0, (fc * at * f).round());
        let (lo, at_peak, hi) = (g(0.97), g(1.0), g(1.03));
        assert!(
            at_peak > lo && at_peak > hi,
            "{fc} Hz BP2: {lo}, {at_peak}, {hi}"
        );
    }
    // NOTCH1 at FREQ, 250 Hz to fs/8 in octaves, and fs/8 itself.
    for fc in [250.0f32, 500.0, 1000.0, 2000.0, 4000.0, FS / 8.0] {
        let sweep: Vec<(f32, f32)> = (-10..=10)
            .map(|i| {
                let f = (fc * (1.0 + 0.01 * i as f32)).round();
                (f, dirty_gain_db(FilterMode::Notch, fc, 0.0, f))
            })
            .collect();
        let (f_min, depth) = sweep
            .iter()
            .copied()
            .fold((0.0, f32::INFINITY), |m, x| if x.1 < m.1 { x } else { m });
        println!("DIRTY {fc} Hz NOTCH1: min {depth:.1} dB at {f_min}");
        assert!(
            (f_min / fc - 1.0).abs() <= 0.05,
            "{fc} Hz NOTCH1: min at {f_min}"
        );
        assert!(depth <= -20.0, "{fc} Hz NOTCH1: {depth:.1} dB");
    }
}

/// Where `dirty_f` starts to clamp at `res`: the FREQ, Hz, by bisection;
/// `None` if not below 20 kHz.
fn dirty_ceiling(res: f32) -> Option<f32> {
    let q = dirty_q(res);
    let binds = |fc: f32| dirty_f(fc, FS, q) < dirty_f_raw(fc, FS);
    if !binds(20_000.0) {
        return None;
    }
    let (mut lo, mut hi) = (20.0f32, 20_000.0f32);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if binds(mid) { hi = mid } else { lo = mid }
    }
    Some(hi)
}

/// Jury's bound at ×2 sets a 12.35 kHz ceiling at RES 0, rising with RES
/// and gone below 20 kHz from the onset (spec § 4).
#[test]
fn dirty_ceiling_is_12k_at_res_0_and_rises_with_res() {
    let at0 = dirty_ceiling(0.0).expect("binds at RES 0");
    println!("DIRTY ceiling at RES 0: {at0:.0} Hz");
    assert!((at0 / 12_350.0 - 1.0).abs() <= 0.01, "{at0}");
    // Below it `f` is FREQ's own, above it the bound.
    assert_eq!(dirty_f(0.99 * at0, FS, 2.0), dirty_f_raw(0.99 * at0, FS));
    assert_eq!(dirty_f(1.01 * at0, FS, 2.0), dirty_f_bound(2.0));
    let mut last = at0;
    for res in [0.25, 0.5, 0.75, 0.9] {
        let c = dirty_ceiling(res);
        println!("DIRTY ceiling at RES {res}: {c:?}");
        if let Some(c) = c {
            assert!(c > last, "RES {res}: {c} after {last}");
            last = c;
        }
    }
    for res in [0.95, 1.0] {
        assert_eq!(dirty_ceiling(res), None, "RES {res}");
    }
    // The bound is Jury's, × 0.95: at it `f² + 2fq < 4` with room.
    for q in [2.0f32, 1.0, 0.1, 0.0, -0.105] {
        let f = dirty_f_bound(q);
        assert!(f * f + 2.0 * f * q < 4.0, "q {q}: f {f}");
    }
}

/// DIRTY's output bound and its states' clamps on the grid (spec test 8),
/// LEVEL 0, ½ and 1; stable at RES 0 to the top (the silence after a saw
/// dies away, which a missing `f` clamp breaks above 12.35 kHz); and 10 s of
/// random per-block FREQ, RES, MODE and LEVEL.
#[test]
fn dirty_is_bounded_everywhere() {
    let (mut max, mut max_state) = (0.0f32, 0.0f32);
    let mut check = |buf: &[f32], f: &Dirty, what: &dyn Fn() -> String| {
        for &v in buf {
            assert!(v.is_finite() && v.abs() <= 1.0, "{}: {v}", what());
            max = max.max(v.abs());
        }
        for s in f.states() {
            assert!(s.is_finite() && s.abs() <= DIRTY_STATE, "{}: {s}", what());
            max_state = max_state.max(s.abs());
        }
    };
    for mode in DIRTY_MODES {
        for fi in 0..24 {
            let fc = 20.0 * 1000f32.powf(fi as f32 / 23.0);
            for ri in 0..=10 {
                for level in [0.0, 0.5, 1.0] {
                    let mut p = dirty(mode, fc, ri as f32 / 10.0);
                    p.level = level;
                    for which in 0..5 {
                        let mut f = Dirty::new();
                        for b in 0..20 {
                            let mut buf: [f32; BLOCK_SIZE] =
                                core::array::from_fn(|i| grid_input(which, b * BLOCK_SIZE + i));
                            f.process(&mut buf, &p, SR, &FilterMods::NONE);
                            check(&buf, &f, &|| {
                                format!("{mode:?} {fc} RES {ri} LEVEL {level} in {which}")
                            });
                        }
                    }
                }
            }
        }
    }
    // Stable at RES 0 at every FREQ: a saw, then silence that dies away.
    for fi in 0..24 {
        let fc = 20.0 * 1000f32.powf(fi as f32 / 23.0);
        let p = dirty(FilterMode::Lp12, fc, 0.0);
        let y = render_filter(&p, (1.2 * FS) as usize, |i| {
            if i < 4800 { grid_input(1, i) } else { 0.0 }
        });
        let tail = y[(1.0 * FS) as usize..]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(tail < 1e-4, "{fc} Hz RES 0: rings on at {tail}");
    }
    // 10 s of random per-block changes.
    let mut seed = 0x9e37_79b9u32;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    let mut f = Dirty::new();
    let mut p = dirty(FilterMode::Lp12, 1000.0, 0.0);
    for b in 0..(10.0 * FS) as usize / BLOCK_SIZE {
        assert!(p.set_mode(DIRTY_MODES[(rnd() * 3.99) as usize]));
        (p.cutoff, p.resonance, p.level) = (20.0 * 1000f32.powf(rnd()), rnd(), rnd());
        let which = (rnd() * 4.99) as usize;
        let mut buf: [f32; BLOCK_SIZE] =
            core::array::from_fn(|i| grid_input(which, b * BLOCK_SIZE + i));
        f.process(&mut buf, &p, SR, &FilterMods::NONE);
        check(&buf, &f, &|| format!("random block {b}"));
    }
    println!("DIRTY grid and random: max |y| {max}, max |state| {max_state}");
}

/// DIRTY's modes that oscillate at RES 1: NOTCH1's zero sits on the pitch
/// it would ring at, as NOTCH2's all-passes never ring on CLEAN.
const DIRTY_OSC_MODES: [FilterMode; 3] = [FilterMode::Lp12, FilterMode::Bp12, FilterMode::Hp12];

/// DIRTY at RES 1 from one impulse, read over 0.5 s after 0.25 s.
fn dirty_oscillation(mode: FilterMode, fc: f32) -> Vec<f32> {
    let n = (0.75 * FS) as usize;
    render_filter(&dirty(mode, fc, 1.0), n, impulse).split_off((0.25 * FS) as usize)
}

/// Spec test 2 on DIRTY: steady (±1 dB) in the shared band, 1 V/oct with
/// FM2 at +100 % up to fs/8, and the onset at RES 0.95 ± 0.01.
#[test]
fn dirty_self_oscillates_in_band_and_on_pitch() {
    let (lo, hi) = OSC_BAND_DB;
    for mode in DIRTY_OSC_MODES {
        let freqs = (0..7).map(|o| 100.0 * (1 << o) as f32).chain([8000.0]);
        let levels: Vec<(f32, f32, f32)> = freqs
            .map(|fc| {
                let y = dirty_oscillation(mode, fc);
                let level = rms_db(&y);
                let wobble = y
                    .chunks(4800)
                    .map(|w| (rms_db(w) - level).abs())
                    .fold(0.0f32, f32::max);
                (fc, level, wobble)
            })
            .collect();
        println!("DIRTY {mode:?} RES 1 (Hz, dBFS, ±dB): {levels:.2?}");
        for (fc, level, wobble) in levels {
            assert!(wobble <= 1.0, "{mode:?} {fc} Hz: ±{wobble:.2} dB");
            assert!(
                level > lo && level < hi && level < 0.0,
                "{mode:?} {fc} Hz: {level:.2} dBFS"
            );
        }
        let mut worst = (0.0f64, 0.0f64);
        for base in [261.626f32, 1250.0] {
            let mut last: Option<f64> = None;
            for note in (36..=84).step_by(12) {
                let fc = routed_cutoff(base, (note as f32 - 60.0) / 120.0);
                let want = f64::from(base) * 2f64.powf((note as f64 - 60.0) / 12.0);
                if want > f64::from(FS) / 8.0 {
                    continue;
                }
                let f = pitch(&dirty_oscillation(mode, fc));
                let c = cents(f, want);
                worst.0 = worst.0.max(c.abs());
                assert!(
                    c.abs() <= 25.0,
                    "{mode:?} base {base} note {note}: {c:.1} cents"
                );
                if let Some(l) = last {
                    let o = cents(f, l) - 1200.0;
                    worst.1 = worst.1.max(o.abs());
                    assert!(
                        o.abs() <= 10.0,
                        "{mode:?} base {base} note {note}: octave {o:.1}"
                    );
                }
                last = Some(f);
            }
        }
        println!(
            "DIRTY {mode:?} pitch: worst {:.1} cents, octave {:.1}",
            worst.0, worst.1
        );
    }
    let n = (1.1 * FS) as usize;
    let tail = (1.0 * FS) as usize;
    for mode in DIRTY_OSC_MODES {
        for fc in [250.0, 1000.0, 4000.0] {
            let at = |res: f32| {
                let y = render_filter(&dirty(mode, fc, res), n, impulse);
                let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                let late = y[tail..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
                20.0 * (late / peak).log10()
            };
            let (below, above) = (at(0.94), at(0.96));
            assert!(below < -60.0, "{mode:?} {fc} Hz RES 0.94: {below:.1} dB");
            assert!(above > -20.0, "{mode:?} {fc} Hz RES 0.96: {above:.1} dB");
        }
    }
}

/// A TYPE switch at RES 1 jumps no more than the shared band is wide (spec
/// H-new-1): CLEAN's modes but NOTCH2, the 2040's REV2 (EXP has its own
/// band) and DIRTY's oscillating modes, at FREQ 1 kHz.
#[test]
fn every_type_self_oscillates_in_one_band() {
    let mut levels = Vec::new();
    let read = |p: &FilterParams| {
        let n = (0.75 * FS) as usize;
        rms_db(&render_filter(p, n, impulse)[(0.25 * FS) as usize..])
    };
    for mode in OSC_MODES {
        let mut p = FilterParams::default();
        assert!(p.set_mode(mode));
        (p.cutoff, p.resonance) = (1000.0, 1.0);
        levels.push((format!("CLEAN {mode:?}"), read(&p)));
    }
    let mut p = FilterParams::default();
    p.set_kind(FilterKind::K2040);
    (p.cutoff, p.resonance, p.voice) = (1000.0, 1.0, K2040Voice::Rev2);
    levels.push(("2040 REV2".into(), read(&p)));
    for mode in DIRTY_OSC_MODES {
        levels.push((format!("DIRTY {mode:?}"), read(&dirty(mode, 1000.0, 1.0))));
    }
    println!("RES 1 at 1 kHz, dBFS: {levels:.2?}");
    let max = levels.iter().map(|l| l.1).fold(f32::MIN, f32::max);
    let min = levels.iter().map(|l| l.1).fold(f32::MAX, f32::min);
    assert!(
        max - min <= OSC_BAND_DB.1 - OSC_BAND_DB.0,
        "spread {:.2} dB",
        max - min
    );
}

#[test]
fn dirty_recovers_from_a_nan_block() {
    for mode in DIRTY_MODES {
        let p = dirty(mode, 1000.0, 0.5);
        let mut f = Dirty::new();
        f.process(&mut [f32::NAN; BLOCK_SIZE], &p, SR, &FilterMods::NONE);
        let mut quiet = [0.0f32; BLOCK_SIZE];
        f.process(&mut quiet, &p, SR, &FilterMods::NONE);
        assert!(quiet.iter().all(|&v| v == 0.0), "{mode:?}: {quiet:?}");
        let x = saw(2 * BLOCK_SIZE);
        let mut b = [0.0f32; BLOCK_SIZE];
        for c in x.chunks(BLOCK_SIZE) {
            b.copy_from_slice(c);
            f.process(&mut b, &p, SR, &FilterMods::NONE);
        }
        assert!(b.iter().all(|v| v.is_finite()), "{mode:?}: {b:?}");
        assert!(b.iter().any(|&v| v != 0.0), "{mode:?}");
    }
}

/// DIRTY's codes and labels, and the fallbacks (spec § 1): an unknown KIND
/// loads as CLEAN LP4; a MODE DIRTY lacks as its LP2; MODE 9 (HP12)
/// round-trips on DIRTY.
#[test]
fn unknown_codes_load_as_clean_lp4() {
    let k = FilterKind::Dirty;
    assert_eq!((k.disk_code(), k.disk_ident()), (6, "DIRTY"));
    assert_eq!(FilterKind::from_disk_code(6), Some(k));
    assert_eq!(k.modes(), DIRTY_MODES);
    assert_eq!(k.default_mode(), FilterMode::Lp12);
    assert_eq!(k.mode_labels(), ["LP2", "BP2", "HP2", "NOTCH1"]);
    let hp = FilterMode::Hp12;
    assert_eq!((hp.disk_code(), hp.disk_ident()), (9, "HP12"));
    assert_eq!(FilterMode::from_disk_code(9), Some(hp));
    assert!(!FilterKind::Svf.modes().contains(&hp));
    assert!(!FilterKind::K2040.modes().contains(&hp));

    let engine = (
        chimera_core::storage::RecordTag::Engine,
        vec![EngineType::Algo.disk_code()],
    );
    let load = |e: &[(u8, [u8; 4])]| {
        let block = (chimera_core::storage::RecordTag::Block, filter_payload(e));
        let f = decode(&sound_file(&[engine.clone(), block]))
            .unwrap()
            .params
            .filter;
        (f.kind(), f.mode())
    };
    let code = |c: u32| c.to_le_bytes();
    // KIND codes no kind has (1 is LADDER's): CLEAN at LP4, whatever MODE
    // says.
    for kind in [2, 3, 5, 7, 255, 1 << 20] {
        assert_eq!(
            load(&[(6, code(kind))]),
            (FilterKind::Svf, FilterMode::Lp24),
            "KIND {kind}"
        );
        assert_eq!(
            load(&[(6, code(kind)), (7, code(9))]),
            (FilterKind::Svf, FilterMode::Lp24),
            "KIND {kind} MODE 9"
        );
    }
    // DIRTY with a MODE it lacks (LP24, or none known): LP2.
    for mode in [2, 0, 5, 7, 8, 255] {
        assert_eq!(
            load(&[(6, code(6)), (7, code(mode))]),
            (FilterKind::Dirty, FilterMode::Lp12),
            "MODE {mode}"
        );
    }
    // Each of DIRTY's modes, MODE before KIND in the file too.
    for m in DIRTY_MODES {
        let c = code(u32::from(m.disk_code()));
        assert_eq!(load(&[(6, code(6)), (7, c)]), (k, m));
        assert_eq!(load(&[(7, c), (6, code(6))]), (k, m));
    }
    // And through a Sound's own encode.
    for m in DIRTY_MODES {
        let mut s = chimera_core::preset::Sound::init(EngineType::Algo);
        s.params.filter.set_kind(k);
        assert!(s.params.filter.set_mode(m));
        let d = decode(&encode(&s)).unwrap();
        assert!(d.bits_eq(&s), "{m:?}");
        assert_eq!((d.params.filter.kind(), d.params.filter.mode()), (k, m));
    }
}

/// A RES jump inside one block stays in Jury's region: at FREQ 20 kHz, RES
/// 0 → 0.9 → 0 a block at a time, a 0.1 sine stays a small signal. (`f` is
/// bounded at the block's larger `q` end; bounded at the new end alone the
/// ramp's line still clears the boundary for RES in 0..1 and FREQ to
/// 0.497·fs, touching it only at RES 0 → 0.59 there, so this can't tell
/// the two apart: the larger end is for taps, which leave the ramp.)
#[test]
fn dirty_res_jumps_stay_inside_jury() {
    for mode in DIRTY_MODES {
        let mut p = dirty(mode, 20_000.0, 0.0);
        let mut f = Dirty::new();
        let mut max = 0.0f32;
        for b in 0..40 {
            p.resonance = if (b / 4) % 2 == 1 { 0.9 } else { 0.0 };
            let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
                0.1 * (core::f32::consts::TAU * 1000.0 * (b * BLOCK_SIZE + i) as f32 / FS).sin()
            });
            f.process(&mut buf, &p, SR, &FilterMods::NONE);
            max = buf.iter().fold(max, |m, v| m.max(v.abs()));
        }
        assert!(max < 0.25, "{mode:?}: {max}");
    }
}

/// LEVEL runs ahead of DIRTY as on CLEAN: ×2 at ¾ (+6 dB in the pass
/// band), its clipper lit past unity, and `Filter` reports it running.
#[test]
fn dirty_takes_level_and_its_clipper() {
    let sine = |i: usize| 0.1 * (core::f32::consts::TAU * 100.0 * i as f32 / FS).sin();
    let mut p = dirty(FilterMode::Lp12, 5000.0, 0.0);
    let n = (0.5 * FS) as usize;
    let half = mag_db(&render_filter(&p, n, sine)[n / 2..], 100.0, FS);
    p.level = 0.75;
    let three = mag_db(&render_filter(&p, n, sine)[n / 2..], 100.0, FS);
    assert!((three - half - 6.0206).abs() < 0.05, "{half} → {three}");
    p.level = 1.0;
    let mut f = Filter::new();
    let mut clip = 0.0f32;
    for b in 0..20 {
        let mut buf: [f32; BLOCK_SIZE] =
            core::array::from_fn(|i| grid_input(1, b * BLOCK_SIZE + i));
        clip = clip.max(f.process(&mut buf, &p, SR, &FilterMods::NONE).get());
    }
    assert!(clip > 0.5, "LED {clip}");
    assert!(f.level_running());
}

/// DIRTY's trims ramp with `q` as CLEAN's do with `k`: untouched for `q ≥
/// ½` (RES ≤ 0.71), the mode's trim at `q ≤ 0`, linear between; NOTCH1
/// untrimmed. `dirty_q` holds RES to 0..1 and a NaN to RES 0 (review F2).
#[test]
fn dirty_trim_ramps_with_q_and_q_is_sanitised() {
    for (mode, t) in [
        (FilterMode::Lp12, DIRTY_TRIM_LP2),
        (FilterMode::Bp12, DIRTY_TRIM_BP2),
        (FilterMode::Hp12, DIRTY_TRIM_HP2),
        (FilterMode::Notch, 1.0),
    ] {
        assert_eq!(dirty_trim(mode, 2.0), 1.0, "{mode:?}");
        assert_eq!(dirty_trim(mode, 0.5), 1.0, "{mode:?}");
        assert_eq!(dirty_trim(mode, 0.0), t, "{mode:?}");
        assert_eq!(dirty_trim(mode, dirty_q(1.0)), t, "{mode:?}");
        let mid = dirty_trim(mode, 0.25);
        assert!((mid - 0.5 * (1.0 + t)).abs() < 1e-6, "{mode:?}: {mid}");
    }
    assert_eq!(dirty_q(f32::NAN), dirty_q(0.0));
    assert_eq!(dirty_q(-3.0), 2.0);
    assert_eq!(dirty_q(7.0), dirty_q(1.0));
    // A NaN RES block leaves the next block's ramp clean.
    let mut f = Dirty::new();
    let mut p = dirty(FilterMode::Lp12, 1000.0, f32::NAN);
    let x = saw(2 * BLOCK_SIZE);
    let mut b = [0.0f32; BLOCK_SIZE];
    b.copy_from_slice(&x[..BLOCK_SIZE]);
    f.process(&mut b, &p, SR, &FilterMods::NONE);
    p.resonance = 0.0;
    let mut want = Dirty::new();
    let mut w = [0.0f32; BLOCK_SIZE];
    w.copy_from_slice(&x[..BLOCK_SIZE]);
    want.process(&mut w, &p, SR, &FilterMods::NONE);
    assert_eq!(b, w);
    b.copy_from_slice(&x[BLOCK_SIZE..]);
    w.copy_from_slice(&x[BLOCK_SIZE..]);
    f.process(&mut b, &p, SR, &FilterMods::NONE);
    want.process(&mut w, &p, SR, &FilterMods::NONE);
    assert_eq!(b, w);
}

/// The trim ramps across the block, never snaps: RES 0.7 → 0.94 (trim 1 →
/// about 0.57) under a 50 Hz sine in LP2's pass band moves the output by
/// no more a sample than the sine itself does, where a snapped trim steps
/// it by about 0.1.
#[test]
fn dirty_trim_never_snaps() {
    let mut p = dirty(FilterMode::Lp12, 2000.0, 0.7);
    let mut f = Dirty::new();
    let sine = |i: usize| 0.5 * (core::f32::consts::TAU * 50.0 * i as f32 / FS).sin();
    let (mut last, mut steady, mut jump) = (0.0f32, 0.0f32, 0.0f32);
    for b in 0..40 {
        if b == 30 {
            p.resonance = 0.94;
        }
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| sine(b * BLOCK_SIZE + i));
        f.process(&mut buf, &p, SR, &FilterMods::NONE);
        for v in buf {
            let d = (v - last).abs();
            if (20..30).contains(&b) {
                steady = steady.max(d);
            } else if b >= 30 {
                jump = jump.max(d);
            }
            last = v;
        }
    }
    println!("DIRTY trim: steady step {steady:.4}, across the RES move {jump:.4}");
    assert!(jump <= 1.5 * steady, "{jump} against {steady}");
}

/// The 2040's one mode can't change, so KIND's band names it (layout A,
/// ADR 0155; the `flt_2040_kind` screen golden draws it); the others have
/// a MODE cell.
#[test]
fn the_2040s_kind_band_names_its_mode() {
    use chimera_core::ui::filter_panel::kind_note;
    assert_eq!(kind_note(FilterKind::K2040), Some("4-POLE LP"));
    for k in [FilterKind::Svf, FilterKind::Dirty, FilterKind::Ladder] {
        assert_eq!(kind_note(k), None, "{k:?}");
    }
}

/// LEVEL reads INPUT LEVEL, INPUT on its cell and column (ADR 0155); its
/// ident stays.
#[test]
fn level_is_input_level() {
    let s = chimera_core::block::find_spec(&FILTER_SPECS, FilterParams::LEVEL).unwrap();
    assert_eq!(
        (s.label, s.short, s.ident),
        ("INPUT LEVEL", Some("INPUT"), "LEVEL")
    );
}
