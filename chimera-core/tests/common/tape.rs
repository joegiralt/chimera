//! Sources for TAPE's tests: `held` bytes as the arena holds them (a
//! descriptor, then packed 12-bit samples) and the signals put in them.
#![allow(dead_code)]

use chimera_core::dsp::clampf;
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape};
use chimera_core::storage::twelve::pack12;

use super::XorShift;

/// `x` in [−1, 1] as a 12-bit sample, rounded; full scale clips to 2047.
pub fn to12(x: f32) -> i16 {
    clampf((x * 2048.0).round(), -2048.0, 2047.0) as i16
}

/// `samples` packed after a `Plain` descriptor at `rate`, held whole.
pub fn held_plain(samples: &[f32], rate: u16) -> Vec<u8> {
    let d = Descriptor {
        shape: Shape::Plain,
        count: 1,
        rate,
        head_frames: 0,
    };
    let s: Vec<i16> = samples.iter().map(|&x| to12(x)).collect();
    let mut held = d.to_bytes().to_vec();
    held.resize(DESC_LEN + 3 * s.len().div_ceil(2), 0);
    pack12(&s, &mut held[DESC_LEN..]);
    held
}

/// `frames` of a full-scale sine `period` frames long. The samples don't
/// depend on `_rate`: a source holds frames, and its rate plays them.
pub fn sine(_rate: u16, period: f32, frames: u32) -> Vec<f32> {
    (0..frames)
        .map(|i| libm::sinf(core::f32::consts::TAU * i as f32 / period))
        .collect()
}

/// `secs` of a 0.5 saw at 220 Hz with 50 ms of seeded white noise at
/// 0.5 from a quarter of the way in: a pitched tone and a transient.
pub fn saw_with_burst(rate: u16, secs: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    let (from, len) = (n / 4, rate as usize / 20);
    let mut rng = XorShift(0x5A3B_0001);
    (0..n)
        .map(|i| {
            let ph = (i as f32 * 220.0 / rate as f32).fract();
            let noise = if (from..from + len).contains(&i) {
                rng.next_u32() as i32 as f32 / i32::MAX as f32 * 0.5
            } else {
                0.0
            };
            (ph - 0.5) + noise
        })
        .collect()
}

/// A `Transwave` of `count` frames at octave levels `top` down to 16, frame
/// `f`'s sine-phase harmonic `h` at amplitude `amp(f, h)`: each level `N`
/// keeps harmonics `1..=⌊0.35 N⌋`, as the importer's do.
pub fn held_transwave(top: u32, count: u8, amp: impl Fn(usize, u32) -> f32) -> Vec<u8> {
    let d = Descriptor {
        shape: Shape::Transwave,
        count,
        rate: 48_000,
        head_frames: 0,
    };
    let mut s: Vec<i16> = Vec::new();
    for f in 0..count as usize {
        let mut n = top;
        while n >= 16 {
            let parts: Vec<(u32, f32)> = (1..=(n as f32 * 0.35) as u32)
                .map(|h| (h, amp(f, h)))
                .filter(|&(_, a)| a != 0.0)
                .collect();
            s.extend((0..n).map(|i| {
                let x = parts.iter().fold(0.0f64, |y, &(h, a)| {
                    let w = core::f64::consts::TAU * (h as u64 * i as u64 % n as u64) as f64;
                    y + a as f64 * (w / n as f64).sin()
                });
                to12(x as f32)
            }));
            n /= 2;
        }
    }
    let mut held = d.to_bytes().to_vec();
    held.resize(DESC_LEN + 3 * s.len() / 2, 0);
    pack12(&s, &mut held[DESC_LEN..]);
    held
}

/// In-place iterative radix-2 FFT; `re.len()` is a power of two.
pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -core::f64::consts::TAU / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// Power per bin, 0..=n/2, of `x` (a power of two long) under a 4-term
/// Blackman-Harris window: sidelobes 92 dB down, a main lobe ±4 bins.
pub fn power_bh(x: &[f32]) -> Vec<f64> {
    let n = x.len();
    let w = |i: usize| {
        let t = core::f64::consts::TAU * i as f64 / n as f64;
        0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos()
    };
    let mut re: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, &v)| v as f64 * w(i))
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    (0..=n / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect()
}
