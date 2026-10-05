//! Design sketches. A new reverb structure (or any DSP idea measured
//! against a reference) is prototyped here, in Rust, against the same
//! metrics as the real devices — not in Python.
//!
//! `Loop` is a float loop reverb: TONE's one-pole, a series-allpass
//! diffuser, L loops of S stages [delay → allpass] with a one-pole damper
//! every `every` stages, and taps on the stage lines. `hall` is ADR 0095's
//! proposed HALL on it (§ HALL, as fitted): HALL's 1 × 4 tank, program 17's
//! six diffuser lengths × SPREAD, 8 taps a side, one damper a loop.

use crate::Config;
use crate::metrics::{Metrics, distance, ir_metrics, noise_band_rt60, onset, residuals, verdict};
use crate::mv::{load_refs, read_json, write_json};
use crate::sig::resample_poly;
use crate::stim::{CLICK_AT, Rng, SR, click, noise_stim};
use rayon::prelude::*;
use serde_json::{Map, Value, json};
use std::f64::consts::PI;
use std::sync::Mutex;

/// The model's run rate, the chip's ring rate.
const FS: f64 = 24_000.0;

/// ms to samples at `FS`, numpy's `round` (half to even), at least 2.
fn smp(ms: f64) -> usize {
    ((ms * FS / 1000.0).round_ties_even() as usize).max(2)
}

/// A tap: loop, stage line, samples behind its write, weight.
#[derive(Clone, Copy)]
struct Tap {
    l: usize,
    s: usize,
    off: usize,
    w: f64,
}

struct Loop {
    diff: Vec<usize>,
    dg: f64,
    delay: Vec<Vec<usize>>,
    ap: Vec<Vec<usize>>,
    gain: Vec<Vec<f64>>,
    ap_g: f64,
    pole: f64,
    every: usize,
    taps: Vec<Tap>,
    tone_a: f64,
    pre_ms: f64,
}

/// A ring of `n` (a power of two) samples.
struct Ring {
    b: Vec<f64>,
    m: usize,
}

impl Ring {
    fn new(len: usize) -> Self {
        let n = (len + 2).next_power_of_two();
        Self {
            b: vec![0.0; n],
            m: n - 1,
        }
    }
    fn at(&self, i: usize, d: usize) -> f64 {
        self.b[i.wrapping_sub(d) & self.m]
    }
    fn set(&mut self, i: usize, v: f64) {
        self.b[i & self.m] = v;
    }
}

impl Loop {
    /// The 24 kHz run: taps from what earlier samples wrote, then the
    /// diffuser, then each loop's stages, the re-entry with the send.
    fn run(&self, x: &[f64]) -> Vec<f64> {
        let mut dr: Vec<Ring> = self.diff.iter().map(|&d| Ring::new(d)).collect();
        let longest = self
            .delay
            .iter()
            .flatten()
            .chain(self.taps.iter().map(|t| &t.off))
            .max()
            .copied()
            .unwrap_or(2);
        let mut lines: Vec<Vec<Ring>> = self
            .delay
            .iter()
            .map(|l| l.iter().map(|_| Ring::new(longest)).collect())
            .collect();
        let mut aps: Vec<Vec<Ring>> = self
            .ap
            .iter()
            .map(|l| l.iter().map(|&d| Ring::new(d)).collect())
            .collect();
        let ns = self.delay[0].len();
        let mut lp = vec![vec![0.0; ns]; self.delay.len()];
        let mut tone = 0.0;
        let mut y = vec![0.0; x.len()];
        for (i, (&xi, yi)) in x.iter().zip(y.iter_mut()).enumerate() {
            *yi = 0.75
                * self
                    .taps
                    .iter()
                    .map(|t| t.w * lines[t.l][t.s].at(i, t.off))
                    .sum::<f64>();
            tone += self.tone_a * (xi - tone);
            let mut u = tone;
            for (r, &d) in dr.iter_mut().zip(&self.diff) {
                let z = r.at(i, d);
                let acc = u + self.dg * z;
                r.set(i, -acc);
                u = -self.dg * acc + z;
            }
            for l in 0..self.delay.len() {
                let mut v = 0.0;
                for s in 0..ns {
                    let xin = lines[l][s].at(i, self.delay[l][s]);
                    let z = aps[l][s].at(i, self.ap[l][s]);
                    let acc = self.gain[l][s] * xin + self.ap_g * z;
                    aps[l][s].set(i, -acc);
                    v = -self.ap_g * acc + z;
                    if (s + 1) % self.every == 0 || s + 1 == ns {
                        lp[l][s] = v + self.pole * lp[l][s];
                        v = (1.0 - self.pole) * lp[l][s];
                    }
                    if s + 1 < ns {
                        lines[l][s + 1].set(i, v);
                    }
                }
                lines[l][0].set(i, v + u);
            }
        }
        y
    }

    /// 48 kHz in, 48 kHz out, through the run rate; PRE-DLY first.
    fn render48(&self, x48: &[f64]) -> Vec<f64> {
        let n = (self.pre_ms * 1e-3 * SR).round_ties_even() as usize;
        let mut x = vec![0.0; n.min(x48.len())];
        x.extend_from_slice(&x48[..x48.len() - x.len()]);
        let y = self.run(&resample_poly(&x, 1, 2));
        let mut y = resample_poly(&y, 2, 1);
        y.resize(x48.len(), 0.0);
        y
    }
}

// ── HALL, as fitted (proposed) ──

/// 23.4 kHz samples to ms: the unit's lengths.
const MV: f64 = 1000.0 / 23_400.0;
/// Program 17's diffuser, samples at 23.4 kHz (ADR 0095 § Provenance).
const DIFF17: [f64; 6] = [321.0, 431.0, 668.0, 808.0, 204.0, 144.0];
/// HALL as built: stage delays at SIZE 1 and stage allpasses, ms.
const DELAY_MS: [f64; 4] = [113.0, 157.0, 124.0, 146.0];
const AP_MS: [f64; 4] = [11.0, 17.0, 13.0, 19.0];
const K0: f64 = 0.111;
/// Each side's taps by rank (the first three always on): (line, fraction
/// of it), our own. Ranks 4–8 fade in with DENSITY.
const TAPS: [[(usize, f64); 8]; 2] = [
    [
        (0, 0.62),
        (1, 0.55),
        (2, 0.30),
        (0, 0.17),
        (1, 0.24),
        (2, 0.48),
        (3, 0.25),
        (3, 0.74),
    ],
    [
        (1, 0.02),
        (2, 0.90),
        (3, 0.93),
        (1, 0.48),
        (2, 0.57),
        (3, 0.26),
        (0, 0.43),
        (0, 0.60),
    ],
];

/// HALL's physical settings.
#[derive(Clone, Copy, Debug)]
pub struct Hall {
    size: f64,
    rt: f64,
    pole: f64,
    tone_hz: f64,
    dg: f64,
    spread: f64,
    dens: f64,
    pre_ms: f64,
}

/// The knob names, in the fit's order (PRE-DLY is solved, not fitted).
const KNOBS: [&str; 7] = [
    "size", "decay", "damp", "tone", "diffuse", "spread", "density",
];

/// How DENSITY is set: its own knob, or from another knob's position.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Dens {
    Own,
    Fixed(f64),
    /// DENSITY follows knob `i`, inverted when `inv`, over [lo, hi] of it.
    Tied {
        i: usize,
        inv: bool,
        lo: f64,
        hi: f64,
    },
}

fn exp_law(lo: f64, hi: f64, p: f64) -> f64 {
    lo * (hi / lo).powf(p.clamp(0.0, 1.0))
}

impl Hall {
    /// The proposed knob laws (ADR 0095 § HALL, as fitted).
    fn from_knobs(k: &[f64], dens: Dens) -> Self {
        let damp_hz = exp_law(14_000.0, 1_300.0, k[2]);
        let d = match dens {
            Dens::Own => k[6],
            Dens::Fixed(v) => v,
            Dens::Tied { i, inv, lo, hi } => {
                let p = ((k[i] - lo) / (hi - lo)).clamp(0.0, 1.0);
                if inv { 1.0 - p } else { p }
            }
        };
        Self {
            size: k[0],
            rt: exp_law(0.8, 16.0, k[1]),
            pole: (-2.0 * PI * damp_hz / FS).exp(),
            // TONE 0 is OFF; past it, 12 kHz → 2 kHz.
            tone_hz: if k[3] < 1.0 / 128.0 {
                f64::INFINITY
            } else {
                exp_law(12_000.0, 2_000.0, k[3])
            },
            dg: 0.3 + 0.5 * k[4],
            spread: 0.05 + 1.05 * k[5],
            dens: d,
            pre_ms: 0.0,
        }
    }

    /// The Python model's physical knobs (`hall_p1.py`'s JSON).
    fn from_python(k: &Map<String, Value>) -> Self {
        let f = |n: &str| k[n].as_f64().unwrap_or(0.0);
        Self {
            size: f("size"),
            rt: f("rt"),
            pole: f("pole"),
            tone_hz: f("tone_hz"),
            dg: f("dg"),
            spread: f("spread"),
            dens: f("dens"),
            pre_ms: f("pre_ms"),
        }
    }

    fn build(&self) -> Loop {
        let k = K0 + (1.0 - K0) * self.size;
        let dl: Vec<f64> = DELAY_MS.iter().map(|d| d * k).collect();
        let gain = (0..4)
            .map(|s| 10f64.powf(-3.0 * (dl[s] + AP_MS[s]) / 1000.0 / self.rt))
            .collect();
        let mut taps = Vec::new();
        for row in &TAPS {
            for (j, &(s, f)) in row.iter().enumerate() {
                let w = if j < 3 {
                    1.0
                } else {
                    (self.dens * 5.0 - (j - 3) as f64).clamp(0.0, 1.0)
                };
                if w > 0.0 {
                    taps.push(Tap {
                        l: 0,
                        s,
                        off: smp(f * dl[s]),
                        w: 0.75 * w * 0.5,
                    });
                }
            }
        }
        Loop {
            diff: DIFF17
                .iter()
                .map(|d| smp((d * MV * self.spread).max(2.0 * MV)))
                .collect(),
            dg: self.dg,
            delay: vec![dl.iter().map(|&d| smp(d)).collect()],
            ap: vec![AP_MS.iter().map(|&d| smp(d)).collect()],
            gain: vec![gain],
            ap_g: self.dg,
            pole: self.pole,
            every: 4,
            taps,
            tone_a: 1.0 - (-2.0 * PI * self.tone_hz.min(1e9) / FS).exp(),
            pre_ms: self.pre_ms,
        }
    }
}

/// Metrics of `h` against `r`, PRE-DLY solved to the reference's first
/// arrival unless `pre` is given; tails long enough for its RT60.
fn measure(h: &Hall, r: &Metrics, seeds: usize, solve_pre: bool) -> (Metrics, f64) {
    let rt250 = r.band_rt60_s[1].unwrap_or(1.0);
    let tail = (1.4 * rt250).max(3.0);
    let mut m = h.build();
    let ck = click(3.0);
    if solve_pre {
        m.pre_ms = 0.0;
        let h0 = m.render48(&ck);
        let t0 = 1000.0 * onset(&h0[CLICK_AT..], -20.0) as f64 / SR;
        m.pre_ms = (r.predelay_ms - t0).max(0.0);
    }
    let (ir, ys) = rayon::join(
        || m.render48(&ck),
        || {
            (0..seeds)
                .into_par_iter()
                .map(|s| m.render48(&noise_stim(s as u32, 1.0, tail).0))
                .collect::<Vec<_>>()
        },
    );
    let cut = SR as usize;
    (
        ir_metrics(&ir[CLICK_AT..], Some(&noise_band_rt60(&ys, cut))),
        m.pre_ms,
    )
}

fn score(h: &Hall, r: &Metrics, seeds: usize) -> f64 {
    distance(&residuals(&measure(h, r, seeds, true).0, r))
}

fn parse_dens(s: &str) -> Result<Dens, String> {
    let knob = |n: &str| {
        KNOBS
            .iter()
            .position(|k| *k == n)
            .ok_or(format!("dens: no knob {n}"))
    };
    Ok(match s {
        "own" => Dens::Own,
        _ if s.starts_with("fixed:") => Dens::Fixed(s[6..].parse().map_err(|_| "dens: fixed:x")?),
        _ => {
            // KNOB[-][:lo:hi], e.g. diffuse, spread-, diffuse:0.5:1
            let mut it = s.split(':');
            let n = it.next().unwrap_or("");
            let (n, inv) = n.strip_suffix('-').map_or((n, false), |n| (n, true));
            let lo = it
                .next()
                .map_or(Ok(0.0), str::parse)
                .map_err(|_| "dens: lo")?;
            let hi = it
                .next()
                .map_or(Ok(1.0), str::parse)
                .map_err(|_| "dens: hi")?;
            Dens::Tied {
                i: knob(n)?,
                inv,
                lo,
                hi,
            }
        }
    })
}

fn line(p: &str, name: &str, m: &Metrics, r: &Metrics) -> (String, bool) {
    let res = residuals(m, r);
    let v = verdict(&res);
    let br: Vec<String> = res.band_rt60_rel[1..6]
        .iter()
        .map(|x| format!("{:+4.0}", x.unwrap_or(0.0) * 100.0))
        .collect();
    let ned: Vec<String> = res
        .ned_rel
        .iter()
        .map(|x| format!("{:+.0}", x * 100.0))
        .collect();
    (
        format!(
            "{p:>2} {name:22.22} d={:7.1} RT {} pre {:+5.1} c50 {:+5.1} ned {} cen {:+.0} {}",
            distance(&res),
            br.join(" "),
            res.predelay_ms,
            res.c50_db,
            ned.join("/"),
            res.centroid_rel * 100.0,
            if v.all() { "PASS" } else { "FAIL" }
        ),
        v.all(),
    )
}

/// `model-check hall FITS`: the Python model's fitted knobs through this
/// model, 16 seeds, against the references.
pub fn check(cfg: &Config, fits: &std::path::Path) -> Result<(), String> {
    let refs = load_refs(cfg)?;
    let Value::Object(f) = read_json(fits)? else {
        return Err("fits: not an object".into());
    };
    let mut keys: Vec<&String> = f.keys().collect();
    keys.sort_by_key(|k| k.parse::<u32>().unwrap_or(0));
    let rows: Vec<(String, bool)> = keys
        .par_iter()
        .map(|p| -> Result<(String, bool), String> {
            let r = Metrics::from_json(refs.get(*p).ok_or("no ref")?)?;
            let h = Hall::from_python(f[*p]["knobs"].as_object().ok_or("knobs")?);
            let (m, _) = measure(&h, &r, 16, false);
            Ok(line(p, f[*p]["preset"].as_str().unwrap_or(""), &m, &r))
        })
        .collect::<Result<_, _>>()?;
    for (l, _) in &rows {
        println!("{l}");
    }
    println!(
        "pass {} of {}",
        rows.iter().filter(|r| r.1).count(),
        rows.len()
    );
    Ok(())
}

/// `model-fit hall TAG [dens=…] [warm=FITS] PROG …`: each program's knobs
/// by a random start (120 at 2 seeds) and a shrinking pattern search (4
/// seeds), judged at 16; into `out/fits/model_hall_TAG.json`.
pub fn fit(cfg: &Config, tag: &str, opts: &[String], progs: &[u32]) -> Result<(), String> {
    let mut dens = Dens::Own;
    let mut warm: Option<Map<String, Value>> = None;
    for o in opts {
        match o.split_once('=') {
            Some(("dens", v)) => dens = parse_dens(v)?,
            Some(("warm", v)) => {
                warm = read_json(std::path::Path::new(v))?.as_object().cloned();
            }
            _ => return Err(format!("model-fit: unknown option {o}")),
        }
    }
    let refs = load_refs(cfg)?;
    let path = cfg.out.join(format!("fits/model_hall_{tag}.json"));
    let out = Mutex::new(Map::new());
    let free: Vec<usize> = (0..KNOBS.len())
        .filter(|&i| i != 6 || dens == Dens::Own)
        .collect();
    progs.par_iter().try_for_each(|&p| -> Result<(), String> {
        let key = p.to_string();
        let rv = refs.get(&key).ok_or(format!("no reference {p}"))?;
        let r = Metrics::from_json(rv)?;
        let mut rng = Rng::new(p);
        let mut cands: Vec<Vec<f64>> = (0..120)
            .map(|_| (0..KNOBS.len()).map(|_| rng.uniform()).collect())
            .collect();
        if let Some(w) = warm.as_ref().and_then(|w| w.get(&key)) {
            cands.push(
                KNOBS
                    .iter()
                    .map(|n| w["knobs"][*n].as_f64().unwrap_or(0.5))
                    .collect(),
            );
        }
        let mut scored: Vec<(f64, Vec<f64>)> = cands
            .into_par_iter()
            .map(|k| (score(&Hall::from_knobs(&k, dens), &r, 2), k))
            .collect();
        if warm.is_some() {
            // The warm start always gets searched.
            let last = scored.len() - 1;
            scored[last].0 = f64::MIN;
        }
        scored.sort_by(|a, b| a.0.total_cmp(&b.0));
        let best = scored[..2]
            .par_iter()
            .map(|(_, k)| {
                let mut k = k.clone();
                let mut sc = score(&Hall::from_knobs(&k, dens), &r, 4);
                let mut step = 0.15;
                for _ in 0..5 {
                    loop {
                        let cs: Vec<Vec<f64>> = free
                            .iter()
                            .flat_map(|&d| [-step, step].map(|s| (d, s)))
                            .filter_map(|(d, s)| {
                                let v = (k[d] + s).clamp(0.0, 1.0);
                                (v != k[d]).then(|| {
                                    let mut c = k.clone();
                                    c[d] = v;
                                    c
                                })
                            })
                            .collect();
                        let (s2, c) = cs
                            .into_par_iter()
                            .map(|c| (score(&Hall::from_knobs(&c, dens), &r, 4), c))
                            .min_by(|a, b| a.0.total_cmp(&b.0))
                            .unwrap_or((f64::MAX, k.clone()));
                        if s2 < sc - 1e-9 {
                            (sc, k) = (s2, c);
                        } else {
                            break;
                        }
                    }
                    step *= 0.45;
                }
                (sc, k)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .ok_or("no fit")?
            .1;
        let h = Hall::from_knobs(&best, dens);
        let (m, pre) = measure(&h, &r, 16, true);
        let name = rv["name"].as_str().unwrap_or("");
        let (l, pass) = line(&key, name, &m, &r);
        let knobs: Map<String, Value> = KNOBS
            .iter()
            .zip(&best)
            .map(|(n, v)| (n.to_string(), json!(v)))
            .chain([("pre_ms".to_string(), json!(pre))])
            .collect();
        println!("{l} {}", serde_json::to_string(&knobs).unwrap_or_default());
        let mut o = out.lock().map_err(|e| e.to_string())?;
        o.insert(
            key,
            json!({"preset": name, "knobs": knobs, "phys": format!("{h:?}"), "pass": pass,
                   "residuals": residuals(&m, &r).to_json(), "metrics": m.to_json()}),
        );
        write_json(&path, &Value::Object(o.clone()))
    })?;
    let o = out.lock().map_err(|e| e.to_string())?;
    let n = o
        .values()
        .filter(|v| v["pass"].as_bool() == Some(true))
        .count();
    println!("pass {n} of {}", o.len());
    Ok(())
}
