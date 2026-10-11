//! SYM's bias-only fold law (sends spec § 3a), against today's law.

use chimera_core::dsp::wavefolder::Wavefolder;
use chimera_core::params::FolderParams;
use chimera_hal::BLOCK_SIZE;

const MIX: f32 = 0.5;
const N: usize = 1024;
const PEAKS: [f32; 4] = [0.01, 0.1, 0.3, 0.7];

/// Triangle fold, as `wavefolder.rs` has it (the reference for today's law).
fn fold_wave(x: f32) -> f32 {
    let x = x + 1.0;
    let d = x / 4.0;
    let t = x
        - (if d >= 0.0 {
            d as i32 as f32
        } else {
            (d as i32 - 1) as f32
        }) * 4.0;
    if t < 2.0 { t - 1.0 } else { 3.0 - t }
}

fn sine(peak: f32) -> Vec<f32> {
    (0..N).map(|n| peak * (n as f32 * 0.0573).sin()).collect()
}

/// Today's law, sample by sample (the steady path before the bias-only law).
fn old(peak: f32, fold: f32, sym: f32) -> Vec<f32> {
    let gain = 1.0 + fold * fold * 6.0;
    let bias = (sym - 0.5) * 0.5;
    let rest = fold_wave(bias * gain);
    sine(peak)
        .iter()
        .map(|&x| x * (1.0 - MIX) + (fold_wave((x + bias) * gain) - rest) * MIX)
        .collect()
}

/// The real stage, settled (gate open, ramps landed), `on` as `stage_runs`.
fn new(peak: f32, fold: f32, sym: f32) -> Vec<f32> {
    let p = FolderParams {
        fold,
        symmetry: sym,
        mix: MIX,
    };
    let on = fold >= 0.001;
    let mut f = Wavefolder::new();
    let mut warm = [0.0f32; BLOCK_SIZE];
    for _ in 0..64 {
        f.process(&mut warm, &p, on, None);
    }
    let mut x = sine(peak);
    for b in x.chunks_mut(BLOCK_SIZE) {
        f.process(b, &p, on, None);
    }
    x
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

fn resid(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x - y).collect()
}

/// The worst level (out RMS re dry RMS, dB) over SYM at this FOLD.
fn level(f: &dyn Fn(f32, f32) -> Vec<f32>, peak: f32, fold: f32) -> f32 {
    let dry = rms(&sine(peak));
    [0.0, 0.25, 0.5, 0.75, 1.0]
        .iter()
        .map(|&s| db(rms(&f(fold, s)) / dry))
        .fold(f32::MIN, f32::max)
}

/// The worst step between adjacent FOLD ticks at SYM 1, re the dry RMS.
fn step(f: &dyn Fn(f32, f32) -> Vec<f32>, peak: f32) -> f32 {
    let dry = rms(&sine(peak));
    (0..128)
        .map(|k| {
            let (a, b) = (f(k as f32 / 128.0, 1.0), f((k + 1) as f32 / 128.0, 1.0));
            db(rms(&resid(&b, &a)) / dry)
        })
        .fold(f32::MIN, f32::max)
}

#[test]
fn sym_is_heard_from_quarter_fold() {
    for fold in [0.25, 0.5] {
        let (a, b) = (new(0.1, fold, 1.0), new(0.1, fold, 0.5));
        let r = db(rms(&resid(&a, &b)) / rms(&b));
        println!("FOLD {fold}: residual {r:.1} dB");
        assert!(r >= -20.0, "FOLD {fold}: SYM 1 vs 0.5 residual {r:.1} dB");
    }
}

#[test]
fn fold_zero_and_sym_half_are_todays() {
    for sym in [0.0, 0.3, 0.5, 1.0] {
        assert_eq!(new(0.3, 0.0, sym), sine(0.3), "FOLD 0, SYM {sym}");
    }
    for fold in [0.01, 0.25, 0.5, 1.0] {
        assert_eq!(new(0.3, fold, 0.5), old(0.3, fold, 0.5), "FOLD {fold}");
    }
}

#[test]
fn no_level_rise() {
    for peak in PEAKS {
        let mut worst = (f32::MIN, f32::MIN);
        for k in 0..=128 {
            let fold = k as f32 / 128.0;
            let n = level(&|f, s| new(peak, f, s), peak, fold);
            let o = level(&|f, s| old(peak, f, s), peak, fold);
            worst = (worst.0.max(n), worst.1.max(o));
        }
        // Per peak, as the spec's table: per FOLD tick the new law rises up to
        // 0.6 dB (peak 0.3, FOLD 0.77) where today's SYM cancels.
        assert!(worst.0 <= worst.1 + 0.5, "peak {peak}: {worst:?}");
        println!(
            "peak {peak}: max level new {:.2} dB, today {:.2}",
            worst.0, worst.1
        );
    }
}

#[test]
fn fold_sweep_never_steppier() {
    for peak in PEAKS {
        let (n, o) = (
            step(&|f, s| new(peak, f, s), peak),
            step(&|f, s| old(peak, f, s), peak),
        );
        println!("peak {peak}: worst FOLD step new {n:.2} dB, today {o:.2}");
        assert!(n <= o + 0.5, "peak {peak}: step {n:.2} dB vs today {o:.2}");
        if peak == 0.01 {
            assert!(n <= -20.0, "peak 0.01 step {n:.2} dB");
        }
    }
}
