//! The formant operators' render fixtures and measures (spec § 3–5).

use super::filter_ref::fft;
use super::{SR, VEL, goertzel, rms};
use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::engine::{AlgoEngine, AlgoLive};
use chimera_core::dsp::algo::formant::R_REF;
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::vowel::VowelSlot;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::taps::TapRows;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use std::f64::consts::TAU;

/// `blocks` blocks of `note` at VEL, held.
pub fn render_algo(p: &AlgoParams, note: u8, blocks: usize) -> Vec<f32> {
    render_algo_with(p, note, blocks, |_, _, _, _| {}).0
}

/// As `render_algo`, with `each_block` run before each block's render. The
/// block's `AlgoLive` is rebuilt from the params first, so a params edit
/// plays from the next block and a live edit in this one. Also returns the
/// grains cut: none until the kernel runs grains.
pub fn render_algo_with(
    p: &AlgoParams,
    note: u8,
    blocks: usize,
    mut each_block: impl FnMut(usize, &mut AlgoParams, &mut AlgoLive, &mut AlgoEngine),
) -> (Vec<f32>, u32) {
    let mut p = *p;
    let mut e = AlgoEngine::new();
    e.note_on(
        MidiNote::new(note).unwrap(),
        Velocity::new(VEL).unwrap(),
        &p,
        SR,
    );
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut blk = [0.0; BLOCK_SIZE];
    for b in 0..blocks {
        let mut live = AlgoLive::from_params(&p);
        each_block(b, &mut p, &mut live, &mut e);
        e.render(&mut blk, &p, &live, SR, &mut TapRows::new([false; 2]));
        out.extend_from_slice(&blk);
    }
    (out, 0)
}

/// Operator 1 alone as `kind` on A1: LEVEL 99, KEY 0, VSLOT OFF.
pub fn one(kind: OpKind, frmt: u8, bw: u8, skirt: u8) -> AlgoParams {
    let mut p = AlgoParams::single(WaveId::W1);
    (p.alg_a, p.alg_b) = (AlgoId::A1.get(), AlgoId::A1.get());
    p.try_set_kind(0, kind).unwrap();
    let o = &mut p.ops[0];
    (o.frmt, o.bw, o.skirt, o.key) = (frmt, bw, skirt, 0);
    o.vslot = VowelSlot::Off.code();
    p
}

/// The carrier's frequency: the mean zero-crossing rate over the half
/// cycles inside one run of non-zero samples (a grain, or overlapping
/// ones) whose lobe tops 1 % of the peak, so the gaps between grains
/// don't count.
pub fn grain_fc(out: &[f32]) -> f32 {
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    // (position, run): an exact zero ends a run.
    let mut crossings: Vec<(f32, usize)> = Vec::new();
    let mut run = 0;
    for (i, w) in out.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        if a == 0.0 || b == 0.0 {
            run += 1;
        } else if (a < 0.0) != (b < 0.0) {
            crossings.push((i as f32 + a / (a - b), run));
        }
    }
    let (mut span, mut halves) = (0.0f64, 0u32);
    for c in crossings.windows(2) {
        let ((c0, r0), (c1, r1)) = (c[0], c[1]);
        if r0 != r1 {
            continue;
        }
        let lobe = &out[c0.ceil() as usize..=c1 as usize];
        if lobe.iter().any(|x| x.abs() > 0.01 * peak) {
            span += f64::from(c1 - c0);
            halves += 1;
        }
    }
    (f64::from(SR) * f64::from(halves) / (2.0 * span)) as f32
}

/// Hann-windowed magnitude of `x[..n]` in bins 0..n/2, dB re the largest;
/// `n` a power of two.
pub fn spectrum_db(x: &[f32], n: usize) -> Vec<f32> {
    let mut re: Vec<f64> = x[..n]
        .iter()
        .enumerate()
        .map(|(i, &v)| f64::from(v) * (0.5 - 0.5 * (TAU * i as f64 / n as f64).cos()))
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let mag: Vec<f64> = (0..n / 2).map(|k| re[k].hypot(im[k])).collect();
    let max = mag.iter().cloned().fold(f64::MIN_POSITIVE, f64::max);
    mag.iter()
        .map(|m| (20.0 * (m / max).max(1e-15).log10()) as f32)
        .collect()
}

/// The energy farther than ±2 bins of an 8,192-point analysis (±11.7 Hz)
/// from every `grid` frequency, dB re the total. Hann's first sidelobe
/// (−31 dB) lies past ±2 bins, so this reads 2¹⁴ samples under a 4-term
/// Blackman–Harris instead: its ±4-bin main lobe is the same ±11.7 Hz, its
/// sidelobes −92 dB.
pub fn off_grid_db(x: &[f32], grid: &[f32]) -> f32 {
    const N: usize = 1 << 14;
    const MASK_HZ: f64 = 2.0 * 48_000.0 / 8192.0;
    assert!(x.len() >= N, "off_grid_db reads {N} samples");
    let mut re: Vec<f64> = x[..N]
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let a = TAU * i as f64 / N as f64;
            let w =
                0.35875 - 0.48829 * a.cos() + 0.14128 * (2.0 * a).cos() - 0.01168 * (3.0 * a).cos();
            f64::from(v) * w
        })
        .collect();
    let mut im = vec![0.0; N];
    fft(&mut re, &mut im);
    let bin = f64::from(SR) / N as f64;
    let (mut total, mut off) = (0.0, 0.0);
    for k in 0..N / 2 {
        let pw = re[k] * re[k] + im[k] * im[k];
        total += pw;
        let f = k as f64 * bin;
        if grid.iter().all(|&g| (f - f64::from(g)).abs() > MASK_HZ) {
            off += pw;
        }
    }
    (10.0 * (off / total).max(1e-30).log10()) as f32
}

/// `common::goertzel`'s magnitude at `hz`, dB.
pub fn harmonic_db(x: &[f32], hz: f32) -> f32 {
    20.0 * goertzel(x, hz, SR).max(1e-12).log10()
}

/// RMS against R, −12 dB of a full-scale sine's RMS.
pub fn level_db_vs_r(x: &[f32]) -> f32 {
    20.0 * (rms(x) / (R_REF / core::f32::consts::SQRT_2)).log10()
}

/// The loudest 10 ms (1 ms hop) against `steady_rms`, dB.
pub fn worst_10ms_db(x: &[f32], steady_rms: f32) -> f32 {
    let (win, hop) = (SR as usize / 100, SR as usize / 1000);
    let worst = (0..=x.len().saturating_sub(win))
        .step_by(hop)
        .map(|i| rms(&x[i..i + win]))
        .fold(0.0f32, f32::max);
    20.0 * (worst / steady_rms).log10()
}
