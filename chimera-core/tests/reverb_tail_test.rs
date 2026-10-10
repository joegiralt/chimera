//! A long tail's end: once the decay meets the i16 lines' storage noise
//! (0.6–2.3 LSB RMS, half of it over 4 kHz, where a damped tail has
//! nothing), the return is that noise alone, a sizzle. `TailGate` fades it
//! with the tail instead (reverb sizzle, 2026-10-11).

mod common;

use chimera_core::dsp::Stereo;
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{KnobMemory, ReverbParams, ReverbType};
use common::SR;
use common::rev::{MS, run, tone};

/// The return's third difference: 1 kHz down 53 dB, 8 kHz at unity.
fn hf(x: &[f32]) -> Vec<f32> {
    x.windows(4)
        .map(|w| w[3] - 3.0 * w[2] + 3.0 * w[1] - w[0])
        .collect()
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-15).log10()
}

/// The loudest 100 ms window's high band, dBFS, where it is within 10 dB
/// of the whole: the tail gone to noise.
fn sizzle_db(t: ReverbType) -> f64 {
    let p = ReverbParams {
        time: 1.0,
        mix: 1.0,
        ..ReverbParams::default().with_type(t, &KnobMemory::new())
    };
    let mut rv = Box::new(Reverb::of_type(t));
    let burst = tone(1000.0, 0.5);
    let (l, _) = run(
        &mut |send| {
            let mut out = Stereo::SILENT;
            rv.process(send, &p, 1.0, Tempo::DEFAULT, SR, &mut out);
            (out.l, out.r)
        },
        &mut |i| if i < 50 * MS { burst(i) } else { 0.0 },
        16.0,
    );
    l.as_chunks::<{ 100 * MS }>()
        .0
        .iter()
        .skip(10)
        .map(|w| (db(rms(&hf(w))), db(rms(w))))
        .filter(|&(h, all)| h > all - 10.0)
        .map(|(h, _)| h)
        .fold(-300.0, f64::max)
}

#[test]
fn a_long_tail_ends_without_sizzle() {
    let worst = [
        ReverbType::Quad,
        ReverbType::Room,
        ReverbType::Hall,
        ReverbType::Bloom,
        ReverbType::Reverse,
    ]
    .map(|t| {
        let s = sizzle_db(t);
        println!("{}: sizzle {s:.1} dBFS", ReverbType::NAMES[t.index()]);
        s
    });
    // Before the gate: −75.8, −78.8, −78.0, −61.0 and −92.2 dBFS.
    assert!(worst.iter().all(|&s| s < -90.0), "sizzle {worst:.1?} dBFS");
}
