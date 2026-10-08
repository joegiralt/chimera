//! Encoder turns through the panel's acceleration, as ticks landing in each
//! 1.33 ms block: the detent timings of the filter root-cause report.
#![allow(dead_code)]

use chimera_hal::{BLOCK_SIZE, ENCODER_BURST_GAP_MS, SAMPLE_RATE, encoder_accel};

pub const MS_PER_BLOCK: f32 = BLOCK_SIZE as f32 * 1000.0 / SAMPLE_RATE as f32;

/// The fast turn's detent period, ms.
pub const FAST_DETENT_MS: f32 = 30.0;

/// Ticks landing in each of `n` blocks from detents at `(ms, dir)`,
/// accelerated as the panel does.
pub fn ticks(detents: &[(f32, i8)], n: usize) -> Vec<i8> {
    let mut out = vec![0i8; n];
    let (mut burst, mut last) = (0u8, f32::NEG_INFINITY);
    for &(t, dir) in detents {
        if t - last > ENCODER_BURST_GAP_MS as f32 {
            burst = 0;
        }
        last = t;
        let b = (t / MS_PER_BLOCK) as usize;
        out[b] = out[b].saturating_add(dir * encoder_accel(burst));
        burst = burst.saturating_add(1);
    }
    out
}

/// The report's fast turn: 12 detents up, one every 30 ms; then 12 down,
/// one every 200 ms (no acceleration). 3.4 s.
pub fn fast_turn() -> Vec<i8> {
    let mut d: Vec<(f32, i8)> = (0..12)
        .map(|i| (100.0 + FAST_DETENT_MS * i as f32, 1))
        .collect();
    d.extend((0..12).map(|i| (800.0 + 200.0 * i as f32, -1)));
    ticks(&d, (3400.0 / MS_PER_BLOCK) as usize)
}

/// The report's slow turn: 8 detents up, one every 200 ms. 2 s.
pub fn slow_turn() -> Vec<i8> {
    let d: Vec<(f32, i8)> = (0..8).map(|i| (100.0 + 200.0 * i as f32, 1)).collect();
    ticks(&d, (2000.0 / MS_PER_BLOCK) as usize)
}
