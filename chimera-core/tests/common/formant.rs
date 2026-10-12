//! The formant operators' render fixtures and measures (spec § 3–5).

use super::filter_ref::fft;
use super::{SR, VEL, fundamental_hz, goertzel, rms};
use chimera_core::dsp::algo::algorithms::{AlgoId, plan};
use chimera_core::dsp::algo::engine::{AlgoEngine, AlgoLive};
use chimera_core::dsp::algo::formant::{R_REF, Skirt, formant_law};
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::morph::{Morph, carrier_norm};
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::plan::{OPS, blend};
use chimera_core::dsp::algo::tx::patch_gain;
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
    each_block: impl FnMut(usize, &mut AlgoParams, &mut AlgoLive, &mut AlgoEngine),
) -> (Vec<f32>, u32) {
    render(AlgoEngine::new(), p, note, blocks, each_block)
}

/// As `render_algo_with`, every formant operator on the per-sample
/// reference path (`formant_tick_ref`) in place of its spans.
pub fn render_algo_ref(
    p: &AlgoParams,
    note: u8,
    blocks: usize,
    each_block: impl FnMut(usize, &mut AlgoParams, &mut AlgoLive, &mut AlgoEngine),
) -> (Vec<f32>, u32) {
    let mut e = AlgoEngine::new();
    e.formants_per_sample();
    render(e, p, note, blocks, each_block)
}

fn render(
    mut e: AlgoEngine,
    p: &AlgoParams,
    note: u8,
    blocks: usize,
    mut each_block: impl FnMut(usize, &mut AlgoParams, &mut AlgoLive, &mut AlgoEngine),
) -> (Vec<f32>, u32) {
    let mut p = *p;
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
    (out, e.grain_cuts())
}

/// Operator 1 alone as `kind` on A1: LEVEL 99, KEY 0, VSLOT OFF.
pub fn one(kind: OpKind, frmt: u8, bw: u8, skirt: u8) -> AlgoParams {
    let mut p = AlgoParams::single(WaveId::W1);
    p.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
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

/// The analysis mask, Hz: ±2 bins of an 8,192-point analysis (±11.7 Hz).
const MASK_HZ: f64 = 2.0 * 48_000.0 / 8192.0;

/// The power of 2¹⁴ samples of `x` in bins 0..N/2, with the bin width:
/// under a 4-term Blackman–Harris, whose ±4-bin main lobe is `MASK_HZ` and
/// sidelobes −92 dB (Hann's first, −31 dB, would lie past ±2 bins).
fn bh_power(x: &[f32]) -> (Vec<f64>, f64) {
    const N: usize = 1 << 14;
    assert!(x.len() >= N, "the analysis reads {N} samples");
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
    let pw = (0..N / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect();
    (pw, f64::from(SR) / N as f64)
}

/// Whether `f` lies within the mask of any of `grid`.
fn near(f: f64, grid: &[f64]) -> bool {
    grid.iter().any(|&g| (f - g).abs() <= MASK_HZ)
}

fn power_db(part: f64, whole: f64) -> f32 {
    (10.0 * (part / whole).max(1e-30).log10()) as f32
}

/// The energy farther than `MASK_HZ` from every `grid` frequency, dB re
/// the total.
pub fn off_grid_db(x: &[f32], grid: &[f32]) -> f32 {
    let grid: Vec<f64> = grid.iter().map(|&g| f64::from(g)).collect();
    let (pw, bin) = bh_power(x);
    let off: f64 = (0..pw.len())
        .filter(|&k| !near(k as f64 * bin, &grid))
        .map(|k| pw[k])
        .sum();
    power_db(off, pw.iter().sum())
}

/// The energy of the sidebands `k·f0 ± fm` that a modulator at `fm` puts
/// round `f0`'s harmonics, dB re the total: the lines within the mask of a
/// sideband and not of a harmonic. A modulator whose `fm` is a multiple of
/// `f0` puts every sideband on a harmonic, where it can't be told apart.
pub fn sideband_energy_db(x: &[f32], f0: f32, fm: f32) -> f32 {
    let (f0, fm) = (f64::from(f0), f64::from(fm));
    let harmonics: Vec<f64> = (0..)
        .map(|k| k as f64 * f0)
        .take_while(|&h| h < 24_000.0)
        .collect();
    let sidebands: Vec<f64> = harmonics
        .iter()
        .flat_map(|&h| [h - fm, h + fm])
        .filter(|&f| f > 0.0 && f < 24_000.0 && !near(f, &harmonics))
        .collect();
    assert!(
        !sidebands.is_empty(),
        "fm {fm} Hz puts every sideband on a harmonic of {f0} Hz"
    );
    let (pw, bin) = bh_power(x);
    let side: f64 = (0..pw.len())
        .filter(|&k| near(k as f64 * bin, &sidebands))
        .map(|k| pw[k])
        .sum();
    power_db(side, pw.iter().sum())
}

/// Operator 6's output peak, its tap row, over blocks `from..blocks` of
/// `note` held: a modulator's depth, which the carrier can't give back.
pub fn op6_peak(p: &AlgoParams, note: u8, blocks: usize, from: usize) -> f32 {
    let mut e = AlgoEngine::new();
    e.note_on(
        MidiNote::new(note).unwrap(),
        Velocity::new(VEL).unwrap(),
        p,
        SR,
    );
    let (mut blk, mut peak) = ([0.0; BLOCK_SIZE], 0.0f32);
    for b in 0..blocks {
        let mut taps = TapRows::new([false, true]);
        e.render(&mut blk, p, &AlgoLive::from_params(p), SR, &mut taps);
        if b >= from {
            peak = taps.rows().unwrap()[1]
                .iter()
                .fold(peak, |m, x| m.max(x.abs()));
        }
    }
    peak
}

/// The mean, summed in f64.
pub fn mean(x: &[f32]) -> f32 {
    (x.iter().map(|&v| f64::from(v)).sum::<f64>() / x.len() as f64) as f32
}

/// An amplitude ratio in dB.
pub fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

/// `x`'s fundamental near `hz` (`common::fundamental_hz`), cents re `hz`.
pub fn fundamental_cents(x: &[f32], hz: f32) -> f32 {
    (1200.0 * (fundamental_hz(x, f64::from(hz)) / f64::from(hz)).log2()) as f32
}

/// The level's steps: 40 ms windows' RMS, each against the one 10 ms
/// before, dB. A 10 ms window holds one or two grains at C3, so its RMS
/// alternates with the window's phase, not the level (Task 11 ruling).
pub fn steps_10ms_db(x: &[f32]) -> Vec<f32> {
    let (win, hop) = (SR as usize / 25, SR as usize / 100);
    let w: Vec<f32> = (0..=x.len().saturating_sub(win))
        .step_by(hop)
        .map(|i| rms(&x[i..i + win]))
        .collect();
    w.windows(2).map(|p| db(p[1] / p[0])).collect()
}

/// The peak of `p`'s one carrier before `carrier_norm`, held at `note`
/// for 4 s: the output over the norm, which `p`'s LEVELs and MORPH set.
pub fn carrier_peak_before_norm(p: &AlgoParams, note: u8) -> f32 {
    let (a, b) = p.algs();
    let plan = plan(a, b);
    let m = Morph::from_param(f32::from(p.morph));
    let carriers = (0..OPS)
        .filter(|&i| blend(plan.carrier_a[i], plan.carrier_b[i], m.get()) > 0.0)
        .count();
    assert_eq!(carriers, 1, "one carrier at the MORPH, so the output is it");
    let gains: [f32; OPS] = core::array::from_fn(|i| patch_gain(f32::from(p.ops[i].level)));
    let norm = carrier_norm(&plan, m, &gains);
    let out = render_algo(p, note, 4 * 750);
    out.iter().fold(0.0f32, |m, x| m.max(x.abs())) / norm
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

/// One period of both lanes at `formant_law`'s latched law, synthesised
/// exactly in f64 with no PM (spec § 3): its RMS against R in dB, and its
/// peak. The level is measured from the waveform, not from the law's gain.
pub fn synth_level(f0: f32, fc: f32, bw: f32, s: Skirt) -> (f32, f32) {
    let l = formant_law(f0, fc, bw, s);
    let (d, q) = (f64::from(l.d), s.q() as i32);
    let o = f64::from(l.slack) / 2.0;
    let cycles = f64::from(l.fc) / f64::from(f0);
    let gain = f64::from(l.g_c) * f64::from(l.edge);
    // A lane `u` periods after its start: delayed by `o`, carrier's zero at
    // the window's centre.
    let lane = |u: f64| {
        let x = (u - o) / d;
        if !(0.0..=1.0).contains(&x) {
            return 0.0;
        }
        (4.0 * x * (1.0 - x)).powi(q) * (TAU * cycles * (u - o - d / 2.0)).sin()
    };
    // At least 128 samples a carrier cycle; a lane restarts every two
    // periods, so only the grains started now and a period ago sound.
    let n = ((cycles * 128.0) as usize).max(8192);
    let (mut sum, mut peak) = (0.0f64, 0.0f64);
    for i in 0..n {
        let t = i as f64 / n as f64;
        let y = gain * (lane(t) + lane(t + 1.0));
        sum += y * y;
        peak = peak.max(y.abs());
    }
    let rms = (sum / n as f64).sqrt();
    let r = f64::from(R_REF) / core::f64::consts::SQRT_2;
    ((20.0 * (rms / r).log10()) as f32, peak as f32)
}
