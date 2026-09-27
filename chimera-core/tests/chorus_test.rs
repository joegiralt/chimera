//! The Juno chorus (FX diet spec § Chorus).

use chimera_core::dsp::Stereo;
use chimera_core::dsp::chorus::{ChorusParams, JunoChorus};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|&s| s as f64 * s as f64).sum::<f64>() / x.len() as f64).sqrt() as f32
}

/// `seconds` of a 220 Hz sine at 0.5 through the chorus.
fn render(p: &ChorusParams, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let mut c = Box::new(JunoChorus::new());
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let mut out = Stereo::SILENT;
    for b in 0..(seconds * SR as f32) as usize / BLOCK_SIZE {
        let send: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
            let n = (b * BLOCK_SIZE + i) as f32;
            0.5 * (2.0 * core::f32::consts::PI * 220.0 * n / SR as f32).sin()
        });
        c.process_wet(&send, p, SR, &mut out);
        l.extend_from_slice(&out.l);
        r.extend_from_slice(&out.r);
    }
    (l, r)
}

#[test]
fn the_mono_sum_keeps_the_wet_and_the_sides_differ() {
    for mode in 1..=3 {
        let p = ChorusParams {
            mode,
            rate: 0.5,
            depth: 0.5,
            mix: 1.0,
        };
        let (l, r) = render(&p, 4.0);
        let mono: Vec<f32> = l.iter().zip(&r).map(|(a, b)| (a + b) / 2.0).collect();
        assert!(
            rms(&mono) >= 0.501 * rms(&l),
            "mode {mode}: mono {} vs L {}",
            rms(&mono),
            rms(&l)
        );
        assert!(l != r, "mode {mode}");
    }
}

/// Review Focus: at full DEPTH the inverted tap reaches below one sample
/// and clamps; every read is a blend of two stored samples.
#[test]
fn the_chorus_stays_bounded_at_full_depth_and_rate() {
    for mode in 1..=3 {
        let p = ChorusParams {
            mode,
            rate: 1.0,
            depth: 1.0,
            mix: 1.0,
        };
        let (l, r) = render(&p, 2.0);
        for &s in l.iter().chain(&r) {
            assert!(s.is_finite() && s.abs() <= 0.5 + 1e-6, "mode {mode}: {s}");
        }
    }
}
