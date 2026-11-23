// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A reverb's metrics, residuals, verdict and distance: the MidiVerb
//! harness's `mvlib` measurer, ported call for call.

use crate::SR;
use crate::grain::{self, Grain};
use crate::sig::{
    ERFC_INV_SQRT2, box_same, butter3_bandpass, hanning, rev_cumsum, slope, sosfilt, tail_mean,
    welch,
};
use serde_json::{Map, Value, json};

pub const BANDS: [u32; 7] = [125, 250, 500, 1000, 2000, 4000, 8000];
/// Judged bands: 125 Hz and 8 kHz are reported only.
pub const JUDGED: [u32; 5] = [250, 500, 1000, 2000, 4000];
pub const NED_AT: [u32; 3] = [20, 50, 100];

/// The match bar, in each metric's units.
pub mod tol {
    pub const BAND_RT60: f64 = 0.05;
    pub const PREDELAY_MS: f64 = 1.0;
    pub const C50_DB: f64 = 1.0;
    pub const NED: f64 = 0.10;
    pub const CENTROID: f64 = 0.05;
}

/// RT60 per band, `None` where the decay never reached the fit range.
pub type BandRt = [Option<f64>; 7];

/// Each band's (and the broadband's) decay RT60 from interrupted noise.
#[derive(Clone, Debug)]
pub struct NoiseRt {
    pub all: Option<f64>,
    pub bands: BandRt,
}

/// A burst's envelope (GATE, REVERSE).
#[derive(Clone, Debug)]
pub struct Gate {
    pub peak_ms: f64,
    pub cut_ms: f64,
    pub body_db_25_50_75: [f64; 3],
    pub body_slope_db_s: f64,
    pub fall_db_s: f64,
}

#[derive(Clone, Debug)]
pub struct Metrics {
    pub predelay_ms: f64,
    pub rt60_s: Option<f64>,
    pub edt_s: Option<f64>,
    pub band_rt60_ir_s: BandRt,
    pub band_rt60_s: BandRt,
    pub c50_db: f64,
    pub ned: [f64; 3],
    pub ned_curve: Vec<f64>,
    pub centroid_hz: f64,
    pub tilt_db_oct: f64,
    pub band_level_db: [f64; 7],
    pub length_s: f64,
    pub gate: Option<Gate>,
    /// GRAIN and HF share, when measured.
    pub grain: Option<Grain>,
}

fn band_sos(fc: u32) -> Vec<crate::sig::Sos> {
    let fc = fc as f64;
    let (lo, hi) = (fc / 2f64.sqrt(), (fc * 2f64.sqrt()).min(0.45 * SR));
    butter3_bandpass(lo, hi, SR)
}

/// Mean energy over `responses` after `cut`, in 1 ms frames, broadband and
/// per band (index 0 broadband, then `BANDS`).
pub fn noise_band_energy(responses: &[Vec<f64>], cut: usize) -> Vec<Vec<f64>> {
    let mut out = Vec::with_capacity(8);
    for b in 0..=BANDS.len() {
        let sos = (b > 0).then(|| band_sos(BANDS[b - 1]));
        let n = responses[0].len();
        let mut e = vec![0.0; n];
        for y in responses {
            let f;
            let y = match &sos {
                Some(s) => {
                    f = sosfilt(s, y);
                    &f
                }
                None => y,
            };
            for (a, v) in e.iter_mut().zip(y) {
                *a += v * v;
            }
        }
        let k = responses.len() as f64;
        let e = &e[cut..];
        let frames = e.len() / 48;
        out.push(
            (0..frames)
                .map(|i| e[i * 48..(i + 1) * 48].iter().map(|v| v / k).sum())
                .collect(),
        );
    }
    out
}

/// T20 (−5 to −25 dB) of each band's energy, floor subtracted.
pub fn band_rt60_from_energy(bands: &[Vec<f64>]) -> NoiseRt {
    let rt = |e: &[f64]| -> Option<f64> {
        let (lo, hi) = (-5.0, -25.0);
        let floor = tail_mean(e);
        let env = box_same(e, 10);
        let thr = floor * 10f64.powf(0.5) + 1e-30;
        let end = env
            .iter()
            .rposition(|&v| v > thr)
            .map_or(e.len(), |i| i + 1);
        let rc = rev_cumsum(&e[..end]);
        let ee: Vec<f64> = rc
            .iter()
            .enumerate()
            .map(|(i, v)| (v - floor * (end - i) as f64).max(1e-30))
            .collect();
        let edc: Vec<f64> = ee.iter().map(|v| 10.0 * (v / ee[0]).log10()).collect();
        fit_range(&edc, lo, hi, 1000.0)
    };
    NoiseRt {
        all: rt(&bands[0]),
        bands: core::array::from_fn(|i| rt(&bands[i + 1])),
    }
}

/// −60 / slope of `edc` regressed over `lo..=hi` dB, `rate` points a second;
/// `None` unless more than four points fall there, the EDC goes below
/// `hi` and the slope falls.
fn fit_range(edc: &[f64], lo: f64, hi: f64, rate: f64) -> Option<f64> {
    let idx: Vec<usize> = (0..edc.len())
        .filter(|&i| edc[i] <= lo && edc[i] >= hi)
        .collect();
    let min = edc.iter().copied().fold(f64::INFINITY, f64::min);
    if idx.len() > 4 && min < hi {
        let t: Vec<f64> = idx.iter().map(|&i| i as f64 / rate).collect();
        let y: Vec<f64> = idx.iter().map(|&i| edc[i]).collect();
        let s = slope(&t, &y);
        if s < 0.0 {
            return Some(-60.0 / s);
        }
    }
    None
}

pub fn noise_band_rt60(responses: &[Vec<f64>], cut: usize) -> NoiseRt {
    band_rt60_from_energy(&noise_band_energy(responses, cut))
}

/// First sample within `rel_db` of the peak magnitude.
pub fn onset(h: &[f64], rel_db: f64) -> usize {
    let peak = h.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let thr = peak * 10f64.powf(rel_db / 20.0);
    h.iter().position(|v| v.abs() >= thr).unwrap_or(0)
}

/// Cut where a 10 ms envelope meets the noise floor (+5 dB): the kept
/// length and the floor.
fn truncate(h: &[f64]) -> (usize, f64) {
    let sq: Vec<f64> = h.iter().map(|v| v * v).collect();
    let e: Vec<f64> = box_same(&sq, 480).into_iter().map(|v| v + 1e-30).collect();
    let noise = tail_mean(&e);
    let pk = argmax(&e);
    let thr = noise * 10f64.powf(0.5);
    let last = e[pk..].iter().rposition(|&v| v > thr).unwrap_or(0);
    (pk + last + 1, noise)
}

fn argmax(x: &[f64]) -> usize {
    let mut best = 0;
    for (i, &v) in x.iter().enumerate() {
        if v > x[best] {
            best = i;
        }
    }
    best
}

fn edc_db(h: &[f64], noise: f64) -> Vec<f64> {
    let sq: Vec<f64> = h.iter().map(|v| v * v).collect();
    let n = h.len();
    let e: Vec<f64> = rev_cumsum(&sq)
        .into_iter()
        .enumerate()
        .map(|(i, v)| (v - noise * (n - i) as f64).max(1e-30))
        .collect();
    e.iter().map(|v| 10.0 * (v / e[0]).log10()).collect()
}

/// RT60 by regression between `lo` and `hi` dB, falling back to T20 then T10.
fn decay_time(edc: &[f64], lo: f64, hi: f64) -> Option<f64> {
    [hi, -25.0, -15.0]
        .into_iter()
        .find_map(|top| fit_range(edc, lo, top, SR))
}

/// Backward-filtered octave band: the filter's ringing lands before the IR.
fn band_filter(h: &[f64], fc: u32) -> Vec<f64> {
    let rev: Vec<f64> = h.iter().rev().copied().collect();
    let mut y = sosfilt(&band_sos(fc), &rev);
    y.reverse();
    y
}

/// Abel–Huang normalised echo density, a 20 ms Hann window centred at each
/// time in `at_ms`.
fn ned(h: &[f64], at_ms: impl Iterator<Item = i64>) -> Vec<f64> {
    let w = (20e-3 * SR) as usize;
    let mut win = hanning(w);
    let s: f64 = win.iter().sum();
    win.iter_mut().for_each(|v| *v /= s);
    at_ms
        .map(|t| {
            let c = (t as f64 * 1e-3 * SR) as i64;
            let a = (c - w as i64 / 2).max(0) as usize;
            let seg = &h[a.min(h.len())..(a + w).min(h.len())];
            let ws: f64 = win[..seg.len()].iter().sum();
            let ww: Vec<f64> = win[..seg.len()].iter().map(|v| v / ws).collect();
            let sd = ww
                .iter()
                .zip(seg)
                .map(|(w, x)| w * x * x)
                .sum::<f64>()
                .sqrt();
            if sd > 0.0 {
                ww.iter()
                    .zip(seg)
                    .filter(|(_, x)| x.abs() > sd)
                    .map(|(w, _)| w)
                    .sum::<f64>()
                    / ERFC_INV_SQRT2
            } else {
                0.0
            }
        })
        .collect()
}

/// Every metric of a wet impulse response `h` (48 kHz, t = 0 the impulse);
/// band RT60 from `noise_rt` when given, else from the IR.
pub fn ir_metrics(h: &[f64], noise_rt: Option<&NoiseRt>) -> Metrics {
    let t0 = onset(h, -20.0);
    let (end, noise) = truncate(h);
    let ht = &h[..end];
    let edc = edc_db(ht, noise);
    let band_ir: BandRt = core::array::from_fn(|i| {
        decay_time(
            &edc_db(&band_filter(ht, BANDS[i]), noise / 3.0),
            -5.0,
            -35.0,
        )
    });
    let (rt60, band_rt) = match noise_rt {
        Some(n) => (n.all, n.bands),
        None => (decay_time(&edc, -5.0, -35.0), band_ir),
    };
    let x = &ht[t0.min(ht.len())..];
    let k50 = (0.05 * SR) as usize;
    let early: f64 = x[..k50.min(x.len())].iter().map(|v| v * v).sum();
    let late: f64 = x[k50.min(x.len())..].iter().map(|v| v * v).sum::<f64>() + 1e-30;
    let ned_at: [f64; 3] = core::array::from_fn(|i| {
        let a = NED_AT[i] as i64;
        let v = ned(x, a - 5..a + 6);
        v.iter().sum::<f64>() / v.len() as f64
    });
    let ned_curve = ned(x, (5..305).step_by(5));
    let from = (0.02 * SR) as usize;
    let tail = if x.len().saturating_sub(from) < 1024 {
        x
    } else {
        &x[from..]
    };
    let (f, p) = welch(tail, SR, tail.len().min(2048));
    let (mut num, mut den) = (0.0, 0.0);
    for (fi, pi) in f.iter().zip(&p) {
        if (60.0..=11_000.0).contains(fi) {
            num += fi * pi;
            den += pi;
        }
    }
    let bands_db: Vec<f64> = BANDS
        .iter()
        .map(|&fc| {
            let fc = fc as f64;
            let s: f64 = f
                .iter()
                .zip(&p)
                .filter(|(fi, _)| **fi >= fc / 2f64.sqrt() && **fi < fc * 2f64.sqrt())
                .map(|(_, pi)| pi)
                .sum();
            10.0 * (s + 1e-30).log10()
        })
        .collect();
    let log2b: Vec<f64> = BANDS.iter().map(|&b| (b as f64).log2()).collect();
    let top = bands_db.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Metrics {
        predelay_ms: 1000.0 * t0 as f64 / SR,
        rt60_s: rt60,
        edt_s: decay_time(&edc, 0.0, -10.0),
        band_rt60_ir_s: band_ir,
        band_rt60_s: band_rt,
        c50_db: 10.0 * (early / late + 1e-30).log10(),
        ned: ned_at,
        ned_curve,
        centroid_hz: num / den,
        tilt_db_oct: slope(&log2b, &bands_db),
        band_level_db: core::array::from_fn(|i| bands_db[i] - top),
        length_s: ht.len() as f64 / SR,
        gate: gate_shape(h),
        grain: None,
    }
}

fn envelope_db(x: &[f64]) -> Vec<f64> {
    let sq: Vec<f64> = x.iter().map(|v| v * v).collect();
    let e: Vec<f64> = box_same(&sq, 240)
        .into_iter()
        .map(|v| v.sqrt() + 1e-12)
        .collect();
    let m = e.iter().copied().fold(0.0, f64::max);
    e.iter().map(|v| 20.0 * (v / m).log10()).collect()
}

/// A burst's envelope: peak time, the level at 25/50/75 % of its body, the
/// body's slope and the cut (where it last falls through −20 dB).
fn gate_shape(h: &[f64]) -> Option<Gate> {
    let e = envelope_db(h);
    let t0 = onset(h, -20.0);
    let cut = e.iter().rposition(|&v| v > -20.0).unwrap_or(t0);
    let body = if cut > t0 { &e[t0..cut] } else { &e[0..0] };
    if body.len() < 8 {
        return None;
    }
    let q = [0.25, 0.5, 0.75].map(|r| body[(body.len() as f64 * r) as usize]);
    let tt: Vec<f64> = (0..body.len()).map(|i| i as f64 / SR).collect();
    let after = &e[cut..(cut + (0.05 * SR) as usize).min(e.len())];
    let fall = if after.len() > 2 {
        (after[0] - after.iter().copied().fold(f64::INFINITY, f64::min)) / 0.05
    } else {
        0.0
    };
    Some(Gate {
        peak_ms: 1000.0 * (argmax(&e) as f64 - t0 as f64) / SR,
        cut_ms: 1000.0 * (cut as f64 - t0 as f64) / SR,
        body_db_25_50_75: q,
        body_slope_db_s: slope(&tt, body),
        fall_db_s: fall,
    })
}

/// Per-metric residuals against a reference, in each tolerance's units.
#[derive(Clone, Debug)]
pub struct Residuals {
    pub band_rt60_rel: BandRt,
    pub predelay_ms: f64,
    pub c50_db: f64,
    pub ned_rel: [f64; 3],
    pub centroid_rel: f64,
    /// GRAIN's six Δ and the three HF shares' Δ, dB, when both sides have
    /// them.
    pub npr_db: Option<[f64; 6]>,
    pub hf_db: Option<[f64; 3]>,
}

pub fn residuals(m: &Metrics, r: &Metrics) -> Residuals {
    Residuals {
        band_rt60_rel: core::array::from_fn(|i| match (m.band_rt60_s[i], r.band_rt60_s[i]) {
            (Some(a), Some(z)) => Some(a / z - 1.0),
            (None, None) => None,
            _ => Some(1.0),
        }),
        predelay_ms: m.predelay_ms - r.predelay_ms,
        c50_db: m.c50_db - r.c50_db,
        ned_rel: core::array::from_fn(|i| {
            if r.ned[i] > 0.0 {
                m.ned[i] / r.ned[i] - 1.0
            } else {
                0.0
            }
        }),
        centroid_rel: m.centroid_hz / r.centroid_hz - 1.0,
        npr_db: m
            .grain
            .zip(r.grain)
            .map(|(a, b)| core::array::from_fn(|i| a.npr[i] - b.npr[i])),
        hf_db: m
            .grain
            .zip(r.grain)
            .map(|(a, b)| core::array::from_fn(|i| a.hf[i] - b.hf[i])),
    }
}

impl Residuals {
    /// Every residual × `f`: the bars read at 1/`f` of themselves.
    pub fn scaled(&self, f: f64) -> Self {
        Self {
            band_rt60_rel: self.band_rt60_rel.map(|x| x.map(|v| v * f)),
            predelay_ms: self.predelay_ms * f,
            c50_db: self.c50_db * f,
            ned_rel: self.ned_rel.map(|v| v * f),
            centroid_rel: self.centroid_rel * f,
            npr_db: self.npr_db.map(|d| d.map(|v| v * f)),
            hf_db: self.hf_db.map(|d| d.map(|v| v * f)),
        }
    }

    /// GRAIN's rms and largest |Δ|, dB.
    pub fn npr_err(&self) -> Option<(f64, f64)> {
        self.npr_db.map(|d| {
            let rms = (d.iter().map(|x| x * x).sum::<f64>() / 6.0).sqrt();
            (rms, d.iter().fold(0.0f64, |m, x| m.max(x.abs())))
        })
    }

    /// HF share's largest |Δ|, dB.
    pub fn hf_err(&self) -> Option<f64> {
        self.hf_db
            .map(|d| d.iter().fold(0.0f64, |m, x| m.max(x.abs())))
    }
}

fn judged(r: &Residuals) -> impl Iterator<Item = Option<f64>> + '_ {
    BANDS
        .iter()
        .zip(r.band_rt60_rel)
        .filter(|(b, _)| JUDGED.contains(b))
        .map(|(_, v)| v)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub band_rt60: bool,
    pub predelay: bool,
    pub c50: bool,
    pub ned: bool,
    pub centroid: bool,
    /// GRAIN and HF share, when measured.
    pub grain: Option<bool>,
    pub hf: Option<bool>,
    /// Whether `all` counts GRAIN and HF share (ROOM), or they're reported
    /// only.
    pub judge_grain: bool,
}

impl Verdict {
    pub fn all(&self) -> bool {
        self.band_rt60
            && self.predelay
            && self.c50
            && self.ned
            && self.centroid
            && (!self.judge_grain || (self.grain == Some(true) && self.hf == Some(true)))
    }
}

/// Pass/fail per metric; band RT60 judged over 250 Hz–4 kHz. GRAIN and HF
/// share count only when `judge_grain`; a judged one unmeasured fails.
pub fn verdict(r: &Residuals, judge_grain: bool) -> Verdict {
    let br: Vec<f64> = judged(r).flatten().map(f64::abs).collect();
    Verdict {
        grain: r
            .npr_err()
            .map(|(rms, max)| rms <= grain::tol::NPR_RMS_DB && max <= grain::tol::NPR_MAX_DB),
        hf: r.hf_err().map(|m| m <= grain::tol::HF_DB),
        judge_grain,
        band_rt60: !br.is_empty() && br.iter().all(|&v| v <= tol::BAND_RT60),
        predelay: r.predelay_ms.abs() <= tol::PREDELAY_MS,
        c50: r.c50_db.abs() <= tol::C50_DB,
        ned: r.ned_rel.iter().all(|v| v.abs() <= tol::NED),
        centroid: r.centroid_rel.abs() <= tol::CENTROID,
    }
}

/// Weighted distance in tolerance units: band RT60 first, then NED and C50,
/// then centroid and pre-delay; with `judge_grain`, GRAIN's rms and each HF
/// share. Each judged metric past `MARGIN` × its bar adds `OVER`, for
/// every TYPE (ADR 0095): the search spends a pass's margin last, and stops
/// short of the bars rather than on them.
pub fn distance(r: &Residuals, judge_grain: bool) -> f64 {
    const OVER: f64 = 4.0;
    const MARGIN: f64 = 0.8;
    let u = |x: f64, t: f64| (x / t) * (x / t);
    let v = verdict(&r.scaled(1.0 / MARGIN), judge_grain);
    let over = [v.band_rt60, v.predelay, v.c50, v.ned, v.centroid]
        .into_iter()
        .chain(
            judge_grain
                .then_some([v.grain, v.hf].map(|x| x == Some(true)))
                .into_iter()
                .flatten(),
        )
        .filter(|ok| !ok)
        .count();
    let mut d = OVER * over as f64;
    for x in judged(r) {
        d += 3.0 * u(x.unwrap_or(1.0), tol::BAND_RT60) / JUDGED.len() as f64;
    }
    for x in r.ned_rel {
        d += 2.0 * u(x, tol::NED) / 3.0;
    }
    if judge_grain {
        if let Some((rms, _)) = r.npr_err() {
            d += u(rms, grain::tol::NPR_RMS_DB);
        }
        for x in r.hf_db.into_iter().flatten() {
            d += 2.0 * u(x, grain::tol::HF_DB);
        }
    }
    d + 2.0 * u(r.c50_db, tol::C50_DB)
        + u(r.centroid_rel, tol::CENTROID)
        + u(r.predelay_ms, tol::PREDELAY_MS)
}

// ── JSON, in the Python harness's shapes, so files compare across both ──

fn opt(v: Option<f64>) -> Value {
    v.map_or(Value::Null, |x| json!(x))
}

fn band_map(v: &[Option<f64>; 7]) -> Value {
    let mut m = Map::new();
    for (b, x) in BANDS.iter().zip(v) {
        m.insert(b.to_string(), opt(*x));
    }
    Value::Object(m)
}

fn ned_map(v: &[f64; 3]) -> Value {
    let mut m = Map::new();
    for (a, x) in NED_AT.iter().zip(v) {
        m.insert(a.to_string(), json!(x));
    }
    Value::Object(m)
}

fn read_band(v: &Value) -> BandRt {
    core::array::from_fn(|i| v[BANDS[i].to_string()].as_f64())
}

fn read_f(v: &Value, k: &str) -> Result<f64, String> {
    v[k].as_f64()
        .ok_or_else(|| format!("metrics: no number `{k}`"))
}

fn read_grain(v: &Value) -> Option<Grain> {
    let arr = |k: &str, n: usize| -> Option<Vec<f64>> {
        let a: Vec<f64> = v[k].as_array()?.iter().filter_map(Value::as_f64).collect();
        (a.len() == n).then_some(a)
    };
    let (n, h) = (arr("npr", 6)?, arr("hf", 3)?);
    Some(Grain {
        npr: core::array::from_fn(|i| n[i]),
        hf: core::array::from_fn(|i| h[i]),
    })
}

impl Metrics {
    pub fn to_json(&self) -> Value {
        json!({
            "predelay_ms": self.predelay_ms,
            "rt60_s": opt(self.rt60_s),
            "edt_s": opt(self.edt_s),
            "band_rt60_ir_s": band_map(&self.band_rt60_ir_s),
            "band_rt60_s": band_map(&self.band_rt60_s),
            "c50_db": self.c50_db,
            "ned": ned_map(&self.ned),
            "ned_curve": self.ned_curve,
            "centroid_hz": self.centroid_hz,
            "tilt_db_oct": self.tilt_db_oct,
            "band_level_db": band_map(&self.band_level_db.map(Some)),
            "length_s": self.length_s,
            "gate": self.gate.as_ref().map_or(Value::Null, |g| json!({
                "peak_ms": g.peak_ms,
                "cut_ms": g.cut_ms,
                "body_db_25_50_75": g.body_db_25_50_75,
                "body_slope_db_s": g.body_slope_db_s,
                "fall_db_s": g.fall_db_s,
            })),
            "grain": self.grain.map_or(Value::Null, |g| json!({"npr": g.npr, "hf": g.hf})),
        })
    }

    /// From the JSON `to_json` (or the Python harness) wrote.
    pub fn from_json(v: &Value) -> Result<Self, String> {
        let g = &v["gate"];
        Ok(Self {
            predelay_ms: read_f(v, "predelay_ms")?,
            rt60_s: v["rt60_s"].as_f64(),
            edt_s: v["edt_s"].as_f64(),
            band_rt60_ir_s: read_band(&v["band_rt60_ir_s"]),
            band_rt60_s: read_band(&v["band_rt60_s"]),
            c50_db: read_f(v, "c50_db")?,
            ned: core::array::from_fn(|i| v["ned"][NED_AT[i].to_string()].as_f64().unwrap_or(0.0)),
            ned_curve: v["ned_curve"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default(),
            centroid_hz: read_f(v, "centroid_hz")?,
            tilt_db_oct: v["tilt_db_oct"].as_f64().unwrap_or(0.0),
            band_level_db: core::array::from_fn(|i| {
                v["band_level_db"][BANDS[i].to_string()]
                    .as_f64()
                    .unwrap_or(0.0)
            }),
            length_s: v["length_s"].as_f64().unwrap_or(0.0),
            gate: g.is_object().then(|| Gate {
                peak_ms: g["peak_ms"].as_f64().unwrap_or(0.0),
                cut_ms: g["cut_ms"].as_f64().unwrap_or(0.0),
                body_db_25_50_75: core::array::from_fn(|i| {
                    g["body_db_25_50_75"][i].as_f64().unwrap_or(0.0)
                }),
                body_slope_db_s: g["body_slope_db_s"].as_f64().unwrap_or(0.0),
                fall_db_s: g["fall_db_s"].as_f64().unwrap_or(0.0),
            }),
            grain: read_grain(&v["grain"]),
        })
    }
}

impl Residuals {
    pub fn to_json(&self) -> Value {
        json!({
            "band_rt60_rel": band_map(&self.band_rt60_rel),
            "predelay_ms": self.predelay_ms,
            "c50_db": self.c50_db,
            "ned_rel": ned_map(&self.ned_rel),
            "centroid_rel": self.centroid_rel,
            "npr_db": self.npr_db.map_or(Value::Null, |d| json!(d)),
            "hf_db": self.hf_db.map_or(Value::Null, |d| json!(d)),
        })
    }
}

impl Verdict {
    pub fn to_json(self) -> Value {
        json!({
            "band_rt60": self.band_rt60,
            "predelay": self.predelay,
            "c50": self.c50,
            "ned": self.ned,
            "centroid": self.centroid,
            "grain": self.grain,
            "hf": self.hf,
            "all": self.all(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rt60_of_an_exponential_decay() {
        // 1 ms frames of energy falling 60 dB in 0.8 s, over a small floor.
        let rt = 0.8;
        let e: Vec<f64> = (0..3000)
            .map(|i| 10f64.powf(-6.0 * (i as f64 / 1000.0) / rt) + 1e-12)
            .collect();
        let bands = vec![e; 8];
        let got = band_rt60_from_energy(&bands);
        let all = got.all.expect("an RT60");
        assert!((all / rt - 1.0).abs() < 0.01, "{all}");
    }

    #[test]
    fn predelay_is_the_first_arrival_within_20_db_of_the_peak() {
        // A −26 dB reflection at 5 ms (ignored), the first arrival at −14 dB
        // at 10 ms, the peak at 30 ms, then a decaying tail.
        let mut h = vec![0.0; 48_000];
        h[240] = 0.05;
        h[480] = 0.2;
        h[1440] = 1.0;
        for (i, v) in h.iter_mut().enumerate().skip(1441) {
            *v = 0.05 * (-(i as f64) / 4800.0).exp() * if i % 2 == 0 { 1.0 } else { -1.0 };
        }
        let m = ir_metrics(&h, None);
        assert!((m.predelay_ms - 10.0).abs() < 1e-9, "{}", m.predelay_ms);
    }

    #[test]
    fn c50_of_a_known_response() {
        // Two pulses: energy 1 in the first 50 ms, 0.1 after: C50 = 10 dB.
        let mut h = vec![0.0; 48_000];
        h[0] = 1.0;
        h[4800] = 0.1f64.sqrt();
        let m = ir_metrics(&h, None);
        assert!((m.c50_db - 10.0).abs() < 1e-6, "{}", m.c50_db);
    }
}
