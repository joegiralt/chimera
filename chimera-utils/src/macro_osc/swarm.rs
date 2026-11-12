//! SWARM: eight of VA's band-limited oscillators (`va::osc::VarOsc`,
//! called a block at a time, never copied), each on a slow random walk.
//! HARMONICS spreads them: `1200·h³` cents across the eight (≈30 cents at
//! 0.3, an octave at 1). TIMBRE breaks the steady cloud into grains: from
//! 0 (all eight always sounding) to short, sparse Hann grains, each grain
//! drawing its oscillator a fresh random place in the spread. MORPH is
//! VA's WAVE from saw (0) through triangle to sine (1).
//!
//! Per sample: eight VarOsc samples (the oscillator's own BLEP cost, ~8 ×
//! VA's one-oscillator figure) plus a grain envelope (one LUT read and two
//! multiply-adds) each; drift and pitch once a block.
//!
//! Debt to Plaits (structure only): a swarm of eight detuned, enveloped
//! oscillators with randomised pitches and grain density. The waveform,
//! drift, spread law and envelopes are ours, and the oscillator is VA's.

use super::{B, Blk, Ctl, FS, Model};
use crate::drums::parts::{Lcg, expo};
use chimera_core::dsp::fast_sin_turns;
use chimera_core::dsp::va::osc::{MAX_INC, MIN_INC, NO_WRAP, Ramps, VarOsc};

pub const VOICES: usize = 8;
/// Each oscillator's place in the spread, evenly −1..1.
const PLACE: [f32; VOICES] = {
    let mut p = [0.0; VOICES];
    let mut i = 0;
    while i < VOICES {
        p[i] = -1.0 + 2.0 * i as f32 / (VOICES - 1) as f32;
        i += 1;
    }
    p
};
/// The sum's gain: eight uncorrelated oscillators back to about one's RMS,
/// with room for the steady cloud's beating peaks.
const SUM: f32 = 0.25;

#[derive(Clone, Copy)]
struct Grain {
    /// 0..1 through the grain; ≥ 1 between grains.
    u: f32,
    /// Samples of gap left before the next grain.
    wait: u32,
    /// This grain's place in the spread, −1..1.
    place: f32,
}

pub struct Swarm {
    osc: [VarOsc; VOICES],
    inc: [f32; VOICES],
    /// The random walk, cents.
    walk: [f32; VOICES],
    grain: [Grain; VOICES],
    wave: f32,
    rng: Lcg,
    fresh: bool,
}

impl Swarm {
    pub fn new(seed: u32) -> Self {
        let mut rng = Lcg(seed.wrapping_mul(0x9E37_79B9) | 1);
        let osc = core::array::from_fn(|_| {
            let mut o = VarOsc::NEW;
            o.prepare(FS);
            o.set_phase(rng.unit());
            o
        });
        let grain = core::array::from_fn(|_| Grain {
            u: rng.unit(),
            wait: 0,
            place: rng.next(),
        });
        Self {
            osc,
            inc: [0.0; VOICES],
            walk: [0.0; VOICES],
            grain,
            wave: 0.0,
            rng,
            fresh: true,
        }
    }

    /// Each oscillator's increment now (pitch, spread, drift): what the
    /// spread test reads.
    #[cfg(test)]
    pub fn incs(&self) -> [f32; VOICES] {
        self.inc
    }
}

impl Model for Swarm {
    fn block(&mut self, out: &mut Blk, hz: f32, c: Ctl) {
        let (h, t, m) = (
            c.harmonics.clamp(0.0, 1.0),
            c.timbre.clamp(0.0, 1.0),
            c.morph.clamp(0.0, 1.0),
        );
        let spread = 1200.0 * h * h * h;
        // grains: g blends steady to enveloped; length 1.5 s to 15 ms,
        // mean gap growing with TIMBRE (sparser)
        let g = (4.0 * t).min(1.0);
        let len = expo(t, 1.5, 0.015);
        let du = 1.0 / (len * FS);
        let gap = len * 3.0 * t * t * FS;
        // the walk: a leaky random step a block, wider with the spread
        let leak = 0.9985;
        let step = 0.35 * (1.0 + 0.01 * spread);
        let wave = 2.0 / 3.0 * (1.0 - m);
        let (wave_from, wave_step) = if self.fresh {
            (wave, 0.0)
        } else {
            (self.wave, (wave - self.wave) / B as f32)
        };
        // the envelope's mean square, to hold the level as grains thin
        let duty = len * FS / (len * FS + gap);
        let ms = (1.0 - g) * (1.0 - g) + 2.0 * g * (1.0 - g) * 0.5 * duty + g * g * 0.375 * duty;
        let level = SUM / ms.max(0.16).sqrt();
        out.fill(0.0);
        let mut buf: Blk = [0.0; B];
        let mut mk = [[NO_WRAP; 2]; B];
        for v in 0..VOICES {
            self.walk[v] = self.walk[v] * leak + step * self.rng.next();
            let gr = &mut self.grain[v];
            let place = PLACE[v] + (gr.place - PLACE[v]) * g;
            let cents = place * 0.5 * spread + self.walk[v];
            let to = (hz * (cents / 1200.0).exp2() / FS).clamp(MIN_INC, MAX_INC);
            let from = if self.fresh { to } else { self.inc[v] };
            self.inc[v] = to;
            let r = Ramps {
                inc: (from, (to - from) / B as f32),
                wave: (wave_from, wave_step),
                shape: (0.0, 0.0),
            };
            self.osc[v].render::<B, false, false, false, false>(
                &mut buf,
                &mut mk,
                &r,
                &mut 1.0,
                &mut || 1.0,
                &[0.0; B],
                (0.0, 0.0),
            );
            for (o, x) in out.iter_mut().zip(buf.iter()) {
                let e = if gr.u < 1.0 {
                    let s = fast_sin_turns(0.5 * gr.u);
                    gr.u += du;
                    s * s
                } else if gr.wait > 0 {
                    gr.wait -= 1;
                    0.0
                } else {
                    gr.u = 0.0;
                    gr.wait = (gap * 2.0 * self.rng.unit()) as u32;
                    gr.place = self.rng.next();
                    0.0
                };
                *o += x * (1.0 - g + g * e);
            }
        }
        for o in out.iter_mut() {
            *o *= level;
        }
        self.wave = wave;
        self.fresh = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustfft::{FftPlanner, num_complex::Complex};

    /// Energy-weighted spread (Hz) of the spectrum within ±40 % of `hz`.
    fn spread_hz(h: f32, hz: f32) -> f32 {
        let mut m = Swarm::new(5);
        let n = 1 << 16;
        let mut x = Vec::with_capacity(n + B);
        let mut b: Blk = [0.0; B];
        while x.len() < n {
            m.block(&mut b, hz, Ctl::new(h, 0.0, 1.0));
            x.extend_from_slice(&b);
        }
        let mut buf: Vec<Complex<f32>> = x[..n]
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let w = 0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / n as f32).cos();
                Complex::new(s * w, 0.0)
            })
            .collect();
        FftPlanner::new().plan_fft_forward(n).process(&mut buf);
        let bin = FS / n as f32;
        let (mut e, mut ef, mut ef2) = (0.0f64, 0.0f64, 0.0f64);
        for (i, v) in buf[..n / 2].iter().enumerate() {
            let f = i as f32 * bin;
            if (f - hz).abs() < 0.4 * hz {
                let p = v.norm_sqr() as f64;
                e += p;
                ef += p * f as f64;
                ef2 += p * (f * f) as f64;
            }
        }
        let mean = ef / e;
        (ef2 / e - mean * mean).max(0.0).sqrt() as f32
    }

    #[test]
    fn harmonics_detunes_the_eight() {
        let incs = |h| {
            let mut m = Swarm::new(5);
            let mut b: Blk = [0.0; B];
            m.block(&mut b, 220.0, Ctl::new(h, 0.0, 0.0));
            let i = m.incs();
            i.iter().fold(0.0f32, |a, &x| a.max(x)) / i.iter().fold(1.0f32, |a, &x| a.min(x))
        };
        assert!(incs(0.0) < 1.001, "spread at 0: {}", incs(0.0));
        assert!(incs(0.5) > 1.08, "spread at ½: {}", incs(0.5));
        // on the sine (MORPH 1), the fundamental's spectral spread grows
        let (s0, s3, s6) = (
            spread_hz(0.0, 220.0),
            spread_hz(0.3, 220.0),
            spread_hz(0.6, 220.0),
        );
        assert!(s3 > 2.0 * s0 && s6 > 2.0 * s3, "{s0} {s3} {s6}");
    }
}
