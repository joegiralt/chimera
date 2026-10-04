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
