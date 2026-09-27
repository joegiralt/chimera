//! The reverb: four stages in a ring, after Sean Costello's description of
//! the Alesis Quadraverb (Valhalla DSP, KVR thread 349039): "4 parallel
//! loops (2 x AP + 1 delay), outputs from delay taps", each loop feeding the
//! next. Half rate, stored as i16 (FX diet spec § Reverb, ADR 0028).

use crate::dsp::algo::math::exp2;
use core::f32::consts::{LOG2_10, LOG2_E, PI};

pub const STAGES: usize = 4;
/// Per stage: allpass 1, allpass 2, delay.
pub const LINES: usize = 3 * STAGES;
pub const SIZE_STEPS: usize = 32;
/// Every line at SIZE 1, in ring samples.
pub const BASE: [u16; LINES] = [
    151, 313, 4903, 181, 353, 5147, 211, 397, 5393, 241, 277, 5641,
];
/// Per stage: the shorter allpass +c, the longer −c.
pub const AP_COEF: [[f32; 2]; STAGES] = [[0.6, -0.6], [0.5, -0.5], [0.6, -0.6], [0.5, -0.5]];
/// (stage, fraction of its delay, sign); left, then right.
pub const TAPS: [[(usize, f32, f32); 3]; 2] = [
    [(0, 0.07, 1.0), (1, 0.41, -1.0), (2, 0.73, 1.0)],
    [(2, 0.11, 1.0), (3, 0.47, -1.0), (0, 0.79, 1.0)],
];
/// 1.0 in i16 LSBs: ±2.0 fits.
pub const FULL_SCALE: f32 = 16_384.0;
/// Ring samples a SIZE step's crossfade lasts (30 ms at 24 kHz).
pub const XFADE: u16 = 720;
pub const MAX_GAIN: f32 = 0.97;

const fn offsets() -> [usize; LINES] {
    let mut o = [0; LINES];
    let mut j = 1;
    while j < LINES {
        o[j] = o[j - 1] + BASE[j - 1] as usize + 1;
        j += 1;
    }
    o
}
const OFFSET: [usize; LINES] = offsets();
pub const RING_LEN: usize = OFFSET[LINES - 1] + BASE[LINES - 1] as usize + 1;
const _: () = assert!(RING_LEN == 23_220);
const _: () = assert!(core::mem::size_of::<[i16; RING_LEN]>() == 46_440);

const fn is_prime(n: u32) -> bool {
    if n < 2 {
        return false;
    }
    let mut d = 2;
    while d * d <= n {
        if n.is_multiple_of(d) {
            return false;
        }
        d += 1;
    }
    true
}

const fn taken(row: &[u16; LINES], upto: usize, n: u32) -> bool {
    let mut j = 0;
    while j < upto {
        if row[j] as u32 == n {
            return true;
        }
        j += 1;
    }
    false
}

/// Step i scales every SIZE-1 length by 0.35 + 0.65·i/31, to the nearest
/// prime not already used in the step (the lower on a tie).
const fn size_table() -> [[u16; LINES]; SIZE_STEPS] {
    let mut t = [[0u16; LINES]; SIZE_STEPS];
    let mut i = 0;
    while i < SIZE_STEPS {
        let s = 0.35 + 0.65 * i as f32 / 31.0;
        let mut j = 0;
        while j < LINES {
            let want = (BASE[j] as f32 * s + 0.5) as u32;
            let mut d = 0;
            loop {
                if is_prime(want - d) && !taken(&t[i], j, want - d) {
                    t[i][j] = (want - d) as u16;
                    break;
                }
                if is_prime(want + d) && !taken(&t[i], j, want + d) {
                    t[i][j] = (want + d) as u16;
                    break;
                }
                d += 1;
            }
            j += 1;
        }
        i += 1;
    }
    t
}
pub const SIZE_TABLE: [[u16; LINES]; SIZE_STEPS] = size_table();

const fn fits() -> bool {
    let mut i = 0;
    while i < SIZE_STEPS {
        let mut j = 0;
        while j < LINES {
            if SIZE_TABLE[i][j] > BASE[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
const _: () = assert!(fits());

/// round(SIZE·31).
pub fn size_step(size: f32) -> u8 {
    (size.clamp(0.0, 1.0) * 31.0 + 0.5) as u8
}

/// Stage k's share of the round trip at `step`: D + M1 + M2.
pub fn stage_len(step: u8, k: usize) -> u16 {
    let t = &SIZE_TABLE[step as usize % SIZE_STEPS];
    t[3 * k] + t[3 * k + 1] + t[3 * k + 2]
}

/// The shortest RT60 at `step`: the longest allpass rings about
/// 13.5·M / fs_ring on its own.
pub fn t_min(step: u8, fs_ring: f32) -> f32 {
    let t = &SIZE_TABLE[step as usize % SIZE_STEPS];
    let m = (0..STAGES).fold(0, |m, k| m.max(t[3 * k]).max(t[3 * k + 1]));
    (1.5 * 13.5 * m as f32 / fs_ring).max(0.3)
}

/// Target RT60 in seconds: 0.3·40^TIME, no shorter than `t_min`.
pub fn rt60(time: f32, step: u8, fs_ring: f32) -> f32 {
    const LOG2_40: f32 = 5.321_928;
    (0.3 * exp2(time.clamp(0.0, 1.0) * LOG2_40)).max(t_min(step, fs_ring))
}

/// g_k = 10^(−3·L_k / (RT60·fs_ring)), capped at `MAX_GAIN`.
pub fn stage_gains(rt60: f32, step: u8, fs_ring: f32) -> [f32; STAGES] {
    core::array::from_fn(|k| {
        let l = stage_len(step, k) as f32;
        exp2(-3.0 * LOG2_10 * l / (rt60 * fs_ring)).min(MAX_GAIN)
    })
}

/// DAMP's one-pole coefficient, 1 − exp(−2π·f_c / fs_ring), with
/// f_c = 11,000·(1.5/11)^DAMP Hz.
pub fn damp_coef(damp: f32, fs_ring: f32) -> f32 {
    const LOG2_RATIO: f32 = -2.874_469; // log2(1.5 / 11)
    let fc = 11_000.0 * exp2(damp.clamp(0.0, 1.0) * LOG2_RATIO);
    1.0 - exp2(-2.0 * PI * fc / fs_ring * LOG2_E)
}

/// The earliest a send can reach the return at `step`, in output samples:
/// the shortest tap at twice the ring's rate (the FIRs only add to it).
pub fn first_reflection(step: u8) -> usize {
    let t = &SIZE_TABLE[step as usize % SIZE_STEPS];
    TAPS.iter()
        .flatten()
        .map(|&(k, frac, _)| 2 * (frac * t[3 * k + 2] as f32) as usize)
        .min()
        .unwrap_or(0)
}

/// GRIT's grid: a step of Δ = 2^(6·GRIT) i16 LSBs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    delta: f32,
    inv: f32,
}

impl Grid {
    pub fn new(grit: f32) -> Self {
        let delta = exp2(6.0 * grit.clamp(0.0, 1.0));
        Self {
            delta,
            inv: 1.0 / delta,
        }
    }

    pub fn delta(self) -> f32 {
        self.delta
    }

    /// `x` (in LSBs) rounded to the nearest grid point, and that to the
    /// nearest LSB, both ties away from zero; `as i16` saturates.
    #[inline(always)]
    pub fn q(self, x: f32) -> i16 {
        rnd(rnd(x * self.inv) as f32 * self.delta) as i16
    }
}

/// Nearest integer, ties away from zero (`core` has no `f32::round`); `as`
/// saturates.
#[inline(always)]
fn rnd(x: f32) -> i32 {
    (x + if x < 0.0 { -0.5 } else { 0.5 }) as i32
}
