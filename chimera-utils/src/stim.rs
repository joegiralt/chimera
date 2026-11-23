// Copyright (c) 2005-2024 NumPy Developers; Copyright (c) 2019 Kevin Sheppard (NumPy, BSD-3-Clause)
// Copyright 2014 Melissa O'Neill (PCG64, Apache-2.0 or MIT; notices below)
// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later AND BSD-3-Clause AND (Apache-2.0 OR MIT)

//! Test signals: the click, interrupted noise (ISO 3382-2), the Latin
//! hypercube and the listening set, drawn as numpy's `default_rng(seed)`
//! draws them (SeedSequence, PCG64, the ziggurat normal), so a seed gives the
//! Python harness's noise.
//!
//! Third-party code, ported:
//!
//! - `seed_state` is NumPy's `SeedSequence` (numpy/random/bit_generator.pyx);
//!   `Rng::normal`, `Rng::uniform` and `Rng::interval` are NumPy's
//!   `random_standard_normal`, `next_double` and `random_interval`
//!   (numpy/random/src/distributions/distributions.c), and `Rng::shuffle` its
//!   `Generator.shuffle`. Under NumPy's BSD-3-Clause license:
//!
//!   Copyright (c) 2005-2024, NumPy Developers.
//!   Copyright (c) 2019 Kevin Sheppard (numpy.random, from randomgen).
//!   All rights reserved.
//!
//!   Redistribution and use in source and binary forms, with or without
//!   modification, are permitted provided that the following conditions are
//!   met:
//!
//!   * Redistributions of source code must retain the above copyright
//!     notice, this list of conditions and the following disclaimer.
//!   * Redistributions in binary form must reproduce the above copyright
//!     notice, this list of conditions and the following disclaimer in the
//!     documentation and/or other materials provided with the distribution.
//!   * Neither the name of the NumPy Developers nor the names of any
//!     contributors may be used to endorse or promote products derived from
//!     this software without specific prior written permission.
//!
//!   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
//!   IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
//!   TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
//!   PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
//!   OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
//!   EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
//!   PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
//!   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
//!   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
//!   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
//!   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
//!
//! - PCG64 (XSL-RR 128/64, `Rng::new`/`step`/`next_u64`) is Melissa
//!   O'Neill's PCG family (pcg-random.org), dual-licensed Apache-2.0 or MIT,
//!   as NumPy's PCG64 is; used here under the MIT license:
//!
//!   Copyright 2014 Melissa O'Neill <oneill@pcg-random.org>
//!
//!   Permission is hereby granted, free of charge, to any person obtaining a
//!   copy of this software and associated documentation files (the
//!   "Software"), to deal in the Software without restriction, including
//!   without limitation the rights to use, copy, modify, merge, publish,
//!   distribute, sublicense, and/or sell copies of the Software, and to
//!   permit persons to whom the Software is furnished to do so, subject to
//!   the following conditions:
//!
//!   The above copyright notice and this permission notice shall be included
//!   in all copies or substantial portions of the Software.
//!
//!   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
//!   OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//!   MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
//!   IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
//!   CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
//!   TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
//!   SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//! - The ziggurat's tables are rebuilt by Marsaglia and Tsang's recurrence
//!   ("The Ziggurat Method for Generating Random Variables", JSS 2000).

use crate::SR;
use crate::sig::{Sos, sosfilt};
use std::f64::consts::PI;
use std::sync::OnceLock;

pub const CLICK_AT: usize = 480;
pub const NOISE_SEEDS: usize = 16;
const NOISE_RMS_DB: f64 = -18.0;

/// numpy's `SeedSequence(seed).generate_state(4, uint64)`: the pool of four
/// 32-bit words mixed from the seed, then drawn out as eight.
fn seed_state(seed: u32) -> [u64; 4] {
    const INIT_A: u32 = 0x43b0_d7e5;
    const MULT_A: u32 = 0x931e_8875;
    const INIT_B: u32 = 0x8b51_f9dd;
    const MULT_B: u32 = 0x58f3_8ded;
    const MIX_L: u32 = 0xca01_f9dd;
    const MIX_R: u32 = 0x4973_f715;
    let mut ha = INIT_A;
    let mut hashmix = |v: u32| {
        let mut v = v ^ ha;
        ha = ha.wrapping_mul(MULT_A);
        v = v.wrapping_mul(ha);
        v ^ (v >> 16)
    };
    let mix = |x: u32, y: u32| {
        let r = MIX_L.wrapping_mul(x).wrapping_sub(MIX_R.wrapping_mul(y));
        r ^ (r >> 16)
    };
    let mut pool = [0u32; 4];
    for (i, w) in pool.iter_mut().enumerate() {
        *w = hashmix(if i == 0 { seed } else { 0 });
    }
    for src in 0..4 {
        for dst in 0..4 {
            if src != dst {
                let h = hashmix(pool[src]);
                pool[dst] = mix(pool[dst], h);
            }
        }
    }
    let mut hb = INIT_B;
    let words: Vec<u32> = (0..8)
        .map(|i| {
            let mut v = pool[i % 4] ^ hb;
            hb = hb.wrapping_mul(MULT_B);
            v = v.wrapping_mul(hb);
            v ^ (v >> 16)
        })
        .collect();
    core::array::from_fn(|i| words[2 * i] as u64 | (words[2 * i + 1] as u64) << 32)
}

/// The ziggurat's tables (256 levels, 52-bit), built by Marsaglia and
/// Tsang's recurrence. numpy's were computed at higher precision; these
/// agree to about 1e-14, below anything an f32 render can see.
struct Zig {
    ki: [u64; 256],
    wi: [f64; 256],
    fi: [f64; 256],
}

const ZIG_R: f64 = 3.654_152_885_361_009;
/// Each layer's area: `r·e^(−r²/2) + √(π/2)·erfc(r/√2)`.
const ZIG_V: f64 = 0.004_928_673_233_974_658;

fn zig() -> &'static Zig {
    static Z: OnceLock<Zig> = OnceLock::new();
    Z.get_or_init(|| {
        let m1 = (1u64 << 52) as f64;
        let mut z = Zig {
            ki: [0; 256],
            wi: [0.0; 256],
            fi: [0.0; 256],
        };
        let (mut dn, mut tn) = (ZIG_R, ZIG_R);
        let q = ZIG_V / (-0.5 * dn * dn).exp();
        z.ki[0] = ((dn / q) * m1) as u64;
        z.wi[0] = q / m1;
        z.wi[255] = dn / m1;
        z.fi[0] = 1.0;
        z.fi[255] = (-0.5 * dn * dn).exp();
        for i in (1..255).rev() {
            dn = (-2.0 * (ZIG_V / dn + (-0.5 * dn * dn).exp()).ln()).sqrt();
            z.ki[i + 1] = ((dn / tn) * m1) as u64;
            tn = dn;
            z.fi[i] = (-0.5 * dn * dn).exp();
            z.wi[i] = dn / m1;
        }
        z
    })
}

/// numpy's `Generator(PCG64(SeedSequence(seed)))`.
pub struct Rng {
    state: u128,
    inc: u128,
    /// The upper half of the last 64-bit draw, unused (`next_uint32`).
    half: Option<u32>,
}

const PCG_MULT: u128 = 0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645;

impl Rng {
    pub fn new(seed: u32) -> Self {
        let v = seed_state(seed);
        let init = (v[0] as u128) << 64 | v[1] as u128;
        let seq = (v[2] as u128) << 64 | v[3] as u128;
        let mut r = Self {
            state: 0,
            inc: (seq << 1) | 1,
            half: None,
        };
        r.step();
        r.state = r.state.wrapping_add(init);
        r.step();
        r
    }

    fn step(&mut self) {
        self.state = self.state.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
    }

    /// XSL-RR of the advanced state.
    pub fn next_u64(&mut self) -> u64 {
        self.step();
        let s = self.state;
        ((s >> 64) as u64 ^ s as u64).rotate_right((s >> 122) as u32)
    }

    fn next_u32(&mut self) -> u32 {
        if let Some(h) = self.half.take() {
            return h;
        }
        let v = self.next_u64();
        self.half = Some((v >> 32) as u32);
        v as u32
    }

    /// `random()`: uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// `standard_normal()`: the ziggurat.
    pub fn normal(&mut self) -> f64 {
        let z = zig();
        loop {
            let r = self.next_u64();
            let idx = (r & 0xff) as usize;
            let r = r >> 8;
            let rabs = (r >> 1) & 0x000f_ffff_ffff_ffff;
            let mut x = rabs as f64 * z.wi[idx];
            if r & 1 == 1 {
                x = -x;
            }
            if rabs < z.ki[idx] {
                return x;
            }
            if idx == 0 {
                loop {
                    let xx = -(1.0 / ZIG_R) * (-self.uniform()).ln_1p();
                    let yy = -(-self.uniform()).ln_1p();
                    if yy + yy > xx * xx {
                        return if (rabs >> 8) & 1 == 1 {
                            -(ZIG_R + xx)
                        } else {
                            ZIG_R + xx
                        };
                    }
                }
            } else if (z.fi[idx - 1] - z.fi[idx]) * self.uniform() + z.fi[idx]
                < (-0.5 * x * x).exp()
            {
                return x;
            }
        }
    }

    /// An integer in `0..=max` by masked rejection (`random_interval`).
    fn interval(&mut self, max: u64) -> u64 {
        if max == 0 {
            return 0;
        }
        let mask = u64::MAX >> max.leading_zeros();
        loop {
            let v = if max <= u32::MAX as u64 {
                self.next_u32() as u64
            } else {
                self.next_u64()
            } & mask;
            if v <= max {
                return v;
            }
        }
    }

    /// `shuffle()`: Fisher–Yates from the end.
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.interval(i as u64) as usize;
            v.swap(i, j);
        }
    }
}

/// `seconds` of silence with a unit impulse at 10 ms.
pub fn click(seconds: f64) -> Vec<f64> {
    let mut x = vec![0.0; (seconds * SR) as usize];
    x[CLICK_AT] = 1.0;
    x
}

/// Gaussian noise at −18 dBFS RMS (clipped at ±0.95, 2 ms raised-cosine
/// ends) for `charge_s`, then `tail_s` of silence; and where it stops.
pub fn noise_stim(seed: u32, charge_s: f64, tail_s: f64) -> (Vec<f64>, usize) {
    let mut rng = Rng::new(seed);
    let n = (charge_s * SR) as usize;
    let g = 10f64.powf(NOISE_RMS_DB / 20.0);
    let mut x: Vec<f64> = (0..n)
        .map(|_| (rng.normal() * g).clamp(-0.95, 0.95))
        .collect();
    let r = (0.002 * SR) as usize;
    for i in 0..r {
        let a = (PI / 2.0) * i as f64 / (r - 1) as f64;
        x[i] *= a.sin().powi(2);
        x[n - r + i] *= a.cos().powi(2);
    }
    x.resize(n + (tail_s * SR) as usize, 0.0);
    (x, n)
}

/// `n` points of a Latin hypercube in `[0, 1)^d`.
pub fn lhs(n: usize, d: usize, seed: u32) -> Vec<Vec<f64>> {
    let mut rng = Rng::new(seed);
    let mut u: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            (0..d)
                .map(|_| (i as f64 + rng.uniform()) / n as f64)
                .collect()
        })
        .collect();
    for j in 0..d {
        let mut col: Vec<f64> = u.iter().map(|r| r[j]).collect();
        rng.shuffle(&mut col);
        for (r, v) in u.iter_mut().zip(col) {
            r[j] = v;
        }
    }
    u
}

// ── the listening set ──

/// The A/B listening stimuli, in page order: each mono at 48 kHz, peaked at
/// −12 dBFS and followed by 12 s of silence for the tail.
pub const LISTEN: [&str; 7] = [
    "snare",
    "bass",
    "stab",
    "sine_short",
    "sine_long",
    "texture",
    "hats",
];
const LISTEN_PEAK_DB: f64 = -12.0;
const LISTEN_TAIL_S: f64 = 12.0;

/// The listening stimulus `name` (one of `LISTEN`).
pub fn listen(name: &str) -> Option<Vec<f64>> {
    let mut x = match name {
        "snare" => snare(),
        "bass" => bass(),
        "stab" => stab(),
        "sine_short" => sine(440.0, 0.08),
        "sine_long" => sine(220.0, 2.0),
        "texture" => texture(),
        "hats" => hats(),
        _ => return None,
    };
    let g = 10f64.powf(LISTEN_PEAK_DB / 20.0) / x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    x.iter_mut().for_each(|v| *v *= g);
    x.resize(x.len() + (LISTEN_TAIL_S * SR) as usize, 0.0);
    Some(x)
}

/// The listening set as mono 24-bit WAVs, `<dir>/<name>.wav` (the effects
/// demos' hats and snare, spec § 4).
pub fn write_listening(dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    listening()
        .into_iter()
        .map(|(name, x)| {
            let p = dir.join(format!("{name}.wav"));
            crate::reference::write_pcm24(&p, &x, SR as u32).map(|()| p)
        })
        .collect()
}

/// Every listening stimulus, by name.
pub fn listening() -> Vec<(&'static str, Vec<f64>)> {
    LISTEN
        .iter()
        .filter_map(|&n| Some((n, listen(n)?)))
        .collect()
}

fn hz(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

/// Raised-cosine (sin²) edges: `rise_s` in, `fall_s` out.
fn edges(x: &mut [f64], rise_s: f64, fall_s: f64) {
    let ramp = |i: usize, m: usize| ((PI / 2.0) * i as f64 / m as f64).sin().powi(2);
    let (r, f) = ((rise_s * SR) as usize, (fall_s * SR) as usize);
    for (i, v) in x.iter_mut().take(r).enumerate() {
        *v *= ramp(i, r);
    }
    for (i, v) in x.iter_mut().rev().take(f).enumerate() {
        *v *= ramp(i, f);
    }
}

/// A band-limited saw at phase `ph` (radians): harmonics 1..=`k`.
fn saw(ph: f64, k: usize) -> f64 {
    (1..=k).map(|j| (j as f64 * ph).sin() / j as f64).sum()
}

/// RBJ cookbook band-pass (0 dB peak) or high-pass, as one section.
fn rbj(f0: f64, q: f64, highpass: bool) -> Sos {
    let w = 2.0 * PI * f0 / SR;
    let (c, alpha) = (w.cos(), w.sin() / (2.0 * q));
    let a0 = 1.0 + alpha;
    let b = if highpass {
        [(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0]
    } else {
        [alpha, 0.0, -alpha]
    };
    [
        b[0] / a0,
        b[1] / a0,
        b[2] / a0,
        1.0,
        -2.0 * c / a0,
        (1.0 - alpha) / a0,
    ]
}

/// `snare`: `tools/ab/stimuli.py`'s dry snare, 300 ms: a body falling
/// 190 → 160 Hz (τ 20 ms) decaying τ 50 ms at 0.6, plus a uniform noise
/// rattle decaying τ 70 ms at 0.8. Seed 343 (the PCG64 draw, not Python's
/// Mersenne Twister: the same recipe, other noise).
fn snare() -> Vec<f64> {
    let mut rng = Rng::new(343);
    let mut ph = 0.0;
    (0..(0.3 * SR) as usize)
        .map(|i| {
            let t = i as f64 / SR;
            ph += 2.0 * PI * (160.0 + 30.0 * (-t / 0.02).exp()) / SR;
            let body = ph.sin() * (-t / 0.05).exp();
            let rattle = (2.0 * rng.uniform() - 1.0) * (-t / 0.07).exp();
            0.6 * body + 0.8 * rattle
        })
        .collect()
}

/// `bass`: A1 (55 Hz) falling from an octave up (τ 8 ms); a sine plus a
/// band-limited saw at 0.25 (harmonics under 20 kHz at 110 Hz), 400 ms:
/// 2 ms rise, decay τ 100 ms (−26 dB at 300 ms), a 10 ms raised-cosine end.
fn bass() -> Vec<f64> {
    let k = (20_000.0 / 110.0) as usize;
    let mut ph = 0.0;
    let mut x: Vec<f64> = (0..(0.4 * SR) as usize)
        .map(|i| {
            let t = i as f64 / SR;
            ph += 2.0 * PI * 55.0 * (1.0 + (-t / 0.008).exp()) / SR;
            (ph.sin() + 0.25 * saw(ph, k)) * (-t / 0.1).exp()
        })
        .collect();
    edges(&mut x, 0.002, 0.01);
    x
}

/// `stab`: an A minor 7 (A3 C4 E4 G4, equal temperament) of band-limited
/// saws (harmonics under 20 kHz), 150 ms: 3 ms rise, decay τ 150 ms, a
/// 15 ms raised-cosine release.
fn stab() -> Vec<f64> {
    let notes = [57.0, 60.0, 64.0, 67.0].map(hz);
    let mut x: Vec<f64> = (0..(0.15 * SR) as usize)
        .map(|i| {
            let t = i as f64 / SR;
            let s: f64 = notes
                .iter()
                .map(|&f| saw(2.0 * PI * f * t, (20_000.0 / f) as usize))
                .sum();
            s * (-t / 0.15).exp()
        })
        .collect();
    edges(&mut x, 0.003, 0.015);
    x
}

/// `sine_short` (440 Hz, 80 ms) and `sine_long` (220 Hz, 2 s): a sine
/// with 5 ms raised-cosine edges.
fn sine(f: f64, seconds: f64) -> Vec<f64> {
    let mut x: Vec<f64> = (0..(seconds * SR) as usize)
        .map(|i| (2.0 * PI * f * i as f64 / SR).sin())
        .collect();
    edges(&mut x, 0.005, 0.005);
    x
}

/// `texture`: 300 ms of 48 band-passed noise grains. Each grain: a Hann
/// window 10–40 ms long, starting anywhere it fits, of Gaussian noise
/// through an RBJ band-pass (Q 6) centred 300 Hz–8 kHz (log-uniform), at
/// 0.3–1.0. Seed 11; per grain the length, start, centre and level are
/// drawn, then the noise.
fn texture() -> Vec<f64> {
    let n = (0.3 * SR) as usize;
    let mut rng = Rng::new(11);
    let mut x = vec![0.0; n];
    for _ in 0..48 {
        let len = ((0.01 + 0.03 * rng.uniform()) * SR) as usize;
        let at = (rng.uniform() * (n - len) as f64) as usize;
        let fc = 300.0 * (8000.0f64 / 300.0).powf(rng.uniform());
        let g = 0.3 + 0.7 * rng.uniform();
        let w: Vec<f64> = (0..len).map(|_| rng.normal()).collect();
        let y = sosfilt(&[rbj(fc, 6.0, false)], &w);
        for (i, v) in y.iter().enumerate() {
            let hann = (PI * i as f64 / (len - 1) as f64).sin().powi(2);
            x[at + i] += g * hann * v;
        }
    }
    x
}

/// `hats`: 16ths at 120 BPM (125 ms apart). Closed hats on steps 0–5
/// (levels 1.0, 0.6 alternating), 40 ms, decay τ 10 ms; an open hat on step
/// 6, 300 ms, decay τ 90 ms; 1.05 s in all. Each a burst of uniform noise
/// through two RBJ high-passes (7 kHz, Q 0.707), with a 0.5 ms rise. Seed 5.
fn hats() -> Vec<f64> {
    const STEP: f64 = 0.125;
    let hp = [rbj(7000.0, 0.707, true), rbj(7000.0, 0.707, true)];
    let mut rng = Rng::new(5);
    let mut x = vec![0.0; (1.05 * SR) as usize];
    let hits = (0..6)
        .map(|s| (s, if s % 2 == 0 { 1.0 } else { 0.6 }, 0.04, 0.01))
        .chain([(6, 1.0, 0.3, 0.09)]);
    for (step, g, len_s, tau) in hits {
        let len = (len_s * SR) as usize;
        let w: Vec<f64> = (0..len).map(|_| 2.0 * rng.uniform() - 1.0).collect();
        let mut y = sosfilt(&hp, &w);
        for (i, v) in y.iter_mut().enumerate() {
            *v *= g * (-(i as f64) / SR / tau).exp();
        }
        edges(&mut y, 0.0005, 0.0);
        let at = (step as f64 * STEP * SR) as usize;
        for (i, v) in y.into_iter().enumerate() {
            x[at + i] += v;
        }
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_draws_what_numpy_default_rng_draws() {
        // np.random.default_rng(s).bit_generator.random_raw(3)
        let mut r = Rng::new(0);
        let raw = [r.next_u64(), r.next_u64(), r.next_u64()];
        assert_eq!(
            raw,
            [
                11749869230777074271,
                4976686463289251617,
                755828109848996024
            ]
        );
        let mut r = Rng::new(15);
        assert_eq!(r.next_u64(), 12778859617612771073);
        // default_rng(1).standard_normal(3)
        let mut r = Rng::new(1);
        let n = [r.normal(), r.normal(), r.normal()];
        // Equal to the tables' precision (about 1e-14).
        let want = [0.345584192064786, 0.8216181435011584, 0.33043707618338714];
        assert!(
            n.iter().zip(want).all(|(a, b)| (a / b - 1.0).abs() < 1e-13),
            "{n:?}"
        );
        // default_rng(1): random(5), then permutation(10) and permutation(7)
        let mut r = Rng::new(1);
        (0..5).for_each(|_| _ = r.uniform());
        let mut p: Vec<u32> = (0..10).collect();
        r.shuffle(&mut p);
        assert_eq!(p, [1, 0, 6, 9, 7, 2, 3, 4, 5, 8]);
        let mut p: Vec<u32> = (0..7).collect();
        r.shuffle(&mut p);
        assert_eq!(p, [1, 3, 0, 6, 4, 5, 2]);
    }

    #[test]
    fn each_listening_stimulus_is_deterministic_finite_peaked_and_has_its_tail() {
        let tail = (LISTEN_TAIL_S * SR) as usize;
        let peak = 10f64.powf(LISTEN_PEAK_DB / 20.0);
        let a = listening();
        assert_eq!(a.len(), LISTEN.len());
        for ((name, x), (_, y)) in a.iter().zip(listening()) {
            let bits = |v: &[f64]| v.iter().map(|s| s.to_bits()).collect::<Vec<_>>();
            assert_eq!(bits(x), bits(&y), "{name}: not byte-identical");
            assert!(x.iter().all(|v| v.is_finite()), "{name}: not finite");
            let pk = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            assert!((pk / peak - 1.0).abs() < 1e-12, "{name}: peak {pk}");
            assert!(x.len() > tail, "{name}");
            assert!(
                x[x.len() - tail..].iter().all(|&v| v == 0.0),
                "{name}: tail"
            );
        }
        assert!(listen("idm").is_none() && listen("strings").is_none());
    }

    #[test]
    fn stim_writes_listening_set() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/stim-test");
        let _ = std::fs::remove_dir_all(&dir);
        let files = write_listening(&dir).unwrap();
        assert_eq!(files.len(), LISTEN.len());
        for name in ["hats", "snare"] {
            let p = dir.join(format!("{name}.wav"));
            let r = hound::WavReader::open(&p).unwrap();
            let s = r.spec();
            assert_eq!((s.channels, s.sample_rate), (1, SR as u32), "{name}");
            assert_eq!(r.len() as usize, listen(name).unwrap().len(), "{name}");
        }
    }
}
