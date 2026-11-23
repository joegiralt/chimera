// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! TAPE's read interpolator A/B (#698 finding 8): today's 4-point Hermite
//! against a polyphase windowed-sinc, on a 24 kHz 12-bit source pitched
//! −12, 0, +12 and +24 st, rendered at 48 kHz for listening. Nothing
//! in the firmware changes: the A/B comes first.
//!
//! The sinc is 8 taps × 32 phases (a 1 KB table at 16 bits), Kaiser β 7,
//! cut at 0.45 of the source rate, its phases linearly interpolated: about
//! 16 multiply-adds a read, +40 cycles a sample a head over Hermite's ~25
//! (est.). `sinc-bl` is the same kernel stretched by `inc` above 1, so a
//! pitch up band-limits instead of folding: `8 · inc` taps, which costs
//! more as the pitch rises; shown so a listener can hear what fold costs.

use std::f64::consts::PI;
use std::path::Path;

use chimera_engine::dsp::tape_engine::read::hermite;

use crate::reference::write_pcm24;
use crate::stim;

/// The source's rate.
const SRC: f64 = 24_000.0;
/// The output's.
const OUT: f64 = crate::SR;
const TAPS: usize = 8;
const PHASES: usize = 32;
/// The cut, a share of the source's rate.
const CUT: f64 = 0.45;
const BETA: f64 = 7.0;
/// The pitches A/B'd, semitones from the source's own.
const PITCHES: [i32; 4] = [-12, 0, 12, 24];
/// The stimuli: transients, noise-like and a chord (A/B listening rule).
const STIMULI: [&str; 3] = ["hats", "texture", "stab"];
/// Seconds of each render.
const SECS: f64 = 3.0;

fn bessel_i0(x: f64) -> f64 {
    let (mut s, mut t) = (1.0, 1.0);
    for k in 1..40 {
        t *= (x / (2.0 * k as f64)).powi(2);
        s += t;
    }
    s
}

/// The windowed sinc at `x` source frames from the read, cut `fc` (cycles
/// a source frame), `half` frames each side.
fn kernel(x: f64, fc: f64, half: f64) -> f64 {
    if x.abs() >= half {
        return 0.0;
    }
    let s = if x == 0.0 {
        2.0 * fc
    } else {
        (2.0 * PI * fc * x).sin() / (PI * x)
    };
    let r = x / half;
    s * bessel_i0(BETA * (1.0 - r * r).sqrt()) / bessel_i0(BETA)
}

/// The polyphase table, `[phase][tap]`, phase `p` reading at `p / PHASES`,
/// each phase normalized to unity gain at DC, quantized to 16 bits as the
/// firmware would hold it.
fn table() -> Vec<[f64; TAPS]> {
    let half = TAPS as f64 / 2.0;
    (0..=PHASES)
        .map(|p| {
            let t = p as f64 / PHASES as f64;
            let mut k = [0.0; TAPS];
            for (j, c) in k.iter_mut().enumerate() {
                *c = kernel(j as f64 - (half - 1.0) - t, CUT, half);
            }
            let g: f64 = k.iter().sum();
            k.map(|c| (c / g * 32_768.0).round() / 32_768.0)
        })
        .collect()
}

/// `x` (48 kHz) at 24 kHz, 12 bits: a long windowed-sinc low-pass and
/// every other sample, as the importer's default source.
fn to_source(x: &[f64]) -> Vec<i32> {
    let half = 32.0;
    let taps: Vec<f64> = (-32..=32).map(|k| kernel(k as f64, 0.23, half)).collect();
    let g: f64 = taps.iter().sum();
    (0..x.len() / 2)
        .map(|n| {
            let i = 2 * n as i64;
            let y: f64 = taps
                .iter()
                .enumerate()
                .map(|(j, c)| {
                    let k = i + j as i64 - 32;
                    if (0..x.len() as i64).contains(&k) {
                        c * x[k as usize]
                    } else {
                        0.0
                    }
                })
                .sum();
            ((y / g * 2048.0).round() as i32).clamp(-2048, 2047)
        })
        .collect()
}

fn at(src: &[i32], i: i64) -> i32 {
    src[i.clamp(0, src.len() as i64 - 1) as usize]
}

/// Today's read: `read::hermite` on the 12-bit taps.
fn read_hermite(src: &[i32], pos: f64) -> f64 {
    let w = pos.floor();
    let i = w as i64;
    let y = [at(src, i - 1), at(src, i), at(src, i + 1), at(src, i + 2)];
    f64::from(hermite(y, (pos - w) as f32))
}

/// The polyphase sinc: the two nearest phases' taps blended.
fn read_sinc(src: &[i32], tab: &[[f64; TAPS]], pos: f64) -> f64 {
    let w = pos.floor();
    let f = (pos - w) * PHASES as f64;
    let (p, u) = (f.floor() as usize, f - f.floor());
    let i = w as i64 - (TAPS as i64 / 2 - 1);
    (0..TAPS)
        .map(|j| {
            let c = tab[p][j] + u * (tab[p + 1][j] - tab[p][j]);
            c * f64::from(at(src, i + j as i64))
        })
        .sum::<f64>()
        / 2048.0
}

/// The sinc stretched by `inc` (> 1): cut at `CUT / inc`, `8 · inc` taps.
fn read_sinc_bl(src: &[i32], pos: f64, inc: f64) -> f64 {
    let half = TAPS as f64 / 2.0 * inc;
    let fc = CUT / inc;
    let (lo, hi) = ((pos - half).ceil() as i64, (pos + half).floor() as i64);
    let (mut y, mut g) = (0.0, 0.0);
    for k in lo..=hi {
        let c = kernel(k as f64 - pos, fc, half);
        y += c * f64::from(at(src, k));
        g += c;
    }
    y / g / 2048.0
}

/// The energy over the source's Nyquist, dB under the whole: images and
/// fold, what the ear calls grain.
fn over_nyquist_db(y: &[f64], inc: f64) -> f64 {
    use rustfft::{FftPlanner, num_complex::Complex};
    let n = 1 << 15;
    let x: Vec<Complex<f64>> = y
        .iter()
        .take(n)
        .enumerate()
        .map(|(i, v)| {
            Complex::new(
                v * (0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()),
                0.0,
            )
        })
        .chain(std::iter::repeat(Complex::new(0.0, 0.0)))
        .take(n)
        .collect();
    let mut x = x;
    FftPlanner::new().plan_fft_forward(n).process(&mut x);
    // The pitched source's band ends at its Nyquist pitched, `inc · OUT /
    // 2`; past the output's, nothing is over it (fold is `vs_reference`'s).
    let edge = (inc * OUT / 2.0).min(OUT / 2.0);
    let (mut all, mut over) = (0.0, 0.0);
    for (k, c) in x.iter().take(n / 2).enumerate() {
        let hz = k as f64 * OUT / n as f64;
        let e = c.norm_sqr();
        all += e;
        if hz > edge * 1.02 {
            over += e;
        }
    }
    10.0 * (over / all.max(1e-30)).max(1e-30).log10()
}

/// `y` against a long band-limited read of the same (32 taps a side, the
/// kernel stretched by `inc` above 1): the error's level, dB under it.
fn vs_reference(src: &[i32], y: &[f64], inc: f64) -> f64 {
    let half = 16.0 * inc.max(1.0);
    let fc = 0.49 / inc.max(1.0);
    let (mut e, mut r) = (0.0, 0.0);
    for (i, v) in y.iter().enumerate().skip(64).step_by(7) {
        let pos = i as f64 * inc;
        let (lo, hi) = ((pos - half).ceil() as i64, (pos + half).floor() as i64);
        let (mut z, mut g) = (0.0, 0.0);
        for k in lo..=hi {
            let c = kernel(k as f64 - pos, fc, half);
            z += c * f64::from(at(src, k));
            g += c;
        }
        let z = z / g / 2048.0;
        (e, r) = (e + (v - z).powi(2), r + z * z);
    }
    10.0 * (e / r.max(1e-30)).max(1e-30).log10()
}

/// Every A/B into `dir`: `<stim>_<±st>_<hermite|sinc|sinc-bl>.wav`, with
/// each one's energy past the band printed.
pub fn write(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tab = table();
    for name in STIMULI {
        let x = stim::listen(name).ok_or_else(|| format!("no stimulus {name}"))?;
        let src = to_source(&x);
        for st in PITCHES {
            let inc = SRC / OUT * 2f64.powf(f64::from(st) / 12.0);
            let n = (SECS * OUT) as usize;
            let len = (src.len() as f64 / inc).min(n as f64) as usize;
            let render =
                |f: &dyn Fn(f64) -> f64| (0..len).map(|i| f(i as f64 * inc)).collect::<Vec<_>>();
            let mut takes = vec![
                ("hermite", render(&|p| read_hermite(&src, p))),
                ("sinc", render(&|p| read_sinc(&src, &tab, p))),
            ];
            if inc > 1.0 {
                takes.push(("sinc-bl", render(&|p| read_sinc_bl(&src, p, inc))));
            }
            for (kind, y) in takes {
                let p = dir.join(format!("{name}_{st:+}st_{kind}.wav"));
                write_pcm24(&p, &y, OUT as u32)?;
                println!(
                    "{:<34} images {:6.1} dB  off ideal {:6.1} dB",
                    p.file_name().map_or("".into(), |f| f.to_string_lossy()),
                    over_nyquist_db(&y, inc),
                    vs_reference(&src, &y, inc)
                );
            }
        }
    }
    Ok(())
}
