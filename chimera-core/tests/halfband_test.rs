//! The half-band FIR (FX diet spec § Rate). Test tones are whole periods of
//! every window so an RMS or a single DFT bin is exact.

use chimera_core::dsp::halfband::{Decimator, HALF, Interpolator, ODD_TAPS};
use chimera_hal::BLOCK_SIZE;

const SR: f32 = 48_000.0;

fn sine(hz: f32, rate: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (2.0 * core::f32::consts::PI * hz * i as f32 / rate).sin())
        .collect()
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&s| s as f64 * s as f64).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(a: f64, b: f64) -> f64 {
    20.0 * (a / b).log10()
}

/// The second half of the output, past the filter's settling.
fn decimate(x: &[f32]) -> Vec<f32> {
    let mut d = Decimator::new();
    let mut out = Vec::new();
    for block in x.as_chunks::<BLOCK_SIZE>().0 {
        let mut y = [0.0; HALF];
        d.process(block, &mut y);
        out.extend_from_slice(&y);
    }
    out.split_off(out.len() / 2)
}

fn interpolate(z: &[f32]) -> Vec<f32> {
    let mut u = Interpolator::new();
    let mut out = Vec::new();
    for block in z.as_chunks::<HALF>().0 {
        let mut y = [0.0; BLOCK_SIZE];
        u.process(block, &mut y);
        out.extend_from_slice(&y);
    }
    out.split_off(out.len() / 2)
}

/// |X(hz)|² of `x` sampled at `rate`: one exact DFT bin.
fn goertzel(x: &[f32], hz: f64, rate: f64) -> f64 {
    let c = 2.0 * (2.0 * std::f64::consts::PI * hz / rate).cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s0 = v as f64 + c * s1 - s2;
        (s2, s1) = (s1, s0);
    }
    s1 * s1 + s2 * s2 - c * s1 * s2
}

#[test]
fn dc_gain_is_within_the_ripple() {
    let dc = 0.5 + 2.0 * ODD_TAPS.iter().sum::<f32>();
    assert!(db(dc as f64, 1.0).abs() <= 0.06, "{dc}");
}

#[test]
fn decimation_keeps_the_passband_and_rejects_the_stopband() {
    let n = 100 * BLOCK_SIZE;
    for hz in [150.0, 1_050.0, 6_000.0, 10_500.0] {
        let x = sine(hz, SR, n);
        let g = db(rms(&decimate(&x)), rms(&x));
        assert!(g.abs() <= 0.06, "{hz} Hz: {g} dB");
    }
    for hz in [13_500.0, 15_000.0, 20_000.0, 23_000.0] {
        let x = sine(hz, SR, n);
        let g = db(rms(&decimate(&x)), rms(&x));
        assert!(g <= -43.0, "{hz} Hz: {g} dB");
    }
}

#[test]
fn interpolation_keeps_the_passband_and_rejects_the_image() {
    let n = 100 * HALF;
    for hz in [1_050.0f32, 6_000.0, 9_000.0] {
        let z = sine(hz, SR / 2.0, n);
        let y = interpolate(&z);
        let g = db(rms(&y), rms(&z));
        assert!(g.abs() <= 0.06, "{hz} Hz: {g} dB");
        let image = 24_000.0 - hz as f64;
        let r =
            10.0 * (goertzel(&y, image, SR as f64) / goertzel(&y, hz as f64, SR as f64)).log10();
        assert!(r <= -43.0, "{hz} Hz, image at {image} Hz: {r} dB");
    }
}
