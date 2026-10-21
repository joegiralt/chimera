//! Signal primitives the circuit labs share, each matching the numpy/scipy
//! call the Python harness made (named on each), in f64. Copied from
//! `chimera-utils/src/sig.rs`.
//!
//! `resample_poly`'s filter design, padding and alignment
//! follow SciPy's `scipy.signal`. Under SciPy's BSD-3-Clause
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
