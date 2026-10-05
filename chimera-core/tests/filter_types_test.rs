//! The filter types' DSP (spec § Tests).

mod common;

use chimera_core::block::{Block, DiskCode};
use chimera_core::dsp::filter::clean::{clean_k, clean_k2, trim};
use chimera_core::dsp::filter::coef::{g_of, tan_accurate};
use chimera_core::dsp::filter::{Clean, FilterKind, FilterMode, routed_cutoff};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, FILTER_SPECS, FilterParams, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::Rig;
use common::filter_ref::{OSC_BAND_DB, clean_ref, g_track, mag_db, ramp_track, rms_db, trim_ref};

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
        f.process(&mut buf, &p, SR);
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

const JUMP_MODES: [FilterMode; 6] = [
    FilterMode::Lp6,
    FilterMode::Lp12,
    FilterMode::Lp24,
    FilterMode::Bp24,
    FilterMode::Hp24,
    FilterMode::Notch,
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
/// below; PHASER untrimmed.
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
    assert_eq!(trim(FilterMode::Phaser, clean_k(1.0)), 1.0);
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
        f.process(&mut buf, &p, SR);
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
/// the grid's max, 12.55 (the ±10 impulses, passed through), × 1.1.
const CLEAN_BOUND: f32 = 13.8;
/// Its raw states' bound, the same way: 3.84 × 1.1. Each is read through
/// `sat_c1`, so the loop never sees more than 1.5.
const CLEAN_STATE_BOUND: f32 = 4.22;

/// The RES 1 jumps' bound on the 0.8 saw: their max, 0.833, × 1.1. It was
/// 0.733 before LP1 and NOTCH1 joined (NOTCH1 passes the saw's low end).
const CLEAN_JUMP_BOUND: f32 = 0.92;

/// Task 2's one-block jumps at RES 1, where self-oscillation makes the
/// reference's relative error meaningless: bounded instead (spec test 4).
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
            let mut p = FilterParams::default();
            assert!(p.set_mode(mode));
            p.resonance = 1.0;
            let x = saw(cutoffs.len() * BLOCK_SIZE);
            let mut f = Clean::new();
            for (b, &fc) in cutoffs.iter().enumerate() {
                p.cutoff = fc;
                let mut buf: [f32; BLOCK_SIZE] =
                    x[b * BLOCK_SIZE..][..BLOCK_SIZE].try_into().unwrap();
                f.process(&mut buf, &p, SR);
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

/// The grid's inputs (spec test 8): silence, a full-scale saw, a square at
/// fs/4, white noise ±1 and impulses of ±10.
fn grid_input(which: usize, i: usize) -> f32 {
    match which {
        0 => 0.0,
        1 => (i % 109) as f32 / 54.5 - 1.0,
        2 => {
            if i % 4 < 2 {
                1.0
            } else {
                -1.0
            }
        }
        3 => {
            let h = (i as u32)
                .wrapping_mul(2_654_435_761)
                .rotate_left(13)
                .wrapping_mul(2_246_822_519);
            h as f32 / u32::MAX as f32 * 2.0 - 1.0
        }
        _ => match i % 480 {
            0 => 10.0,
            240 => -10.0,
            _ => 0.0,
        },
    }
}

#[test]
fn clean_is_bounded_on_the_grid() {
    let (mut max, mut max_state) = (0.0f32, 0.0f32);
    for mode in FilterMode::ALL {
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
                        f.process(&mut buf, &p, SR);
                        for v in buf {
                            assert!(v.is_finite(), "{mode:?} {fc} RES {ri}: {v}");
                            max = max.max(v.abs());
                        }
                        for s in f.states() {
                            assert!(s.is_finite());
                            max_state = max_state.max(s.abs());
                        }
                    }
                }
            }
        }
    }
    println!("grid: max |y| {max}, max |state| {max_state}");
    assert!(max <= CLEAN_BOUND, "{max}");
    assert!(max_state <= CLEAN_STATE_BOUND, "{max_state}");
}

#[test]
fn clean_recovers_from_a_nan_block() {
    for mode in FilterMode::ALL {
        let mut p = FilterParams::default();
        assert!(p.set_mode(mode));
        (p.cutoff, p.resonance) = (1000.0, 0.5);
        let mut f = Clean::new();
        f.process(&mut [f32::NAN; BLOCK_SIZE], &p, SR);
        // Cleared: silence in, silence out.
        let mut quiet = [0.0f32; BLOCK_SIZE];
        f.process(&mut quiet, &p, SR);
        assert!(quiet.iter().all(|&v| v == 0.0), "{mode:?}: {quiet:?}");
        let x = saw(2 * BLOCK_SIZE);
        let mut b = [0.0f32; BLOCK_SIZE];
        for c in x.chunks(BLOCK_SIZE) {
            b.copy_from_slice(c);
            f.process(&mut b, &p, SR);
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

/// ALGO and MODAL INIT with no VCA route ring on after note-off, at RES 1,
/// until the engine ends (#374): the voice then fades the ring over one
/// block, so its end steps no more than the ring itself does.
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
        assert!(ring > 0.01, "{engine:?}: no ring ({ring})");
        assert!(
            end <= ring * 1.1,
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
        assert!(ring > 0.01, "{engine:?}: no ring ({ring})");
        assert!(
            end <= ring,
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
