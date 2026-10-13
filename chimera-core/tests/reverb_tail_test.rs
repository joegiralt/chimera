//! A long tail's end: once the decay nears the i16 lines' storage noise
//! (white, half of it over 4 kHz, where a damped tail has nothing), the
//! return sizzles: first a hiss over the dark tail, at the end the noise
//! alone. `TailGate` low-passes the one and fades the other (reverb
//! sizzle, 2026-10-11; the unit's take, 2026-10-11).

mod common;

use chimera_core::dsp::Stereo;
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{KnobMemory, ReverbParams, ReverbType};
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::rev::{MS, Noise, run, tone};

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-15).log10()
}

fn render(t: ReverbType) -> Vec<f32> {
    let p = ReverbParams {
        time: 1.0,
        mix: 1.0,
        ..ReverbParams::default().with_type(t, &KnobMemory::new())
    };
    let mut rv = Box::new(Reverb::of_type(t));
    let burst = tone(1000.0, 0.5);
    run(
        &mut |send| {
            let mut out = Stereo::SILENT;
            rv.process(send, &p, 1.0, Tempo::DEFAULT, SR, &mut out);
            (out.l, out.r)
        },
        &mut |i| if i < 50 * MS { burst(i) } else { 0.0 },
        16.0,
    )
    .0
}

/// An RBJ biquad, Q 0.707.
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    fn new(high: bool, hz: f64) -> Self {
        let w = std::f64::consts::TAU * hz / SR as f64;
        let (c, al) = (w.cos(), w.sin() / std::f64::consts::SQRT_2);
        let b = if high {
            [(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0]
        } else {
            [(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0]
        };
        let a0 = 1.0 + al;
        Self {
            b: b.map(|v| v / a0),
            a: [-2.0 * c / a0, (1.0 - al) / a0],
            z: [0.0; 2],
        }
    }

    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The 4–16 kHz band: fourth-order high-pass at 4 kHz, low-pass at 16.
fn band(x: &[f32]) -> Vec<f32> {
    let mut f = [
        Biquad::new(true, 4_000.0),
        Biquad::new(true, 4_000.0),
        Biquad::new(false, 16_000.0),
        Biquad::new(false, 16_000.0),
    ];
    x.iter()
        .map(|&v| f.iter_mut().fold(v as f64, |a, q| q.run(a)) as f32)
        .collect()
}

/// How much brighter than its own body the tail gets: per 100 ms window
/// from its loudest on, the 4–16 kHz band's share of the whole (dB), less
/// the brightest share among the body's windows (within 20 dB of the
/// loudest); the most of it over every window whose band is above −105
/// dBFS (−94 at the DAC, ADR 0109's +11 dB).
fn excess_db(l: &[f32]) -> f64 {
    let h = band(l);
    let w = 100 * MS;
    let win: Vec<(f64, f64)> = (0..l.len() / w)
        .map(|c| {
            let (a, b) = (
                db(rms(&l[c * w..(c + 1) * w])),
                db(rms(&h[c * w..(c + 1) * w])),
            );
            (a, b - a)
        })
        .collect();
    let (top, peak) = win.iter().enumerate().fold(
        (0, f64::MIN),
        |m, (i, &(a, _))| if a > m.1 { (i, a) } else { m },
    );
    let body = win[top..]
        .iter()
        .filter(|&&(a, _)| a > peak - 20.0)
        .map(|&(_, r)| r)
        .fold(f64::MIN, f64::max);
    win[top..]
        .iter()
        .filter(|&&(a, r)| a + r > -105.0)
        .map(|&(_, r)| r - body)
        .fold(f64::MIN, f64::max)
}

/// The tail's high band never more than 9 dB brighter than its own body:
/// the dark late tail carries no hiss. 02d039e5 (the gate alone) reached
/// +20.9 to +30.4 dB.
#[test]
fn a_dark_tail_stays_dark() {
    let worst = ReverbType::ALL.map(|t| {
        let e = excess_db(&render(t));
        println!("{}: {e:+.1} dB", ReverbType::NAMES[t.index()]);
        e
    });
    assert!(worst.iter().all(|&e| e <= 9.0), "{worst:.1?} dB");
}

/// Every TYPE's return after a 30 ms noise stab at `amp`, 8 s.
fn stab(t: ReverbType, amp: f32) -> Vec<f32> {
    let p = ReverbParams {
        mix: 1.0,
        ..ReverbParams::default().with_type(t, &KnobMemory::new())
    };
    let mut rv = Box::new(Reverb::of_type(t));
    let mut n = Noise(7);
    run(
        &mut |send| {
            let mut out = Stereo::SILENT;
            rv.process(send, &p, 1.0, Tempo::DEFAULT, SR, &mut out);
            (out.l, out.r)
        },
        &mut |i| {
            let v = n.next();
            if i < 30 * MS { amp * v } else { 0.0 }
        },
        8.0,
    )
    .0
}

/// The worst block edge: its |x[i+1] − 2x[i] + x[i−1]| over the largest
/// in-block one within ±2 blocks, past the onset. With the sample.
fn edge_ratio(x: &[f32]) -> (f32, usize) {
    let d2: Vec<f32> = (1..x.len() - 1)
        .map(|i| (x[i + 1] - 2.0 * x[i] + x[i - 1]).abs())
        .collect();
    // d2[j] bends about x[j + 1]: the edge x[i − 1] → x[i] is d2[i − 2], d2[i − 1].
    (20..d2.len() / BLOCK_SIZE - 3)
        .map(|b| b * BLOCK_SIZE)
        .filter_map(|i| {
            let peer = (i - 2 * BLOCK_SIZE..i + 2 * BLOCK_SIZE)
                .filter(|j| (3..BLOCK_SIZE - 2).contains(&((j + 1) % BLOCK_SIZE)))
                .map(|j| d2[j])
                .fold(0.0f32, f32::max);
            (peer > 1e-7).then(|| (d2[i - 2].max(d2[i - 1]) / peer, i))
        })
        .fold((0.0, 0), |m, r| if r.0 > m.0 { r } else { m })
}

/// No tail bends harder at a block edge than within its blocks by more
/// than the listen QA's click bar (3×): `TailGate`'s gain ramps up as it
/// ramps down, snapping only on an onset. Opening at once stepped HALL
/// 3.4×, BLOOM 14.5×, QUAD 4.9× and REVERSE 3.5× (#499).
#[test]
fn a_tail_never_steps_on_a_block_edge() {
    for t in ReverbType::ALL {
        for amp in [0.5, 0.05, 0.01] {
            let (r, at) = edge_ratio(&stab(t, amp));
            let name = ReverbType::NAMES[t.index()];
            println!("{name} {amp}: {r:.2}× at {at}");
            assert!(r <= 3.0, "{name} at {amp}: {r:.2}× at sample {at}");
        }
    }
}
