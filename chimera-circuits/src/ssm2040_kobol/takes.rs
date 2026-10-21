//! The measures, fits and stimuli the Kobol lab read the owner's takes
//! with (`kobol.rs`, `kab.rs`, `kexp.rs` on `exp-circuit-lab`): harmonic
//! analysis, the Kobol's own VCO as a wave table, response-shape fits, the
//! self-oscillation note finder, the drive steps, the phrases' sequences
//! and envelopes, and the A/B's jobs.
//!
//! The takes (20-channel f32 WAVs from the MOTU, the Kobol on input 5)
//! never enter the repo: `session()` reads their directory from
//! `KOBOL_DIR`. What the lab derived from them and wrote down (the sweeps'
//! FREQ tracks, the phrases' fitted patches) is in
//! `docs/ssm2040_kobol/compare-*.md` and read from there.
//!
//! Every renderer is a closure `(x, fc, res_or_k) → y` at 48 kHz, so any
//! model (the circuit, EXP-C, the firmware's 2040) can be read the same
//! way.

use crate::FS;
use crate::util::sig::fft;
use rustfft::num_complex::Complex64;
use std::f64::consts::PI;
use std::path::PathBuf;

/// A renderer: `x`, FREQ (Hz) and RES or `k` per sample → output.
pub type Render<'a> = &'a dyn Fn(&[f64], &dyn Fn(usize) -> f64, &dyn Fn(usize) -> f64) -> Vec<f64>;

/// The lab's sweeps' FREQ tracks and fit tables (`kobol sweeps`).
pub const SWEEPS_MD: &str = include_str!("../../docs/ssm2040_kobol/compare-sweeps.md");
/// The phrases' fitted patches (`kobol phrases`).
pub const PHRASES_MD: &str = include_str!("../../docs/ssm2040_kobol/compare-phrases.md");

/// The takes' directory, from `KOBOL_DIR` (the owner's is
/// `.superpowers/ledgers/kobol-session`), if it is set and exists.
pub fn session() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("KOBOL_DIR")?);
    p.is_dir().then_some(p)
}

/// Input 5 of a 20-channel f32 take in `dir`, raw after `data`.
pub fn load(dir: &std::path::Path, name: &str) -> Result<Vec<f64>, String> {
    let p = dir.join(format!("{name}.wav"));
    let b = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let d = b
        .windows(4)
        .position(|w| w == b"data")
        .ok_or("no data chunk")?;
    let body = &b[d + 8..];
    let ch = 20;
    let n = body.len() / (4 * ch);
    Ok((0..n)
        .map(|i| {
            let o = (i * ch + 4) * 4;
            f64::from(f32::from_le_bytes([
                body[o],
                body[o + 1],
                body[o + 2],
                body[o + 3],
            ]))
        })
        .collect())
}

/// `x` from `t0` to `t1` s.
pub fn seg(x: &[f64], t0: f64, t1: f64) -> &[f64] {
    &x[(t0 * FS) as usize..((t1 * FS) as usize).min(x.len())]
}

/// Blackman-Harris, 4 term.
pub fn bh(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let t = 2.0 * PI * i as f64 / (n - 1) as f64;
            0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos()
        })
        .collect()
}

/// Amplitude of the sinusoid at `f` in windowed `x` (a sine of peak A
/// reads A).
pub fn amp_at(x: &[f64], w: &[f64], f: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    let dw = 2.0 * PI * f / FS;
    let (c, s) = (dw.cos(), dw.sin());
    let (mut cr, mut ci) = (1.0f64, 0.0f64);
    for (i, (&v, &wi)) in x.iter().zip(w).enumerate() {
        re += v * wi * cr;
        im -= v * wi * ci;
        let t = cr * c - ci * s;
        ci = cr * s + ci * c;
        cr = t;
        if i % 1024 == 1023 {
            let m = (cr * cr + ci * ci).sqrt();
            cr /= m;
            ci /= m;
        }
    }
    2.0 * (re * re + im * im).sqrt() / w.iter().sum::<f64>()
}

/// Complex amplitude at `f` (a sine of peak A reads |A|), phase re the
/// segment's first sample.
pub fn c_at(x: &[f64], w: &[f64], f: f64) -> Complex64 {
    let (mut re, mut im) = (0.0, 0.0);
    let dw = 2.0 * PI * f / FS;
    for (i, (&v, &wi)) in x.iter().zip(w).enumerate() {
        let a = dw * i as f64;
        re += v * wi * a.cos();
        im -= v * wi * a.sin();
    }
    Complex64::new(re, im) * (2.0 / w.iter().sum::<f64>())
}

/// Magnitude spectrum of Blackman-Harris-windowed `x`, zero-padded to `n`.
pub fn mag(x: &[f64], n: usize) -> Vec<f64> {
    let w = bh(x.len());
    let mut b = vec![0.0; n];
    for i in 0..x.len().min(n) {
        b[i] = x[i] * w[i];
    }
    let s = w.iter().sum::<f64>();
    fft(&b)[..n / 2]
        .iter()
        .map(|c| 2.0 * c.norm() / s)
        .collect()
}

/// The loudest bin of `m` (an `n`-point spectrum) between `lo` and `hi`
/// Hz, refined by a log-parabola: (Hz, amplitude).
fn peak_in(m: &[f64], n: usize, lo: f64, hi: f64) -> (f64, f64) {
    let df = FS / n as f64;
    let (a, b) = ((lo / df) as usize, ((hi / df) as usize).min(m.len() - 2));
    let k = (a.max(1)..=b)
        .max_by(|&i, &j| m[i].total_cmp(&m[j]))
        .unwrap_or(1);
    let (l, c, r) = (m[k - 1].ln(), m[k].ln(), m[k + 1].ln());
    let d = 0.5 * (l - r) / (l - 2.0 * c + r);
    ((k as f64 + d) * df, m[k])
}

/// f0 near `guess`, refined on harmonic `h`.
pub fn f0_near(x: &[f64], guess: f64, h: usize) -> f64 {
    let n = (x.len() * 4).next_power_of_two();
    let m = mag(x, n);
    let (f1, _) = peak_in(&m, n, guess * 0.8, guess * 1.2);
    let (fh, _) = peak_in(&m, n, f1 * h as f64 - f1 * 0.3, f1 * h as f64 + f1 * 0.3);
    fh / h as f64
}

/// The loudest partial, 20 Hz – 15 kHz (self-oscillation): (Hz, amplitude).
pub fn f_peak(x: &[f64]) -> (f64, f64) {
    let n = (x.len() * 4).next_power_of_two();
    let m = mag(x, n);
    peak_in(&m, n, 20.0, 15_000.0)
}

pub fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

pub use crate::util::measure::db;

/// Harmonic amplitudes 1..=n of `x` at `f0` (those under 23 kHz).
pub fn harmonics(x: &[f64], f0: f64, n: usize) -> Vec<f64> {
    let w = bh(x.len());
    (1..=n)
        .filter(|&k| k as f64 * f0 < 23_000.0)
        .map(|k| amp_at(x, &w, k as f64 * f0))
        .collect()
}

/// H2…H5 of `y` at `f` re H1 (dBc), and H1 itself: `kexp`'s report.
pub fn hreport(y: &[f64], f: f64) -> (f64, [f64; 4]) {
    let w = bh(y.len());
    let h: Vec<f64> = (1..=5).map(|k| amp_at(y, &w, k as f64 * f)).collect();
    (h[0], std::array::from_fn(|i| db(h[i + 1]) - db(h[0])))
}

/// A periodic wave as harmonic amplitudes and phases (index 0 = H1).
#[derive(Clone)]
pub struct Wave {
    pub amp: Vec<f64>,
    pub ph: Vec<f64>,
}

impl Wave {
    /// A plain band-limited saw (sine phases), peak about 1.
    pub fn saw() -> Self {
        Wave {
            amp: (1..=800).map(|k| 2.0 / PI / k as f64).collect(),
            ph: vec![0.0; 800],
        }
    }

    /// The Kobol's own VCO, read off a held note: H_k re H1's phase.
    pub fn measured(x: &[f64], f0: f64) -> Self {
        let w = bh(x.len());
        let n = (22_000.0 / f0) as usize;
        let c: Vec<Complex64> = (1..=n).map(|k| c_at(x, &w, k as f64 * f0)).collect();
        let p1 = c[0].arg();
        Wave {
            amp: c.iter().map(|v| v.norm()).collect(),
            ph: c
                .iter()
                .enumerate()
                .map(|(i, v)| v.arg() - (i + 1) as f64 * p1)
                .collect(),
        }
    }

    /// One period at `f0` (harmonics under 20 kHz) on 2^16 points at peak
    /// 1, and its harmonic amplitudes at that peak.
    pub fn table(&self, f0: f64) -> (Vec<f64>, Vec<f64>) {
        let n = 1 << 16;
        let kmax = ((20_000.0 / f0) as usize).min(self.amp.len());
        // Each harmonic by its own rotation: exact to ~1e-12 on 2^16.
        let mut t = vec![0.0; n];
        for k in 0..kmax {
            let (a, p) = (self.amp[k], self.ph[k]);
            let d = 2.0 * PI * (k + 1) as f64 / n as f64;
            for (i, v) in t.iter_mut().enumerate() {
                *v += a * (d * i as f64 + p).cos();
            }
        }
        let pk = t.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        t.iter_mut().for_each(|v| *v /= pk);
        (t, self.amp[..kmax].iter().map(|a| a / pk).collect())
    }
}

/// Read a table at `f(i)` Hz (cubic Hermite), `n` samples.
pub fn play(t: &[f64], f: &dyn Fn(usize) -> f64, n: usize) -> Vec<f64> {
    let m = t.len();
    let mut ph = 0.0f64;
    (0..n)
        .map(|i| {
            let p = ph * m as f64;
            let j = p.floor() as usize;
            let u = p - j as f64;
            let at = |d: isize| t[(j as isize + d).rem_euclid(m as isize) as usize];
            let (y0, y1, y2, y3) = (at(-1), at(0), at(1), at(2));
            let c1 = 0.5 * (y2 - y0);
            let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
            ph = (ph + f(i) / FS).fract();
            ((c3 * u + c2) * u + c1) * u + y1
        })
        .collect()
}

/// White noise at RMS `a` (xorshift, fixed seed).
pub fn noise(a: f64, n: usize) -> Vec<f64> {
    let mut s = 0x2545_f491_4f6c_dd1du64;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            a * 3f64.sqrt() * ((s >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0)
        })
        .collect()
}

// ------------------------------------------------------------ responses

/// A held note's frames: (centre s, RMS, harmonic amplitudes k·f0 < fmax).
pub fn frames(x: &[f64], f0: f64, len: usize, hop: usize, fmax: f64) -> Vec<(f64, f64, Vec<f64>)> {
    let w = bh(len);
    let kmax = (fmax / f0) as usize;
    (0..)
        .map(|i| i * hop)
        .take_while(|s| s + len <= x.len())
        .map(|s| {
            let seg = &x[s..s + len];
            let h = (1..=kmax).map(|k| amp_at(seg, &w, k as f64 * f0)).collect();
            ((s + len / 2) as f64 / FS, rms(seg), h)
        })
        .collect()
}

/// The response frames: 0.25 s, harmonics under 8 kHz.
pub const FLEN: usize = 12_000;
pub const FMAX: f64 = 8_000.0;

/// FREQ, 15 Hz up in 24ths of an octave, under 18.5 kHz.
pub fn fc_grid() -> Vec<f64> {
    (0..=250)
        .map(|i| 15.0 * 2f64.powf(i as f64 / 24.0))
        .filter(|f| *f < 18_500.0)
        .collect()
}

/// RES 0 → 1 in 40ths.
pub fn res_grid() -> Vec<f64> {
    (0..=40).map(|i| i as f64 * 0.025).collect()
}

/// A model's steady response (dB, per harmonic under FMAX) at each (res,
/// fc), on `tab` (a table at `f0`, harmonic amplitudes `ain`) at peak
/// `amp`: `[res][fc][harmonic]`.
#[allow(clippy::too_many_arguments)]
pub fn response_table(
    model: Render,
    tab: &[f64],
    ain: &[f64],
    f0: f64,
    amp: f64,
    rs: &[f64],
    fcs: &[f64],
) -> Vec<Vec<Vec<f64>>> {
    let settle = (0.3 * FS) as usize;
    let x: Vec<f64> = play(tab, &|_| f0, settle + FLEN)
        .iter()
        .map(|v| v * amp)
        .collect();
    let w = bh(FLEN);
    let kmax = ((FMAX / f0) as usize).min(ain.len());
    rs.iter()
        .map(|&r| {
            fcs.iter()
                .map(|&fc| {
                    let y = model(&x, &|_| fc, &|_| r);
                    let seg = &y[settle..];
                    (1..=kmax)
                        .map(|k| db(amp_at(seg, &w, k as f64 * f0) / (ain[k - 1] * amp)))
                        .collect()
                })
                .collect()
        })
        .collect()
}

/// Shape error between measured `r` (dB) and model `h` (dB) over the
/// harmonics within 45 dB of `r`'s top, the gain free: (mean square, gain).
pub fn shape_err(r: &[f64], h: &[f64]) -> (f64, f64) {
    let top = r
        .iter()
        .filter(|v| v.is_finite())
        .fold(f64::MIN, |m, v| m.max(*v));
    let use_k: Vec<usize> = (0..r.len().min(h.len()))
        .filter(|&i| r[i].is_finite() && r[i] > top - 45.0)
        .collect();
    let n = use_k.len().max(1) as f64;
    let g = use_k.iter().map(|&i| r[i] - h[i]).sum::<f64>() / n;
    let e = use_k
        .iter()
        .map(|&i| (r[i] - h[i] - g).powi(2))
        .sum::<f64>()
        / n;
    (e, g)
}

/// The best fc (index into the row) for one frame: (index, error, gain).
pub fn best_fc(r: &[f64], row: &[Vec<f64>]) -> (usize, f64, f64) {
    let mut best = (0, f64::MAX, 0.0);
    for (j, h) in row.iter().enumerate() {
        let (e, g) = shape_err(r, h);
        if e < best.1 {
            best = (j, e, g);
        }
    }
    best
}

/// Per frame of a response track: the peak above H1 (dB), the peak's
/// frequency, its −3 dB width over its frequency, and H1 itself (dB).
#[derive(Clone, Copy, Debug, Default)]
pub struct Shape {
    pub peak: f64,
    pub fpk: f64,
    pub width: f64,
    pub h1: f64,
}

/// `h` re `aref` in dB, NaN where the take is at its floor (−100 dBFS) or
/// the reference wave has a notch (10 dB under a saw's 1/k).
pub fn masked(h: &[f64], aref: &[f64]) -> Vec<f64> {
    h.iter()
        .zip(aref)
        .enumerate()
        .map(|(i, (a, b))| {
            if *a < 1e-5 || db(b * (i + 1) as f64 / aref[0]) < -10.0 {
                f64::NAN
            } else {
                db(a / b)
            }
        })
        .collect()
}

/// A response's `Shape`, after a 3-harmonic median (one noisy partial is
/// not a peak).
pub fn shape_of(r: &[f64], f0: f64) -> Shape {
    let r: Vec<f64> = (0..r.len())
        .map(|i| {
            let mut w: Vec<f64> = r[i.saturating_sub(1)..(i + 2).min(r.len())]
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .collect();
            if w.is_empty() || !r[i].is_finite() {
                return f64::NAN;
            }
            w.sort_by(f64::total_cmp);
            w[w.len() / 2]
        })
        .collect();
    let Some((i, &pk)) = r
        .iter()
        .enumerate()
        .filter(|v| v.1.is_finite())
        .max_by(|a, b| a.1.total_cmp(b.1))
    else {
        return Shape::default();
    };
    let ok = |j: usize| !r[j].is_finite() || r[j] > pk - 3.0;
    let mut lo = i;
    while lo > 0 && ok(lo - 1) {
        lo -= 1;
    }
    let mut hi = i;
    while hi + 1 < r.len() && ok(hi + 1) {
        hi += 1;
    }
    Shape {
        peak: pk - r[0],
        fpk: (i + 1) as f64 * f0,
        width: (hi - lo + 1) as f64 / (i + 1) as f64,
        h1: r[0],
    }
}

/// `v` over `t`, interpolated in log at `at` (held past the ends).
pub fn interp_log(t: &[f64], v: &[f64], at: f64) -> f64 {
    if at <= t[0] {
        return v[0];
    }
    for i in 1..t.len() {
        if at <= t[i] {
            let u = (at - t[i - 1]) / (t[i] - t[i - 1]);
            return (v[i - 1].ln() * (1.0 - u) + v[i].ln() * u).exp();
        }
    }
    v[v.len() - 1]
}

/// A gate following a take's level (on above −65 dBFS RMS in 10 ms), 5 ms
/// ramps.
pub fn gate_of(x: &[f64]) -> Vec<f64> {
    let w = 480;
    let on: Vec<bool> = x
        .chunks(w)
        .map(|c| rms(c) > 10f64.powf(-65.0 / 20.0))
        .collect();
    let mut g = 0.0f64;
    let step = 1.0 / 240.0;
    (0..x.len())
        .map(|i| {
            let t = if on[i / w] { 1.0 } else { 0.0 };
            g += (t - g).clamp(-step, step);
            g
        })
        .collect()
}

/// A held take's f0, read over its 50–85 % stretch.
pub fn take_f0(x: &[f64]) -> f64 {
    let n = x.len();
    f0_near(&x[n / 2..n * 85 / 100], 56.4, 4)
}

/// The ideal linear 4-pole (four equal poles at `fc`, loop gain `k`, a
/// share `c` of the input added to the feedback): |H| in dB at `f`.
pub fn lin4(f: f64, fc: f64, k: f64, c: f64) -> f64 {
    let s = Complex64::new(0.0, f / fc);
    let one = Complex64::new(1.0, 0.0);
    let g = one / (one + s).powi(4);
    db(((1.0 + c * k) * g / (1.0 + k * g)).norm())
}

/// `kobol comp`'s fit: the linear 4-pole at loop gain `k` and input share
/// `c` against masked response frames `rk` (dB re the open plateau, at
/// f0 `f0`; the model re itself at `open` Hz), each frame's FREQ free on
/// `fcs`: (mean square error, the FREQs).
pub fn fit_lin4(
    rk: &[Vec<f64>],
    f0: f64,
    open: f64,
    fcs: &[f64],
    k: f64,
    c: f64,
) -> (f64, Vec<f64>) {
    let mut tot = 0.0;
    let mut fcv = Vec::new();
    for r in rk {
        let (mut be, mut bf) = (f64::MAX, 0.0);
        let top = r
            .iter()
            .filter(|v| v.is_finite())
            .fold(f64::MIN, |m, v| m.max(*v));
        for &fc in fcs {
            let (mut e, mut n) = (0.0, 0.0);
            for (i, &v) in r.iter().enumerate() {
                if v.is_finite() && v > top - 45.0 {
                    let f = (i + 1) as f64 * f0;
                    let m = lin4(f, fc, k, c) - lin4(f, open, 0.0, 0.0);
                    e += (v - m).powi(2);
                    n += 1.0;
                }
            }
            if e / n < be {
                (be, bf) = (e / n, fc);
            }
        }
        tot += be;
        fcv.push(bf);
    }
    (tot / rk.len() as f64, fcv)
}

/// Steps of each harmonic re the quietest take, H1's step taken out: the
/// shape change with level. (H1 step, peak-band change, top-band change,
/// worst |change|, every harmonic's change), over harmonics within 50 dB
/// of the strongest.
pub fn steps(q: &[f64], x: &[f64]) -> (f64, f64, f64, f64, Vec<f64>) {
    let top = q.iter().fold(0.0f64, |m, v| m.max(*v));
    let d1 = db(x[0] / q[0]);
    let d: Vec<f64> = q
        .iter()
        .zip(x)
        .map(|(a, b)| {
            if *a > top * 10f64.powf(-50.0 / 20.0) {
                db(b / a) - d1
            } else {
                f64::NAN
            }
        })
        .collect();
    let pk = (0..q.len())
        .max_by(|&i, &j| q[i].total_cmp(&q[j]))
        .unwrap_or(0);
    let band = |lo: usize, hi: usize| {
        let v: Vec<f64> = d[lo.min(d.len())..hi.min(d.len())]
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        v.iter().sum::<f64>() / v.len().max(1) as f64
    };
    let worst = d
        .iter()
        .filter(|v| v.is_finite())
        .fold(0.0f64, |m, v| m.max(v.abs()));
    (
        d1,
        band(pk.saturating_sub(1), pk + 2),
        band(pk * 3, pk * 4),
        worst,
        d,
    )
}

/// A self-oscillation note: (start s, end s, Hz, peak dBFS, H2 dBc, H3 dBc).
pub type Note = (f64, f64, f64, f64, f64, f64);

/// The self-oscillating notes of `x`: runs of 0.2 s frames whose loudest
/// partial holds within 1 %, measured on their inner frames.
pub fn osc_notes(x: &[f64]) -> Vec<Note> {
    let len = 9600;
    let fr: Vec<(f64, f64, f64)> = x
        .chunks(len)
        .enumerate()
        .map(|(i, c)| {
            if c.len() < len || rms(c) < 1e-3 {
                return (i as f64 * 0.2, 0.0, 0.0);
            }
            let (f, a) = f_peak(c);
            (i as f64 * 0.2, f, a)
        })
        .collect();
    let mut notes = Vec::new();
    let mut i = 0;
    while i < fr.len() {
        if fr[i].1 == 0.0 {
            i += 1;
            continue;
        }
        let mut j = i;
        while j + 1 < fr.len() && fr[j + 1].1 > 0.0 && (fr[j + 1].1 / fr[i].1 - 1.0).abs() < 0.01 {
            j += 1;
        }
        // Inner frames only: the edges hold the step.
        if j >= i + 2 {
            let s0 = ((fr[i].0 + 0.2) * FS) as usize;
            let s1 = (fr[j].0 * FS) as usize;
            let segx = &x[s0..s1];
            let f = f0_near(segx, fr[i + 1].1, 1);
            let h = harmonics(segx, f, 3);
            let pk = segx.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            notes.push((
                fr[i].0,
                fr[j].0 + 0.2,
                f,
                db(pk),
                db(h[1] / h[0]),
                db(h[2] / h[0]),
            ));
        }
        i = j + 1;
    }
    notes
}

/// Notes' summary: (count, peak-level spread dB, mean H2, mean H3 dBc).
pub fn osc_summary(n: &[Note]) -> (usize, f64, f64, f64) {
    let pk: Vec<f64> = n.iter().map(|m| m.3).collect();
    let spread =
        pk.iter().fold(f64::MIN, |m, v| m.max(*v)) - pk.iter().fold(f64::MAX, |m, v| m.min(*v));
    let l = n.len().max(1) as f64;
    (
        n.len(),
        spread,
        n.iter().map(|m| m.4).sum::<f64>() / l,
        n.iter().map(|m| m.5).sum::<f64>() / l,
    )
}

/// The ring at `RES max`, the VCO playing: the loudest partial 0.8–1.3
/// kHz more than 8 Hz off any harmonic of `f0`, re H1 (dB): `kobol
/// squelch`'s measure, on `y` (FREQ 1010 Hz, off every harmonic).
pub fn ring_over(y: &[f64], f0: f64) -> f64 {
    let w = bh(y.len());
    let h1 = amp_at(y, &w, f0);
    let n = (y.len() * 4).next_power_of_two();
    let m = mag(y, n);
    let df = FS / n as f64;
    let (lo, hi) = ((800.0 / df) as usize, (1300.0 / df) as usize);
    let best = m[lo..hi]
        .iter()
        .enumerate()
        .filter(|(j, _)| {
            let f = (lo + j) as f64 * df;
            (f / f0 - (f / f0).round()).abs() * f0 > 8.0
        })
        .fold(0.0f64, |a, (_, v)| a.max(*v));
    db(best / h1)
}

/// Level (dB) and spectral centroid (log2 Hz, 60 Hz–16 kHz) every 10 ms,
/// 2048-sample frames.
pub fn ltrack(x: &[f64]) -> Vec<(f64, f64)> {
    let n = 2048;
    let hop = 480;
    let w = bh(n);
    (0..)
        .map(|i| i * hop)
        .take_while(|s| s + n <= x.len())
        .map(|s| {
            let b: Vec<f64> = x[s..s + n].iter().zip(&w).map(|(a, b)| a * b).collect();
            let m = fft(&b);
            let (mut a, mut e) = (0.0, 0.0);
            for (i, v) in m[..n / 2].iter().enumerate() {
                let f = i as f64 * FS / n as f64;
                if (60.0..16_000.0).contains(&f) {
                    let pw = v.norm_sqr();
                    a += pw * f.log2();
                    e += pw;
                }
            }
            (
                10.0 * e.max(1e-30).log10(),
                if e > 0.0 { a / e } else { 0.0 },
            )
        })
        .collect()
}

/// The centroid error of track `o` against `k` (octaves rms), over `k`'s
/// frames within 40 dB of its top.
pub fn centroid_err(k: &[(f64, f64)], o: &[(f64, f64)]) -> f64 {
    let top = k.iter().fold(f64::MIN, |m, v| m.max(v.0));
    let idx: Vec<usize> = (0..k.len().min(o.len()))
        .filter(|&i| k[i].0 > top - 40.0)
        .collect();
    (idx.iter().map(|&i| (k[i].1 - o[i].1).powi(2)).sum::<f64>() / idx.len().max(1) as f64).sqrt()
}

/// The phrase fit's score: level (gain free, in 6 dB units) and centroid
/// (octaves), mean square, over `k`'s frames within 40 dB of its top.
pub fn phrase_err(k: &[(f64, f64)], o: &[(f64, f64)]) -> f64 {
    let top = k.iter().fold(f64::MIN, |m, v| m.max(v.0));
    let idx: Vec<usize> = (0..k.len().min(o.len()))
        .filter(|&i| k[i].0 > top - 40.0)
        .collect();
    let g = idx.iter().map(|&i| k[i].0 - o[i].0).sum::<f64>() / idx.len() as f64;
    idx.iter()
        .map(|&i| ((k[i].0 - o[i].0 - g) / 6.0).powi(2) + (k[i].1 - o[i].1).powi(2))
        .sum::<f64>()
        / idx.len() as f64
}

/// Harmonic shape distance (dB rms over harmonics within 50 dB of each
/// frame's strongest, below 8 kHz), 0.25 s frames, gain-free per frame:
/// how alike the spectra of `b` and `a` are, not their level.
pub fn shape_lsd(a: &[f64], b: &[f64], f0: f64) -> f64 {
    let len = 12_000;
    let kmax = (8_000.0 / f0) as usize;
    let mut tot = Vec::new();
    for s in (len..a.len().min(b.len()).saturating_sub(len)).step_by(len) {
        if rms(&a[s..s + len]) < 1e-4 {
            continue;
        }
        let (ha, hb) = (
            harmonics(&a[s..s + len], f0, kmax),
            harmonics(&b[s..s + len], f0, kmax),
        );
        let top = ha.iter().fold(0.0f64, |m, v| m.max(*v));
        let d: Vec<f64> = ha
            .iter()
            .zip(&hb)
            .filter(|(x, _)| **x > top * 10f64.powf(-2.5))
            .map(|(x, y)| db(*y) - db(*x))
            .collect();
        let g = d.iter().sum::<f64>() / d.len().max(1) as f64;
        tot.push((d.iter().map(|v| (v - g).powi(2)).sum::<f64>() / d.len().max(1) as f64).sqrt());
    }
    tot.iter().sum::<f64>() / tot.len().max(1) as f64
}

/// RMS over the louder half of 0.1 s frames: the A/B's level match.
pub fn loud_rms(x: &[f64]) -> f64 {
    let mut e: Vec<f64> = x.chunks(4800).map(rms).collect();
    e.sort_by(|a, b| b.total_cmp(a));
    let h = &e[..(e.len() / 2).max(1)];
    (h.iter().map(|v| v * v).sum::<f64>() / h.len() as f64).sqrt()
}

// ------------------------------------------------------------ phrases

/// A gate: (on, off) s.
pub type Gate = (f64, f64);

/// A sequence step: the note (MIDI) or a rest, and whether it slides.
pub type Step = (Option<u8>, bool);

/// `play.py`'s sequences (16ths): "bass" (110 BPM) and "acid" (136).
pub fn sequence(name: &str) -> (Vec<Step>, f64) {
    let (e1, a1, e2, g1, d2, b1) = (28u8, 33u8, 40u8, 31u8, 38u8, 35u8);
    let n = |v: u8| Some(v);
    if name == "bass" {
        let bar = [
            (n(e1), false),
            (None, false),
            (n(e1), false),
            (n(e2), false),
            (None, false),
            (n(e1), false),
            (n(g1), false),
            (n(a1), false),
            (n(e1), false),
            (None, false),
            (n(e1), false),
            (n(d2), false),
            (None, false),
            (n(b1), false),
            (n(a1), false),
            (n(g1), false),
        ];
        (bar.repeat(4), 110.0)
    } else {
        let (a2, c3, d3, e3, g3, a3) = (45u8, 48u8, 50u8, 52u8, 55u8, 57u8);
        let bar = [
            (n(a2), false),
            (n(a2), true),
            (n(a3), false),
            (n(a2), false),
            (n(c3), true),
            (n(d3), false),
            (n(a2), false),
            (None, false),
            (n(a2), false),
            (n(e3), true),
            (n(d3), true),
            (n(c3), false),
            (n(a2), false),
            (n(g3), true),
            (n(a3), false),
            (n(a2), false),
        ];
        (bar.repeat(4), 136.0)
    }
}

/// Gates (on, off) and pitch changes (t, note) for a sequence from `t0`;
/// a note's gate is 60 % of its 16th unless it slides.
pub fn events(name: &str, t0: f64) -> (Vec<Gate>, Vec<(f64, u8)>) {
    let (steps, bpm) = sequence(name);
    let six = 60.0 / bpm / 4.0;
    let (mut gates, mut pitch) = (Vec::new(), Vec::new());
    let mut t = t0;
    let mut open: Option<f64> = None;
    for (note, slide) in steps {
        match note {
            None => {
                if let Some(on) = open.take() {
                    gates.push((on, t));
                }
            }
            Some(v) => {
                pitch.push((t, v));
                let on = *open.get_or_insert(t);
                if !slide {
                    open = None;
                    gates.push((on, t + 0.6 * six));
                }
            }
        }
        t += six;
    }
    if let Some(on) = open {
        gates.push((on, t));
    }
    (gates, pitch)
}

/// A phrase's synth patch around the filter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    /// FREQ at A1 with the envelope at 0 (Hz).
    pub base: f64,
    /// Octaves of FREQ per octave of keys.
    pub track: f64,
    /// The envelope's octaves at its peak.
    pub amt: f64,
    /// The filter envelope's decay τ (s) and sustain.
    pub df: f64,
    pub sf: f64,
    /// The amp envelope's decay τ and sustain.
    pub da: f64,
    pub sa: f64,
    pub res: f64,
    /// The VCO's tuning re A1 = 55 Hz.
    pub cents: f64,
}

/// An ADS that retriggers on each gate: attack 2 ms, decay τ to sustain,
/// release at the decay's τ.
pub fn ads(gates: &[(f64, f64)], n: usize, tau: f64, sus: f64) -> Vec<f64> {
    let mut e = 0.0f64;
    let mut gi = 0;
    let ka = 1.0 / (0.002 * FS);
    let kd = 1.0 - (-1.0 / (tau * FS)).exp();
    let mut attack = false;
    (0..n)
        .map(|i| {
            let t = i as f64 / FS;
            while gi < gates.len() && gates[gi].1 < t {
                gi += 1;
            }
            let on = gi < gates.len() && t >= gates[gi].0;
            if on && (i == 0 || ((i as f64 - 1.0) / FS) < gates[gi].0) {
                attack = true;
            }
            if !on {
                attack = false;
                e += (0.0 - e) * kd;
            } else if attack {
                e += ka;
                if e >= 1.0 {
                    e = 1.0;
                    attack = false;
                }
            } else {
                e += (sus - e) * kd;
            }
            e
        })
        .collect()
}

/// A phrase's stimulus: the VCO (a plain saw at peak ½, each note's own
/// table, phase kept), FREQ per sample, and the amp envelope.
pub struct Phrase {
    pub x: Vec<f64>,
    pub fc: Vec<f64>,
    pub vca: Vec<f64>,
}

/// Sequence `name` from `t0` for `n` samples through `p`.
pub fn phrase(name: &str, t0: f64, n: usize, p: &Patch) -> Phrase {
    let (gates, pitch) = events(name, t0);
    let ef = ads(&gates, n, p.df, p.sf);
    let vca = ads(&gates, n, p.da, p.sa);
    let note_at = |i: usize| {
        let t = i as f64 / FS;
        pitch
            .iter()
            .rev()
            .find(|(pt, _)| *pt <= t)
            .map_or(pitch[0].1, |x| x.1)
    };
    let hz = |u: u8| 55.0 * 2f64.powf((f64::from(u) - 33.0 + p.cents / 100.0) / 12.0);
    let mut uniq: Vec<u8> = pitch.iter().map(|q| q.1).collect();
    uniq.sort();
    uniq.dedup();
    let tabs: Vec<(u8, Vec<f64>)> = uniq
        .iter()
        .map(|&u| (u, Wave::saw().table(hz(u)).0))
        .collect();
    let mut x = vec![0.0; n];
    let mut ph = 0.0f64;
    for (i, s) in x.iter_mut().enumerate() {
        let nt = note_at(i);
        let tab = &tabs
            .iter()
            .find(|(k, _)| *k == nt)
            .map_or(&tabs[0].1, |t| &t.1);
        let m = tab.len();
        let pp = ph * m as f64;
        let j = pp as usize;
        let u = pp - j as f64;
        *s = 0.5 * (tab[j % m] * (1.0 - u) + tab[(j + 1) % m] * u);
        ph = (ph + hz(nt) / FS).fract();
    }
    let fc = (0..n)
        .map(|i| {
            (p.base * 2f64.powf(p.track * (f64::from(note_at(i)) - 33.0) / 12.0 + p.amt * ef[i]))
                .clamp(20.0, 18_000.0)
        })
        .collect();
    Phrase { x, fc, vca }
}

/// Where a phrase take starts: its first 10 ms frame within 30 dB of its
/// top, plus half a frame.
pub fn phrase_start(take: &[f64]) -> f64 {
    let kt = ltrack(take);
    let top = kt.iter().fold(f64::MIN, |m, v| m.max(v.0));
    kt.iter().position(|v| v.0 > top - 30.0).unwrap_or(0) as f64 * 0.01 + 1024.0 / FS
}

/// `kobol phrases`' fit: coordinate descent on the patch from `seed`, the
/// score `phrase_err` of `model` (rendering `Phrase`s at RES `p.res`) on
/// the take's track `kt`. Four rounds, the steps × 0.6 a round.
pub fn fit_phrase(
    kt: &[(f64, f64)],
    seed: Patch,
    score: &dyn Fn(&Patch) -> Vec<(f64, f64)>,
) -> (Patch, f64) {
    let mut p = seed;
    let mut e = phrase_err(kt, &score(&p));
    let mut step = [0.5f64, 0.5, 1.0, 0.5, 0.2, 0.5, 0.2, 0.1];
    for _ in 0..4 {
        for (d, st) in step.iter().enumerate() {
            for sg in [-1.0, 1.0] {
                let mut q = p;
                let s = sg * st;
                match d {
                    0 => q.base *= 2f64.powf(s),
                    1 => q.track = (q.track + s).clamp(0.0, 2.5),
                    2 => q.amt = (q.amt + s).clamp(0.0, 8.0),
                    3 => q.df = (q.df * 2f64.powf(s)).clamp(0.01, 3.0),
                    4 => q.sf = (q.sf + s).clamp(0.0, 1.0),
                    5 => q.da = (q.da * 2f64.powf(s)).clamp(0.01, 3.0),
                    6 => q.sa = (q.sa + s).clamp(0.0, 1.0),
                    _ => q.res = (q.res + s).clamp(0.0, 1.0),
                }
                let eq = phrase_err(kt, &score(&q));
                if eq < e {
                    (p, e) = (q, eq);
                }
            }
        }
        step.iter_mut().for_each(|s| *s *= 0.6);
    }
    (p, e)
}

/// The fit's 54 seeds: RES 0.5/0.8/0.97 × base 80/300/1200 Hz × env
/// 1/3/5 oct × track 1/2.
pub fn phrase_seeds() -> Vec<Patch> {
    let mut v = Vec::new();
    for res in [0.5, 0.8, 0.97] {
        for base in [80.0, 300.0, 1200.0] {
            for amt in [1.0, 3.0, 5.0] {
                for track in [1.0, 2.0] {
                    v.push(Patch {
                        base,
                        track,
                        amt,
                        df: 0.15,
                        sf: 0.5,
                        da: 0.2,
                        sa: 0.6,
                        res,
                        cents: -120.0,
                    });
                }
            }
        }
    }
    v
}

/// The patch the 2026-10-20 comparison fitted for the firmware's EXP on
/// phrase take `take` (`compare-phrases.md`), the A/B's patch for every
/// filter.
pub fn phrase_patch(take: &str) -> Option<Patch> {
    let l = PHRASES_MD
        .lines()
        .find(|l| l.starts_with(&format!("- {take} exp:")))?;
    let num = |key: &str| -> Option<f64> {
        let i = l.find(key)? + key.len();
        l[i..]
            .trim_start()
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .next()?
            .parse()
            .ok()
    };
    Some(Patch {
        base: num("FREQ@A1")?,
        track: num("track")?,
        amt: num("env")?,
        df: num("filter decay")?,
        sf: num("sustain")?,
        da: num("amp decay")?,
        sa: l
            .rsplit("sustain")
            .next()
            .and_then(|s| s.trim().split(',').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0),
        res: num("RES")?,
        cents: -120.0,
    })
}

// ------------------------------------------------------------ the A/B

/// One take's A/B job: the input (full scale 1), FREQ per sample, the
/// loop gain for the circuit and EXP-C, RES for the firmware's EXP and
/// REV2, and the gain after the filter (the VCA, 1 when none).
pub struct Job {
    pub take: &'static str,
    pub x: Vec<f64>,
    pub fc: Vec<f64>,
    pub k: Vec<f64>,
    pub res_exp: f64,
    pub res_rev2: f64,
    pub vca: Vec<f64>,
}

/// The FREQ tracks the 2026-10-20 comparison inferred per 0.25 s frame of
/// a sweep take (`compare-sweeps.md`, REV2's column), interpolated in log,
/// `n` samples.
pub fn sweep_track(take: &str, n: usize) -> Option<Vec<f64>> {
    let mut on = false;
    let (mut ts, mut fs) = (Vec::new(), Vec::new());
    for l in SWEEPS_MD.lines() {
        if let Some(h) = l.strip_prefix("## ") {
            on = h.trim() == take;
            continue;
        }
        if on && l.starts_with("| ") {
            let c: Vec<&str> = l.split('|').map(str::trim).collect();
            if let (Ok(t), Ok(f)) = (c[1].parse::<f64>(), c[2].parse::<f64>()) {
                ts.push(t);
                fs.push(f.clamp(20.0, 18_000.0));
            }
        }
    }
    (!ts.is_empty()).then(|| {
        (0..n)
            .map(|i| interp_log(&ts, &fs, i as f64 / FS))
            .collect()
    })
}

/// A plain saw at f0 through `gate`, at `level_db` re full scale.
fn gated_saw(f0: f64, gate: &[f64], level_db: f64) -> Vec<f64> {
    let a = 10f64.powf(level_db / 20.0);
    let tab = Wave::saw().table(f0).0;
    play(&tab, &|_| f0, gate.len())
        .iter()
        .zip(gate)
        .map(|(s, g)| s * g * a)
        .collect()
}

/// The sweeps (sweep1, sweep_res5, sweep_res10): the take's f0 and gate,
/// a saw at ½ VCO1 (−10.02 dB, the drive series' middle: the sweeps'
/// level is unknown), the inferred FREQ track; k 0 / 2.13 (the schematic's
/// T3 law at mid knob) / 5.59 (its top), the firmware's voices at the
/// same k on their law (k = 4.21·RES).
pub fn sweep_job(take: &'static str, kobol: &[f64]) -> Option<Job> {
    let (k, r) = match take {
        "sweep1" => (0.0, 0.0),
        "sweep_res5" => (2.13, 2.13 / 4.21),
        "sweep_res10" => (5.59, 1.0),
        _ => return None,
    };
    let f0 = f0_near(seg(kobol, 12.0, 18.0), 56.4, 8);
    let n = kobol.len();
    Some(Job {
        take,
        x: gated_saw(f0, &gate_of(kobol), -10.02),
        fc: sweep_track(take, n)?,
        k: vec![k; n],
        res_exp: r,
        res_rev2: r,
        vca: vec![1.0; n],
    })
}

/// The drive series at RES 7¼ (drive_q, drive_h, drive_f): k 2.32 (the
/// shape fit of the ¼ take, compare-drive.md), FREQ 1017 Hz, VCO1 ¼ / ½ /
/// full at −17.27 / −10.02 / 0 dB (the takes' own H1 steps, the filter
/// being linear there); the firmware's EXP at RES 0.525, REV2 at 0.55.
pub fn drive_job(take: &'static str, kobol: &[f64]) -> Option<Job> {
    let d = DRIVE.iter().find(|t| t.0 == take)?.1;
    let f0 = f0_near(seg(kobol, 2.0, 8.0), 56.4, 4);
    let n = kobol.len();
    Some(Job {
        take,
        x: gated_saw(f0, &gate_of(kobol), d),
        fc: vec![1017.0; n],
        k: vec![2.32; n],
        res_exp: 0.525,
        res_rev2: 0.55,
        vca: vec![1.0; n],
    })
}

/// The drive takes and their levels re full (dB).
pub const DRIVE: [(&str, f64); 3] = [("drive_q", -17.27), ("drive_h", -10.02), ("drive_f", 0.0)];

/// A phrase take's job: `phrase` from the take's start through its EXP
/// patch; k = 4.21·RES for the circuit and EXP-C.
pub fn phrase_job(take: &'static str, kobol: &[f64]) -> Option<Job> {
    let seq = if take.contains("bass") {
        "bass"
    } else {
        "acid"
    };
    let p = phrase_patch(take)?;
    let n = kobol.len();
    let ph = phrase(seq, phrase_start(kobol), n, &p);
    Some(Job {
        take,
        x: ph.x,
        fc: ph.fc,
        k: vec![4.21 * p.res; n],
        res_exp: p.res,
        res_rev2: p.res,
        vca: ph.vca,
    })
}

/// The self-oscillation take (track2): a −100 dBFS noise seed, FREQ per
/// note at the Kobol's ring pitch (before each model's own tuning, see
/// `tune_to`), k 5.59, the take's gate after.
pub fn selfosc_job(kobol: &[f64]) -> (Job, Vec<Note>) {
    let n = kobol.len();
    let notes = osc_notes(kobol);
    let fc = (0..n).map(|i| notes[note_index(&notes, i)].2).collect();
    (
        Job {
            take: "track2",
            x: noise(1e-5, n),
            fc,
            k: vec![5.59; n],
            res_exp: 1.0,
            res_rev2: 1.0,
            vca: gate_of(kobol),
        },
        notes,
    )
}

/// Which of `notes` sample `i` belongs to (the last begun by 0.1 s on).
pub fn note_index(notes: &[Note], i: usize) -> usize {
    let t = i as f64 / FS;
    notes.iter().rposition(|m| m.0 <= t + 0.1).unwrap_or(0)
}

/// Per note, the FREQ ratio that makes a model whose ring sits at
/// `ratio(fc)·fc` ring at the note's pitch: two passes of `f/ratio(f)`.
pub fn tune_to(notes: &[Note], ratio: &dyn Fn(f64) -> f64) -> Vec<f64> {
    notes
        .iter()
        .map(|nt| {
            let f1 = nt.2 / ratio(nt.2);
            let f2 = f1 * nt.2 / (f1 * ratio(f1));
            f2 / nt.2
        })
        .collect()
}

/// A model's ring pitch over FREQ at `k_or_res`: `f_ring/fc` after 0.5 s
/// on a −100 dBFS noise seed.
pub fn ring_ratio(model: Render, fc: f64, k_or_res: f64) -> f64 {
    let n = (0.8 * FS) as usize;
    let y = model(&noise(1e-5, n), &|_| fc, &|_| k_or_res);
    f_peak(seg(&y, 0.5, 0.8)).0 / fc
}

/// The circuit's ring over FREQ (k 5.59, CI5 at 2 mV), from the lab's
/// ngspice probes at 340 Hz, 1 kHz and 5.3 kHz (`spice/v2/u20*`),
/// log-interpolated: what the A/B tuned the circuit with.
pub fn circuit_ratio(fc: f64) -> f64 {
    let (f, r) = (
        [340.0, 1000.0, 5300.0],
        [348.21 / 340.0, 1014.13 / 1000.0, 5328.41 / 5300.0],
    );
    interp_log(&f, &r, fc)
}
