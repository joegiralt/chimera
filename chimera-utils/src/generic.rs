//! The generic A/B: any reference program against any Chimera device, on
//! noise (and a sine, for an effect) or from nothing (a source).

use crate::Config;
use crate::devices::{Device, SR};
use crate::mv::rms_match;
use crate::reference::{Reference, write_pcm24};
use crate::stim::Rng;
use rustfft::FftPlanner;
use rustfft::num_complex::Complex64;
use serde_json::json;
use std::f64::consts::PI;

const SECONDS: f64 = 2.0;
const SINE_HZ: f64 = 1000.0;
const NPERSEG: usize = 4096;
/// Response bins below this, re the reference's peak, are not compared.
const FLOOR_DB: f64 = -60.0;

/// Octave bands, by centre: 31.25 Hz to 16 kHz.
fn octaves() -> Vec<f64> {
    (0..10).map(|k| 31.25 * 2f64.powi(k)).collect()
}

fn fft(x: &[f64]) -> Vec<Complex64> {
    let mut b: Vec<Complex64> = x.iter().map(|&v| Complex64::new(v, 0.0)).collect();
    FftPlanner::new().plan_fft_forward(b.len()).process(&mut b);
    b
}

fn hann(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos())
        .collect()
}

/// Welch cross and auto spectra: (Pxy, Pxx, Pyy) per bin, Hann, half overlap.
fn spectra(x: &[f64], y: &[f64]) -> (Vec<Complex64>, Vec<f64>, Vec<f64>) {
    let n = NPERSEG;
    let w = hann(n);
    let bins = n / 2 + 1;
    let (mut pxy, mut pxx, mut pyy) = (
        vec![Complex64::new(0.0, 0.0); bins],
        vec![0.0; bins],
        vec![0.0; bins],
    );
    let mut s = 0;
    while s + n <= x.len().min(y.len()) {
        let a = fft(&(0..n).map(|i| x[s + i] * w[i]).collect::<Vec<_>>());
        let b = fft(&(0..n).map(|i| y[s + i] * w[i]).collect::<Vec<_>>());
        for k in 0..bins {
            pxy[k] += a[k].conj() * b[k];
            pxx[k] += a[k].norm_sqr();
            pyy[k] += b[k].norm_sqr();
        }
        s += n / 2;
    }
    (pxy, pxx, pyy)
}

fn bin_hz(k: usize) -> f64 {
    k as f64 * SR as f64 / NPERSEG as f64
}

/// Each side's magnitude response (dB per bin): |Pxy|/Pxx of an effect on
/// `input`, or the output's own spectrum for a source.
fn response(input: Option<&[f64]>, y: &[f64]) -> Vec<f64> {
    match input {
        Some(x) => {
            let (pxy, pxx, _) = spectra(x, y);
            pxy.iter()
                .zip(&pxx)
                .map(|(c, p)| 20.0 * (c.norm() / p + 1e-30).log10())
                .collect()
        }
        None => {
            let (_, _, pyy) = spectra(y, y);
            pyy.iter().map(|p| 10.0 * (p + 1e-30).log10()).collect()
        }
    }
}

/// The largest |ours − ref| (dB) in each octave band, over the bins where
/// the reference is within `FLOOR_DB` of its peak; `None` where none are.
fn band_errors(r: &[f64], o: &[f64]) -> Vec<(f64, Option<f64>)> {
    let top = r.iter().skip(1).copied().fold(f64::NEG_INFINITY, f64::max);
    octaves()
        .into_iter()
        .map(|fc| {
            let e = (1..r.len())
                .filter(|&k| bin_hz(k) >= fc / 2f64.sqrt() && bin_hz(k) < fc * 2f64.sqrt())
                .filter(|&k| r[k] >= top + FLOOR_DB)
                .map(|k| (o[k] - r[k]).abs())
                .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
            (fc, e)
        })
        .collect()
}

/// Null residual: `ours` aligned to `r` (best lag within ±100 ms, best
/// gain), the residual's energy re the reference's, in dB; with the lag
/// (samples, positive: ours late) and the gain applied to ours (dB).
pub fn null_residual(r: &[f64], ours: &[f64]) -> (f64, i64, f64) {
    let n = r.len().min(ours.len());
    let m = (2 * n).next_power_of_two();
    let pad = |x: &[f64]| {
        let mut v = x[..n].to_vec();
        v.resize(m, 0.0);
        v
    };
    let (a, b) = (fft(&pad(r)), fft(&pad(ours)));
    let mut c: Vec<Complex64> = a.iter().zip(&b).map(|(x, y)| x.conj() * y).collect();
    FftPlanner::new().plan_fft_inverse(m).process(&mut c);
    let max_lag = (SR / 10) as i64;
    let lag = (-max_lag..=max_lag)
        .max_by(|&p, &q| {
            let at = |l: i64| c[l.rem_euclid(m as i64) as usize].re;
            at(p).total_cmp(&at(q))
        })
        .unwrap_or(0);
    // ours[i + lag] lines up with r[i].
    let pairs = || {
        (0..n as i64)
            .filter(move |&i| (0..n as i64).contains(&(i + lag)))
            .map(move |i| (r[i as usize], ours[(i + lag) as usize]))
    };
    let (rb, bb) = pairs().fold((0.0, 0.0), |(rb, bb), (x, y)| (rb + x * y, bb + y * y));
    let g = if bb > 0.0 { rb / bb } else { 0.0 };
    let res: f64 = pairs().map(|(x, y)| (x - g * y).powi(2)).sum();
    let er: f64 = r[..n].iter().map(|v| v * v).sum();
    (
        10.0 * (res / er + 1e-30).log10(),
        lag,
        20.0 * g.abs().max(1e-30).log10(),
    )
}

/// THD (ratio) of `y`'s steady second half: harmonics 2–10 against the
/// fundamental (near `f0`, or the strongest bin), Blackman–Harris window.
pub fn thd(y: &[f64], f0: Option<f64>) -> f64 {
    let half = &y[y.len() / 2..];
    let n = 1usize << (usize::BITS - 1 - half.len().leading_zeros());
    let w = |i: usize| {
        let t = 2.0 * PI * i as f64 / n as f64;
        0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos()
    };
    let s = fft(&(0..n).map(|i| half[i] * w(i)).collect::<Vec<_>>());
    let p: Vec<f64> = s[..n / 2].iter().map(|c| c.norm_sqr()).collect();
    let hz = SR as f64 / n as f64;
    let k0 = match f0 {
        Some(f) => {
            let k = (f / hz).round() as usize;
            (k.saturating_sub(3)..=(k + 3).min(p.len() - 1))
                .max_by(|&a, &b| p[a].total_cmp(&p[b]))
                .unwrap_or(k)
        }
        None => (1..p.len())
            .max_by(|&a, &b| p[a].total_cmp(&p[b]))
            .unwrap_or(1),
    };
    let f = k0 as f64 * hz;
    let power = |fh: f64| {
        let k = (fh / hz).round() as usize;
        p[k.saturating_sub(4)..(k + 5).min(p.len())]
            .iter()
            .sum::<f64>()
    };
    let fund = power(f);
    let harm: f64 = (2..=10)
        .map(|h| h as f64 * f)
        .filter(|&fh| fh < 0.5 * SR as f64 - 8.0 * hz)
        .map(power)
        .sum();
    (harm / fund).sqrt()
}

/// Spectral centroid (20 Hz–20 kHz).
pub fn centroid(y: &[f64]) -> f64 {
    let (_, _, p) = spectra(y, y);
    let (mut num, mut den) = (0.0, 0.0);
    for (k, v) in p.iter().enumerate() {
        let f = bin_hz(k);
        if (20.0..=20_000.0).contains(&f) {
            num += f * v;
            den += v;
        }
    }
    num / den
}

fn noise() -> Vec<f64> {
    let mut rng = Rng::new(1);
    let g = 10f64.powf(-18.0 / 20.0);
    (0..(SECONDS * SR as f64) as usize)
        .map(|_| (rng.normal() * g).clamp(-0.95, 0.95))
        .collect()
}

fn sine() -> Vec<f64> {
    let g = 10f64.powf(-12.0 / 20.0);
    (0..(SECONDS * SR as f64) as usize)
        .map(|i| g * (2.0 * PI * SINE_HZ * i as f64 / SR as f64).sin())
        .collect()
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-30).log10()
}

/// `just ab REF DEVICE [k=v …]`: renders both, prints the metrics, writes
/// the A/B WAVs and `report.json` to `out/ab/<ref>_<device>/`. Pairs go to
/// both sides; `ref.k=v` to the reference alone.
pub fn ab(cfg: &Config, ref_name: &str, dev_name: &str, args: &[String]) -> Result<(), String> {
    let dev = Device::find(dev_name)?;
    let mut both = Vec::new();
    let mut ref_only = Vec::new();
    for a in args {
        let (k, v) = a.split_once('=').ok_or(format!("`{a}`: want key=value"))?;
        match k.strip_prefix("ref.") {
            Some(k) => ref_only.push((k.to_string(), v.to_string())),
            None => both.push((k.to_string(), v.to_string())),
        }
    }
    let values = dev.values(&both)?;
    let r = Reference::find(
        ref_name,
        &cfg.emu().unwrap_or_default(),
        cfg.refs_dir.as_deref(),
    )?;
    let rp: Vec<(String, String)> = both.iter().chain(&ref_only).cloned().collect();
    let tmp = cfg.tmp();
    let frames = (SECONDS * SR as f64) as usize;
    let render = |x: Option<&[f64]>| -> Result<(Vec<f64>, Vec<f64>), String> {
        let theirs = r.render(&rp, x, frames, &tmp)?;
        let xf: Vec<f32> = x.unwrap_or(&[]).iter().map(|&v| v as f32).collect();
        let ours: Vec<f64> = dev
            .render(&values, &xf, frames)
            .into_iter()
            .map(f64::from)
            .collect();
        Ok((theirs, ours))
    };
    let (input, (theirs, ours)) = if dev.is_source() {
        (None, render(None)?)
    } else {
        let x = noise();
        let out = render(Some(&x))?;
        (Some(x), out)
    };
    let (rr, ro) = (
        response(input.as_deref(), &theirs),
        response(input.as_deref(), &ours),
    );
    let bands = band_errors(&rr, &ro);
    let max_err = bands.iter().filter_map(|b| b.1).fold(0.0, f64::max);
    let (null_db, lag, gain_db) = null_residual(&theirs, &ours);
    let (thd_r, thd_o) = if dev.is_source() {
        (thd(&theirs, None), thd(&ours, None))
    } else {
        let (a, b) = render(Some(&sine()))?;
        (thd(&a, Some(SINE_HZ)), thd(&b, Some(SINE_HZ)))
    };
    let (cr, co) = (centroid(&theirs), centroid(&ours));
    println!("{ref_name} vs {dev_name} {}", args.join(" "));
    println!("  null residual   {null_db:+.1} dB (lag {lag} smp, gain {gain_db:+.2} dB)");
    println!("  response error  max {max_err:.2} dB; by octave:");
    for (fc, e) in &bands {
        match e {
            Some(e) => println!("    {fc:>8.1} Hz  {e:6.2} dB"),
            None => println!("    {fc:>8.1} Hz       – (below {FLOOR_DB} dB)"),
        }
    }
    println!(
        "  THD             ref {:.3} % ({:+.1} dB), ours {:.3} % ({:+.1} dB)",
        100.0 * thd_r,
        db(thd_r),
        100.0 * thd_o,
        db(thd_o)
    );
    println!(
        "  centroid        ref {cr:.0} Hz, ours {co:.0} Hz ({:+.1} %)",
        100.0 * (co / cr - 1.0)
    );
    let ref_slug = std::path::Path::new(ref_name)
        .file_name()
        .map_or(ref_name.into(), |s| s.to_string_lossy());
    let dir = cfg.out.join("ab").join(format!("{ref_slug}_{dev_name}"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let [a, b] = rms_match([&theirs, &ours]);
    write_pcm24(&dir.join("ref.wav"), &a, SR)?;
    write_pcm24(&dir.join("chimera.wav"), &b, SR)?;
    let report = json!({
        "reference": ref_name, "device": dev_name, "args": args,
        "null_residual_db": null_db, "lag_samples": lag, "gain_db": gain_db,
        "response_max_error_db": max_err,
        "response_error_db_by_octave": bands.iter().map(|(f, e)| json!([f, e])).collect::<Vec<_>>(),
        "thd_ref": thd_r, "thd_ours": thd_o, "centroid_ref_hz": cr, "centroid_ours_hz": co,
    });
    std::fs::write(
        dir.join("report.json"),
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("  {}", dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delayed_scaled_copy_nulls_and_a_pure_sine_has_no_thd() {
        let x = noise();
        let mut y = vec![0.0; 37];
        y.extend(x.iter().map(|v| 0.5 * v));
        let (null, lag, gain) = null_residual(&x, &y);
        assert_eq!(lag, 37);
        assert!(null < -100.0 && (gain - 6.02).abs() < 0.01, "{null} {gain}");
        assert!(thd(&sine(), Some(SINE_HZ)) < 1e-4);
    }
}
