//! `voice-stats`: what a sung or spoken patch does over its phrase, read off
//! any WAV, so ours and a reference are measured the same way.
//!
//! - **Loudness:** BS.1770-4 gated integrated LUFS, and the sample peak.
//! - **Pitch movement:** tonal peaks are tracked frame to frame. Each track's
//!   cents against its own median split at a 0.4 s triangular average
//!   (about 1 Hz) into vibrato (the fast part: RMS depth, and the rate where
//!   its spectrum peaks) and drift (the slow part). Detuned partials beating inside one bin read as
//!   movement too, as they are heard.
//! - **Breath share:** the energy off the steady tonal peaks, 80 Hz–12 kHz.
//! - **Formants:** each 200 ms window's spectral envelope (its local maxima
//!   joined, then smoothed), and the envelope's peaks with their −3 dB
//!   widths.
//! - **Vowel motion:** the envelope's centroid and the F2 region's peak over
//!   time (their 5–95 % range), and the envelope's shape change every
//!   250 ms, level removed.
//! - **Level movement:** 50 ms RMS against its 0.5 s average.
//! - **Brightness:** the long-term spectrum's 2–5 kHz over its 150 Hz–1 kHz.

use crate::SR;
use crate::sig::plan;
use rustfft::num_complex::Complex64;
use std::f64::consts::PI;

/// STFT size and hop: 85 ms frames resolve C2's harmonics, and a 5.3 ms hop
/// follows a 7 Hz vibrato.
const N: usize = 4096;
const HOP: usize = 256;
/// A tonal peak stands `TONAL_DB` over the `TONAL_PCT` percentile of the
/// ±`NEAR` bins (±280 Hz) round it: white noise passes it about once in
/// 10⁶ bins, and C2's harmonics, 5.6 bins apart, still clear it.
const TONAL_DB: f64 = 20.0;
const TONAL_PCT: usize = 15;
const NEAR: usize = 24;
/// A track follows a peak at most this far from its last frame, cents.
const LINK_CENTS: f64 = 30.0;

#[derive(Clone, Debug, Default)]
pub struct VoiceStats {
    pub lufs: f64,
    pub peak: f64,
    /// RMS of the fast pitch movement, cents.
    pub vibrato_cents: f64,
    /// Where the fast movement's spectrum peaks, Hz (1–15).
    pub vibrato_hz: f64,
    /// RMS of the slow pitch movement, cents.
    pub drift_cents: f64,
    /// The energy off the tonal peaks, 0..1.
    pub breath: f64,
    /// The envelope's prominent peaks, 150 Hz–5 kHz: the median count.
    pub formants: f64,
    /// The median first three envelope peaks and their −3 dB widths, Hz.
    pub f: [f64; 3],
    pub bw: [f64; 3],
    /// The envelope's power centroid (150 Hz–5 kHz) and the F2 region's
    /// (800 Hz–3 kHz) peak: their 5–95 % range, cents.
    pub centroid_range: f64,
    pub f2_range: f64,
    /// The envelope's mean shape change per 250 ms, dB RMS, level removed.
    pub flux_db: f64,
    /// The 50 ms level's RMS against its 0.5 s average, dB.
    pub am_db: f64,
    /// The long-term spectrum's 2–5 kHz power over its 150 Hz–1 kHz, dB.
    pub bright_db: f64,
}

/// Every measure of `chans` (each at `SR`).
pub fn measure(chans: &[Vec<f64>]) -> VoiceStats {
    let len = chans.iter().map(Vec::len).min().unwrap_or(0);
    let mono: Vec<f64> = (0..len)
        .map(|i| chans.iter().map(|c| c[i]).sum::<f64>() / chans.len() as f64)
        .collect();
    let peak = chans
        .iter()
        .flat_map(|c| c.iter())
        .fold(0.0f64, |m, v| m.max(v.abs()));
    let frames = stft(&mono);
    let (vibrato_cents, vibrato_hz, drift_cents) = pitch_movement(&frames);
    let env = envelopes(&frames);
    let (formants, f, bw) = formant_medians(&env);
    let (centroid_range, f2_range, flux_db) = vowel_motion(&env);
    VoiceStats {
        lufs: lufs_gated(chans),
        peak,
        vibrato_cents,
        vibrato_hz,
        drift_cents,
        breath: breath_share(&frames),
        formants,
        f,
        bw,
        centroid_range,
        f2_range,
        flux_db,
        am_db: level_movement(&mono),
        bright_db: brightness(&frames),
    }
}

// ── Loudness ───────────────────────────────────────────────────────────

/// BS.1770's K-weighting at 48 kHz: the shelf, then the high-pass.
fn k_weight(x: &[f64]) -> Vec<f64> {
    let biquad = |b: [f64; 3], a: [f64; 2], s: &[f64]| -> Vec<f64> {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        s.iter()
            .map(|&x0| {
                let y0 = b[0] * x0 + b[1] * x1 + b[2] * x2 - a[0] * y1 - a[1] * y2;
                (x2, x1, y2, y1) = (x1, x0, y1, y0);
                y0
            })
            .collect()
    };
    let shelf = biquad(
        [
            1.535_124_859_586_97,
            -2.691_696_189_406_38,
            1.198_392_810_852_85,
        ],
        [-1.690_659_293_182_41, 0.732_480_774_215_85],
        x,
    );
    biquad(
        [1.0, -2.0, 1.0],
        [-1.990_047_454_833_98, 0.990_072_250_366_21],
        &shelf,
    )
}

/// BS.1770-4 integrated loudness: 400 ms blocks at 75 % overlap, gated at
/// −70 LUFS and then 10 LU under what passed.
pub fn lufs_gated(chans: &[Vec<f64>]) -> f64 {
    let k: Vec<Vec<f64>> = chans.iter().map(|c| k_weight(c)).collect();
    let len = k.iter().map(Vec::len).min().unwrap_or(0);
    let (n, hop) = ((0.4 * SR) as usize, (0.1 * SR) as usize);
    let z: Vec<f64> = (0..len.saturating_sub(n) / hop + 1)
        .map(|i| {
            let w = i * hop..(i * hop + n).min(len);
            let m = w.len().max(1) as f64;
            k.iter()
                .map(|c| c[w.clone()].iter().map(|v| v * v).sum::<f64>() / m)
                .sum()
        })
        .collect();
    let loud = |m: f64| -0.691 + 10.0 * m.max(1e-20).log10();
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    let abs: Vec<f64> = z.into_iter().filter(|&m| loud(m) > -70.0).collect();
    let gate = loud(mean(&abs)) - 10.0;
    let rel: Vec<f64> = abs.into_iter().filter(|&m| loud(m) > gate).collect();
    loud(mean(&rel))
}

// ── The STFT and its tonal peaks ───────────────────────────────────────

/// One frame: each bin's power and phase, 0..=N/2.
struct Frame {
    pow: Vec<f64>,
    phase: Vec<f64>,
}

fn stft(x: &[f64]) -> Vec<Frame> {
    let win: Vec<f64> = (0..N)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / N as f64).cos())
        .collect();
    let fft = plan(N, false);
    (0..x.len().saturating_sub(N) / HOP + 1)
        .filter(|_| x.len() >= N)
        .map(|t| {
            let mut b: Vec<Complex64> = x[t * HOP..t * HOP + N]
                .iter()
                .zip(&win)
                .map(|(v, w)| Complex64::new(v * w, 0.0))
                .collect();
            fft.process(&mut b);
            Frame {
                pow: b[..=N / 2].iter().map(|c| c.norm_sqr()).collect(),
                phase: b[..=N / 2].iter().map(|c| c.arg()).collect(),
            }
        })
        .collect()
}

fn bin_hz(k: f64) -> f64 {
    k * SR / N as f64
}

fn hz_bin(f: f64) -> usize {
    (f * N as f64 / SR).round() as usize
}

/// The bins of `pow` that are tonal peaks between `lo` and `hi` Hz: local
/// maxima over ±2 bins, `TONAL_DB` over their neighbourhood.
fn tonal_peaks(pow: &[f64], lo: f64, hi: f64) -> Vec<usize> {
    let (a, b) = (hz_bin(lo).max(2), hz_bin(hi).min(pow.len() - 3));
    let gate = 10f64.powf(TONAL_DB / 10.0);
    (a..=b)
        .filter(|&k| {
            let p = pow[k];
            p > 0.0 && (k - 2..=k + 2).all(|j| j == k || pow[j] < p) && {
                let mut nb: Vec<f64> =
                    pow[k.saturating_sub(NEAR)..(k + NEAR + 1).min(pow.len())].to_vec();
                nb.sort_by(f64::total_cmp);
                p > gate * nb[nb.len() * TONAL_PCT / 100]
            }
        })
        .collect()
}

/// The peak's frequency from the phase advance since the last frame, Hz.
fn inst_hz(prev: &Frame, cur: &Frame, k: usize) -> f64 {
    let expect = 2.0 * PI * HOP as f64 * k as f64 / N as f64;
    let d = cur.phase[k] - prev.phase[k] - expect;
    let d = d - 2.0 * PI * (d / (2.0 * PI)).round();
    bin_hz(k as f64 + d * N as f64 / (2.0 * PI * HOP as f64))
}

/// The share of the energy off the steady tonal peaks (±3 bins each), 80
/// Hz–12 kHz, over the frames within 30 dB of the loudest. A peak is steady
/// when its frequency's second difference over three hops is under 3 Hz
/// plus 0.2 %, as a 7 Hz vibrato's is. A noise band 200 Hz wide reads 0.9
/// of it as breath; one 50 Hz wide, nearly a tone, 0.4.
fn breath_share(frames: &[Frame]) -> f64 {
    let (a, b) = (hz_bin(80.0), hz_bin(12_000.0));
    let sums: Vec<(f64, f64)> = frames
        .windows(4)
        .map(|w| {
            let fr = &w[1];
            let total: f64 = fr.pow[a..=b].iter().sum();
            let mut mask = vec![false; fr.pow.len()];
            let steady = |k: &&usize| {
                let k = **k;
                let f: Vec<f64> = w.windows(2).map(|p| inst_hz(&p[0], &p[1], k)).collect();
                (f[2] - 2.0 * f[1] + f[0]).abs() < 3.0 + 0.002 * f[1]
            };
            for k in tonal_peaks(&fr.pow, 80.0, 12_000.0).iter().filter(steady) {
                let k = *k;
                for m in &mut mask[k - 3..=(k + 3).min(fr.pow.len() - 1)] {
                    *m = true;
                }
            }
            let tonal: f64 = (a..=b).filter(|&k| mask[k]).map(|k| fr.pow[k]).sum();
            (total, tonal)
        })
        .collect();
    let loudest = sums.iter().fold(0.0f64, |m, s| m.max(s.0));
    let (t, v) = sums
        .iter()
        .filter(|s| s.0 > loudest * 1e-3)
        .fold((0.0, 0.0), |(t, v), s| (t + s.0, v + s.1));
    if t > 0.0 { 1.0 - v / t } else { 0.0 }
}

// ── Pitch movement ─────────────────────────────────────────────────────

struct Track {
    hz: Vec<f64>,
    pow: f64,
    open: bool,
}

fn cents(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

/// `x` averaged over `w` points round each, the window shrinking at the ends.
fn moving_mean(x: &[f64], w: usize) -> Vec<f64> {
    let h = w / 2;
    (0..x.len())
        .map(|i| {
            let s = &x[i.saturating_sub(h)..(i + h + 1).min(x.len())];
            s.iter().sum::<f64>() / s.len() as f64
        })
        .collect()
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// The tracks of the tonal peaks (50 Hz–5 kHz) lasting at least 0.4 s.
fn tracks(frames: &[Frame]) -> Vec<Track> {
    let mut done: Vec<Track> = Vec::new();
    let mut live: Vec<Track> = Vec::new();
    for t in 1..frames.len() {
        let (prev, cur) = (&frames[t - 1], &frames[t]);
        let mut peaks: Vec<(f64, f64)> = tonal_peaks(&cur.pow, 50.0, 5_000.0)
            .into_iter()
            .map(|k| (inst_hz(prev, cur, k), cur.pow[k]))
            .filter(|p| p.0 > 0.0)
            .collect();
        peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
        peaks.truncate(32);
        let mut taken = vec![false; live.len()];
        let mut born = Vec::new();
        for (hz, p) in peaks {
            let best = live
                .iter()
                .enumerate()
                .filter(|(i, _)| !taken[*i])
                .map(|(i, tr)| (i, cents(hz, *tr.hz.last().unwrap()).abs()))
                .filter(|&(_, c)| c < LINK_CENTS)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            match best {
                Some((i, _)) => {
                    taken[i] = true;
                    live[i].hz.push(hz);
                    live[i].pow += p;
                }
                None => born.push(Track {
                    hz: vec![hz],
                    pow: p,
                    open: true,
                }),
            }
        }
        for (tr, &k) in live.iter_mut().zip(&taken) {
            tr.open = k;
        }
        let (keep, ended): (Vec<Track>, Vec<Track>) = live.into_iter().partition(|t| t.open);
        done.extend(ended);
        live = keep;
        live.extend(born);
    }
    done.extend(live);
    let min = (0.4 * SR / HOP as f64) as usize;
    done.retain(|t| t.hz.len() >= min);
    done
}

/// The vibrato's RMS depth (cents) and rate (Hz), and the drift's RMS
/// depth (cents), weighted by each track's energy.
fn pitch_movement(frames: &[Frame]) -> (f64, f64, f64) {
    let rate = SR / HOP as f64;
    let trim = (0.05 * rate) as usize;
    let grid: Vec<f64> = (4..=60).map(|i| f64::from(i) * 0.25).collect();
    let mut spec = vec![0.0; grid.len()];
    let (mut fast, mut slow, mut wsum) = (0.0, 0.0, 0.0);
    for tr in tracks(frames) {
        let hz = &tr.hz[trim..tr.hz.len() - trim];
        let mid = median(&mut hz.to_vec());
        let c: Vec<f64> = hz.iter().map(|&f| cents(f, mid)).collect();
        let w = (0.4 * rate) as usize;
        let s = moving_mean(&moving_mean(&c, w), w);
        let f: Vec<f64> = c.iter().zip(&s).map(|(a, b)| a - b).collect();
        let ms = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64;
        let w = tr.pow;
        fast += w * ms(&f);
        slow += w * ms(&s);
        wsum += w;
        for (g, p) in grid.iter().zip(&mut spec) {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, v) in f.iter().enumerate() {
                let ph = 2.0 * PI * g * i as f64 / rate;
                re += v * ph.cos();
                im -= v * ph.sin();
            }
            *p += w * (re * re + im * im) / f.len() as f64;
        }
    }
    if wsum == 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let at = spec
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0.0, |(i, _)| grid[i]);
    ((fast / wsum).sqrt(), at, (slow / wsum).sqrt())
}

// ── Spectral envelope and formants ─────────────────────────────────────

/// One 200 ms window's envelope, dB per bin (0..=N/2), and its level.
struct Env {
    db: Vec<f64>,
    level: f64,
    /// The window's middle, seconds.
    t: f64,
}

/// The envelope every 50 ms over 200 ms windows: the window's mean power
/// spectrum, its peaks joined in dB, smoothed by a Gaussian as wide as the
/// harmonics are apart.
fn envelopes(frames: &[Frame]) -> Vec<Env> {
    let per = (0.2 * SR / HOP as f64) as usize;
    let step = (0.05 * SR / HOP as f64) as usize;
    let bins = N / 2 + 1;
    let mut out = Vec::new();
    let mut t = 0;
    while t + per <= frames.len() {
        let mut p = vec![0.0; bins];
        for fr in &frames[t..t + per] {
            for (a, b) in p.iter_mut().zip(&fr.pow) {
                *a += b / per as f64;
            }
        }
        let db: Vec<f64> = p.iter().map(|v| 10.0 * v.max(1e-30).log10()).collect();
        // The tonal peaks, and where there are none near (a noise band),
        // every local maximum.
        let tonal = tonal_peaks(&p, 20.0, 20_000.0);
        let mut near = vec![false; bins];
        for &k in &tonal {
            for n in &mut near[k.saturating_sub(48)..(k + 49).min(bins)] {
                *n = true;
            }
        }
        let mut maxima: Vec<usize> = (1..bins - 1)
            .filter(|&k| !near[k] && db[k] >= db[k - 1] && db[k] > db[k + 1])
            .chain(tonal)
            .collect();
        maxima.sort_unstable();
        // Wide enough to bridge the harmonics: 60 % of the maxima's median
        // spacing below 3 kHz, at least 60 Hz.
        let mut gaps: Vec<f64> = maxima
            .windows(2)
            .filter(|w| w[1] < hz_bin(3_000.0))
            .map(|w| (w[1] - w[0]) as f64)
            .collect();
        let sigma = (0.6 * median(&mut gaps)).max(60.0 * N as f64 / SR);
        let kern: Vec<f64> = (-(3.0 * sigma) as i64..=(3.0 * sigma) as i64)
            .map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp())
            .collect();
        let half = kern.len() / 2;
        let mut joined = db.clone();
        for w in maxima.windows(2) {
            let (a, b) = (w[0], w[1]);
            for (k, j) in joined.iter_mut().enumerate().take(b).skip(a) {
                *j = db[a] + (db[b] - db[a]) * (k - a) as f64 / (b - a) as f64;
            }
        }
        let smooth: Vec<f64> = (0..bins)
            .map(|k| {
                let (mut s, mut w) = (0.0, 0.0);
                for (i, g) in kern.iter().enumerate() {
                    let j = k as i64 + i as i64 - half as i64;
                    if (0..bins as i64).contains(&j) {
                        s += g * joined[j as usize];
                        w += g;
                    }
                }
                s / w
            })
            .collect();
        out.push(Env {
            db: smooth,
            level: p.iter().sum(),
            t: ((t + per / 2) * HOP + N / 2) as f64 / SR,
        });
        t += step;
    }
    let loudest = out.iter().fold(0.0f64, |m, e| m.max(e.level));
    out.retain(|e| e.level > loudest * 1e-3);
    out
}

/// What the envelope measures look at: within this of its top, dB.
const FLOOR_DB: f64 = 35.0;

/// The envelope's peaks between 150 Hz and 5 kHz, within `FLOOR_DB` of the
/// top, standing at least 3 dB
/// over the higher of their two valleys: each one's Hz and −3 dB width.
fn envelope_peaks(db: &[f64]) -> Vec<(f64, f64)> {
    let (a, b) = (hz_bin(150.0), hz_bin(5_000.0));
    let floor = db[a..=b].iter().fold(f64::MIN, |m, &v| m.max(v)) - FLOOR_DB;
    (a..=b)
        .filter(|&k| db[k] >= db[k - 1] && db[k] > db[k + 1] && db[k] > floor)
        .filter_map(|k| {
            let valley = |dir: i64| {
                let (mut j, mut low) = (k as i64, db[k]);
                loop {
                    j += dir;
                    if j < 1 || j as usize >= db.len() - 1 || db[j as usize] > db[k] {
                        return low;
                    }
                    low = low.min(db[j as usize]);
                }
            };
            let prom = db[k] - valley(-1).max(valley(1));
            (prom >= 3.0).then(|| {
                let edge = |dir: i64| {
                    let mut j = k as i64;
                    while j > 0 && (j as usize) < db.len() - 1 && db[j as usize] > db[k] - 3.0 {
                        j += dir;
                    }
                    j
                };
                (bin_hz(k as f64), bin_hz((edge(1) - edge(-1)) as f64))
            })
        })
        .collect()
}

/// Each sounding 200 ms window's middle (s) and its envelope's peaks (Hz,
/// −3 dB width), as `measure` reads them.
pub fn envelope_tracks(x: &[f64]) -> Vec<(f64, Vec<(f64, f64)>)> {
    envelopes(&stft(x))
        .iter()
        .map(|e| (e.t, envelope_peaks(&e.db)))
        .collect()
}

fn formant_medians(env: &[Env]) -> (f64, [f64; 3], [f64; 3]) {
    let peaks: Vec<Vec<(f64, f64)>> = env.iter().map(|e| envelope_peaks(&e.db)).collect();
    let mut count: Vec<f64> = peaks.iter().map(|p| p.len() as f64).collect();
    let mut f = [0.0; 3];
    let mut bw = [0.0; 3];
    for i in 0..3 {
        f[i] = median(
            &mut peaks
                .iter()
                .filter_map(|p| p.get(i).map(|x| x.0))
                .collect::<Vec<_>>(),
        );
        bw[i] = median(
            &mut peaks
                .iter()
                .filter_map(|p| p.get(i).map(|x| x.1))
                .collect::<Vec<_>>(),
        );
    }
    (median(&mut count), f, bw)
}

/// The 5–95 % spread of `v`.
fn spread(v: &mut [f64]) -> f64 {
    if v.len() < 2 {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    at(0.95) - at(0.05)
}

fn vowel_motion(env: &[Env]) -> (f64, f64, f64) {
    let region = |e: &Env, lo: f64, hi: f64| {
        let (a, b) = (hz_bin(lo), hz_bin(hi));
        let k = (a..=b)
            .max_by(|&i, &j| e.db[i].total_cmp(&e.db[j]))
            .unwrap_or(a);
        1200.0 * bin_hz(k as f64).log2()
    };
    let mut centroid: Vec<f64> = env
        .iter()
        .map(|e| {
            let (a, b) = (hz_bin(150.0), hz_bin(5_000.0));
            let (mut s, mut w) = (0.0, 0.0);
            for (k, db) in e.db.iter().enumerate().take(b + 1).skip(a) {
                let p = 10f64.powf(db / 10.0);
                s += p * bin_hz(k as f64).log2();
                w += p;
            }
            1200.0 * s / w
        })
        .collect();
    let mut f2: Vec<f64> = env.iter().map(|e| region(e, 800.0, 3_000.0)).collect();
    // 48 points a third of an octave apart, 200 Hz to 5 kHz.
    let grid: Vec<usize> = (0..48)
        .map(|i| hz_bin(200.0 * 2f64.powf(f64::from(i) * 4.64 / 47.0)))
        .collect();
    let lag = 5;
    let flux: Vec<f64> = (lag..env.len())
        .map(|t| {
            let (x, y) = (&env[t].db, &env[t - lag].db);
            let top = grid.iter().fold(f64::MIN, |m, &k| m.max(x[k]).max(y[k]));
            let d: Vec<f64> = grid
                .iter()
                .filter(|&&k| x[k].max(y[k]) > top - FLOOR_DB)
                .map(|&k| x[k] - y[k])
                .collect();
            let m = d.iter().sum::<f64>() / d.len() as f64;
            (d.iter().map(|x| (x - m).powi(2)).sum::<f64>() / d.len() as f64).sqrt()
        })
        .collect();
    let flux = flux.iter().sum::<f64>() / flux.len().max(1) as f64;
    (spread(&mut centroid), spread(&mut f2), flux)
}

// ── Level movement and brightness ──────────────────────────────────────

fn brightness(frames: &[Frame]) -> f64 {
    let band = |lo: f64, hi: f64| -> f64 {
        frames
            .iter()
            .map(|f| f.pow[hz_bin(lo)..=hz_bin(hi)].iter().sum::<f64>())
            .sum()
    };
    10.0 * (band(2_000.0, 5_000.0) / band(150.0, 1_000.0).max(1e-30))
        .max(1e-30)
        .log10()
}

fn level_movement(x: &[f64]) -> f64 {
    let n = (0.05 * SR) as usize;
    let db: Vec<f64> = x
        .chunks_exact(n)
        .map(|c| {
            10.0 * (c.iter().map(|v| v * v).sum::<f64>() / n as f64)
                .max(1e-20)
                .log10()
        })
        .collect();
    let top = db.iter().fold(f64::MIN, |m, &v| m.max(v));
    let avg = moving_mean(&db, 10);
    let d: Vec<f64> = db
        .iter()
        .zip(&avg)
        .filter(|(v, _)| **v > top - 30.0)
        .map(|(v, a)| v - a)
        .collect();
    (d.iter().map(|x| x * x).sum::<f64>() / d.len().max(1) as f64).sqrt()
}

// ── The command ────────────────────────────────────────────────────────

pub const HEADER: &str = "file\tLUFS\tpeak\tvib¢\tvibHz\tdrift¢\tbreath%\tnF\tF1\tF2\tF3\tBW1\tBW2\tBW3\tcentroid¢\tF2rng¢\tflux dB\tAM dB\tbright dB";

pub fn row(name: &str, s: &VoiceStats) -> String {
    format!(
        "{name}\t{:.2}\t{:.3}\t{:.1}\t{:.2}\t{:.1}\t{:.1}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{:.2}\t{:.2}\t{:.1}",
        s.lufs,
        s.peak,
        s.vibrato_cents,
        s.vibrato_hz,
        s.drift_cents,
        100.0 * s.breath,
        s.formants,
        s.f[0],
        s.f[1],
        s.f[2],
        s.bw[0],
        s.bw[1],
        s.bw[2],
        s.centroid_range,
        s.f2_range,
        s.flux_db,
        s.am_db,
        s.bright_db
    )
}

pub fn json(name: &str, s: &VoiceStats) -> serde_json::Value {
    serde_json::json!({
        "file": name, "lufs": s.lufs, "peak": s.peak,
        "vibrato_cents": s.vibrato_cents, "vibrato_hz": s.vibrato_hz,
        "drift_cents": s.drift_cents, "breath": s.breath, "formants": s.formants,
        "f": s.f, "bw": s.bw, "centroid_range_cents": s.centroid_range,
        "f2_range_cents": s.f2_range, "flux_db": s.flux_db, "am_db": s.am_db,
        "bright_db": s.bright_db,
    })
}

/// Each WAV's row; with `json`, every row there too.
pub fn stats(paths: &[String], json_out: Option<&std::path::Path>) -> Result<(), String> {
    use rayon::prelude::*;
    let rows: Vec<(String, VoiceStats)> = paths
        .par_iter()
        .map(|p| {
            let (chans, sr) = crate::reference::read_channels(std::path::Path::new(p))?;
            if f64::from(sr) != SR {
                return Err(format!("{p}: {sr} Hz, not {SR}"));
            }
            let name = std::path::Path::new(p)
                .file_stem()
                .map_or(p.clone(), |s| s.to_string_lossy().into_owned());
            Ok((name, measure(&chans)))
        })
        .collect::<Result<_, String>>()?;
    println!("{HEADER}");
    for (n, s) in &rows {
        println!("{}", row(n, s));
    }
    if let Some(path) = json_out {
        let v: Vec<_> = rows.iter().map(|(n, s)| json(n, s)).collect();
        std::fs::write(path, serde_json::to_string_pretty(&v).unwrap())
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECS: f64 = 3.0;

    /// Harmonics of `f0(t)` up to 5 kHz, each weighted by `amp(Hz)`.
    fn voice(f0: impl Fn(f64) -> f64, amp: impl Fn(f64) -> f64) -> Vec<f64> {
        let n = (SECS * SR) as usize;
        let mut ph = 0.0;
        let mut x = Vec::with_capacity(n);
        for i in 0..n {
            let f = f0(i as f64 / SR);
            let mut v = 0.0;
            let mut h = 1.0;
            while h * f < 5_000.0 {
                v += amp(h * f) * (h * ph).sin();
                h += 1.0;
            }
            x.push(v);
            ph += 2.0 * PI * f / SR;
        }
        // A floor 60 dB under the peak, as any recording has.
        let pk = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        x.iter()
            .zip(noise(9))
            .map(|(v, n)| 0.5 * v / pk + 1e-3 * n)
            .collect()
    }

    fn noise(seed: u64) -> Vec<f64> {
        let mut s = seed;
        (0..(SECS * SR) as usize)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 11) as f64 / (1u64 << 53) as f64 - 0.5
            })
            .collect()
    }

    /// Two resonances, 700 Hz and 1.2 kHz, each 100 Hz wide, over a floor
    /// 50 dB down.
    fn vowel(f: f64) -> f64 {
        let r = |c: f64| 1.0 / (1.0 + ((f - c) / 50.0).powi(2));
        r(700.0) + r(1_200.0) + 0.003
    }

    #[test]
    fn a_steady_sine_reads_its_loudness() {
        let x: Vec<f64> = (0..(SECS * SR) as usize)
            .map(|i| 0.1 * (2.0 * PI * 997.0 * i as f64 / SR).sin())
            .collect();
        // −20 dBFS on each side: −20 LUFS (BS.1770's 997 Hz check, two channels).
        let l = lufs_gated(&[x.clone(), x]);
        assert!((l + 20.0).abs() < 0.1, "{l}");
    }

    #[test]
    fn vibrato_reads_its_depth_and_rate() {
        // ±30 cents at 5.5 Hz: an RMS of 21.2 cents.
        let x = voice(
            |t| 220.0 * 2f64.powf(30.0 / 1200.0 * (2.0 * PI * 5.5 * t).sin()),
            vowel,
        );
        let s = measure(&[x]);
        assert!((s.vibrato_cents - 21.2).abs() < 4.0, "{s:?}");
        assert!((s.vibrato_hz - 5.5).abs() <= 0.5, "{s:?}");
        assert!(s.drift_cents < 3.0, "{s:?}");
    }

    #[test]
    fn a_steady_voice_has_no_movement_or_breath() {
        // C2's harmonics, 5.6 bins apart, still read as tonal.
        let low = voice(
            |t| 65.41 * 2f64.powf(30.0 / 1200.0 * (2.0 * PI * 5.5 * t).sin()),
            vowel,
        );
        let b = breath_share(&stft(&low));
        assert!(b < 0.05, "{b}");
        let s = measure(&[voice(|_| 130.81, vowel)]);
        assert!(s.vibrato_cents < 1.0 && s.drift_cents < 1.0, "{s:?}");
        assert!(s.breath < 0.02, "{s:?}");
        assert!(
            s.centroid_range < 20.0 && s.f2_range < 50.0 && s.flux_db < 0.5,
            "{s:?}"
        );
        assert!(s.am_db < 0.2, "{s:?}");
        // Formants at 700 Hz and 1.2 kHz: little over 2 kHz.
        assert!(s.bright_db < -25.0, "{s:?}");
    }

    #[test]
    fn the_envelope_finds_both_formants() {
        let s = measure(&[voice(|_| 110.0, vowel)]);
        assert_eq!(s.formants, 2.0, "{s:?}");
        assert!(
            (s.f[0] - 700.0).abs() < 40.0 && (s.f[1] - 1_200.0).abs() < 60.0,
            "{s:?}"
        );
        assert!(s.bw[0] > 60.0 && s.bw[0] < 250.0, "{s:?}");
    }

    fn breath_share_of(x: &[f64]) -> f64 {
        breath_share(&stft(x))
    }

    #[test]
    fn noise_is_breath_and_a_mix_is_part() {
        assert!(breath_share_of(&noise(1)) > 0.97);
        // A band 200 Hz wide at 1 kHz (RBJ's band-pass) is still breath.
        let (w, q) = (2.0 * PI * 1_000.0 / SR, 1_000.0 / 200.0);
        let al = w.sin() / (2.0 * q);
        let (b0, a0, a1, a2) = (al, 1.0 + al, -2.0 * w.cos(), 1.0 - al);
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        let band: Vec<f64> = noise(3)
            .into_iter()
            .map(|x0| {
                let y0 = (b0 * x0 - b0 * x2 - a1 * y1 - a2 * y2) / a0;
                (x2, x1, y2, y1) = (x1, x0, y1, y0);
                y0
            })
            .collect();
        let b = breath_share_of(&band);
        assert!(b > 0.9, "{b}");
        let v = voice(|_| 196.0, vowel);
        let rms = |x: &[f64]| (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
        let n = noise(2);
        let g = rms(&v) / rms(&n);
        let mix: Vec<f64> = v.iter().zip(&n).map(|(a, b)| a + g * b).collect();
        // Half the white noise lies over 12 kHz: in band it is a third.
        let b = breath_share_of(&mix);
        assert!((b - 1.0 / 3.0).abs() < 0.05, "{b}");
    }

    #[test]
    fn a_moving_vowel_reads_as_motion() {
        // F2 glides 900 Hz → 1.8 kHz and back, once a second.
        let n = (SECS * SR) as usize;
        let mut ph = 0.0;
        let x: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / SR;
                let f2 = 1_350.0 - 450.0 * (2.0 * PI * t).cos();
                let r = |f: f64, c: f64| 1.0 / (1.0 + ((f - c) / 60.0).powi(2));
                ph += 2.0 * PI * 110.0 / SR;
                (1..45)
                    .map(|h| {
                        let f = h as f64 * 110.0;
                        (r(f, 600.0) + r(f, f2) + 0.01) * (h as f64 * ph).sin()
                    })
                    .sum::<f64>()
                    * 0.02
            })
            .collect();
        let s = measure(&[x]);
        assert!(s.f2_range > 900.0, "{s:?}");
        assert!(s.flux_db > 2.0, "{s:?}");
        assert!(s.centroid_range > 200.0 && s.vibrato_cents < 1.0, "{s:?}");
    }
}
