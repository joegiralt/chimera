//! The delay's MECHANICS: a slow wow plus an irregular flutter, one knob
//! (ADR 0053).

mod common;

use chimera_core::addr::BlockRef;
use chimera_core::dsp::delay::{
    CAPSTAN_HZ, DELAY_SPECS, DelayParams, MAX_DELAY_SAMPLES, Tap, TapeDelay, Transport,
};
use chimera_core::storage::ValidAddr;
use chimera_hal::BLOCK_SIZE;
use common::fnv1a;

const SR: u32 = 48_000;

fn params(time_ms: f32, mech: f32) -> DelayParams {
    DelayParams {
        time_ms,
        feedback: 0.6,
        wow_flutter: mech,
        mix: 0.5,
        ..DelayParams::default()
    }
}

/// The delay's insert output for a 50-block saw burst, then silence.
fn render(p: &DelayParams) -> Vec<f32> {
    let mut d = Box::new(TapeDelay::new());
    let mut out = Vec::new();
    let mut phase = 0.0f32;
    for b in 0..450 {
        let mut block = [0.0f32; BLOCK_SIZE];
        if b < 50 {
            for s in block.iter_mut() {
                *s = phase - 0.5;
                phase = (phase + 110.0 / SR as f32).fract();
            }
        }
        d.process(&mut block, p, SR);
        out.extend_from_slice(&block);
    }
    out
}

/// `n` samples of the transport's unclamped read delay.
fn transport(p: &DelayParams, n: usize) -> Vec<f32> {
    let tap = Tap::new(p, SR);
    let mut t = Transport::new();
    (0..n).map(|_| t.next(&tap)).collect()
}

#[test]
fn mech_zero_is_todays_delay_at_wow_zero() {
    // Recorded from the delay before MECHANICS (ff02c75) at WOW 0.
    let today = [
        (10.0, 0x64a203f883890bd1),
        (120.0, 0x496dcfdfed4ba0ab),
        (375.0, 0x108cf3247927e80d),
        (500.0, 0xf16f40ef5151fc0d),
    ];
    for (time, want) in today {
        let got = fnv1a(&render(&params(time, 0.0)));
        assert_eq!(got, want, "TIME {time}: {got:#018x}");
    }
}

#[test]
fn read_stays_inside_the_buffer() {
    let top = (MAX_DELAY_SAMPLES - 2) as f32;
    for time in [10.0, 10.5, 50.0, 250.0, 375.0, 499.5, 500.0] {
        for mech in [0.0, 0.25, 0.5, 0.75, 1.0] {
            // Four wow cycles.
            for (n, d) in transport(&params(time, mech), 8 * SR as usize)
                .into_iter()
                .enumerate()
            {
                assert!(
                    (1.0..=top).contains(&d),
                    "TIME {time} MECH {mech} sample {n}: {d}"
                );
            }
        }
    }
}

/// RMS pitch deviation in cents: the read's rate of change. Measured at a
/// short TIME, where the delay's `f32` resolution is far below it.
fn rms_cents(run: &[f32]) -> f32 {
    let sq: f64 = run
        .windows(2)
        .map(|w| {
            let c = 1200.0 * (1.0 - f64::from(w[1] - w[0])).log2();
            c * c
        })
        .sum();
    (sq / (run.len() - 1) as f64).sqrt() as f32
}

#[test]
fn depth_rises_with_the_knob() {
    let depth: Vec<f32> = [0.0, 0.15, 0.25, 0.5, 0.75, 1.0]
        .iter()
        .map(|&m| rms_cents(&transport(&params(10.0, m), 10 * SR as usize)))
        .collect();
    assert_eq!(depth[0], 0.0);
    assert!(depth.windows(2).all(|w| w[0] < w[1]), "{depth:?}");
    // Full MECH: a worn machine's flutter, a few cents RMS.
    assert!((4.0..=10.0).contains(&depth[5]), "{depth:?}");
}

/// Mean normalised autocorrelation of the pitch deviation at 1–4 capstan
/// periods: ~1 for a steady vibrato, lower once noise decorrelates cycles.
fn periodicity(run: &[f32]) -> f32 {
    let dev: Vec<f64> = run.windows(2).map(|w| f64::from(w[1] - w[0])).collect();
    let mean = dev.iter().sum::<f64>() / dev.len() as f64;
    let x: Vec<f64> = dev.iter().map(|v| v - mean).collect();
    let var = x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64;
    let period = (SR as f32 / CAPSTAN_HZ).round() as usize;
    let ac = |lag: usize| {
        let n = x.len() - lag;
        x.iter().zip(&x[lag..]).map(|(a, b)| a * b).sum::<f64>() / n as f64 / var
    };
    ((1..=4).map(|k| ac(k * period)).sum::<f64>() / 4.0) as f32
}

#[test]
fn flutter_is_irregular() {
    let vibrato: Vec<f32> = (0..10 * SR)
        .map(|n| {
            let turns = (f64::from(CAPSTAN_HZ) * f64::from(n) / f64::from(SR)).fract();
            4.0 * (core::f64::consts::TAU * turns).sin() as f32
        })
        .collect();
    assert!(periodicity(&vibrato) > 0.99, "the metric sees a vibrato");
    for mech in [0.15, 1.0] {
        let p = periodicity(&transport(&params(10.0, mech), 10 * SR as usize));
        assert!(p < 0.8, "MECH {mech}: periodicity {p}");
    }
}

#[test]
fn deterministic() {
    for p in [params(375.0, 1.0), params(10.0, 0.6)] {
        assert_eq!(fnv1a(&render(&p)), fnv1a(&render(&p)));
        assert_eq!(transport(&p, SR as usize), transport(&p, SR as usize));
    }
}

/// The knob is relabelled, not re-identified: id 2 and ident WOW are frozen
/// (ADR 0045), so every stored WOW loads as MECHANICS.
#[test]
fn mechanics_is_wow_relabelled() {
    let s = &DELAY_SPECS[2];
    assert_eq!(s.id, DelayParams::WOW_FLUTTER);
    assert_eq!(
        (s.label, s.short, s.ident, s.default),
        ("MECHANICS", Some("MECH"), "WOW", 0.15)
    );
    assert_eq!(DELAY_SPECS.len(), 7);
    assert!(ValidAddr::find(BlockRef::Delay, DelayParams::WOW_FLUTTER).is_some());
}
