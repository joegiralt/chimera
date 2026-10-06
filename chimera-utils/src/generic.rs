//! The generic A/B: any reference program against any Chimera device, on
//! noise (and a sine, for an effect) or from nothing (a source).

use crate::devices::{Device, Values};
use crate::mv::rms_match;
use crate::reference::{Reference, write_pcm24};
use crate::sig::{fft, plan, spectra};
use crate::stim::{Rng, listening};
use crate::{Config, SR};
use rustfft::num_complex::Complex64;
use serde_json::json;
use std::f64::consts::PI;
use std::path::Path;

const SECONDS: f64 = 2.0;
const SINE_HZ: f64 = 1000.0;
const NPERSEG: usize = 4096;
/// Response bins below this, re the reference's peak, are not compared.
const FLOOR_DB: f64 = -60.0;

/// Octave bands, by centre: 31.25 Hz to 16 kHz.
fn octaves() -> Vec<f64> {
    (0..10).map(|k| 31.25 * 2f64.powi(k)).collect()
}

fn bin_hz(k: usize) -> f64 {
    k as f64 * SR / NPERSEG as f64
}

/// Each side's magnitude response (dB per bin): |Pxy|/Pxx of an effect on
/// `input`, or the output's own spectrum for a source.
fn response(input: Option<&[f64]>, y: &[f64]) -> Vec<f64> {
    match input {
        Some(x) => {
            let s = spectra(x, Some(y), SR, NPERSEG);
            s.pxy
                .iter()
                .zip(&s.pxx)
                .map(|(c, p)| 20.0 * (c.norm() / p + 1e-30).log10())
                .collect()
        }
        None => spectra(y, None, SR, NPERSEG)
            .pxx
            .iter()
            .map(|p| 10.0 * (p + 1e-30).log10())
            .collect(),
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

/// How well `ours` cancels the reference.
#[derive(Clone, Copy, Debug)]
pub struct Null {
    /// The residual's energy re the reference's, dB.
    pub db: f64,
    /// Samples; positive: ours is late.
    pub lag: i64,
    /// |gain| applied to ours, dB.
    pub gain_db: f64,
    /// Ours cancels best inverted.
    pub inverted: bool,
}

/// Null residual: `ours` aligned to `r` (the lag within ±100 ms of the
/// largest |correlation|, then the best signed gain). Reference samples
/// outside the overlap count as residual.
pub fn null_residual(r: &[f64], ours: &[f64]) -> Null {
    let n = r.len().min(ours.len());
    let m = (2 * n).next_power_of_two();
    let pad = |x: &[f64]| {
        let mut v = x[..n].to_vec();
        v.resize(m, 0.0);
        v
    };
    let (a, b) = (fft(&pad(r)), fft(&pad(ours)));
    let mut c: Vec<Complex64> = a.iter().zip(&b).map(|(x, y)| x.conj() * y).collect();
    plan(m, true).process(&mut c);
    let max_lag = (SR / 10.0) as i64;
    let at = |l: i64| c[l.rem_euclid(m as i64) as usize].re.abs();
    let lag = (-max_lag..=max_lag)
        .max_by(|&p, &q| at(p).total_cmp(&at(q)))
        .unwrap_or(0);
    // ours[i + lag] lines up with r[i].
    let other = |i: usize| {
        let j = i as i64 + lag;
        (0..ours.len() as i64)
            .contains(&j)
            .then(|| ours[j as usize])
    };
    let (rb, bb) = (0..n)
        .filter_map(|i| other(i).map(|y| (r[i], y)))
        .fold((0.0, 0.0), |(rb, bb), (x, y)| (rb + x * y, bb + y * y));
    let g = if bb > 0.0 { rb / bb } else { 0.0 };
    let res: f64 = (0..n)
        .map(|i| (r[i] - g * other(i).unwrap_or(0.0)).powi(2))
        .sum();
    let er: f64 = r[..n].iter().map(|v| v * v).sum();
    Null {
        db: 10.0 * (res / er + 1e-30).log10(),
        lag,
        gain_db: 20.0 * g.abs().max(1e-30).log10(),
        inverted: g < 0.0,
    }
}

/// `x` delayed by `lag` samples (advanced if negative), zero-filled.
fn shifted(x: &[f64], lag: i64) -> Vec<f64> {
    (0..x.len() as i64)
        .map(|j| {
            let i = j - lag;
            if (0..x.len() as i64).contains(&i) {
                x[i as usize]
            } else {
                0.0
            }
        })
        .collect()
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
    let hz = SR / n as f64;
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
        .filter(|&fh| fh < 0.5 * SR - 8.0 * hz)
        .map(power)
        .sum();
    (harm / fund).sqrt()
}

/// Spectral centroid (20 Hz–20 kHz).
pub fn centroid(y: &[f64]) -> f64 {
    let s = spectra(y, None, SR, NPERSEG);
    let (mut num, mut den) = (0.0, 0.0);
    for (f, v) in s.f.iter().zip(&s.pxx) {
        if (20.0..=20_000.0).contains(f) {
            num += f * v;
            den += v;
        }
    }
    num / den
}

fn noise() -> Vec<f64> {
    let mut rng = Rng::new(1);
    let g = 10f64.powf(-18.0 / 20.0);
    (0..(SECONDS * SR) as usize)
        .map(|_| (rng.normal() * g).clamp(-0.95, 0.95))
        .collect()
}

fn sine() -> Vec<f64> {
    let g = 10f64.powf(-12.0 / 20.0);
    (0..(SECONDS * SR) as usize)
        .map(|i| g * (2.0 * PI * SINE_HZ * i as f64 / SR).sin())
        .collect()
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-30).log10()
}

/// Both sides' renders and what was measured between them.
pub struct Comparison {
    pub theirs: Vec<f64>,
    pub ours: Vec<f64>,
    pub null: Null,
    /// Max |Δ| dB per octave (centre Hz), after the null's alignment.
    pub bands: Vec<(f64, Option<f64>)>,
    pub max_err: f64,
    pub thd: (f64, f64),
    pub centroid: (f64, f64),
}

/// Renders `dev` at `values` and reference `r` with `params` on the same
/// stimulus (noise for an effect, nothing for a source) and measures them.
pub fn compare(
    r: &Reference,
    params: &[(String, String)],
    dev: &Device,
    values: &Values,
    tmp: &Path,
) -> Result<Comparison, String> {
    let frames = (SECONDS * SR) as usize;
    let render = |x: Option<&[f64]>| -> Result<(Vec<f64>, Vec<f64>), String> {
        let theirs = r.render(params, x, frames, tmp)?;
        let xf: Vec<f32> = x.unwrap_or(&[]).iter().map(|&v| v as f32).collect();
        let ours = dev
            .render(values, &xf, frames)
            .into_iter()
            .map(f64::from)
            .collect();
        Ok((theirs, ours))
    };
    let input = (!dev.is_source()).then(noise);
    let (theirs, ours) = render(input.as_deref())?;
    let null = null_residual(&theirs, &ours);
    // The reference moved onto ours, so latency costs no coherence; both
    // judged over the span the move leaves whole.
    let n = theirs.len().min(ours.len());
    let lag = null.lag.unsigned_abs() as usize;
    let span = if null.lag >= 0 { lag..n } else { 0..n - lag };
    let x = input.as_deref().map(|x| &x[span.clone()]);
    let rr = response(x, &shifted(&theirs[..n], null.lag)[span.clone()]);
    let ro = response(x, &ours[span]);
    let bands = band_errors(&rr, &ro);
    let max_err = bands.iter().filter_map(|b| b.1).fold(0.0, f64::max);
    let thd = if dev.is_source() {
        (thd(&theirs, None), thd(&ours, None))
    } else {
        let (a, b) = render(Some(&sine()))?;
        (thd(&a, Some(SINE_HZ)), thd(&b, Some(SINE_HZ)))
    };
    let centroid = (centroid(&theirs), centroid(&ours));
    Ok(Comparison {
        theirs,
        ours,
        null,
        bands,
        max_err,
        thd,
        centroid,
    })
}

/// `just ab REF DEVICE [k=v …]`: compares, prints the metrics, writes the
/// A/B WAVs and `report.json` to `out/ab/<ref>_<device>/`: the measured
/// pair, and for an effect `<source>_{ref,chimera}.wav` on the listening set.
/// Pairs go to both sides; `ref.k=v` to the reference alone.
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
    let r = Reference::find(ref_name, || cfg.emu(), cfg.refs_dir.as_deref())?;
    let rp: Vec<(String, String)> = both.iter().chain(&ref_only).cloned().collect();
    let c = compare(&r, &rp, &dev, &values, &cfg.tmp())?;
    let n = c.null;
    let ((thd_r, thd_o), (cr, co)) = (c.thd, c.centroid);
    println!("{ref_name} vs {dev_name} {}", args.join(" "));
    println!(
        "  null residual   {:+.1} dB (lag {} smp, gain {:+.2} dB{})",
        n.db,
        n.lag,
        n.gain_db,
        if n.inverted { ", inverted" } else { "" }
    );
    println!("  response error  max {:.2} dB; by octave:", c.max_err);
    for (fc, e) in &c.bands {
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
    let ref_slug = Path::new(ref_name)
        .file_name()
        .map_or(ref_name.into(), |s| s.to_string_lossy());
    let dir = cfg.out.join("ab").join(format!("{ref_slug}_{dev_name}"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let [a, b] = rms_match([&c.theirs, &c.ours]);
    write_pcm24(&dir.join("ref.wav"), &a, SR as u32)?;
    write_pcm24(&dir.join("chimera.wav"), &b, SR as u32)?;
    if !dev.is_source() {
        for (s, x) in listening() {
            let xf: Vec<f32> = x.iter().map(|&v| v as f32).collect();
            let theirs = r.render(&rp, Some(&x), x.len(), &cfg.tmp())?;
            let ours: Vec<f64> = dev
                .render(&values, &xf, x.len())
                .into_iter()
                .map(f64::from)
                .collect();
            let [a, b] = rms_match([&theirs, &ours]);
            write_pcm24(&dir.join(format!("{s}_ref.wav")), &a, SR as u32)?;
            write_pcm24(&dir.join(format!("{s}_chimera.wav")), &b, SR as u32)?;
        }
    }
    let report = json!({
        "reference": ref_name, "device": dev_name, "args": args,
        "null_residual_db": n.db, "lag_samples": n.lag, "gain_db": n.gain_db, "inverted": n.inverted,
        "response_max_error_db": c.max_err,
        "response_error_db_by_octave": c.bands.iter().map(|(f, e)| json!([f, e])).collect::<Vec<_>>(),
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
    use crate::reference::{read_wav, write_f32};

    #[test]
    fn a_delayed_inverted_copy_nulls_and_a_pure_sine_has_no_thd() {
        let x = noise();
        let mut y = vec![0.0; 37];
        y.extend(x.iter().map(|v| -0.5 * v));
        let n = null_residual(&x[..x.len() - 37], &y);
        assert_eq!(n.lag, 37);
        assert!(n.inverted && (n.gain_db - 6.02).abs() < 0.01, "{n:?}");
        assert!(n.db < -100.0, "{n:?}");
        assert!(thd(&sine(), Some(SINE_HZ)) < 1e-4);
    }

    const FAKE_ENV: &str = "CHIMERA_UTILS_FAKE_REF";

    /// The fake reference, run by the test binary on itself: the SVF
    /// (CLEAN) on the contract, with `fault=invert|latency|short`.
    #[test]
    #[ignore]
    fn fake_reference() {
        let Ok(args) = std::env::var(FAKE_ENV) else {
            return;
        };
        let kv: Vec<(String, String)> = args
            .split_whitespace()
            .filter_map(|a| {
                a.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect();
        let get = |k: &str| kv.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
        let dev = Device::find("clean-filter").unwrap();
        let knobs: Vec<(String, String)> = kv
            .iter()
            .filter(|(k, _)| ["mode", "freq", "res"].contains(&k.as_str()))
            .cloned()
            .collect();
        let (x, _) = read_wav(Path::new(get("in").unwrap())).unwrap();
        let xf: Vec<f32> = x.iter().map(|&v| v as f32).collect();
        let mut y = dev.render(&dev.values(&knobs).unwrap(), &xf, xf.len());
        match get("fault") {
            Some("invert") => y.iter_mut().for_each(|v| *v = -*v),
            Some("latency") => {
                y.splice(0..0, [0.0; 2000]);
            }
            Some("short") => y.truncate(y.len() / 2),
            _ => {}
        }
        let sr: u32 = get("sr").unwrap().parse().unwrap();
        write_f32(Path::new(get("out").unwrap()), &y, sr).unwrap();
    }

    #[test]
    fn a_fake_reference_is_measured_through_inversion_latency_and_short_output() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join("chimera-utils-fake");
        // Removed when the test ends, passed or failed.
        let scratch = crate::reference::Scratch::new(&root).unwrap();
        let dir = scratch.path("");
        let script = dir.join("fake-svf");
        let me = std::env::current_exe().unwrap();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n{FAKE_ENV}=\"$*\" exec '{}' --exact generic::tests::fake_reference --ignored -q >/dev/null\n",
                me.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let r = Reference::find(script.to_str().unwrap(), || Err("no emu".into()), None).unwrap();
        let dev = Device::find("clean-filter").unwrap();
        let knobs = [("mode", "LP2"), ("freq", "1000"), ("res", "0.5")]
            .map(|(k, v)| (k.to_string(), v.to_string()));
        let values = dev.values(&knobs).unwrap();
        let run = |fault: &str| {
            let mut p = knobs.to_vec();
            p.push(("fault".into(), fault.into()));
            compare(&r, &p, &dev, &values, &dir.join("tmp"))
        };
        let clean = run("none").unwrap();
        assert!(clean.null.db < -100.0 && clean.null.lag == 0 && !clean.null.inverted);
        assert!(clean.max_err < 0.01, "{}", clean.max_err);

        let inv = run("invert").unwrap();
        assert!(inv.null.db < -100.0 && inv.null.inverted, "{:?}", inv.null);
        assert!(inv.max_err < 0.01, "{}", inv.max_err);

        let late = run("latency").unwrap();
        assert_eq!(late.null.lag, -2000);
        assert!(late.null.db < -100.0, "{:?}", late.null);
        assert!(
            late.max_err < 0.01,
            "latency costs the response {} dB",
            late.max_err
        );

        let short = run("short").err().unwrap_or_default();
        assert!(short.contains("samples, asked"), "{short}");
        // Every render's scratch directory is gone, the failed one's too.
        assert_eq!(std::fs::read_dir(dir.join("tmp")).unwrap().count(), 0);
    }
}
