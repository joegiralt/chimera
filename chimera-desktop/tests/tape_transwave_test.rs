//! The importer's transwave (TAPE spec § 6 step 5): whole cycles cut from
//! the input, each built into octave levels by FFT truncation. Precedent:
//! PPG Wave and Waldorf Microwave (band-limited copies per octave).

use chimera_core::dsp::tape_engine::wave::level_offset;
use chimera_desktop::import::tape::transwave::{DEFAULT_TOP, MAX_FRAMES, cut_cycles, levels};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

/// Magnitude per bin, 0..=n/2, of `x`.
fn spectrum(x: &[f32]) -> Vec<f64> {
    let mut b: Vec<Complex<f64>> = x.iter().map(|&v| Complex::new(v as f64, 0.0)).collect();
    FftPlanner::new().plan_fft_forward(b.len()).process(&mut b);
    b[..=x.len() / 2].iter().map(|c| c.norm()).collect()
}

/// A cycle with energy in every harmonic: a saw plus seeded noise, 301
/// samples long (not a power of two, as a real cycle isn't).
fn rich_cycle() -> Vec<f32> {
    let mut s = 0x5EED_u32;
    (0..301)
        .map(|i| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (i as f32 / 301.0 - 0.5) + 0.2 * (s as f32 / u32::MAX as f32 - 0.5)
        })
        .collect()
}

/// Each level's FFT: no DC, nothing above ⌊0.35 N⌋ (under −100 dB of its
/// largest bin), and each kept harmonic as the cycle has it.
#[test]
fn levels_hold_no_harmonic_above_limit() {
    let c = rich_cycle();
    let src = spectrum(&c);
    for top in [512u32, 1_024] {
        let lv = levels(&c, top);
        let mut n = top;
        while n >= 16 {
            let off = level_offset(top, n.trailing_zeros()) as usize;
            let sp = spectrum(&lv[off..off + n as usize]);
            let peak = sp.iter().fold(0.0f64, |m, &x| m.max(x));
            let limit = (0.35 * n as f64) as usize;
            assert!(sp[0] / peak < 1e-5, "top {top}, level {n}: DC");
            for (k, &m) in sp.iter().enumerate().skip(limit + 1) {
                let db = 20.0 * (m / peak).log10();
                assert!(
                    db < -100.0,
                    "top {top}, level {n}: harmonic {k} at {db:.1} dB"
                );
            }
            // The kept harmonics are the cycle's, scaled by length.
            for k in 1..=limit.min(c.len() / 2) {
                let want = src[k] / c.len() as f64;
                let got = sp[k] / n as f64;
                assert!(
                    (got - want).abs() < 1e-4,
                    "top {top}, level {n}, harmonic {k}: {got} against {want}"
                );
            }
            n /= 2;
        }
    }
}

/// 1 008 samples a frame at the default top, 2 032 at 1 024.
#[test]
fn level_sizes() {
    let c = rich_cycle();
    assert_eq!(DEFAULT_TOP, 512);
    assert_eq!(levels(&c, DEFAULT_TOP).len(), 1_008);
    assert_eq!(levels(&c, 1_024).len(), 2_032);
}

/// Whole cycles from across the input, at most `MAX_FRAMES` (32), each
/// starting at a rising zero crossing, one cycle long.
#[test]
fn cut_cycles_takes_whole_cycles() {
    let period = 109.37f32;
    let x: Vec<f32> = (0..48_000)
        .map(|i| (std::f32::consts::TAU * (i as f32 + 17.0) / period).sin())
        .collect();
    let cycles = cut_cycles(&x, period);
    assert_eq!(MAX_FRAMES, 32);
    assert_eq!(cycles.len(), 32);
    for c in &cycles {
        assert_eq!(c.len(), period.ceil() as usize);
        // Exactly one cycle, from its rising zero crossing.
        for (j, &y) in c.iter().enumerate() {
            let want = (std::f32::consts::TAU * j as f32 / c.len() as f32).sin();
            assert!((y - want).abs() < 2e-3, "sample {j}: {y} against {want}");
        }
    }
    // Fewer cycles in the input, fewer frames.
    assert_eq!(cut_cycles(&x[..1_000], period).len(), 8);
    assert!(cut_cycles(&x[..50], period).is_empty());
    assert!(cut_cycles(&x, 0.0).is_empty());
}
