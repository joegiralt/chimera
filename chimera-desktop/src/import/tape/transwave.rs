//! A transwave from a recording (TAPE spec § 6 step 5): whole cycles cut
//! from across it, each built into octave levels from `top` down to 16, each
//! level keeping harmonics `1..=⌊0.35 N⌋` by FFT truncation. The PPG Wave
//! and Waldorf Microwave approach (band-limited copies per octave), for the
//! Ensoniq VFX/SD-1's transwaves. Host only: the firmware reads the levels.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

/// The top level unless `--top 1024` asks for a seventh.
pub const DEFAULT_TOP: u32 = 512;
/// A transwave's most frames (the descriptor's `count`).
pub const MAX_FRAMES: usize = 32;

/// `x` at fractional index `t`: the 4-point Hermite, ends clamped.
fn at(x: &[f32], t: f64) -> f32 {
    let i = t.floor() as isize;
    let f = (t - i as f64) as f32;
    let y = |k: isize| x[(i + k).clamp(0, x.len() as isize - 1) as usize];
    let (y0, y1, y2, y3) = (y(-1), y(0), y(1), y(2));
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * f + c2) * f + c1) * f + y1
}

/// Whole cycles of `cycle` samples (fractional) from across `x`, at most
/// `MAX_FRAMES`: consecutive when `x` holds fewer, else spread evenly over
/// it. Each starts at the first rising zero crossing from its place
/// (interpolated) and is `⌈cycle⌉` samples, read at a step of
/// `cycle / ⌈cycle⌉`. Too short an `x`, or no cycle length, gives none.
pub fn cut_cycles(x: &[f32], cycle: f32) -> Vec<Vec<f32>> {
    if !(cycle.is_finite() && cycle >= 2.0) {
        return Vec::new();
    }
    let (p, len) = (cycle as f64, cycle.ceil() as usize);
    // A period of room after each place for its zero crossing.
    let span = x.len() as f64 - len as f64 - p;
    if span < 0.0 {
        return Vec::new();
    }
    let count = ((span / p) as usize + 1).min(MAX_FRAMES);
    let gap = if count > 1 {
        span / (count - 1) as f64
    } else {
        0.0
    };
    (0..count)
        .map(|k| {
            let from = (k as f64 * gap) as usize;
            let to = (from + p.ceil() as usize).min(x.len() - 1);
            let t0 = (from..to)
                .find(|&i| x[i] <= 0.0 && x[i + 1] > 0.0)
                .map_or(from as f64, |i| {
                    i as f64 + (x[i] / (x[i] - x[i + 1])) as f64
                });
            let step = p / len as f64;
            (0..len).map(|j| at(x, t0 + j as f64 * step)).collect()
        })
        .collect()
}

/// `cycle`'s levels, `top` (a power of two, 16 or more) down to 16, one
/// after another: `2 · top − 16` samples. Level `N` is the cycle's
/// harmonics `1..=⌊0.35 N⌋` (those it has: under half its length) at their
/// own amplitudes and phases, and nothing else, not even DC.
pub fn levels(cycle: &[f32], top: u32) -> Vec<f32> {
    let l = cycle.len();
    let mut planner = FftPlanner::<f64>::new();
    let mut x: Vec<Complex<f64>> = cycle.iter().map(|&v| Complex::new(v as f64, 0.0)).collect();
    if l > 0 {
        planner.plan_fft_forward(l).process(&mut x);
    }
    let mut out = Vec::with_capacity(2 * top as usize - 16);
    let mut n = top as usize;
    while n >= 16 {
        let keep = ((0.35 * n as f64) as usize).min(l.saturating_sub(1) / 2);
        let mut y = vec![Complex::new(0.0, 0.0); n];
        let scale = 1.0 / l as f64;
        for k in 1..=keep {
            y[k] = x[k] * scale;
            y[n - k] = x[k].conj() * scale;
        }
        planner.plan_fft_inverse(n).process(&mut y);
        out.extend(y.iter().map(|c| c.re as f32));
        n /= 2;
    }
    out
}
