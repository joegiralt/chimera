//! The 2040 lab's harness (`circuit2040.rs`), shared by every four-pole
//! lab: one render's input and controls (`Case`), the rest pads, the output
//! coupling, the band limit, the saw and the sweeps and probes.

use super::sig::firwin_kaiser;
use crate::FS;
use std::f64::consts::PI;

/// One render's input and controls, at 48 kHz.
pub struct Case {
    pub x: Vec<f64>,
    pub fc: Vec<f64>,
    pub res: Vec<f64>,
}

/// `v` held-and-ramped to `up` × its rate: each sample to the next, linearly.
pub fn lerp_up(v: &[f64], up: usize) -> Vec<f64> {
    let mut o = Vec::with_capacity(v.len() * up);
    for i in 0..v.len() {
        let (a, b) = (v[i], v[(i + 1).min(v.len() - 1)]);
        for j in 0..up {
            o.push(a + (b - a) * j as f64 / up as f64);
        }
    }
    o
}

/// 50 ms of rest at each end, the controls held: every renderer starts at
/// its no-input operating point, and the resamplers' edges (which see the
/// offsets' DC as a step) fall outside what is kept.
pub const PAD: usize = (0.05 * FS) as usize;

pub fn padded(c: &Case) -> Case {
    let p = |v: &[f64], a: f64, b: f64| -> Vec<f64> {
        std::iter::repeat_n(a, PAD)
            .chain(v.iter().copied())
            .chain(std::iter::repeat_n(b, PAD))
            .collect()
    };
    let (n, x) = (c.x.len(), &c.x);
    Case {
        x: p(x, 0.0, 0.0),
        fc: p(&c.fc, c.fc[0], c.fc[n - 1]),
        res: p(&c.res, c.res[0], c.res[n - 1]),
    }
}

/// `f` on the padded case, the pads cut off again.
pub fn pad_run(
    c: &Case,
    f: impl FnOnce(&Case) -> Result<Vec<f64>, String>,
) -> Result<Vec<f64>, String> {
    let y = f(&padded(c))?;
    Ok(y[PAD..PAD + c.x.len()].to_vec())
}

/// C413 0.1 µF into R471 100 K, the coupling into the FIN VCA (15.9 Hz),
/// on all three: the offsets' DC never reaches the output.
pub fn ac(x: &[f64]) -> Vec<f64> {
    let w = (PI * 15.9 / FS).tan();
    let (b0, a1) = (1.0 / (1.0 + w), (1.0 - w) / (1.0 + w));
    let (mut xp, mut yp) = (x.first().copied().unwrap_or(0.0), 0.0);
    x.iter()
        .map(|&v| {
            yp = b0 * (v - xp) + a1 * yp;
            xp = v;
            yp
        })
        .collect()
}

/// Linear-phase low-pass at 19 kHz (255 taps, Kaiser β 10), delay
/// removed: no input sits in any renderer's resampling transition band, so
/// the offsets' intermodulation is the input's, not the upsampler's.
pub fn band_limit(x: &[f64]) -> Vec<f64> {
    let h = firwin_kaiser(255, 19_000.0 / (FS / 2.0), 10.0);
    let d = h.len() / 2;
    (0..x.len())
        .map(|i| {
            let mut acc = 0.0;
            for (j, hj) in h.iter().enumerate() {
                if let Some(v) = (i + d).checked_sub(j).and_then(|k| x.get(k)) {
                    acc += hj * v;
                }
            }
            acc
        })
        .collect()
}

/// A band-limited saw at `f`, peak about 1.
pub fn saw(f: f64, n: usize) -> Vec<f64> {
    let k = (20_000.0 / f) as usize;
    (0..n)
        .map(|i| {
            let ph = 2.0 * PI * f * i as f64 / FS;
            (1..=k)
                .map(|j| (j as f64 * ph).sin() / j as f64)
                .sum::<f64>()
                * (2.0 / PI)
                * 0.92
        })
        .collect()
}

/// 5 s of a 110 Hz saw, the cutoff 40 Hz → 15 kHz exponentially.
pub fn cutoff_sweep(res: f64) -> Case {
    let n = (5.0 * FS) as usize;
    let x = saw(110.0, n);
    let fc = (0..n)
        .map(|i| 40.0 * (15_000.0f64 / 40.0).powf(i as f64 / n as f64))
        .collect();
    Case {
        x,
        fc,
        res: vec![res; n],
    }
}

/// A 110 Hz saw at 800 Hz, RES 0 → 1 over 4 s, held; the saw stops at
/// 4.5 s and the loop rings on alone for 1.5 s.
pub fn res_sweep() -> Case {
    let n = (6.0 * FS) as usize;
    let mut x = saw(110.0, n);
    let stop = (4.5 * FS) as usize;
    for (i, v) in x.iter_mut().enumerate().skip(stop) {
        let t = (i - stop) as f64 / (0.005 * FS);
        *v *= (1.0 - t).max(0.0);
    }
    let res = (0..n).map(|i| (i as f64 / (4.0 * FS)).min(1.0)).collect();
    Case {
        x,
        fc: vec![800.0; n],
        res,
    }
}

/// 0.6 s of `wave` at `f0`, the last 0.25 s measured.
pub fn probe(f0: f64, fc: f64, res: f64, level_db: f64, sine: bool) -> Case {
    let n = (0.6 * FS) as usize;
    let g = 10f64.powf(level_db / 20.0);
    let x: Vec<f64> = if sine {
        (0..n)
            .map(|i| g * (2.0 * PI * f0 * i as f64 / FS).sin())
            .collect()
    } else {
        saw(f0, n).into_iter().map(|v| v * g).collect()
    };
    Case {
        x,
        fc: vec![fc; n],
        res: vec![res; n],
    }
}

/// The last 0.25 s: what a probe measures.
pub fn tail(y: &[f64]) -> &[f64] {
    &y[y.len() - (0.25 * FS) as usize..]
}

impl Case {
    /// A `Voice` call's input and controls as a `Case`.
    pub fn of(x: &[f64], c: &[crate::Controls]) -> Case {
        Case {
            x: x.to_vec(),
            fc: c.iter().map(|c| c.cutoff).collect(),
            res: c.iter().map(|c| c.res).collect(),
        }
    }
}
