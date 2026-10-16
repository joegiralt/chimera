//! The drop-out analyser: finds gaps, repeats and skips in a recording of
//! stream 0 (the MOTU's) against the bed it played.
//!
//! The reference is the bed upsampled to 48 kHz the way `fill_listen` does
//! (linearly) and looped. A 20 ms window of the recording is matched to the
//! reference at the lag nearest the last window's (the two clocks drift), a
//! gain is fitted and the residual measured. A repeated or skipped chunk
//! moves the lag by whole chunks, so those are tried too.

use crate::beds::opt;
use chimera_engine::stream::layout::CHUNK_FRAMES;
use rustfft::{FftPlanner, num_complex::Complex};

/// The recording's rate: the MOTU's.
const OUT_RATE: u32 = 48_000;
/// A window is 20 ms.
const WINDOW: usize = 960;
/// The lag search either side of the last window's: 1 ms.
const SEARCH: i64 = 48;
/// A window whose residual is over −40 dB of the reference is a drop-out.
const RESIDUAL_DB: f64 = -40.0;
/// A lag jump over this many samples is one.
const JUMP: f64 = 2.0;
/// Windows whose reference is under −50 dBFS (rms) are skipped.
const QUIET: f64 = 0.003_162_277_7;
/// Chunks a lag may jump either way and still be found.
const MAX_CHUNKS: i64 = 2;
/// The lock at the start: the first 1 s of sound, after a margin.
const LOCK: usize = OUT_RATE as usize;
const LOCK_MARGIN: usize = 4_800;

/// The bed at 48 kHz, looped.
pub struct Reference {
    r: Vec<f64>,
}

impl Reference {
    /// `src` (a bed at `rate`, ±1 full scale) upsampled linearly; frame
    /// `src.len()` is frame 0, as `fill_listen` wraps.
    pub fn new(src: &[f64], rate: u32) -> Reference {
        let step = f64::from(rate) / f64::from(OUT_RATE);
        let n = (src.len() as f64 / step) as usize;
        let r = (0..n)
            .map(|m| {
                let p = m as f64 * step;
                let (i, f) = (p as usize, p.fract());
                src[i % src.len()] * (1.0 - f) + src[(i + 1) % src.len()] * f
            })
            .collect();
        Reference { r }
    }

    /// Sample `p` (fractional), cubic between the samples: a bandlimited
    /// recording lies between them, not on the upsampler's straight lines.
    fn at(&self, p: f64) -> f64 {
        let n = self.r.len() as i64;
        let i = p.floor();
        let f = p - i;
        let i = i as i64;
        let s = |k: i64| self.r[(i + k).rem_euclid(n) as usize];
        let (a, b, c, d) = (s(-1), s(0), s(1), s(2));
        b + 0.5 * f * (c - a + f * (2.0 * a - 5.0 * b + 4.0 * c - d + f * (3.0 * (b - c) + d - a)))
    }

    /// The window of `W` samples whose first sample is at `from`.
    fn window(&self, from: f64, out: &mut [f64]) {
        if from.fract() == 0.0 {
            let n = self.r.len() as i64;
            let at = from as i64;
            out.iter_mut()
                .zip(at..)
                .for_each(|(o, k)| *o = self.r[k.rem_euclid(n) as usize]);
        } else {
            out.iter_mut()
                .zip(0..)
                .for_each(|(o, k)| *o = self.at(from + f64::from(k)));
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    /// The window's first sample.
    pub at: usize,
    pub resid_db: f64,
    pub lag: f64,
    /// Against the last window's lag, in samples.
    pub jump: f64,
}

/// How `x` fits `r` at the best gain: the residual's energy over `r`'s,
/// and that gain.
fn fit(x: &[f64], r: &[f64]) -> (f64, f64) {
    let (xx, xr, rr) = x
        .iter()
        .zip(r)
        .fold((0.0, 0.0, 0.0), |(xx, xr, rr), (&x, &r)| {
            (xx + x * x, xr + x * r, rr + r * r)
        });
    if rr == 0.0 {
        return (f64::INFINITY, 0.0);
    }
    (((xx - xr * xr / rr) / rr).max(0.0), xr / rr)
}

/// The sample offset into `r` where `rec[from..from + LOCK]` lies: the
/// peak of their circular cross-correlation.
fn lock(rec: &[f64], from: usize, r: &Reference) -> usize {
    let n = r.r.len();
    let mut seg: Vec<Complex<f64>> = rec[from..from + LOCK.min(rec.len() - from)]
        .iter()
        .map(|&x| Complex::new(x, 0.0))
        .collect();
    seg.resize(n, Complex::new(0.0, 0.0));
    let mut full: Vec<Complex<f64>> = r.r.iter().map(|&x| Complex::new(x, 0.0)).collect();
    let mut planner = FftPlanner::new();
    let (fwd, inv) = (planner.plan_fft_forward(n), planner.plan_fft_inverse(n));
    fwd.process(&mut seg);
    fwd.process(&mut full);
    let mut x: Vec<Complex<f64>> = full.iter().zip(&seg).map(|(r, s)| r * s.conj()).collect();
    inv.process(&mut x);
    x.iter()
        .enumerate()
        .max_by(|a, b| a.1.re.total_cmp(&b.1.re))
        .map_or(0, |(i, _)| i)
}

/// Every window of `rec` (48 kHz) that is not the reference: its residual
/// over −40 dB, or its lag jumping over 2 samples. `chunk` is the stream's
/// chunk in samples.
pub fn analyse(rec: &[f64], r: &Reference, chunk: usize) -> Result<Vec<Hit>, String> {
    let loud = |w: &[f64]| (w.iter().map(|x| x * x).sum::<f64>() / w.len() as f64).sqrt() > QUIET;
    let first = rec
        .as_chunks::<WINDOW>()
        .0
        .iter()
        .position(|w| loud(w))
        .map(|w| w * WINDOW + LOCK_MARGIN)
        .filter(|&s| s + LOCK / 4 <= rec.len())
        .ok_or("the recording has no 0.25 s of sound to lock on")?;
    let n = r.r.len() as f64;
    let mut lag = (lock(rec, first, r) as f64 - first as f64).rem_euclid(n);
    let (mut hits, mut want) = (Vec::new(), vec![0.0; WINDOW]);
    for at in (first - LOCK_MARGIN..=rec.len() - WINDOW).step_by(WINDOW) {
        let x = &rec[at..at + WINDOW];
        let try_lag = |center: f64| {
            // The best whole lag near `center`, then halved steps either side.
            let mut w = vec![0.0; WINDOW];
            let resid = |l: f64, w: &mut [f64]| {
                r.window(at as f64 + l, w);
                fit(x, w).0
            };
            let (mut best, mut at_best) = (f64::INFINITY, center);
            for d in -SEARCH..=SEARCH {
                let l = center.round() + d as f64;
                let e = resid(l, &mut w);
                if e < best {
                    (best, at_best) = (e, l);
                }
            }
            let mut step = 0.5;
            while step > 0.01 {
                for l in [at_best - step, at_best + step] {
                    let e = resid(l, &mut w);
                    if e < best {
                        (best, at_best) = (e, l);
                    }
                }
                step /= 2.0;
            }
            (best, at_best)
        };
        r.window(at as f64 + lag, &mut want);
        if want.iter().map(|x| x * x).sum::<f64>() / (WINDOW as f64) < QUIET * QUIET {
            continue;
        }
        let (mut e, mut l) = try_lag(lag);
        if 10.0 * e.log10() > RESIDUAL_DB {
            // A repeated or skipped chunk moves the lag by whole chunks.
            for m in (-MAX_CHUNKS..=MAX_CHUNKS).filter(|&m| m != 0) {
                let (e2, l2) = try_lag(lag + (m * chunk as i64) as f64);
                if e2 < e {
                    (e, l) = (e2, l2);
                }
            }
        }
        let (db, jump) = (10.0 * e.log10(), l - lag);
        if db > RESIDUAL_DB || jump.abs() > JUMP {
            hits.push(Hit {
                at,
                resid_db: db,
                lag: l,
                jump,
            });
        }
        // A window that fits nothing (a gap, a seam) moves nothing.
        if db <= RESIDUAL_DB {
            lag = l;
        }
    }
    Ok(hits)
}

/// `dropouts find REC.WAV --ref BED.WAV [--channel N]`: false if any.
pub fn find(args: &[String]) -> Result<bool, String> {
    let mut args = args.to_vec();
    let channel: usize = opt(&mut args, "--channel", 0)?;
    let bed: String = opt(&mut args, "--ref", String::new())?;
    let [rec] = args.as_slice() else {
        return Err("dropouts find REC.WAV --ref BED.WAV [--channel N]".into());
    };
    if bed.is_empty() {
        return Err("dropouts find needs --ref BED.WAV".into());
    }
    let (rec, rec_rate) = read(rec, channel)?;
    if rec_rate != OUT_RATE {
        return Err(format!(
            "{rec_rate} Hz: the recording must be {OUT_RATE} Hz"
        ));
    }
    let (bed, rate) = read(&bed, 0)?;
    let chunk = (u64::from(CHUNK_FRAMES.get()) * u64::from(OUT_RATE) / u64::from(rate)) as usize;
    let hits = analyse(&rec, &Reference::new(&bed, rate), chunk)?;
    for h in &hits {
        println!(
            "{:9.3} s  residual {:6.1} dB  lag {:+10.2}  jump {:+8.2}",
            h.at as f64 / f64::from(OUT_RATE),
            h.resid_db,
            h.lag,
            h.jump
        );
    }
    println!("{} drop-outs", hits.len());
    Ok(hits.is_empty())
}

/// One channel of a WAV, ±1 full scale, and its rate.
fn read(path: &str, channel: usize) -> Result<(Vec<f64>, u32), String> {
    let mut w = hound::WavReader::open(path).map_err(|e| format!("{path}: {e}"))?;
    let spec = w.spec();
    let ch = usize::from(spec.channels);
    if channel >= ch {
        return Err(format!("{path}: {ch} channels, no channel {channel}"));
    }
    let all: Result<Vec<f64>, _> = match spec.sample_format {
        hound::SampleFormat::Float => w.samples::<f32>().map(|s| s.map(f64::from)).collect(),
        hound::SampleFormat::Int => {
            let full = f64::from(1u32 << (spec.bits_per_sample - 1));
            w.samples::<i32>()
                .map(|s| s.map(|s| f64::from(s) / full))
                .collect()
        }
    };
    let all = all.map_err(|e| format!("{path}: {e}"))?;
    Ok((
        all.into_iter().skip(channel).step_by(ch).collect(),
        spec.sample_rate,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beds::chirp;

    const C: usize = CHUNK_FRAMES.get() as usize * 2;

    fn reference() -> Reference {
        let bed: Vec<f64> = chirp(0, 6.0, 24_000)
            .iter()
            .map(|&s| f64::from(s) / 32768.0)
            .collect();
        Reference::new(&bed, 24_000)
    }

    /// The reference's linear interpolation at `p` samples.
    fn linear(r: &Reference, p: f64) -> f64 {
        let n = r.r.len();
        let i = p.floor();
        let f = p - i;
        let i = (i as i64).rem_euclid(n as i64) as usize;
        r.r[i] * (1.0 - f) + r.r[(i + 1) % n] * f
    }

    /// A recording: 0.1 s of silence, then the bed from sample `from` for
    /// `secs`, its clock `ppm` fast, with noise at −80 dB; `warp(i)` moves
    /// the position read at recorded sample `i` (a jump is a repeat or skip).
    fn record(
        r: &Reference,
        from: f64,
        secs: f64,
        ppm: f64,
        warp: impl Fn(usize) -> f64,
    ) -> Vec<f64> {
        let n = (secs * f64::from(OUT_RATE)) as usize;
        let mut seed = 12_345u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (f64::from(seed >> 8) / f64::from(1u32 << 24) - 0.5) * 1e-4 * 3.464
        };
        let mut rec = vec![0.0; 4_800];
        rec.extend(
            (0..n).map(|i| linear(r, from + i as f64 * (1.0 + ppm * 1e-6) + warp(i)) + noise()),
        );
        rec
    }

    fn secs(h: &Hit) -> f64 {
        h.at as f64 / f64::from(OUT_RATE)
    }

    #[test]
    fn clean_copy_has_no_dropouts() {
        let r = reference();
        let rec = record(&r, 70_000.0, 3.0, 40.0, |_| 0.0);
        let hits = analyse(&rec, &r, C).unwrap();
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn a_1_ms_gap_is_found_at_its_time() {
        let r = reference();
        let mut rec = record(&r, 70_000.0, 3.0, 40.0, |_| 0.0);
        let at = 4_800 + 100_000;
        rec[at..at + 48].fill(0.0);
        let hits = analyse(&rec, &r, C).unwrap();
        let t = at as f64 / f64::from(OUT_RATE);
        assert!(!hits.is_empty() && hits.len() <= 2, "{hits:?}");
        assert!(
            hits.iter()
                .all(|h| secs(h) > t - 0.02 && secs(h) < t + 0.001),
            "{hits:?} at {t}"
        );
    }

    #[test]
    fn a_repeated_chunk_is_found() {
        let r = reference();
        let ev = 100_000;
        let rec = record(&r, 70_000.0, 3.0, 40.0, |i| {
            if i >= ev { -(C as f64) } else { 0.0 }
        });
        let hits = analyse(&rec, &r, C).unwrap();
        let t = (4_800 + ev) as f64 / f64::from(OUT_RATE);
        assert!(!hits.is_empty() && hits.len() <= 3, "{hits:?}");
        assert!(
            hits.iter()
                .all(|h| secs(h) > t - 0.02 && secs(h) < t + 0.06),
            "{hits:?} at {t}"
        );
        assert!(
            hits.iter().any(|h| (h.jump + C as f64).abs() < 3.0),
            "{hits:?}"
        );
    }

    #[test]
    fn a_skipped_chunk_is_found() {
        let r = reference();
        let ev = 100_000;
        let rec = record(&r, 70_000.0, 3.0, 40.0, |i| {
            if i >= ev { C as f64 } else { 0.0 }
        });
        let hits = analyse(&rec, &r, C).unwrap();
        let t = (4_800 + ev) as f64 / f64::from(OUT_RATE);
        assert!(!hits.is_empty() && hits.len() <= 3, "{hits:?}");
        assert!(
            hits.iter()
                .all(|h| secs(h) > t - 0.02 && secs(h) < t + 0.06),
            "{hits:?} at {t}"
        );
        assert!(
            hits.iter().any(|h| (h.jump - C as f64).abs() < 3.0),
            "{hits:?}"
        );
    }
}
