//! The reverb TYPEs' shared test helpers (reverb TYPEs spec § Tests): the
//! render plumbing, the measures, and the checks every engine runs. All at
//! the bus rate, `SR`; a `Render` is one engine on its settings, fed a
//! block of send and returning its L and R return.

use std::f64::consts::TAU;

use chimera_core::block::ParamId;
use chimera_core::dsp::rev::parts::clamp_counts;
use chimera_hal::BLOCK_SIZE;

use super::sweep::power;
use super::{SR, rms};

pub type Render<'a> =
    &'a mut dyn FnMut(&[f32; BLOCK_SIZE]) -> ([f32; BLOCK_SIZE], [f32; BLOCK_SIZE]);

/// One millisecond, in samples.
pub const MS: usize = SR as usize / 1000;

pub fn blocks(seconds: f32) -> usize {
    (seconds * SR as f32 / BLOCK_SIZE as f32).ceil() as usize
}

/// `seconds` of `r` fed `input(sample index)`, whole blocks.
pub fn run(r: Render, input: &mut dyn FnMut(usize) -> f32, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut rr) = (Vec::new(), Vec::new());
    for b in 0..blocks(seconds) {
        let send = core::array::from_fn(|i| input(b * BLOCK_SIZE + i));
        let (bl, br) = r(&send);
        l.extend_from_slice(&bl);
        rr.extend_from_slice(&br);
    }
    (l, rr)
}

/// Xorshift32 white noise, uniform in ±1.
pub struct Noise(pub u32);

impl Noise {
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        2.0 * (self.0 as f32 / u32::MAX as f32) - 1.0
    }
}

/// `seconds` of pink noise (Kellet's economy filter) at `rms_dbfs`.
pub fn pink(seed: u32, seconds: f32, rms_dbfs: f32) -> Vec<f32> {
    let mut n = Noise(seed);
    let (mut b0, mut b1, mut b2) = (0.0f32, 0.0f32, 0.0f32);
    let mut x: Vec<f32> = (0..(seconds * SR as f32) as usize)
        .map(|_| {
            let w = n.next();
            b0 = 0.99765 * b0 + w * 0.0990460;
            b1 = 0.96300 * b1 + w * 0.2965164;
            b2 = 0.57000 * b2 + w * 1.0526913;
            b0 + b1 + b2 + w * 0.1848
        })
        .collect();
    let g = 10f32.powf(rms_dbfs / 20.0) / rms(&x);
    x.iter_mut().for_each(|s| *s *= g);
    x
}

/// sin(2π·`hz`·i / SR) × `amp`.
pub fn tone(hz: f32, amp: f32) -> impl Fn(usize) -> f32 {
    move |i| amp * (TAU * hz as f64 * i as f64 / SR as f64).sin() as f32
}

// ── measures ──

/// RT60 by T20: the Schroeder EDC of L² + R², a least-squares line from
/// −5 to −25 dB, extended to −60.
pub fn rt60_t20(l: &[f32], r: &[f32]) -> f32 {
    let mut edc: Vec<f64> = l
        .iter()
        .zip(r)
        .map(|(&a, &b)| a as f64 * a as f64 + b as f64 * b as f64)
        .collect();
    for i in (0..edc.len() - 1).rev() {
        edc[i] += edc[i + 1];
    }
    let (mut n, mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, &e) in edc.iter().enumerate() {
        let db = 10.0 * (e / edc[0]).log10();
        if (-25.0..=-5.0).contains(&db) {
            let x = i as f64 / SR as f64;
            (n, sx, sy, sxx, sxy) = (n + 1.0, sx + x, sy + db, sxx + x * x, sxy + x * db);
        }
    }
    (-60.0 * (n * sxx - sx * sx) / (n * sxy - sx * sy)) as f32
}

/// RMS of each `w`-sample window.
fn windows(x: &[f32], w: usize) -> Vec<f32> {
    x.chunks_exact(w).map(rms).collect()
}

/// The first 1 ms whose RMS is above −40 dB of the loudest 1 ms, in ms
/// from the start.
pub fn onset_ms(x: &[f32]) -> f32 {
    let w = windows(x, MS);
    let peak = w.iter().copied().fold(0.0, f32::max);
    w.iter().position(|&v| v > 0.01 * peak).expect("silent") as f32
}

/// The loudest 10 ms window's centre, in ms.
pub fn peak_ms(x: &[f32]) -> f32 {
    let w = windows(x, 10 * MS);
    let i = (0..w.len())
        .max_by(|&a, &b| w[a].total_cmp(&w[b]))
        .expect("empty");
    (i as f32 + 0.5) * 10.0
}

/// The end of the last 10 ms window within 20 dB of the loudest, in ms.
pub fn end_20db_ms(x: &[f32]) -> f32 {
    let w = windows(x, 10 * MS);
    let peak = w.iter().copied().fold(0.0, f32::max);
    (w.iter().rposition(|&v| v >= 0.1 * peak).expect("silent") as f32 + 1.0) * 10.0
}

/// Zero-lag normalised correlation.
pub fn corr(a: &[f32], b: &[f32]) -> f32 {
    let (mut ab, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = (x as f64, y as f64);
        (ab, aa, bb) = (ab + x * y, aa + x * x, bb + y * y);
    }
    (ab / (aa * bb).sqrt()) as f32
}

pub fn rms_db(x: &[f32]) -> f32 {
    20.0 * rms(x).log10()
}

/// The largest |x[n] − 2x[n−1] + x[n−2]|.
pub fn max_d2(x: &[f32]) -> f32 {
    x.windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .fold(0.0, f32::max)
}

/// Hann-windowed DFT magnitude of `x` at exactly `hz`.
fn mag_at(x: &[f32], hz: f64) -> f64 {
    let n = x.len() as f64;
    let w = TAU * hz / SR as f64;
    let (mut re, mut im) = (0.0, 0.0);
    for (i, &s) in x.iter().enumerate() {
        let h = 0.5 - 0.5 * (TAU * i as f64 / n).cos();
        re += s as f64 * h * (w * i as f64).cos();
        im += s as f64 * h * (w * i as f64).sin();
    }
    re.hypot(im)
}

/// Total harmonic distortion of a steady `f0` (harmonics 2–9 below
/// Nyquist), as a ratio of the fundamental.
pub fn thd(x: &[f32], f0: f32) -> f32 {
    let f0 = f0 as f64;
    let h: f64 = (2..=9)
        .map(|k| k as f64 * f0)
        .filter(|&f| f < SR as f64 / 2.0 - 100.0)
        .map(|f| mag_at(x, f).powi(2))
        .sum();
    (h.sqrt() / mag_at(x, f0)) as f32
}

/// The largest power of two that fits in `len`.
fn pow2_in(len: usize) -> usize {
    1 << (usize::BITS - 1 - len.leading_zeros())
}

fn bin_of(hz: f64, n: usize) -> usize {
    (hz * n as f64 / SR as f64).round() as usize
}

/// Energy away from `f0` and its harmonics (aliases, folded products,
/// noise), from 20 Hz up, against the fundamental's, in dB. One Hann frame
/// over the end of `x`.
pub fn alias_db(x: &[f32], f0: f32) -> f32 {
    let n = pow2_in(x.len());
    let p = power(&x[x.len() - n..], n);
    let near = |k: usize, f: f64| k.abs_diff(bin_of(f, n)) <= 4;
    let fund: f64 = (0..p.len())
        .filter(|&k| near(k, f0 as f64))
        .map(|k| p[k])
        .sum();
    let other: f64 = (bin_of(20.0, n)..p.len())
        .filter(|&k| {
            !(1..)
                .take_while(|&h| (h as f64 * f0 as f64) < SR as f64 / 2.0)
                .any(|h| near(k, h as f64 * f0 as f64))
        })
        .map(|k| p[k])
        .sum();
    (10.0 * (other / fund).log10()) as f32
}

/// The strongest spectral peak from 20 Hz up, refined between bins: one
/// Hann frame over the end of `x`.
pub fn settled_peak_hz(x: &[f32]) -> f32 {
    let n = pow2_in(x.len());
    let p = power(&x[x.len() - n..], n);
    let k = (bin_of(20.0, n).max(1)..p.len() - 1)
        .max_by(|&a, &b| p[a].total_cmp(&p[b]))
        .expect("empty");
    let (a, b, c) = (p[k - 1].ln(), p[k].ln(), p[k + 1].ln());
    let d = 0.5 * (a - c) / (a - 2.0 * b + c);
    ((k as f64 + d) * SR as f64 / n as f64) as f32
}

/// The largest normalised autocorrelation of `x` over lags `lags_ms.0` to
/// `lags_ms.1`, in dB (20·log10 |ρ|).
pub fn autocorr_peak_db(x: &[f32], lags_ms: (f32, f32)) -> f32 {
    let x: Vec<f64> = x.iter().map(|&s| s as f64).collect();
    rho_peak_db(&x, lags_ms)
}

fn rho_peak_db(x: &[f64], lags_ms: (f32, f32)) -> f32 {
    let lag = |ms: f32| (ms * MS as f32).round() as usize;
    let rho = |k: usize| {
        let (a, b) = (&x[..x.len() - k], &x[k..]);
        let ab: f64 = a.iter().zip(b).map(|(p, q)| p * q).sum();
        let e = |v: &[f64]| v.iter().map(|s| s * s).sum::<f64>();
        (ab / (e(a) * e(b)).sqrt()).abs()
    };
    let most = (lag(lags_ms.0)..=lag(lags_ms.1))
        .map(rho)
        .fold(0.0, f64::max);
    (20.0 * most.log10()) as f32
}

/// `x` less its own order-16 linear prediction (Levinson–Durbin): the
/// short-term spectrum flattened, so a dark tail no longer correlates at
/// 1 ms by its colour, while anything longer than 16 samples (an echo) is
/// left in the residual.
fn whiten(x: &[f32]) -> Vec<f64> {
    const P: usize = 16;
    let x: Vec<f64> = x.iter().map(|&s| s as f64).collect();
    let r: Vec<f64> = (0..=P)
        .map(|k| {
            x[..x.len() - k]
                .iter()
                .zip(&x[k..])
                .map(|(a, b)| a * b)
                .sum()
        })
        .collect();
    let (mut a, mut e) = (vec![0.0; P + 1], r[0]);
    a[0] = 1.0;
    for i in 1..=P {
        let k = -(0..i).map(|j| a[j] * r[i - j]).sum::<f64>() / e;
        let prev = a.clone();
        for j in 1..=i {
            a[j] = prev[j] + k * prev[i - j];
        }
        e *= 1.0 - k * k;
    }
    (P..x.len())
        .map(|n| (0..=P).map(|j| a[j] * x[n - j]).sum())
        .collect()
}

/// Zero-crossing frequency over `x`: rising crossings, interpolated.
fn zc_hz(x: &[f32]) -> Option<f64> {
    let t: Vec<f64> = x
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
        .map(|(i, w)| i as f64 + (w[0] / (w[0] - w[1])) as f64)
        .collect();
    (t.len() >= 3).then(|| (t.len() - 1) as f64 * SR as f64 / (t[t.len() - 1] - t[0]))
}

fn cents(f: f64, f0: f64) -> f32 {
    (1200.0 * (f / f0).log2()) as f32
}

/// Each `loop_s` pass's pitch, from zero crossings, in cents from `f0`;
/// stops at the first pass too quiet to cross.
pub fn pitch_per_pass_cents(x: &[f32], f0: f32, loop_s: f32) -> Vec<f32> {
    let w = (loop_s * SR as f32) as usize;
    x.chunks_exact(w)
        .map_while(zc_hz)
        .map(|f| cents(f, f0 as f64))
        .collect()
}

// ── shared checks ──

/// A 1 kHz sine for 0.5 s, then silence, with `step(block)` called before
/// each block (true while a move runs). Once the move is done, plus one
/// round trip `loop_s`, the tail's peak over max(0.5 s, 4·`loop_s`) sits
/// within 1/`loop_s` Hz of 1 kHz. The in-crossfade deviation is printed.
pub fn check_settles_on_pitch(r: Render, step: &mut dyn FnMut(usize) -> bool, loop_s: f32) {
    let sine = tone(1_000.0, 0.25);
    let sine_end = blocks(0.5);
    let (trip, window) = (blocks(loop_s), blocks(0.5f32.max(4.0 * loop_s)));
    let (mut l, mut moved_until) = (Vec::new(), 0);
    let mut b = 0;
    let settled = loop {
        if step(b) {
            moved_until = b + 1;
        }
        let send = core::array::from_fn(|i| {
            let n = b * BLOCK_SIZE + i;
            if b < sine_end { sine(n) } else { 0.0 }
        });
        l.extend_from_slice(&r(&send).0);
        let settled = sine_end.max(moved_until) + trip;
        b += 1;
        if b >= settled + window {
            break settled;
        }
        assert!(b < blocks(120.0), "the move never ends");
    };
    let moving = pitch_per_pass_cents(
        &l[sine_end * BLOCK_SIZE..settled * BLOCK_SIZE],
        1_000.0,
        loop_s,
    );
    let worst = moving.iter().fold(0.0f32, |m, c| m.max(c.abs()));
    println!("in-crossfade deviation: up to {worst:.1} cents per pass");
    let f = settled_peak_hz(&l[settled * BLOCK_SIZE..]);
    assert!(
        (f - 1_000.0).abs() <= 1.0 / loop_s,
        "settled on {f} Hz, loop {loop_s} s"
    );
}

/// 10 ms windows' energy in dB, less its own least-squares trend, up to
/// the first window 60 dB under the loudest.
fn off_trend_db(x: &[f32]) -> Vec<f64> {
    let mut db: Vec<f64> = windows(x, 10 * MS)
        .iter()
        .map(|&v| 20.0 * (v as f64).log10())
        .collect();
    let top = db.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if let Some(end) = db.iter().position(|&d| d < top - 60.0) {
        db.truncate(end);
    }
    let n = db.len() as f64;
    let (sx, sy) = ((n - 1.0) * n / 2.0, db.iter().sum::<f64>());
    let sxx: f64 = (0..db.len()).map(|i| (i * i) as f64).sum();
    let sxy: f64 = db.iter().enumerate().map(|(i, &y)| i as f64 * y).sum();
    let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    let icept = (sy - slope * sx) / n;
    db.iter()
        .enumerate()
        .map(|(i, &y)| y - (icept + slope * i as f64))
        .collect()
}

/// A tail after a move against a static tail at the end setting: once
/// each is whitened (`whiten`), no lag of 1–10 ms above −20 dB nor more
/// than 3 dB over `fixed`'s; and no 10 ms window dipping more than 3 dB
/// further below its own decay trend than `fixed`'s deepest dip.
pub fn check_no_slap(moved: &[f32], fixed: &[f32]) {
    let (m, f) = (
        rho_peak_db(&whiten(moved), (1.0, 10.0)),
        rho_peak_db(&whiten(fixed), (1.0, 10.0)),
    );
    assert!(m <= -20.0, "slap: autocorrelation {m} dB");
    assert!(m <= f + 3.0, "slap: autocorrelation {m} dB, static {f} dB");
    let deepest = off_trend_db(fixed).into_iter().fold(0.0, f64::min);
    for (i, d) in off_trend_db(moved).into_iter().enumerate() {
        assert!(
            d >= deepest - 3.0,
            "dip at {} ms: {d:.1} dB, static {deepest:.1}",
            i * 10
        );
    }
}

/// The tail `check_smear_bound` judges: a 1 kHz sine for 0.5 s, then
/// from 1.5 s to `seconds`, cut where it falls 40 dB: below that the
/// lines' own LSBs scatter the zero crossings.
fn smear_tail(r: Render, seconds: f32) -> Vec<f32> {
    let sine = tone(1_000.0, 0.25);
    let (l, _) = run(
        r,
        &mut |i| if i < SR as usize / 2 { sine(i) } else { 0.0 },
        seconds,
    );
    let tail = &l[3 * SR as usize / 2..];
    let first = rms(&tail[..10 * MS]);
    let end = windows(tail, 10 * MS)
        .iter()
        .position(|&v| v < 1e-2 * first)
        .map_or(tail.len(), |w| w * 10 * MS);
    tail[..end].to_vec()
}

/// The pitch `check_smear_bound` judges against: the same engine's tail
/// with its wander frozen at the centre, from zero crossings.
pub fn tail_pitch_hz(r: Render) -> f32 {
    zc_hz(&smear_tail(r, 5.5)).expect("a tail") as f32
}

/// How far the strongest line in `x`'s spectrum stands over the median of
/// the bins around it (±8 above DC, its own ±1 left out), in dB, from
/// bin 2 up:
/// a period shows as a line, a random drift's smooth spectrum as none.
/// Bins up to `top` (of `x`'s length / 2) only. Welch frames of half `x`,
/// its mean taken out.
pub fn line_db(x: &[f32], top: f32) -> f32 {
    let mean = x.iter().sum::<f32>() / x.len() as f32;
    let x: Vec<f32> = x.iter().map(|&v| v - mean).collect();
    let n = pow2_in(x.len() / 2);
    let p = power(&x, n);
    let top = ((top * p.len() as f32) as usize).min(p.len() - 9);
    (2..top)
        .map(|k| {
            let mut near: Vec<f64> = (k.saturating_sub(8).max(1)..=k + 8)
                .filter(|&j| j.abs_diff(k) > 1)
                .map(|j| p[j])
                .collect();
            near.sort_by(f64::total_cmp);
            10.0 * (p[k] / near[near.len() / 2]).log10()
        })
        .fold(f64::NEG_INFINITY, f64::max) as f32
}

/// The largest share of `x`'s variance one sinusoid explains, at any
/// frequency from 0.2 Hz to `max_hz` (`x` sampled at `fs`), Hann-windowed:
/// 1 for a pure vibrato, small for a drift spread over many frequencies.
pub fn vibrato_share(x: &[f32], fs: f32, max_hz: f32) -> f32 {
    let n = x.len() as f64;
    let mean = x.iter().map(|&v| v as f64).sum::<f64>() / n;
    let xw: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, &v)| (v as f64 - mean) * (0.5 - 0.5 * (TAU * i as f64 / n).cos()))
        .collect();
    let total: f64 = xw.iter().map(|v| v * v).sum();
    let mut best = 0.0f64;
    let mut f = 0.2;
    while f <= max_hz as f64 {
        let w = TAU * f / fs as f64;
        let (re, im) = xw.iter().enumerate().fold((0.0, 0.0), |(re, im), (i, v)| {
            (re + v * (w * i as f64).cos(), im + v * (w * i as f64).sin())
        });
        best = best.max(3.0 * (re * re + im * im) / (n * total));
        f += fs as f64 / (8.0 * n);
    }
    best as f32
}

/// SMEAR at its maximum, set by the caller at a wander of `rate_hz`: a
/// 1 kHz sine for 0.5 s circulates, and its tail from 1.5 s is judged
/// (§ Pitch bounds).
/// - Every `loop_s` pass is within ±25 cents of `f_ref`: `tail_pitch_hz`
///   at the same SMEAR with the wander frozen at its centre, so the reads
///   sit `D` short, still (SMEAR only shortens a read, so its centre is a
///   slightly shorter loop, on its own modes).
/// - Each 4/`rate_hz` window's mean is within ±1 cent of it: no drift.
/// - No one sinusoid below half the loop's rate explains half the pitch
///   trace's variance (20 ms windows): no vibrato. (A tail repeats each
///   pass, so the trace has lines at 1/`loop_s` and up.)
pub fn check_smear_bound(r: Render, loop_s: f32, f_ref: f32, rate_hz: f32) {
    let window = (4.0 / rate_hz * SR as f32) as usize;
    let tail = smear_tail(r, 1.5 + 2.0 * window as f32 / SR as f32 + 0.5);
    assert!(
        tail.len() >= window,
        "the tail dies in {} ms",
        tail.len() / MS
    );
    let passes = pitch_per_pass_cents(&tail, f_ref, loop_s);
    assert!(!passes.is_empty());
    for (i, c) in passes.iter().enumerate() {
        assert!(c.abs() <= 25.0, "pass {i}: {c} cents");
    }
    for (i, w) in tail.chunks_exact(window).enumerate() {
        let c = cents(zc_hz(w).expect("a window"), f_ref as f64);
        assert!(c.abs() <= 1.0, "window {i}: mean {c} cents");
    }
    let trace: Vec<f32> = tail
        .as_chunks::<{ 20 * MS }>()
        .0
        .iter()
        .map_while(|w| zc_hz(w))
        .map(|f| cents(f, f_ref as f64))
        .collect();
    let share = vibrato_share(&trace, 50.0, 0.5 / loop_s);
    println!("pitch trace: one sinusoid explains {:.0} %", 100.0 * share);
    assert!(
        share <= 0.5,
        "one sinusoid explains {share} of the pitch: vibrato"
    );
}

/// `r`'s block with the clamp hits and writes its own stores made.
fn counted(
    r: Render,
    send: &[f32; BLOCK_SIZE],
) -> ([f32; BLOCK_SIZE], [f32; BLOCK_SIZE], u64, u64) {
    let (h0, w0) = clamp_counts();
    let (l, rr) = r(send);
    let (h1, w1) = clamp_counts();
    (l, rr, h1 - h0, w1 - w0)
}

/// `seconds` of `input` through `r` alone: (L, hits, writes).
fn run_counted(r: Render, input: &dyn Fn(usize) -> f32, seconds: f32) -> (Vec<f32>, u64, u64) {
    let (mut l, mut hits, mut writes) = (Vec::new(), 0, 0);
    for b in 0..blocks(seconds) {
        let send = core::array::from_fn(|i| input(b * BLOCK_SIZE + i));
        let (bl, _, h, w) = counted(r, &send);
        l.extend_from_slice(&bl);
        (hits, writes) = (hits + h, writes + w);
    }
    (l, hits, writes)
}

/// `pad` through `r` and `other` side by side: (`r`'s clamp hits, `other`'s
/// energy, the difference's energy); or, unless `against_f32`, asserts the
/// two bit-identical.
fn lockstep(pad: &[f32], r: Render, other: Render, against_f32: bool) -> (u64, f64, f64) {
    let (mut hits, mut sq, mut diff) = (0, 0.0f64, 0.0f64);
    for b in 0..pad.len() / BLOCK_SIZE {
        let send = core::array::from_fn(|i| pad[b * BLOCK_SIZE + i]);
        let (l, rr, h, _) = counted(r, &send);
        let (ol, or) = other(&send);
        hits += h;
        for (x, y) in l.iter().chain(&rr).zip(ol.iter().chain(&or)) {
            if against_f32 {
                sq += (*y as f64).powi(2);
                diff += (*x as f64 - *y as f64).powi(2);
            } else {
                assert_eq!(
                    x.to_bits(),
                    y.to_bits(),
                    "pad at the default DECAY: not bit-identical in block {b}"
                );
            }
        }
    }
    (hits, sq, diff)
}

/// § Level plan, held (the hot-send stimuli), in order:
/// 1. −12 dBFS pink for 10 s at the default DECAY through `r`,
///    `r_unclipped` and `r_f32`: `r` is bit-identical to `r_unclipped`,
///    with no clamp hit.
/// 2. `set_max()`: maximum DECAY on `r`'s engine and `r_f32`'s.
/// 3. The same pad again: `r` within −40 dB of `r_f32`, no clamp hit.
/// 4. A 0 dBFS sine on the loop mode nearest 500 Hz (`loop_mode_hz` is the
///    mode spacing, 1/L) held 30 s: THD ≤ 3 %, no clamp hit.
/// 5. The same on the mode nearest 5 kHz: aliases ≤ −40 dB.
/// 6. A 0 dBFS sweep, 50 Hz–10 kHz over 10 s: clamp hits ≤ 0.1 % of writes.
pub fn check_level_plan(
    r: Render,
    set_max: &mut dyn FnMut(),
    r_unclipped: Render,
    r_f32: Render,
    loop_mode_hz: f32,
) {
    let pad = pink(0x91ab, 10.0, -12.0);
    // 1. Default DECAY: the clip never engages. `r_f32` keeps in step.
    let (hits, _, _) = lockstep(&pad, &mut *r, &mut *r_unclipped, false);
    for b in 0..pad.len() / BLOCK_SIZE {
        r_f32(&core::array::from_fn(|i| pad[b * BLOCK_SIZE + i]));
    }
    assert_eq!(hits, 0, "pad at the default DECAY: clamp hits");
    // 2–3. Maximum DECAY: the clip shaves peaks only.
    set_max();
    let (hits, sq, diff) = lockstep(&pad, &mut *r, &mut *r_f32, true);
    let db = 10.0 * (diff / sq).log10();
    assert_eq!(hits, 0, "pad at maximum DECAY: clamp hits");
    assert!(db <= -40.0, "pad at maximum DECAY: {db:.1} dB from f32");
    // 4–5. Held loop modes.
    let mode = |near: f32| (near / loop_mode_hz).round().max(1.0) * loop_mode_hz;
    let (f, last) = (mode(500.0), SR as usize);
    let (l, hits, _) = run_counted(&mut *r, &tone(f, 1.0), 30.0);
    let d = thd(&l[l.len() - last..], f);
    assert_eq!(hits, 0, "held {f} Hz: clamp hits");
    assert!(d <= 0.03, "held {f} Hz: THD {:.2} %", 100.0 * d);
    let f = mode(5_000.0);
    let (l, _, _) = run_counted(&mut *r, &tone(f, 1.0), 30.0);
    let a = alias_db(&l[l.len() - last..], f);
    assert!(a <= -40.0, "held {f} Hz: aliases {a:.1} dB");
    // 6. The sweep's transients.
    let k = (10_000.0f64 / 50.0).ln() / 10.0;
    let sweep = |i: usize| {
        let t = i as f64 / SR as f64;
        (TAU * 50.0 / k * ((k * t).exp() - 1.0)).sin() as f32
    };
    let (_, hits, writes) = run_counted(&mut *r, &sweep, 10.0);
    assert!(writes > 0, "no store went through parts::store_*");
    assert!(
        hits as f64 <= 1e-3 * writes as f64,
        "sweep: {hits} clamp hits in {writes} writes"
    );
}

/// Energy from 20 Hz to 10.5 kHz outside 1 kHz ± 50 Hz, 2 s into a steady
/// −12 dB 1 kHz sine, in dB: the storage noise floor, as `reverb_test`'s
/// `floor_db`.
fn floor_db(r: Render) -> f64 {
    let (l, _) = run(r, &mut tone(1_000.0, 0.251), 3.4);
    let n = 1 << 16;
    let p = power(&l[2 * SR as usize..][..n], n);
    let (lo, hi) = (bin_of(950.0, n), bin_of(1_050.0, n));
    let floor: f64 = p[bin_of(20.0, n)..lo]
        .iter()
        .chain(&p[hi..bin_of(10_500.0, n)])
        .sum();
    10.0 * floor.log10()
}

/// The return's RMS in dB, both sides, over 3 s from a 0.5 s noise burst.
fn burst_db(r: Render) -> f32 {
    let mut n = Noise(0x5eed);
    let (l, rr) = run(
        r,
        &mut |i| {
            if i < SR as usize / 2 {
                0.5 * n.next()
            } else {
                0.0
            }
        },
        3.0,
    );
    rms_db(&[l, rr].concat())
}

/// `r`'s default return within ±1 dB of QUAD's (`quad`) on the same burst,
/// then its storage noise floor no more than `extra_db` above QUAD's. The
/// caller runs both at the same MIX and at GRIT 0.
pub fn check_noise_and_loudness(r: Render, quad: Render, extra_db: f32) {
    let (a, q) = (burst_db(&mut *r), burst_db(&mut *quad));
    assert!((a - q).abs() <= 1.0, "loudness {a:.2} dB, QUAD {q:.2} dB");
    let (a, q) = (floor_db(&mut *r), floor_db(&mut *quad));
    println!("noise floor {a:.1} dB, QUAD {q:.1} dB");
    assert!(
        a <= q + extra_db as f64,
        "noise floor {a:.1} dB, QUAD {q:.1} + {extra_db} dB"
    );
}

/// Each knob in `ids` stepped 0 → 1 mid-tail (0.2 s after 0.5 s of noise):
/// over the next 50 ms the largest second difference is at most 1.5× that
/// of the 50 ms before (`reverb_test`'s `no_click` bound, judged against
/// the tail itself as `fx_swap_test` does).
pub fn check_knobs_ease(r: Render, ids: &[ParamId], set: &mut dyn FnMut(ParamId, f32)) {
    let mut n = Noise(4242);
    for &id in ids {
        set(id, 0.0);
        let (bl, br) = run(
            &mut *r,
            &mut |i| if i < SR as usize / 2 { n.next() } else { 0.0 },
            0.7,
        );
        set(id, 1.0);
        let (al, ar) = run(&mut *r, &mut |_| 0.0, 0.05);
        let w = 50 * MS;
        let d2 = |before: &[f32], after: &[f32]| {
            let joined = [&before[before.len() - 2..], after].concat();
            (
                max_d2(&before[before.len() - w..]),
                max_d2(&joined[..w + 2]),
            )
        };
        let ((bl2, al2), (br2, ar2)) = (d2(&bl, &al), d2(&br, &ar));
        let (before, after) = (bl2.max(br2), al2.max(ar2));
        assert!(
            after <= 1.5 * before + 1e-4,
            "{id:?} 0 → 1: d2 {after} after, {before} before"
        );
    }
}
