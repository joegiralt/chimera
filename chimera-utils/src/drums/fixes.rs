// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The three fixes the first renders' measurements flagged (design note,
//! "Ship list"): each rendered `_before` (the 2cacaeae prototype) and
//! `_after`, with the numbers that say what moved.
//!
//! - FM's sweep: ALGO's kernel holds `inc` a block, so a fast sweep is a
//!   staircase at 750 Hz; DRUM's own operator loop ramps it a sample.
//!   Measured on the carrier alone (MOD 0): the instantaneous frequency
//!   (Hilbert) against the exact per-sample sweep.
//! - BODY's balance: MODAL's bank lets the upper modes lead; DRUM's mode
//!   bank gives the fundamental the lead. Spectral centroid, the strongest
//!   partial and the fundamental's share of the energy.
//! - 4-bit ADPCM's attack: a fitted first step and a one-sample look-ahead
//!   in the encoder (IMA's decoder unchanged). Peak and SNR of the first
//!   2 ms and of the whole clip against the 12-bit clip.

use super::machines::{Id, Knobs, build, build_before};
use super::parts::{B, Blk, FS};
use super::sample::{self, CLIP_RATE, Depth, Dir, Mode, pad, store};
use super::{Out, hit, knob_map, knobs, run_hit, settings};
use rustfft::{FftPlanner, num_complex::Complex};
use serde_json::{Value, json};

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn db(v: f64) -> f64 {
    r2(20.0 * v.max(1e-12).log10())
}

pub fn run(out: &mut Out) -> Result<Value, String> {
    Ok(json!({
        "fm_sweep": fm(out)?,
        "body_balance": body(out)?,
        "adpcm_attack": adpcm(out)?,
        "hand_exciter": hand(out)?,
    }))
}

// ------------------------------------------------------------------ fm

/// The analytic signal's instantaneous frequency, Hz a sample.
fn inst_freq(x: &[f32]) -> Vec<f64> {
    let n = x.len().next_power_of_two() * 2;
    let mut buf: Vec<Complex<f64>> = x.iter().map(|&v| Complex::new(v as f64, 0.0)).collect();
    buf.resize(n, Complex::new(0.0, 0.0));
    let mut p = FftPlanner::new();
    p.plan_fft_forward(n).process(&mut buf);
    for (i, c) in buf.iter_mut().enumerate() {
        if i == 0 || i == n / 2 {
            continue;
        }
        *c *= if i < n / 2 { 2.0 } else { 0.0 };
    }
    p.plan_fft_inverse(n).process(&mut buf);
    let ph: Vec<f64> = buf[..x.len()].iter().map(|c| c.im.atan2(c.re)).collect();
    ph.windows(2)
        .map(|w| {
            let mut d = w[1] - w[0];
            while d > std::f64::consts::PI {
                d -= std::f64::consts::TAU;
            }
            while d < -std::f64::consts::PI {
                d += std::f64::consts::TAU;
            }
            d * FS as f64 / std::f64::consts::TAU
        })
        .collect()
}

/// FM KICK's sweep at sample `n` (the exact law, a sample on, as both
/// paths reach each block's end value at its last sample).
fn kick_law(k: &Knobs, n: usize) -> f64 {
    let f0 = 30.0 * (120.0f64 / 30.0).powf(k[0] as f64);
    let oct = 48.0 * k[2] as f64 / 12.0;
    let tau = 0.005 * (0.15f64 / 0.005).powf(k[6] as f64);
    f0 * (oct * (-((n + 1) as f64) / (tau * FS as f64)).exp()).exp2()
}

fn render(mut m: Box<dyn super::machines::Machine>, secs: f32) -> Vec<f32> {
    let mut y = Vec::new();
    while y.len() < (secs * FS) as usize {
        let mut b: Blk = [0.0; B];
        m.block(&mut b, None);
        y.extend_from_slice(&b);
    }
    y
}

/// A pure swept sine (50 to 500 Hz here) has next to nothing above
/// 650 Hz; a staircase's 750 Hz steps put their sidebands there. The
/// energy above 650 Hz against all of it, Hann-windowed.
fn sidebands(x: &[f32]) -> f64 {
    let n = x.len().next_power_of_two() * 2;
    let l = x.len() as f64;
    let mut buf: Vec<Complex<f64>> = (0..n)
        .map(|i| {
            let v = x.get(i).map_or(0.0, |&v| {
                v as f64 * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / l).cos())
            });
            Complex::new(v, 0.0)
        })
        .collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    let p: Vec<f64> = buf[..n / 2].iter().map(|c| c.norm_sqr()).collect();
    let cut = (650.0 * n as f64 / FS as f64) as usize;
    let hi: f64 = p[cut..].iter().sum();
    r2(10.0 * (hi / p.iter().sum::<f64>()).log10())
}

fn fm(out: &mut Out) -> Result<Value, String> {
    let mut rows = serde_json::Map::new();
    for (name, k, _) in settings(Id::FmKick) {
        let kn = knobs(Id::FmKick, k);
        // The carrier alone: MOD 0.
        let mut bare = kn;
        bare[3] = 0.0;
        let mut row = serde_json::Map::new();
        for (tag, m) in [
            ("before", build_before(Id::FmKick, bare, 1.0, 1)),
            ("after", build(Id::FmKick, bare, 1.0, 1)),
        ] {
            let y = render(m, 0.25);
            let f = inst_freq(&y);
            // 5 to 150 ms: past the onset's ripple in the analytic
            // signal, while the sweep moves.
            let (a, b) = ((0.005 * FS) as usize, (0.15 * FS) as usize);
            // The ripple: the trace less its two-block centred mean, which
            // a staircase leaves as a 750 Hz sawtooth and a ramp does not.
            let w = 2 * B;
            let mut pk = 0.0f64;
            let mut ms = 0.0f64;
            for n in a..b {
                let mean = f[n - w / 2..n + w / 2].iter().sum::<f64>() / w as f64;
                let r = f[n] - mean;
                pk = pk.max(r.abs());
                ms += r * r;
            }
            // The law's own steps: each block's end value, held a block
            // (before) or reached a sample at a time (after).
            let blk = |i: usize| kick_law(&kn, (i + 1) * B - 1);
            let step = (1..(0.15 * FS) as usize / B)
                .map(|i| (blk(i) - blk(i - 1)).abs())
                .fold(0.0, f64::max);
            let step = if tag == "before" {
                step
            } else {
                step / B as f64
            };
            row.insert(
                tag.into(),
                json!({
                    "block_rate_sidebands_db": sidebands(&y[(0.001 * FS) as usize..(0.03 * FS) as usize]),
                    "largest_pitch_step_hz": r2(step),
                    "ripple_peak_hz": r2(pk),
                    "ripple_rms_hz": r2((ms / (b - a) as f64).sqrt()),
                    "trace_hz_every_ms_0_to_20": (0..=20).map(|ms| r2(f[(ms as f32 * 0.001 * FS) as usize + 2])).collect::<Vec<_>>(),
                }),
            );
        }
        // With no sweep the two paths are one: the null says DRUM's loop
        // plays ALGO's operators as the kernel does.
        let mut flat = kn;
        flat[2] = 0.0;
        let a = render(build_before(Id::FmKick, flat, 1.0, 1), 0.5);
        let b = render(build(Id::FmKick, flat, 1.0, 1), 0.5);
        let e: f64 = a
            .iter()
            .zip(&b)
            .map(|(x, y)| ((x - y) as f64).powi(2))
            .sum();
        let s: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum();
        row.insert(
            "sweep_0_null_vs_kernel_db".into(),
            json!(db((e / s).sqrt())),
        );
        rows.insert(name.into(), Value::Object(row));
        for (tag, m) in [
            ("before", build_before(Id::FmKick, kn, 1.0, 1)),
            ("after", build(Id::FmKick, kn, 1.0, 1)),
        ] {
            let x = run_hit(m, 6.0);
            out.write(
                &format!("fix-fm-sweep_fm-kick-{name}_{tag}.wav"),
                &x,
                json!({
                    "kind": "fix", "family": "FIX", "fix": "fm-sweep", "machine": "FM KICK", "setting": name,
                    "version": tag, "knobs": knob_map(Id::FmKick, k), "velocity": 127,
                    "listen_for": if tag == "before" { "the sweep as ALGO's kernel plays it: a 750 Hz staircase, heard as a buzz or zipper on the drop" } else { "the same kick, its sweep a sample at a time: a smooth drop" },
                }),
            )?;
        }
    }
    Ok(json!({
        "method": "FM KICK at each setting with MOD 0 (the carrier alone), 250 ms; block_rate_sidebands_db = energy above 650 Hz against all (1-30 ms, the fast part of the sweep, Hann), where a 750 Hz staircase puts its sidebands; instantaneous frequency from the analytic signal (FFT Hilbert), 5-150 ms; ripple = the trace less its 128-sample centred mean (a held-block staircase leaves a 750 Hz sawtooth). largest_pitch_step_hz is the sweep law's own: the biggest block-to-block jump (before) or per-sample move (after). Renders at the full settings.",
        "settings": rows,
    }))
}

// ---------------------------------------------------------------- body

/// The first 0.5 s's spectrum (Hann): centroid, the strongest partial, and
/// the energy within ±15 % of `f0` against all of it.
fn balance(x: &[f32], f0: f32) -> Value {
    let n = 32_768usize;
    let len = x.len().min((0.5 * FS) as usize);
    let mut buf: Vec<Complex<f64>> = (0..n)
        .map(|i| {
            let v = if i < len {
                x[i] as f64 * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / len as f64).cos())
            } else {
                0.0
            };
            Complex::new(v, 0.0)
        })
        .collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    let hz = |i: usize| i as f64 * FS as f64 / n as f64;
    let p: Vec<f64> = buf[..n / 2].iter().map(|c| c.norm_sqr()).collect();
    let tot: f64 = p.iter().skip(1).sum();
    let cen = p
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, v)| hz(i) * v)
        .sum::<f64>()
        / tot;
    let peak = (1..n / 2).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap();
    let fund: f64 = (1..n / 2)
        .filter(|&i| (hz(i) - f0 as f64).abs() <= 0.15 * f0 as f64)
        .map(|i| p[i])
        .sum();
    json!({
        "centroid_hz": cen.round(),
        "strongest_hz": hz(peak).round(),
        "fundamental_share_db": db((fund / tot).sqrt()),
        "peak_dbfs": db(x.iter().fold(0.0f32, |a, v| a.max(v.abs())) as f64),
    })
}

/// The fundamental each BODY setting's PITCH asks for.
fn body_f0(id: Id, k: &Knobs) -> f32 {
    let e = |k: f32, lo: f32, hi: f32| lo * (hi / lo).powf(k);
    match id {
        Id::BodyKick => e(k[0], 35.0, 120.0),
        Id::BodySnare => e(k[0], 120.0, 400.0),
        Id::BodyTom => e(k[0], 60.0, 400.0),
        Id::BodyBell => e(k[0], 150.0, 1500.0),
        _ => unreachable!(),
    }
}

fn body(out: &mut Out) -> Result<Value, String> {
    let mut rows = serde_json::Map::new();
    for (id, mname) in [
        (Id::BodyKick, "BODY KICK"),
        (Id::BodySnare, "BODY SNARE"),
        (Id::BodyTom, "BODY TOM"),
        (Id::BodyBell, "BODY BELL"),
    ] {
        let mut per = serde_json::Map::new();
        for (si, (name, k, _)) in settings(id).into_iter().enumerate() {
            let kn = knobs(id, k);
            let f0 = body_f0(id, &kn);
            let before = run_hit(build_before(id, kn, 1.0, 1 + si as u32), 6.0);
            let after = hit(id, k, 1.0, 1 + si as u32, 6.0);
            per.insert(
                name.into(),
                json!({"f0_hz": f0.round(), "before": balance(&before, f0), "after": balance(&after, f0)}),
            );
            for (tag, x) in [("before", &before), ("after", &after)] {
                out.write(
                    &format!("fix-body-balance_{}-{name}_{tag}.wav", mname.to_ascii_lowercase().replace(' ', "-")),
                    x,
                    json!({
                        "kind": "fix", "family": "FIX", "fix": "body-balance", "machine": mname, "setting": name,
                        "version": tag, "knobs": knob_map(id, k), "velocity": 127,
                        "listen_for": if tag == "before" { "MODAL's bank: the upper modes lead, the body reads high and thin" } else { "DRUM's mode bank: the fundamental leads, the upper modes fall away faster" },
                    }),
                )?;
            }
        }
        rows.insert(mname.into(), Value::Object(per));
    }
    Ok(json!({
        "method": "each BODY setting's single hit at velocity 127: the first 0.5 s, Hann, 32768-point FFT; centroid (power-weighted), the strongest bin, and the energy within +-15 % of PITCH's fundamental as a share of all (dB). Before: MODAL's 16-mode bank; after: DRUM's 8-mode bank.",
        "machines": rows,
    }))
}

// --------------------------------------------------------------- adpcm

fn adpcm(out: &mut Out) -> Result<Value, String> {
    let snare = hit(Id::ToneSnare, settings(Id::ToneSnare)[0].1, 1.0, 3, 0.5);
    let mut rows = serde_json::Map::new();
    for (cname, _, x) in sample::clips(&snare) {
        let ref12 = store(&x, Depth::B12).q;
        let a2 = (0.002 * CLIP_RATE) as usize;
        let stats = |q: &[i32]| {
            let snr = |r: std::ops::Range<usize>| {
                let s: f64 = ref12[r.clone()].iter().map(|v| (*v as f64).powi(2)).sum();
                let e: f64 = ref12[r.clone()]
                    .iter()
                    .zip(&q[r])
                    .map(|(a, b)| ((a - b) as f64).powi(2))
                    .sum();
                r2(10.0 * (s / e.max(1e-9)).log10())
            };
            let pk = |v: &[i32]| v.iter().map(|s| s.abs()).max().unwrap_or(0) as f64;
            json!({
                "peak_loss_db": db(pk(q) / pk(&ref12)),
                "attack_2ms_snr_db": snr(0..a2.min(q.len())),
                "clip_snr_db": snr(0..q.len()),
            })
        };
        let before = store(&x, Depth::Adpcm4First);
        let after = store(&x, Depth::Adpcm4);
        rows.insert(
            cname.into(),
            json!({"before": stats(&before.q), "after": stats(&after.q)}),
        );
        for (tag, c) in [("before", &before), ("after", &after)] {
            out.write(
                &format!("fix-adpcm-attack_{cname}_{tag}.wav"),
                &pad(None, Some((c, Dir::Forward)), (0.0, 1.0), Mode::Mix, 0.0, 0.4),
                json!({
                    "kind": "fix", "family": "FIX", "fix": "adpcm-attack", "machine": "OFF", "setting": format!("{cname} 4-bit ADPCM"),
                    "version": tag, "knobs": {}, "clip": cname, "depth": "4-bit ADPCM", "clip_rate_hz": 16000,
                    "listen_for": if tag == "before" { "the codec from its smallest step: the first few ms climb, the click's edge rounded" } else { "the start step fitted to the clip and a one-sample look-ahead: the edge kept" },
                }),
            )?;
        }
    }
    Ok(json!({
        "method": "each clip at 16 kHz coded to 4-bit IMA-style ADPCM and decoded, against its 12-bit store: peak change, SNR over the first 2 ms and over the whole clip. Before: every clip from step 0; after: the encoder picks the start index that codes the first 32 ms best (header byte) and chooses each code with a one-sample look-ahead; the decoder is unchanged.",
        "clips": rows,
    }))
}

// ---------------------------------------------------------------- hand

/// The first 20 ms's centroid and its 1–5 kHz share (the skin's band), and
/// the peak.
fn attack(x: &[f32]) -> Value {
    let n = 4096usize;
    let len = (0.02 * FS) as usize;
    let mut buf: Vec<Complex<f64>> = (0..n)
        .map(|i| {
            Complex::new(
                if i < len {
                    x.get(i).copied().unwrap_or(0.0) as f64
                } else {
                    0.0
                },
                0.0,
            )
        })
        .collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    let hz = |i: usize| i as f64 * FS as f64 / n as f64;
    let p: Vec<f64> = buf[..n / 2].iter().map(|c| c.norm_sqr()).collect();
    let tot: f64 = p.iter().skip(1).sum();
    let cen = p
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, v)| hz(i) * v)
        .sum::<f64>()
        / tot;
    let band: f64 = (1..n / 2)
        .filter(|&i| (1000.0..=5000.0).contains(&hz(i)))
        .map(|i| p[i])
        .sum();
    json!({
        "attack_20ms_centroid_hz": cen.round(),
        "attack_20ms_1k_5k_share_db": db((band / tot).sqrt()),
        "peak_dbfs": db(x.iter().fold(0.0f32, |a, v| a.max(v.abs())) as f64),
    })
}

/// BODY HAND with HAND at 0 (the mallet, as first rendered) and at each
/// setting's HAND.
fn hand(out: &mut Out) -> Result<Value, String> {
    let mut rows = serde_json::Map::new();
    for (si, (name, k, _)) in settings(Id::BodyHand).into_iter().enumerate() {
        let after = knobs(Id::BodyHand, k);
        let mut before = after;
        before[2] = 0.0;
        let mut kb = k.to_vec();
        kb[2] = 0;
        let mut row = serde_json::Map::new();
        for (tag, kn, kk) in [("before", before, kb.as_slice()), ("after", after, k)] {
            let x = run_hit(build(Id::BodyHand, kn, 1.0, 1 + si as u32), 6.0);
            row.insert(tag.into(), attack(&x));
            out.write(
                &format!("fix-hand-exciter_{name}_{tag}.wav"),
                &x,
                json!({
                    "kind": "fix", "family": "FIX", "fix": "hand-exciter", "machine": "BODY HAND", "setting": name,
                    "version": tag, "knobs": knob_map(Id::BodyHand, kk), "velocity": 127,
                    "listen_for": if tag == "before" { "HAND 0: the mallet's strike, as first rendered" } else { "the HAND exciter: palm contact, skin noise, the stroke's damping and bend" },
                }),
            )?;
        }
        rows.insert(name.into(), Value::Object(row));
    }
    Ok(json!({
        "method": "each BODY HAND setting at velocity 127, HAND 0 (mallet) against its HAND: the first 20 ms, 4096-point FFT; centroid and the 1-5 kHz (skin) share of the energy.",
        "settings": rows,
    }))
}
