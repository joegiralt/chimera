//! Signal primitives, each matching the numpy/scipy call the Python harness
//! made (named on each), in f64.
//!
//! `resample_poly`'s filter design and padding and `butter3_bandpass`'s
//! pole placement follow SciPy's `scipy.signal`. Under SciPy's BSD-3-Clause
//! license:
//!
//!   Copyright (c) 2001-2002 Enthought, Inc. 2003, SciPy Developers.
//!   All rights reserved.
//!
//!   Redistribution and use in source and binary forms, with or without
//!   modification, are permitted provided that the following conditions are
//!   met:
//!
//!   1. Redistributions of source code must retain the above copyright
//!      notice, this list of conditions and the following disclaimer.
//!   2. Redistributions in binary form must reproduce the above copyright
//!      notice, this list of conditions and the following disclaimer in the
//!      documentation and/or other materials provided with the distribution.
//!   3. Neither the name of the copyright holder nor the names of its
//!      contributors may be used to endorse or promote products derived from
//!      this software without specific prior written permission.
//!
//!   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
//!   IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
//!   TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
//!   PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
//!   HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//!   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED
//!   TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
//!   PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
//!   LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
//!   NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
//!   SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use rustfft::num_complex::Complex64;
use rustfft::{Fft, FftPlanner};
use std::cell::RefCell;
use std::f64::consts::PI;
use std::sync::Arc;

thread_local! {
    /// One planner a thread: it keeps every plan it has made.
    static PLANNER: RefCell<FftPlanner<f64>> = RefCell::new(FftPlanner::new());
}

/// A forward (or inverse) FFT of length `n`, planned once a thread.
pub fn plan(n: usize, inverse: bool) -> Arc<dyn Fft<f64>> {
    PLANNER.with(|p| {
        let mut p = p.borrow_mut();
        if inverse {
            p.plan_fft_inverse(n)
        } else {
            p.plan_fft_forward(n)
        }
    })
}

/// The forward FFT of real `x`.
pub fn fft(x: &[f64]) -> Vec<Complex64> {
    let mut b: Vec<Complex64> = x.iter().map(|&v| Complex64::new(v, 0.0)).collect();
    plan(b.len(), false).process(&mut b);
    b
}

/// `scipy.special.erfc(1 / sqrt(2))`: the share of a Gaussian beyond 1 σ.
pub const ERFC_INV_SQRT2: f64 = 0.317_310_507_862_914_15;

/// Zeroth-order modified Bessel function of the first kind (power series).
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, q) = (1.0, 1.0, x * x / 4.0);
    let mut k = 1.0;
    while term > sum * 1e-17 {
        term *= q / (k * k);
        sum += term;
        k += 1.0;
    }
    sum
}

/// `scipy.signal.windows.kaiser(n, beta, sym=True)`.
pub fn kaiser(n: usize, beta: f64) -> Vec<f64> {
    let m = (n - 1) as f64;
    (0..n)
        .map(|i| {
            let r = 2.0 * i as f64 / m - 1.0;
            bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(beta)
        })
        .collect()
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// `scipy.signal.firwin(n, cutoff, window=("kaiser", beta))`: a low-pass,
/// cutoff relative to Nyquist, unit gain at DC.
pub fn firwin_kaiser(n: usize, cutoff: f64, beta: f64) -> Vec<f64> {
    let alpha = 0.5 * (n - 1) as f64;
    let w = kaiser(n, beta);
    let mut h: Vec<f64> = (0..n)
        .map(|i| cutoff * sinc(cutoff * (i as f64 - alpha)) * w[i])
        .collect();
    let s: f64 = h.iter().sum();
    h.iter_mut().for_each(|v| *v /= s);
    h
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// `scipy.signal.resample_poly(x, up, down, window=("kaiser", 10.0))`:
/// polyphase, linear phase, zero padding, the same filter and alignment.
/// The `half_len`/`pre_pad`/`pre_remove` arithmetic mirrors SciPy's own.
pub fn resample_poly(x: &[f64], up: usize, down: usize) -> Vec<f64> {
    let g = gcd(up, down);
    let (up, down) = (up / g, down / g);
    if up == 1 && down == 1 {
        return x.to_vec();
    }
    let n_in = x.len();
    let n_out = (n_in * up).div_ceil(down);
    let max_rate = up.max(down);
    let half_len = 10 * max_rate;
    let h: Vec<f64> = firwin_kaiser(2 * half_len + 1, 1.0 / max_rate as f64, 10.0)
        .into_iter()
        .map(|v| v * up as f64)
        .collect();
    let pre_pad = down - half_len % down;
    let pre_remove = (half_len + pre_pad) / down;
    // Output k is upfirdn's sample (k + pre_remove)·down of the padded
    // filter: Σ x[i]·h[m − i·up], m = (k + pre_remove)·down − pre_pad.
    (0..n_out)
        .map(|k| {
            let m = ((k + pre_remove) * down) as isize - pre_pad as isize;
            if m < 0 {
                return 0.0;
            }
            let m = m as usize;
            let i_hi = (m / up).min(n_in.saturating_sub(1));
            let i_lo = m.saturating_sub(h.len() - 1).div_ceil(up);
            let mut acc = 0.0;
            let mut i = i_lo;
            while i <= i_hi {
                acc += x[i] * h[m - i * up];
                i += 1;
            }
            acc
        })
        .collect()
}

/// One second-order section `[b0, b1, b2, 1, a1, a2]`.
pub type Sos = [f64; 6];

/// `scipy.signal.butter(3, [lo, hi], "bandpass", fs=fs, output="sos")`: the
/// same poles, zeros and gain (sections paired differently; the cascade is
/// the same filter).
pub fn butter3_bandpass(lo: f64, hi: f64, fs: f64) -> Vec<Sos> {
    let n = 3;
    // Analog prototype (buttap), then lp2bp_zpk and bilinear_zpk at fs = 2.
    let proto: Vec<Complex64> = (0..n)
        .map(|i| {
            let m = (-(n as i32) + 1 + 2 * i as i32) as f64;
            -Complex64::from_polar(1.0, PI * m / (2 * n) as f64)
        })
        .collect();
    let warp = |f: f64| 4.0 * (PI * (2.0 * f / fs) / 2.0).tan();
    let (w1, w2) = (warp(lo), warp(hi));
    let (bw, wo) = (w2 - w1, (w1 * w2).sqrt());
    let mut poles = Vec::with_capacity(2 * n);
    for &p in &proto {
        let p = p * bw / 2.0;
        let r = (p * p - wo * wo).sqrt();
        poles.push(p + r);
    }
    for &p in &proto {
        let p = p * bw / 2.0;
        let r = (p * p - wo * wo).sqrt();
        poles.push(p - r);
    }
    let k_analog = bw.powi(n as i32);
    let fs2 = 4.0;
    let zpoles: Vec<Complex64> = poles.iter().map(|&p| (fs2 + p) / (fs2 - p)).collect();
    // Zeros: three at 0 (→ z = 1), three at infinity (→ z = −1).
    let den = poles
        .iter()
        .fold(Complex64::new(1.0, 0.0), |acc, &p| acc * (fs2 - p));
    let k = k_analog * (Complex64::new(fs2.powi(n as i32), 0.0) / den).re;
    // Conjugate pairs: each upper-half pole with its mirror.
    let mut ups: Vec<Complex64> = zpoles.iter().copied().filter(|p| p.im > 1e-14).collect();
    let mut reals: Vec<f64> = zpoles
        .iter()
        .filter(|p| p.im.abs() <= 1e-14)
        .map(|p| p.re)
        .collect();
    ups.sort_by(|a, b| a.norm().total_cmp(&b.norm()));
    reals.sort_by(f64::total_cmp);
    let mut sos: Vec<Sos> = ups
        .iter()
        .map(|p| [1.0, 0.0, -1.0, 1.0, -2.0 * p.re, p.norm_sqr()])
        .collect();
    for pair in reals.chunks(2) {
        let (a, b) = (pair[0], *pair.get(1).unwrap_or(&0.0));
        sos.push([1.0, 0.0, -1.0, 1.0, -(a + b), a * b]);
    }
    for s in &mut sos[..1] {
        s[0] *= k;
        s[1] *= k;
        s[2] *= k;
    }
    sos
}

/// `scipy.signal.sosfilt`: transposed direct form II, zero initial state.
pub fn sosfilt(sos: &[Sos], x: &[f64]) -> Vec<f64> {
    let mut y = x.to_vec();
    for s in sos {
        let (mut z0, mut z1) = (0.0, 0.0);
        for v in y.iter_mut() {
            let xi = *v;
            let yi = s[0] * xi + z0;
            z0 = s[1] * xi - s[4] * yi + z1;
            z1 = s[2] * xi - s[5] * yi;
            *v = yi;
        }
    }
    y
}

/// `np.convolve(x, np.ones(w) / w, "same")`.
pub fn box_same(x: &[f64], w: usize) -> Vec<f64> {
    let n = x.len();
    let off = (w - 1) / 2;
    let inv = 1.0 / w as f64;
    (0..n)
        .map(|i| {
            // full[i + off] = Σ x[j], j in (i + off − w, i + off].
            let hi = (i + off).min(n - 1);
            let lo = (i + off + 1).saturating_sub(w);
            x[lo..=hi].iter().sum::<f64>() * inv
        })
        .collect()
}

/// `np.cumsum(x[::-1])[::-1]`.
pub fn rev_cumsum(x: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; x.len()];
    let mut acc = 0.0;
    for i in (0..x.len()).rev() {
        acc += x[i];
        out[i] = acc;
    }
    out
}

/// `np.polyfit(x, y, 1)[0]`: the least-squares slope.
pub fn slope(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx) * (a - mx);
    }
    sxy / sxx
}

/// `np.hanning(m)` (symmetric).
pub fn hanning(m: usize) -> Vec<f64> {
    if m == 1 {
        return vec![1.0];
    }
    (0..m)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / (m - 1) as f64).cos())
        .collect()
}

/// `np.mean(x[-len(x) // 10:])`: the mean of the last ⌈n/10⌉ values.
pub fn tail_mean(x: &[f64]) -> f64 {
    let k = x.len().div_ceil(10);
    x[x.len() - k..].iter().sum::<f64>() / k as f64
}

/// Welch-averaged spectra, one-sided density: of `x` (`pxx`) and, given `y`,
/// their cross spectrum (`pxy` = conj(X)·Y).
pub struct Spectra {
    pub f: Vec<f64>,
    pub pxx: Vec<f64>,
    pub pxy: Vec<Complex64>,
}

/// `scipy.signal.welch`/`csd` with `nperseg=n`: periodic Hann, half overlap,
/// constant detrend, one-sided density.
pub fn spectra(x: &[f64], y: Option<&[f64]>, fs: f64, nperseg: usize) -> Spectra {
    let n = nperseg;
    let win: Vec<f64> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos())
        .collect();
    let scale = 1.0 / (fs * win.iter().map(|w| w * w).sum::<f64>());
    let step = n - n / 2;
    let len = y.map_or(x.len(), |y| x.len().min(y.len()));
    let segs = (len - n / 2) / step;
    let fft = plan(n, false);
    let bins = n / 2 + 1;
    let mut pxx = vec![0.0; bins];
    let mut pxy = vec![Complex64::new(0.0, 0.0); bins];
    let zero = Complex64::new(0.0, 0.0);
    let (mut a, mut b) = (vec![zero; n], vec![zero; n]);
    let windowed = |sig: &[f64], buf: &mut [Complex64]| {
        let mean = sig.iter().sum::<f64>() / n as f64;
        for (i, v) in buf.iter_mut().enumerate() {
            *v = Complex64::new((sig[i] - mean) * win[i], 0.0);
        }
        fft.process(buf);
    };
    for s in 0..segs {
        let at = s * step..s * step + n;
        windowed(&x[at.clone()], &mut a);
        if let Some(y) = y {
            windowed(&y[at], &mut b);
        }
        for k in 0..bins {
            pxx[k] += a[k].norm_sqr();
            if y.is_some() {
                pxy[k] += a[k].conj() * b[k];
            }
        }
    }
    for k in 0..bins {
        let one_sided = if k != 0 && !(n.is_multiple_of(2) && k == bins - 1) {
            2.0
        } else {
            1.0
        };
        let g = scale / segs as f64 * one_sided;
        pxx[k] *= g;
        pxy[k] *= g;
    }
    let f = (0..bins).map(|k| k as f64 * fs / n as f64).collect();
    Spectra { f, pxx, pxy }
}

/// `scipy.signal.welch(x, fs, nperseg=n)`: (frequencies, PSD).
pub fn welch(x: &[f64], fs: f64, nperseg: usize) -> (Vec<f64>, Vec<f64>) {
    let s = spectra(x, None, fs, nperseg);
    (s.f, s.pxx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_to_the_midiverb_rate_and_back_keeps_a_band_limited_signal() {
        // Tones well inside 11.7 kHz survive 48 k → 23.4 k → 48 k.
        let x: Vec<f64> = (0..48_000)
            .map(|i| {
                let t = i as f64 / 48_000.0;
                0.5 * (2.0 * PI * 440.0 * t).sin() + 0.3 * (2.0 * PI * 5_000.0 * t).sin()
            })
            .collect();
        let down = resample_poly(&x, 39, 80);
        assert_eq!(down.len(), (48_000usize * 39).div_ceil(80));
        let back = resample_poly(&down, 80, 39);
        assert!(back.len() >= x.len());
        let mid = 2_000..46_000;
        let err = mid
            .clone()
            .map(|i| (back[i] - x[i]).abs())
            .fold(0.0, f64::max);
        assert!(err < 1e-3, "round-trip error {err}");
    }

    /// SciPy's own outputs on one input (scipy 1.x, `resample_poly` with
    /// `window=("kaiser", 10.0)`, `butter(3, …, output="sos")` + `sosfilt`,
    /// `welch(nperseg=64)`), pinned.
    #[test]
    fn scipy_outputs_are_reproduced() {
        let x: Vec<f64> = (0..200)
            .map(|i| {
                let t = i as f64;
                (0.1 * t).sin() + 0.5 * (0.37 * t).cos() + if i == 50 { 1.0 } else { 0.0 }
            })
            .collect();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12 * b.abs().max(1.0);
        let d = resample_poly(&x, 39, 80);
        assert_eq!(d.len(), 98);
        for (i, v) in [
            (0, 0.3886585959874708),
            (7, 1.2737411793117404),
            (48, -0.2600839778265102),
            (97, 0.538777462120233),
        ] {
            assert!(close(d[i], v), "down[{i}] {} vs {v}", d[i]);
        }
        let u = resample_poly(&x, 80, 39);
        assert_eq!(u.len(), 411);
        for (i, v) in [
            (0, 0.49999966460134126),
            (13, 0.24324036711749297),
            (102, 0.3605867212046057),
            (409, 0.5071572087984056),
        ] {
            assert!(close(u[i], v), "up[{i}] {} vs {v}", u[i]);
        }
        let s = butter3_bandpass(1000.0 / 2f64.sqrt(), 1000.0 * 2f64.sqrt(), 48_000.0);
        let y = sosfilt(&s, &x);
        for (i, v) in [
            (3, 0.0026293688973878953),
            (60, 0.42595657286682004),
            (199, 0.2920525316461442),
        ] {
            assert!(close(y[i], v), "butter[{i}] {} vs {v}", y[i]);
        }
        let (_, p) = welch(&x, 48_000.0, 64);
        for (i, v) in [
            (0, 9.073400214895127e-06),
            (5, 1.1799987132237528e-05),
            (32, 9.009933501210447e-08),
        ] {
            assert!((p[i] / v - 1.0).abs() < 1e-9, "welch[{i}] {} vs {v}", p[i]);
        }
    }

    #[test]
    fn a_butterworth_band_passes_its_centre_and_stops_far_off() {
        let sos = butter3_bandpass(1000.0 / 2f64.sqrt(), 1000.0 * 2f64.sqrt(), 48_000.0);
        let gain = |f: f64| {
            let x: Vec<f64> = (0..48_000)
                .map(|i| (2.0 * PI * f * i as f64 / 48_000.0).sin())
                .collect();
            let y = sosfilt(&sos, &x);
            (y[24_000..].iter().map(|v| v * v).sum::<f64>() / 12_000.0).sqrt()
        };
        assert!((gain(1000.0) - 1.0).abs() < 0.02);
        assert!(gain(100.0) < 0.01 && gain(10_000.0) < 0.01);
    }
}
