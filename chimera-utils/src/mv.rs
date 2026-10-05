//! The MidiVerb II workflow on top of the A/B core: references, the knob
//! fit, verification, listening pairs and the program listings.

use crate::Config;
use crate::devices::{self, render_reverb, reverb_knob_names};
use crate::metrics::{
    self, BANDS, Metrics, NoiseRt, distance, ir_metrics, noise_band_rt60, residuals, verdict,
};
use crate::reference::{Reference, read_wav, write_f32, write_pcm24};
use crate::stim::{CLICK_AT, NOISE_SEEDS, SR, click, lhs, noise_stim};
use chimera_core::dsp::reverb::ReverbType;
use rayon::prelude::*;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SOURCES: [&str; 5] = ["snare", "kick", "strings", "plucked", "idm"];
const AB_SOURCES: [&str; 3] = ["idm", "snare", "strings"];

/// Each TYPE's fitted knobs, by `devices::reverb_knob_names`.
fn dims(t: ReverbType) -> &'static [&'static str] {
    match t {
        ReverbType::Room => &[
            "size", "time", "tone", "diffuse", "predelay", "smear", "damping", "shape",
        ],
        ReverbType::Gate | ReverbType::Reverse => &["size", "time", "tone", "diffuse", "shape"],
        ReverbType::Quad => &["size", "time", "damping"],
        ReverbType::Hall => &[
            "size", "time", "tone", "diffuse", "predelay", "damping", "smear",
        ],
        ReverbType::Bloom => &["size", "time", "tone", "diffuse", "predelay"],
    }
}

const SWEEP_N: usize = 320;
/// The sweep starts static, as the MidiVerb is; refinement may move it.
const FIX: [(&str, f64); 1] = [("smear", 0.0)];

/// Knob settings by name; the rest stay at the TYPE's start.
pub type Knobs = Map<String, Value>;

/// The values a render takes: the TYPE's start overridden by `k`, each
/// rounded to 6 decimals as the Python harness passed them.
fn values(t: ReverbType, k: &Knobs) -> Vec<f32> {
    let dev = devices::Device::Reverb(t);
    let mut v: Vec<f32> = dev.knobs().iter().map(|k| k.default).collect();
    for (i, n) in reverb_knob_names().iter().enumerate() {
        if let Some(x) = k.get(*n).and_then(Value::as_f64) {
            v[i] = format!("{x:.6}").parse().unwrap_or(x as f32);
        }
    }
    v
}

/// Click, charge and tail lengths: BLOOM's 11 s tails need the references'.
fn lengths(t: ReverbType) -> (f64, f64, f64) {
    if t == ReverbType::Bloom {
        (20.0, 2.0, 20.0)
    } else {
        (3.0, 1.0, 3.0)
    }
}

/// The measurement stimuli: the click and `seeds` noise bursts.
struct Stimuli {
    click: Vec<f32>,
    noise: Vec<Vec<f32>>,
    cut: usize,
}

fn stimuli(t: ReverbType, seeds: usize) -> Stimuli {
    let (click_s, charge_s, tail_s) = lengths(t);
    let f32s = |x: Vec<f64>| x.into_iter().map(|v| v as f32).collect::<Vec<f32>>();
    let noise = (0..seeds)
        .map(|s| f32s(noise_stim(s as u32, charge_s, tail_s).0))
        .collect();
    Stimuli {
        click: f32s(click(click_s)),
        noise,
        cut: (charge_s * SR) as usize,
    }
}

fn f64s(x: Vec<f32>) -> Vec<f64> {
    x.into_iter().map(f64::from).collect()
}

/// Metrics of TYPE `t` at each knob setting: the click's IR and the band
/// RT60s of `seeds` noise bursts.
pub fn measure(t: ReverbType, knobs: &[Knobs], seeds: usize) -> Result<Vec<Metrics>, String> {
    let st = stimuli(t, seeds);
    Ok(knobs
        .par_iter()
        .map(|k| {
            let v = values(t, k);
            let (h, ys) = rayon::join(
                || f64s(render_reverb(t, &v, &st.click)),
                || {
                    st.noise
                        .par_iter()
                        .map(|x| f64s(render_reverb(t, &v, x)))
                        .collect::<Vec<_>>()
                },
            );
            ir_metrics(&h[CLICK_AT..], Some(&noise_band_rt60(&ys, st.cut)))
        })
        .collect())
}

fn read_json(p: &Path) -> Result<Value, String> {
    let s = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("{}: {e}", p.display()))
}

fn write_json(p: &Path, v: &Value) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let s = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(p, s).map_err(|e| format!("{}: {e}", p.display()))
}

fn refs_path(cfg: &Config) -> PathBuf {
    cfg.refs
        .clone()
        .unwrap_or_else(|| cfg.out.join("refs/metrics.json"))
}

fn fits_path(cfg: &Config, t: ReverbType) -> PathBuf {
    cfg.out
        .join(format!("fits/{}.json", t_name(t).to_lowercase()))
}

fn t_name(t: ReverbType) -> &'static str {
    ReverbType::NAMES[t.index()]
}

fn load_refs(cfg: &Config) -> Result<Map<String, Value>, String> {
    match read_json(&refs_path(cfg))? {
        Value::Object(m) => Ok(m),
        _ => Err("refs: not an object".into()),
    }
}

fn reference(cfg: &Config) -> Result<Reference, String> {
    Ok(Reference::MidiVerb(cfg.emu()?))
}

/// The emulator's program names (index = program).
fn names(cfg: &Config) -> Result<Vec<String>, String> {
    let p = cfg.emu()?.with_file_name("names-midiverb2.h");
    let txt = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(txt
        .lines()
        .filter(|l| l.trim().starts_with('"'))
        .filter_map(|l| l.split('"').nth(1).map(str::to_string))
        .collect())
}

fn mv(
    cfg: &Config,
    r: &Reference,
    prog: u32,
    x: &[f64],
    normalise: bool,
) -> Result<Vec<f64>, String> {
    let mut p = vec![("prog".to_string(), prog.to_string())];
    if !normalise {
        p.push(("normalise".into(), "0".into()));
    }
    r.render(&p, Some(x), x.len(), &cfg.tmp())
}

fn read_stim(cfg: &Config, name: &str) -> Result<Vec<f64>, String> {
    let (x, sr) = read_wav(&cfg.stim()?.join(format!("{name}.wav")))?;
    if sr != SR as u32 {
        return Err(format!("{name}.wav: {sr} Hz, want 48000"));
    }
    Ok(x)
}

// ── mv-refs ──

/// Programs `progs` on the click (20 s), 16 noise bursts (2 s + 20 s) and
/// the dry stimuli: IRs, stimuli renders and metrics in `out/refs`.
pub fn refs(cfg: &Config, progs: &[u32], sources: bool) -> Result<(), String> {
    let r = reference(cfg)?;
    let names = names(cfg)?;
    let dir = cfg.out.join("refs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stims: Vec<(&str, Vec<f64>)> = if sources {
        SOURCES
            .iter()
            .map(|&s| read_stim(cfg, s).map(|x| (s, x)))
            .collect::<Result<_, _>>()?
    } else {
        Vec::new()
    };
    let mpath = dir.join("metrics.json");
    let all = Mutex::new(match read_json(&mpath) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    });
    progs.par_iter().try_for_each(|&p| -> Result<(), String> {
        let (h, rt) = rayon::join(
            || mv(cfg, &r, p, &click(20.0), true).map(|y| y[CLICK_AT..].to_vec()),
            || ref_noise_rt(cfg, &r, p),
        );
        let (h, rt) = (h?, rt?);
        let hf: Vec<f32> = h.iter().map(|&v| v as f32).collect();
        write_f32(&dir.join(format!("p{p:02}_ir.wav")), &hf, SR as u32)?;
        let mut m = ir_metrics(&h, Some(&rt)).to_json();
        let name = names.get(p as usize).cloned().unwrap_or_default();
        m["name"] = json!(name);
        stims
            .par_iter()
            .try_for_each(|(s, x)| -> Result<(), String> {
                let y: Vec<f32> = mv(cfg, &r, p, x, true)?
                    .into_iter()
                    .map(|v| v as f32)
                    .collect();
                write_f32(&dir.join(format!("p{p:02}_{s}.wav")), &y, SR as u32)
            })?;
        println!("{}", ref_line(p, &m));
        let mut a = all.lock().map_err(|e| e.to_string())?;
        a.insert(p.to_string(), m);
        write_json(&mpath, &Value::Object(a.clone()))
    })
}

fn ref_noise_rt(cfg: &Config, r: &Reference, p: u32) -> Result<NoiseRt, String> {
    let (charge_s, tail_s) = (2.0, 20.0);
    let stims: Vec<Vec<f64>> = (0..NOISE_SEEDS)
        .map(|s| noise_stim(s as u32, charge_s, tail_s).0)
        .collect();
    let cut = (charge_s * SR) as usize;
    let ys = stims
        .par_iter()
        .map(|x| mv(cfg, r, p, x, false))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(noise_band_rt60(&ys, cut))
}

fn ref_line(p: u32, m: &Value) -> String {
    let f = |v: &Value| v.as_f64().unwrap_or(0.0);
    format!(
        "{p:2} {:28} rt {:5.2} 1k {:5.2} pre {:5.1} c50 {:5.1} ned {:.2}/{:.2}/{:.2} cen {:5.0}",
        m["name"].as_str().unwrap_or(""),
        f(&m["rt60_s"]),
        f(&m["band_rt60_s"]["1000"]),
        f(&m["predelay_ms"]),
        f(&m["c50_db"]),
        f(&m["ned"]["20"]),
        f(&m["ned"]["50"]),
        f(&m["ned"]["100"]),
        f(&m["centroid_hz"]),
    )
}

// ── mv-fit ──

fn score(m: &Metrics, r: &Metrics) -> f64 {
    distance(&residuals(m, r))
}

/// A shrinking pattern search from `start`: each knob ±step, the best move
/// taken until none helps, then the next step. Knobs in `fixed` stay put.
fn refine(
    cfg: &Config,
    t: ReverbType,
    start: &Knobs,
    r: &Metrics,
    steps: &[f64],
    fixed: &[&str],
) -> Result<(Knobs, Metrics, f64), String> {
    let mut best_k = start.clone();
    let mut best_m = measure(t, std::slice::from_ref(&best_k), cfg.refine_seeds)?.remove(0);
    let mut best = score(&best_m, r);
    for &st in steps {
        let mut improved = true;
        while improved {
            improved = false;
            let mut cands = Vec::new();
            for d in dims(t).iter().filter(|d| !fixed.contains(d)) {
                let cur = best_k[*d].as_f64().unwrap_or(0.0);
                for s in [-st, st] {
                    let v = (cur + s).clamp(0.0, 1.0);
                    if v != cur {
                        let mut k = best_k.clone();
                        k.insert(d.to_string(), json!(v));
                        cands.push(k);
                    }
                }
            }
            let ms = measure(t, &cands, cfg.refine_seeds)?;
            for (k, m) in cands.into_iter().zip(ms) {
                let sc = score(&m, r);
                if sc < best - 1e-9 {
                    (best, best_k, best_m, improved) = (sc, k, m, true);
                }
            }
        }
    }
    Ok((best_k, best_m, best))
}

/// The sweep shared by every preset of a TYPE: measured once, kept.
fn sweep(cfg: &Config, t: ReverbType) -> Result<Vec<(Knobs, Metrics)>, String> {
    let path = cfg
        .out
        .join(format!("fits/{}_sweep.json", t_name(t).to_lowercase()));
    if let Ok(Value::Array(a)) = read_json(&path) {
        return a
            .iter()
            .map(|e| {
                let k = e[0].as_object().cloned().ok_or("sweep: knobs")?;
                Ok((k, Metrics::from_json(&e[1])?))
            })
            .collect();
    }
    let ds = dims(t);
    let knobs: Vec<Knobs> = lhs(SWEEP_N, ds.len(), 1)
        .into_iter()
        .map(|u| {
            let mut k = Knobs::new();
            for (d, x) in ds.iter().zip(u) {
                k.insert(d.to_string(), json!(x));
            }
            for (d, x) in FIX {
                if ds.contains(&d) {
                    k.insert(d.into(), json!(x));
                }
            }
            k
        })
        .collect();
    let ms = measure(t, &knobs, 4)?; // ranking only
    let out: Vec<Value> = knobs
        .iter()
        .zip(&ms)
        .map(|(k, m)| json!([k, m.to_json()]))
        .collect();
    write_json(&path, &Value::Array(out))?;
    Ok(knobs.into_iter().zip(ms).collect())
}

fn fit_line(
    p: &str,
    name: &str,
    d: f64,
    pass: &metrics::Verdict,
    t: ReverbType,
    k: &Knobs,
) -> String {
    let knobs: Vec<String> = dims(t)
        .iter()
        .map(|d| format!("{d}={:.3}", k[*d].as_f64().unwrap_or(0.0)))
        .collect();
    format!(
        "{p:>2} {name:26} d={d:8.1} pass={} knobs={}",
        pass.to_json(),
        knobs.join(" ")
    )
}

/// Fits TYPE `t` to each program in `progs` (kept fits are skipped unless
/// `redo`) into `out/fits/<type>.json`: from the sweep's best two, or, with
/// `start`, from that fits file's knobs by finer steps with SMEAR held (the
/// Python harness's `room_v2_rust.py --again`).
pub fn fit(
    cfg: &Config,
    t: ReverbType,
    progs: &[u32],
    redo: bool,
    start: Option<&Path>,
) -> Result<(), String> {
    let refs = load_refs(cfg)?;
    let seeds = match start {
        Some(p) => match read_json(p)? {
            Value::Object(m) => Some(m),
            _ => return Err("start: not an object".into()),
        },
        None => None,
    };
    let sweep = if seeds.is_some() {
        Vec::new()
    } else {
        sweep(cfg, t)?
    };
    let path = fits_path(cfg, t);
    let out = Mutex::new(match read_json(&path) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    });
    progs.par_iter().try_for_each(|&p| -> Result<(), String> {
        let key = p.to_string();
        if !redo && out.lock().map_err(|e| e.to_string())?.contains_key(&key) {
            return Ok(());
        }
        let rv = refs
            .get(&key)
            .ok_or(format!("no reference for program {p}: run mv-refs"))?;
        let r = Metrics::from_json(rv)?;
        let results = match &seeds {
            Some(s) => {
                let k = s[&key]["knobs"]
                    .as_object()
                    .cloned()
                    .ok_or(format!("start: no fit {p}"))?;
                vec![refine(cfg, t, &k, &r, &[0.03, 0.012], &["smear"])?]
            }
            None => {
                let mut ranked: Vec<&(Knobs, Metrics)> = sweep.iter().collect();
                ranked.sort_by(|a, b| score(&a.1, &r).total_cmp(&score(&b.1, &r)));
                ranked[..2]
                    .par_iter()
                    .map(|(k, _)| refine(cfg, t, k, &r, &[0.08, 0.025], &[]))
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        let (k, m, sc) = results
            .into_iter()
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .ok_or("no result")?;
        let res = residuals(&m, &r);
        let v = verdict(&res);
        let name = rv["name"].as_str().unwrap_or("").to_string();
        println!("{}", fit_line(&key, &name, sc, &v, t, &k));
        let mut o = out.lock().map_err(|e| e.to_string())?;
        o.insert(
            key,
            json!({
                "preset": name, "type": t_name(t), "knobs": k, "distance": sc,
                "residuals": res.to_json(), "pass": v.to_json(), "metrics": m.to_json(), "ref": rv,
            }),
        );
        write_json(&path, &Value::Object(o.clone()))
    })
}

// ── mv-verify ──

fn pct(v: f64) -> String {
    format!("{:+.0}%", v * 100.0)
}

/// Re-measures each fit (16 seeds) against the references and rewrites its
/// residuals and verdict; prints a line per preset. A fits file from
/// elsewhere (`fits`) is left alone: the result goes to `out/fits`.
pub fn verify(cfg: &Config, t: ReverbType, fits: Option<&Path>) -> Result<bool, String> {
    let own = fits_path(cfg, t);
    let src = fits.map_or(own.clone(), Path::to_path_buf);
    let dst = if fits.is_some() {
        cfg.out
            .join(format!("fits/{}_verify.json", t_name(t).to_lowercase()))
    } else {
        own
    };
    let mut fits = match read_json(&src)? {
        Value::Object(m) => m,
        _ => return Err("fits: not an object".into()),
    };
    let refs = load_refs(cfg)?;
    let mut keys: Vec<String> = fits.keys().cloned().collect();
    keys.sort_by_key(|k| k.parse::<u32>().unwrap_or(0));
    let knobs: Vec<Knobs> = keys
        .iter()
        .map(|k| {
            fits[k]["knobs"]
                .as_object()
                .cloned()
                .ok_or(format!("fit {k}: no knobs"))
        })
        .collect::<Result<_, _>>()?;
    let ms = measure(t, &knobs, NOISE_SEEDS)?;
    let mut all = true;
    for (p, m) in keys.iter().zip(ms) {
        let rv = refs.get(p).ok_or(format!("no reference for program {p}"))?;
        let r = Metrics::from_json(rv)?;
        let res = residuals(&m, &r);
        let v = verdict(&res);
        all &= v.all();
        let bands: Vec<String> = BANDS
            .iter()
            .zip(res.band_rt60_rel)
            .map(|(b, x)| format!("{b}:{}", pct(x.unwrap_or(0.0))))
            .collect();
        let ned: Vec<String> = res.ned_rel.iter().map(|&x| pct(x)).collect();
        println!(
            "{p:>2} {:24} {} pre {:+.1}ms c50 {:+.1}dB ned {} cen {} {}",
            rv["name"].as_str().unwrap_or(""),
            bands.join(" "),
            res.predelay_ms,
            res.c50_db,
            ned.join("/"),
            pct(res.centroid_rel),
            if v.all() { "PASS" } else { "FAIL" }
        );
        let f = fits
            .get_mut(p)
            .and_then(Value::as_object_mut)
            .ok_or("fit entry")?;
        f.insert("metrics".into(), m.to_json());
        f.insert("ref".into(), rv.clone());
        f.insert("residuals".into(), res.to_json());
        f.insert("pass".into(), v.to_json());
        f.insert("distance".into(), json!(distance(&res)));
    }
    write_json(&dst, &Value::Object(fits))?;
    Ok(all)
}

// ── mv-ab ──

/// Gated RMS (dB): 50 ms energy, frames within 50 dB of the loudest.
fn gated_rms_db(x: &[f64]) -> f64 {
    let w = (0.05 * SR) as usize;
    let mut pre = vec![0.0; x.len() + 1];
    for (i, v) in x.iter().enumerate() {
        pre[i + 1] = pre[i] + v * v;
    }
    let off = (w - 1) / 2;
    let e: Vec<f64> = (0..x.len())
        .map(|i| {
            let hi = (i + off).min(x.len() - 1) + 1;
            let lo = (i + off + 1).saturating_sub(w);
            (pre[hi] - pre[lo]) / w as f64
        })
        .collect();
    let top = e.iter().copied().fold(0.0, f64::max);
    let kept: Vec<f64> = e.into_iter().filter(|&v| v > top * 1e-5).collect();
    10.0 * (kept.iter().sum::<f64>() / kept.len().max(1) as f64 + 1e-30).log10()
}

/// Gains putting each of `pair` at −23 dB gated RMS, with one trim for both
/// keeping the louder peak at −1 dBFS.
pub fn rms_match(pair: [&[f64]; 2]) -> [Vec<f64>; 2] {
    let gains = pair.map(|x| 10f64.powf((-23.0 - gated_rms_db(x)) / 20.0));
    let pk = pair
        .iter()
        .zip(gains)
        .map(|(x, g)| x.iter().fold(0.0f64, |m, v| m.max((v * g).abs())))
        .fold(0.0, f64::max);
    let trim = (0.89 / pk).min(1.0);
    [0, 1].map(|i| pair[i].iter().map(|v| v * gains[i] * trim).collect())
}

/// Each fitted preset's reference and the TYPE at its knobs on idm, snare
/// and strings, RMS-matched, into `out/ab/<type>/`.
pub fn ab(cfg: &Config, t: ReverbType) -> Result<(), String> {
    let fits = match read_json(&fits_path(cfg, t))? {
        Value::Object(m) => m,
        _ => return Err("fits: not an object".into()),
    };
    let r = reference(cfg)?;
    let dir = cfg.out.join(format!("ab/{}", t_name(t).to_lowercase()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stims: Vec<(&str, Vec<f64>)> = AB_SOURCES
        .iter()
        .map(|&s| read_stim(cfg, s).map(|x| (s, x)))
        .collect::<Result<_, _>>()?;
    let mut keys: Vec<&String> = fits.keys().collect();
    keys.sort_by_key(|k| k.parse::<u32>().unwrap_or(0));
    let jobs: Vec<(&String, &(&str, Vec<f64>))> = keys
        .iter()
        .flat_map(|&k| stims.iter().map(move |s| (k, s)))
        .collect();
    let manifest = jobs
        .par_iter()
        .map(|&(p, (s, x))| -> Result<Vec<Value>, String> {
            let f = &fits[p];
            let k = f["knobs"].as_object().cloned().unwrap_or_default();
            let prog: u32 = p.parse().map_err(|_| "program")?;
            let xf: Vec<f32> = x.iter().map(|&v| v as f32).collect();
            let ours = f64s(render_reverb(t, &values(t, &k), &xf));
            let theirs = mv(cfg, &r, prog, x, true)?;
            let [a, b] = rms_match([&theirs, &ours]);
            let preset = f["preset"].as_str().unwrap_or("");
            let slug = preset.to_lowercase().replace(' ', "_").replace('.', "");
            let mut out = Vec::new();
            for (y, side, tag) in [
                (
                    a,
                    "MidiVerb II (reference)".to_string(),
                    "midiverb2".to_string(),
                ),
                (
                    b,
                    format!("Chimera {}", t_name(t)),
                    t_name(t).to_lowercase(),
                ),
            ] {
                let name = format!("p{prog:02}_{slug}_{s}_{tag}.wav");
                write_pcm24(&dir.join(&name), &y, SR as u32)?;
                out.push(json!({
                    "preset": preset, "program": prog, "source": s, "side": side, "file": name,
                    "fitted_knobs": k, "residuals": f["residuals"], "pass": f["pass"],
                    "gated_rms_dbfs": (gated_rms_db(&y) * 100.0).round() / 100.0,
                }));
            }
            Ok(out)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let manifest: Vec<Value> = manifest.into_iter().flatten().collect();
    let n = manifest.len();
    write_json(&dir.join("manifest.json"), &Value::Array(manifest))?;
    println!("{} {n} files", dir.display());
    Ok(())
}

// ── mv-structure ──

fn parse_num(s: &str) -> Option<(u32, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 {
        None
    } else {
        s[..end].parse().ok().map(|n| (n, &s[end..]))
    }
}

/// `LINE(a, b, c)` at the start of `s` (after "LINE("): (a, b, c, rest).
fn parse_line_args(s: &str) -> Option<(u32, u32, u32, &str)> {
    let (a, s) = parse_num(s)?;
    let (b, s) = parse_num(s.strip_prefix(", ")?)?;
    let (c, s) = parse_num(s.strip_prefix(", ")?)?;
    Some((a, b, c, s.strip_prefix(')')?))
}

/// Every 3-argument `LINE(…)` in `s`.
fn line_refs(s: &str) -> Vec<(u32, u32, u32)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("LINE(") {
        rest = &rest[i + 5..];
        if let Some((a, b, c, _)) = parse_line_args(rest) {
            out.push((a, b, c));
        }
    }
    out
}

/// A program's structure from the decompiled listing, read as
/// documentation: counts and lengths only.
pub fn structure(cfg: &Config, progs: &[u32]) -> Result<(), String> {
    let path = cfg.listing()?;
    let txt = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    for &n in progs {
        let head = format!("effect {n}:");
        let at = txt.find(&head).ok_or(format!("no effect {n}"))?;
        let from = at + txt[at..].find('\n').ok_or("listing")? + 1;
        let body = &txt[from..from + txt[from..].find("#undef WRITE_LINE").ok_or("listing")?];
        let b: Vec<&str> = body.lines().collect();
        let mut taps = Vec::new();
        for (side, var) in [("L", "out_left"), ("R", "out_right")] {
            let l = b
                .iter()
                .find(|l| l.contains(&format!("*{var} =")))
                .ok_or("no output")?;
            let src = l
                .split('=')
                .nth(1)
                .unwrap_or("")
                .trim_matches(|c| c == ' ' || c == ';');
            let tl = b
                .iter()
                .position(|l| l.trim_start().starts_with(&format!("{src} = Acc")));
            let t = match tl {
                Some(i) if i > 0 => {
                    let v: Vec<String> = line_refs(b[i - 1])
                        .iter()
                        .map(|(a, _, c)| format!("({a}, {c})"))
                        .collect();
                    format!("({}): [{}]", v.len(), v.join(", "))
                }
                _ => format!("({src}): {src}"),
            };
            taps.push((side, t));
        }
        let mut aps = Vec::new();
        for (i, l) in b.iter().enumerate() {
            let Some(r) = l.trim_start().strip_prefix("WRITE_LINE(") else {
                continue;
            };
            let Some((lid, r)) = parse_num(r) else {
                continue;
            };
            let Some((w, r)) = r.strip_prefix(", ").and_then(parse_num) else {
                continue;
            };
            if !r.starts_with(") = -Acc;") || i == 0 {
                continue;
            }
            let rd = line_refs(b[i - 1])
                .into_iter()
                .find(|&(a, bb, _)| a == lid && bb == w);
            let g = if b[i - 1].contains("* 3 / 4")
                || b.get(i + 1).is_some_and(|n| n.contains("* -3 / 4"))
            {
                "3/4"
            } else {
                "1/2"
            };
            if let Some((_, _, d)) = rd {
                aps.push(format!("({lid}, {d}, '{g}')"));
            }
        }
        // One-poles: `LINE(id, x, 1) / 2;` ending a line whose next line
        // writes that id.
        let mut onepoles = 0;
        for (i, l) in b.iter().enumerate() {
            let Some(j) = l.rfind("LINE(") else { continue };
            let Some((id, _, one, rest)) = parse_line_args(&l[j + 5..]) else {
                continue;
            };
            let next = b[i + 1..]
                .iter()
                .map(|n| n.trim_start())
                .find(|n| !n.is_empty());
            if one == 1
                && rest == " / 2;"
                && next.is_some_and(|n| n.starts_with(&format!("WRITE_LINE({id}")))
            {
                onepoles += 1;
            }
        }
        // Two-sample averages: `-LINE(id, x, a) / 4 + -LINE(id, y, b) / 4`.
        let mut avg2 = 0;
        for l in &b {
            let mut rest = *l;
            while let Some(j) = rest.find("-LINE(") {
                rest = &rest[j + 6..];
                let Some((id, _, _, r)) = parse_line_args(rest) else {
                    continue;
                };
                let Some(r) = r.strip_prefix(" / 4 + -LINE(") else {
                    continue;
                };
                let Some((id2, _, _, r)) = parse_line_args(r) else {
                    continue;
                };
                if id2 == id && r.starts_with(" / 4") {
                    avg2 += 1;
                    rest = &r[4..];
                }
            }
        }
        let inj = body.matches("tmp_b").count() as i64 - 1;
        println!("#{n}: injections {inj}, one-poles {onepoles}, 2-sample averages {avg2}");
        println!("  allpasses (line, delay smp, g): [{}]", aps.join(", "));
        for (s, t) in taps {
            println!("  taps {s} {t}");
        }
    }
    Ok(())
}
