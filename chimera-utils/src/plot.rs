// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `voice-plot`: WAVs side by side as an SVG, for comparing formant
//! operators by eye: each one's waveform over a few note periods at one
//! moment (peak-normalised, stacked), their spectra there overlaid (dB to
//! each one's peak), and each one's spectral-envelope peaks over time.

use crate::SR;
use crate::sig::plan;
use crate::voice::envelope_tracks;
use rustfft::num_complex::Complex64;
use std::f64::consts::PI;
use std::fmt::Write;

const W: f64 = 900.0;
const PANEL_H: f64 = 220.0;
const PAD: f64 = 48.0;
/// One line colour a file, in order; readable on white.
const INK: [&str; 4] = ["#1f5fbf", "#c4381c", "#2c8a3a", "#8a4fbf"];

/// The spectrum of 8192 Hann-windowed samples from `at`, dB re its peak,
/// 0 Hz–`top`, one point a bin.
fn spectrum(x: &[f64], at: usize, top: f64) -> Vec<(f64, f64)> {
    const N: usize = 8192;
    let at = at.min(x.len().saturating_sub(N));
    let fft = plan(N, false);
    let mut b: Vec<Complex64> = (0..N)
        .map(|i| {
            let w = 0.5 - 0.5 * (2.0 * PI * i as f64 / N as f64).cos();
            Complex64::new(x.get(at + i).copied().unwrap_or(0.0) * w, 0.0)
        })
        .collect();
    fft.process(&mut b);
    let db: Vec<f64> = b[..N / 2]
        .iter()
        .map(|c| 10.0 * c.norm_sqr().max(1e-30).log10())
        .collect();
    let peak = db.iter().fold(f64::MIN, |m, &v| m.max(v));
    db.iter()
        .enumerate()
        .map(|(k, &d)| (k as f64 * SR / N as f64, d - peak))
        .take_while(|&(f, _)| f <= top)
        .collect()
}

fn polyline(out: &mut String, pts: &[(f64, f64)], ink: &str) {
    let p: Vec<String> = pts.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
    let _ = writeln!(
        out,
        r#"<polyline fill="none" stroke="{ink}" stroke-width="1" points="{}"/>"#,
        p.join(" ")
    );
}

fn text(out: &mut String, x: f64, y: f64, s: &str, ink: &str) {
    let _ = writeln!(
        out,
        r#"<text x="{x:.0}" y="{y:.0}" font-family="sans-serif" font-size="12" fill="{ink}">{s}</text>"#
    );
}

/// An SVG of `files` (name, mono samples at `SR`): waveforms over
/// `periods` of `f0` from `at` seconds, spectra to `top` Hz there, then one
/// envelope-peak track a file.
pub fn svg(files: &[(String, Vec<f64>)], at: f64, f0: f64, periods: f64, top: f64) -> String {
    let n = files.len();
    let h = PAD + PANEL_H * 2.0 + (PANEL_H * 0.7) * n as f64 + PAD * (n as f64 + 2.0);
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{h:.0}" viewBox="0 0 {W} {h:.0}"><rect width="100%" height="100%" fill="white"/>"#
    );
    let (x0, x1) = (PAD, W - PAD / 2.0);
    let span = x1 - x0;
    // Waveforms.
    let mut y = PAD;
    text(
        &mut s,
        x0,
        y - 10.0,
        &format!("waveform, {periods} periods of {f0:.1} Hz from {at} s (each to its peak)"),
        "#333",
    );
    let len = (periods * SR / f0) as usize;
    let start = (at * SR) as usize;
    for (i, (name, x)) in files.iter().enumerate() {
        let seg = &x[start.min(x.len())..(start + len).min(x.len())];
        let pk = seg.iter().fold(1e-12f64, |m, v| m.max(v.abs()));
        let row = PANEL_H / n as f64;
        let mid = y + row * (i as f64 + 0.5);
        let pts: Vec<(f64, f64)> = seg
            .iter()
            .enumerate()
            .map(|(k, v)| (x0 + span * k as f64 / len as f64, mid - 0.45 * row * v / pk))
            .collect();
        polyline(&mut s, &pts, INK[i % INK.len()]);
        text(&mut s, x0 + 4.0, mid - 0.3 * row, name, INK[i % INK.len()]);
    }
    // Spectra.
    y += PANEL_H + PAD;
    text(
        &mut s,
        x0,
        y - 10.0,
        &format!("spectrum at {at} s, 0–{top:.0} Hz, 0 to −80 dB re each one's peak"),
        "#333",
    );
    for (i, (_, x)) in files.iter().enumerate() {
        let pts: Vec<(f64, f64)> = spectrum(x, start, top)
            .into_iter()
            .map(|(f, d)| (x0 + span * f / top, y + PANEL_H * (-d.max(-80.0) / 80.0)))
            .collect();
        polyline(&mut s, &pts, INK[i % INK.len()]);
    }
    for k in 0..=(top / 1000.0) as usize {
        let gx = x0 + span * k as f64 * 1000.0 / top;
        text(&mut s, gx, y + PANEL_H + 14.0, &format!("{k}k"), "#666");
    }
    // Envelope-peak tracks.
    y += PANEL_H + PAD;
    for (i, (name, x)) in files.iter().enumerate() {
        let th = PANEL_H * 0.7;
        let tracks = envelope_tracks(x);
        let dur = x.len() as f64 / SR;
        text(
            &mut s,
            x0,
            y - 6.0,
            &format!("{name}: spectral-envelope peaks over {dur:.1} s, 0–5 kHz"),
            INK[i % INK.len()],
        );
        let _ = writeln!(
            s,
            r##"<rect x="{x0}" y="{y:.0}" width="{span}" height="{th:.0}" fill="none" stroke="#ccc"/>"##
        );
        for (t, peaks) in &tracks {
            for &(f, _) in peaks {
                let _ = writeln!(
                    s,
                    r#"<circle cx="{:.1}" cy="{:.1}" r="1.6" fill="{}"/>"#,
                    x0 + span * t / dur,
                    y + th * (1.0 - f / 5_000.0),
                    INK[i % INK.len()]
                );
            }
        }
        y += th + PAD;
    }
    s.push_str("</svg>\n");
    s
}

/// `voice-plot OUT.svg WAV …`: `at`, `f0`, `periods` and `top` as given.
pub fn plot(
    out: &str,
    wavs: &[String],
    at: f64,
    f0: f64,
    periods: f64,
    top: f64,
) -> Result<(), String> {
    let files: Vec<(String, Vec<f64>)> = wavs
        .iter()
        .map(|p| {
            let path = std::path::Path::new(p);
            let (x, sr) = crate::reference::read_wav(path)?;
            if f64::from(sr) != SR {
                return Err(format!("{p}: {sr} Hz, not {SR}"));
            }
            let name = path
                .file_stem()
                .map_or(p.clone(), |s| s.to_string_lossy().into_owned());
            Ok((name, x))
        })
        .collect::<Result<_, String>>()?;
    std::fs::write(out, svg(&files, at, f0, periods, top)).map_err(|e| format!("{out}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_plots_one_spectral_peak_at_its_frequency() {
        let x: Vec<f64> = (0..48_000)
            .map(|i| (2.0 * PI * 1_000.0 * i as f64 / SR).sin())
            .collect();
        let sp = spectrum(&x, 0, 4_000.0);
        let top = sp.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        assert!((top.0 - 1_000.0).abs() < 6.0, "{top:?}");
        let s = svg(&[("sine".into(), x)], 0.5, 1_000.0, 3.0, 4_000.0);
        assert!(s.starts_with("<svg") && s.trim_end().ends_with("</svg>"));
    }
}
