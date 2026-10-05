//! The filter types' DSP (spec § Tests).

mod common;

use chimera_core::dsp::fast_tan;
use chimera_core::dsp::filter::coef::{g_of, tan_accurate};
use chimera_core::dsp::filter::probe::clean_probe;
use chimera_core::dsp::filter::{Clean, FilterMode, routed_cutoff};
use chimera_core::params::FilterParams;
use chimera_hal::BLOCK_SIZE;
use common::filter_ref::{clean_ref, g_track};

const SR: u32 = 48_000;
const FS: f32 = SR as f32;

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

/// `cutoff[b]` for block b through `Clean`, and through `clean_ref` with
/// the ramp's `g` per sample: max |y − y_ref| / max |y_ref|.
fn jump_error(mode: FilterMode, cutoff: &[f32]) -> f64 {
    let mut p = FilterParams::default();
    assert!(p.set_mode(mode));
    let x = saw(cutoff.len() * BLOCK_SIZE);
    let mut f = Clean::new();
    let (mut y, mut g) = (Vec::new(), Vec::new());
    for (b, &fc) in cutoff.iter().enumerate() {
        p.cutoff = fc;
        let mut buf: [f32; BLOCK_SIZE] = x[b * BLOCK_SIZE..][..BLOCK_SIZE].try_into().unwrap();
        f.process(&mut buf, &p, SR);
        y.extend_from_slice(&buf);
        let from = cutoff[b.saturating_sub(1)];
        g.extend(g_track(from.into(), fc.into(), FS.into()));
    }
    // RES 0: today's k until Task 3's law.
    let k = vec![2.01f32 as f64; x.len()];
    let r = clean_ref(&x, &g, &k, mode);
    let peak = r.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let err = y
        .iter()
        .zip(&r)
        .fold(0.0f64, |m, (&a, b)| m.max((f64::from(a) - b).abs()));
    err / peak
}

#[test]
fn clean_one_block_jumps_match_the_exact_reference_at_res_0() {
    let held = |a: f32, b: f32| -> Vec<f32> { [[a; 6], [b; 6]].concat() };
    // ENV 1 → CUTOFF at +100 % from 100 Hz, its 1 ms attack starting at
    // block 4, read per block as the voice does.
    let env: Vec<f32> = (0..12usize)
        .map(|b| {
            let t = (b.saturating_sub(4) * BLOCK_SIZE) as f32 / (0.001 * FS);
            routed_cutoff(100.0, t.min(1.0))
        })
        .collect();
    for mode in [
        FilterMode::Lp12,
        FilterMode::Lp24,
        FilterMode::Bp24,
        FilterMode::Hp24,
    ] {
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

#[test]
fn steady_and_moving_agree_on_a_steady_block() {
    let mut p = FilterParams::default();
    (p.cutoff, p.resonance) = (1500.0, 0.6);
    let x = saw(4 * BLOCK_SIZE);
    // LP24: each mode's loop is its own, but the coefficients are shared.
    let mut a = Clean::new();
    for b in 0..3 {
        let mut buf: [f32; BLOCK_SIZE] = x[b * BLOCK_SIZE..][..BLOCK_SIZE].try_into().unwrap();
        a.process(&mut buf, &p, SR);
    }
    let mut m = a.clone();
    let mut s: [f32; BLOCK_SIZE] = x[3 * BLOCK_SIZE..].try_into().unwrap();
    let mut v = s;
    clean_probe::<2, false>(&mut a, &mut s, &p);
    clean_probe::<2, true>(&mut m, &mut v, &p);
    let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.01);
    for (i, (a, b)) in s.iter().zip(&v).enumerate() {
        assert!((a - b).abs() <= 1e-6 * peak, "sample {i}: {a} vs {b}");
    }
}
