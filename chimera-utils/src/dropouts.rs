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
/// What is compared: the band under this. The reference's linear upsample
/// bakes an image of the bed at (24 kHz - f) that a converter's filter
/// changes, -38 dB re a 2.5 kHz tone: over the threshold, and no drop-out.
const BAND_HZ: f64 = 8_000.0;
/// This many flagged windows in a row (100 ms) and the lag is lost: lock again.
const LOST: usize = 5;
/// The lock at the start: the first 1 s of sound, after a margin.
const LOCK: usize = OUT_RATE as usize;

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
    // A gain under zero is a half period away, not a fit.
    if xr <= 0.0 {
        return (xx / rr, 0.0);
    }
    (((xx - xr * xr / rr) / rr).max(0.0), xr / rr)
}

/// The sample offset into `r` where `rec[from..]` (up to 1 s, at least
/// 0.25 s) lies: the peak of their circular cross-correlation.
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

/// `x` through a linear-phase low-pass at `BAND_HZ` (63 taps, Hann), the
/// ends wrapped when `looped`, else zeros. Both sides get the same, so
/// their delay cancels.
fn lowpass(x: &[f64], looped: bool) -> Vec<f64> {
    use std::f64::consts::PI;
    const HALF: i64 = 31;
    let fc = BAND_HZ / f64::from(OUT_RATE);
    let taps: Vec<f64> = (-HALF..=HALF)
        .map(|k| {
            let k = k as f64;
            let sinc = if k == 0.0 {
                2.0 * fc
            } else {
                (2.0 * PI * fc * k).sin() / (PI * k)
            };
            sinc * (0.5 + 0.5 * (PI * k / (HALF as f64 + 1.0)).cos())
        })
        .collect();
    let gain: f64 = taps.iter().sum();
    let n = x.len() as i64;
    let at = |i: i64| match (looped, i) {
        (true, _) => x[i.rem_euclid(n) as usize],
        (false, 0..) if i < n => x[i as usize],
        _ => 0.0,
    };
    (0..n)
        .map(|i| {
            taps.iter()
                .zip(-HALF..)
                .map(|(h, k)| h * at(i + k))
                .sum::<f64>()
                / gain
        })
        .collect()
}

/// Every window of `rec` (48 kHz) that is not the reference: its residual
/// over −40 dB, or its lag jumping over 2 samples. `chunk` is the stream's
/// chunk in samples.
pub fn analyse(rec: &[f64], r: &Reference, chunk: usize) -> Result<Vec<Hit>, String> {
    let (rec, r) = (
        &lowpass(rec, false)[..],
        &Reference {
            r: lowpass(&r.r, true),
        },
    );
    let loud = |w: &[f64]| (w.iter().map(|x| x * x).sum::<f64>() / w.len() as f64).sqrt() > QUIET;
    let first = rec
        .as_chunks::<WINDOW>()
        .0
        .iter()
        .position(|w| loud(w))
        // Past the onset, which the filter smears into the window before.
        .map(|w| (w + 2) * WINDOW)
        .filter(|&s| s + LOCK / 4 <= rec.len())
        .ok_or("the recording has no 0.25 s of sound to lock on")?;
    let n = r.r.len() as f64;
    let mut lag = (lock(rec, first, r) as f64 - first as f64).rem_euclid(n);
    let (mut hits, mut want) = (Vec::new(), vec![0.0; WINDOW]);
    // The flagged windows in a row, held until a clean one or a re-lock.
    let mut run: Vec<Hit> = Vec::new();
    // Not the last 31 samples either: the filter's zeros.
    for at in (first..=rec.len().saturating_sub(WINDOW + 32)).step_by(WINDOW) {
        let x = &rec[at..at + WINDOW];
        let try_lag = |center: f64| {
            let w = std::cell::RefCell::new(vec![0.0; WINDOW]);
            let resid = |l: f64| {
                let mut w = w.borrow_mut();
                r.window(at as f64 + l, &mut w);
                fit(x, &w).0
            };
            // Halved steps either side of `from`.
            let refine = |from: f64, mut step: f64| {
                let (mut best, mut at_best) = (resid(from), from);
                while step > 0.01 {
                    for l in [at_best - step, at_best + step] {
                        let e = resid(l);
                        if e < best {
                            (best, at_best) = (e, l);
                        }
                    }
                    step /= 2.0;
                }
                (best, at_best)
            };
            // The clocks drift slowly: a few samples either side first.
            let near = (-12..=12)
                .map(|d| refine(center + f64::from(d) * 0.25, 0.125))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap_or((f64::INFINITY, center));
            if 10.0 * near.0.log10() <= RESIDUAL_DB {
                return near;
            }
            // Else the ms either side: the few best whole lags (a tone fits a
            // period away nearly as well), each refined.
            let mut whole: Vec<(f64, f64)> = (-SEARCH..=SEARCH)
                .map(|d| {
                    let l = center.round() + d as f64;
                    (resid(l), l)
                })
                .collect();
            whole.sort_by(|a, b| a.0.total_cmp(&b.0));
            whole
                .iter()
                .take(4)
                .map(|&(_, l)| refine(l, 0.5))
                .fold(near, |a, b| if b.0 < a.0 { b } else { a })
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
        if db > RESIDUAL_DB {
            // A window that fits nothing (a gap, a seam) moves nothing.
            run.push(Hit {
                at,
                resid_db: db,
                lag: l,
                jump,
            });
            if run.len() == LOST && at + LOCK / 4 <= rec.len() {
                // Lost: the lag changed by something else than chunks.
                let found = (lock(rec, at, r) as f64 - at as f64).rem_euclid(n);
                let slip = (found - lag + n / 2.0).rem_euclid(n) - n / 2.0;
                if slip.abs() > JUMP {
                    let first = run[0];
                    hits.push(Hit {
                        jump: slip,
                        lag: found,
                        ..first
                    });
                    run.clear();
                    lag = found;
                }
            }
            continue;
        }
        hits.append(&mut run);
        if jump.abs() > JUMP {
            hits.push(Hit {
                at,
                resid_db: db,
                lag: l,
                jump,
            });
        }
        lag = l;
    }
    hits.append(&mut run);
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

    #[test]
    fn a_slip_that_is_no_chunk_is_one_event_then_clean() {
        let r = reference();
        let ev = 100_000;
        let rec = record(
            &r,
            70_000.0,
            3.0,
            40.0,
            |i| if i >= ev { 1_000.0 } else { 0.0 },
        );
        let hits = analyse(&rec, &r, C).unwrap();
        let t = (4_800 + ev) as f64 / f64::from(OUT_RATE);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(
            secs(&hits[0]) > t - 0.02 && secs(&hits[0]) < t + 0.06,
            "{hits:?} at {t}"
        );
        assert!((hits[0].jump - 1_000.0).abs() < 3.0, "{hits:?}");
    }

    /// Bed 7 (to 2.5 kHz) as a converter gives it: the reference's samples
    /// low-passed at 21 kHz and delayed 0.37 sample (windowed sinc), not
    /// the reference's own interpolation.
    #[test]
    fn a_filtered_delayed_high_bed_is_clean() {
        let bed: Vec<f64> = chirp(7, 6.0, 24_000)
            .iter()
            .map(|&s| f64::from(s) / 32768.0)
            .collect();
        let r = Reference::new(&bed, 24_000);
        let (fc, delay, half) = (21.0 / 48.0, 0.37, 32i64);
        let pi = std::f64::consts::PI;
        let h = |k: i64| {
            let x = k as f64 - delay;
            let sinc = if x == 0.0 {
                2.0 * fc
            } else {
                (2.0 * pi * fc * x).sin() / (pi * x)
            };
            sinc * (0.5 + 0.5 * (pi * x / (half as f64 + 1.0)).cos())
        };
        let n = r.r.len() as i64;
        let mut seed = 99u32;
        let mut rec = vec![0.0; 4_800];
        rec.extend((70_000..70_000 + 3 * 48_000).map(|m: i64| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (f64::from(seed >> 8) / f64::from(1u32 << 24) - 0.5) * 1e-4 * 3.464;
            (-half..=half + 1)
                .map(|k| h(k) * r.r[(m - k).rem_euclid(n) as usize])
                .sum::<f64>()
                + noise
        }));
        let hits = analyse(&rec, &r, C).unwrap();
        assert!(hits.is_empty(), "{hits:?}");
    }
}
