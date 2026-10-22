//! GRAIN's lock where the source carries no PERIOD (ADR 0204): the
//! window's own period, read off its first frames at note-on.
//!
//! Grains a quarter GRAIN apart read a tone at phases that differ by
//! `(speed − tstep) · interval / period` of a cycle; with no lock they
//! cancel near half a cycle, and a lift can't bring back what cancels.
//! So a window whose first frames repeat is given the period they repeat
//! at, and the grains lock to it as to YIN's: a chord's common period, a
//! pad's if it has one. A window that doesn't repeat (noise, a smear)
//! gets none, and its grains add in power, as the level's lift expects.
//!
//! The search is a normalised autocorrelation: coarse on the window
//! averaged down to about 6 kHz, a parabola through its best for the
//! fraction, then a climb at the source's rate to the nearest peak and a
//! parabola there. A multiple of the period may win on a high tone: it
//! locks in phase as well. Cold, once a note: at 48 kHz about 950 decodes
//! and 5,200 multiply-adds coarse, and 4 to 6 fine lags of 64 to 256.

use super::super::read::WholeRow;
use super::units::Period;

/// The analysis rate the coarse search runs at, at most: the source's
/// averaged down by a whole factor.
const RATE: u32 = 6_000;
/// The pitches looked for, Hz.
const LO_HZ: u32 = 70;
const HI_HZ: u32 = 1_500;
/// Analysis frames a coarse lag compares.
const N: usize = 32;
/// The longest lag at the analysis rate: `RATE / LO_HZ`.
const LAGS: usize = (RATE / LO_HZ) as usize + 1;
/// A lag's normalised correlation a lock needs: grains a period apart
/// read clearly alike.
const LOCK: f32 = 0.5;
/// The first lag within this share of the best is the period, so a
/// tone's period wins over its multiples.
const NEAR_BEST: f32 = 0.9;
/// The fine search's frames, at most: about a period (64 frames at
/// least), so a bright tone's correlation peaks where its period is.
const FINE: usize = 256;

/// The period of the window `frame(0..len)` reads (its frames the way it
/// plays), at the source's `rate`: `None` if it doesn't repeat between
/// 70 Hz and 1.5 kHz, or fewer than two periods fit.
pub fn of(v: &WholeRow<'_>, rate: u32, len: u32, frame: impl Fn(u32) -> u32) -> Option<Period> {
    let d = rate.div_ceil(RATE).max(1);
    let ar = rate / d;
    let lo = (ar / HI_HZ).max(2) as usize;
    // Two periods fit the window (the lock's own rule), and every lag's
    // frames, coarse and fine, are inside it.
    let room = len / d;
    let hi = (ar / LO_HZ)
        .min(len / (2 * d))
        .min(room.saturating_sub(N as u32 + 2))
        .min(len.saturating_sub(FINE as u32 + 2 * d + 2) / d) as usize;
    if hi < lo + 2 {
        return None;
    }
    let s = |u: u32| v.sample(frame(u)) as f32;
    let mut x = [0.0f32; N + LAGS + 2];
    for (k, x) in x.iter_mut().enumerate().take(N + hi + 2) {
        let at = k as u32 * d;
        *x = (0..d).map(|j| s(at + j)).sum::<f32>();
    }
    let e0: f32 = x[..N].iter().map(|a| a * a).sum();
    if e0 <= 0.0 {
        return None;
    }
    let corr = |t: usize| {
        let (mut c, mut e) = (0.0f32, 0.0f32);
        for i in 0..N {
            c += x[i] * x[i + t];
            e += x[i + t] * x[i + t];
        }
        if e > 0.0 {
            c / libm::sqrtf(e0 * e)
        } else {
            0.0
        }
    };
    let mut r = [0.0f32; LAGS + 2];
    for (t, r) in r.iter_mut().enumerate().take(hi + 2).skip(lo - 1) {
        *r = corr(t);
    }
    let best = r[lo..=hi].iter().fold(0.0f32, |a, &b| a.max(b));
    if best < LOCK {
        return None;
    }
    let t =
        (lo..=hi).find(|&t| r[t] >= NEAR_BEST * best && r[t] >= r[t - 1] && r[t] >= r[t + 1])?;
    let tc = t as f32 + vertex(r[t - 1], r[t], r[t + 1]);
    refine(&s, libm::roundf(tc * d as f32) as u32, d)
}

/// The fine period about `p0` (within about a coarse frame, `d`): from it
/// the climb to the nearest peak of the normalised correlation over about
/// a period, at most `d` steps, and the parabola there.
fn refine(s: &impl Fn(u32) -> f32, p0: u32, d: u32) -> Option<Period> {
    let n = (p0 as usize).clamp(FINE / 4, FINE);
    let mut a = [0.0f32; FINE];
    for (i, a) in a.iter_mut().enumerate().take(n) {
        *a = s(i as u32);
    }
    let a = &a[..n];
    let e0: f32 = a.iter().map(|x| x * x).sum();
    let corr = |l: u32| {
        let (mut c, mut e1) = (0.0f32, 0.0f32);
        for (i, x) in a.iter().enumerate() {
            let y = s(i as u32 + l);
            c += x * y;
            e1 += y * y;
        }
        if e0 > 0.0 && e1 > 0.0 {
            c / libm::sqrtf(e0 * e1)
        } else {
            0.0
        }
    };
    let mut l = p0.max(2);
    let (mut lo, mut mid, mut hi) = (corr(l - 1), corr(l), corr(l + 1));
    for _ in 0..d {
        if hi > mid && hi >= lo {
            (lo, mid, l) = (mid, hi, l + 1);
            hi = corr(l + 1);
        } else if lo > mid && l > 2 {
            (hi, mid, l) = (mid, lo, l - 1);
            lo = corr(l - 1);
        } else {
            break;
        }
    }
    Period::new(l as f32 + vertex(lo, mid, hi))
}

/// The offset of the parabola's top through `(−1, a)`, `(0, b)` and
/// `(1, c)`, held within half a step: 0 where it doesn't open down.
fn vertex(a: f32, b: f32, c: f32) -> f32 {
    let den = a - 2.0 * b + c;
    if den < 0.0 {
        (0.5 * (a - c) / den).clamp(-0.5, 0.5)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use super::*;
    use crate::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape};

    fn held(x: impl Fn(f32) -> f32, rate: u16, frames: usize) -> Vec<u8> {
        let d = Descriptor::whole(Shape::Plain, 1, rate).unwrap();
        let s: Vec<i16> = (0..frames)
            .map(|i| libm::roundf(x(i as f32 / f32::from(rate)) * 2_047.0) as i16)
            .collect();
        let mut h = d.to_bytes().to_vec();
        h.resize(DESC_LEN + 3 * frames.div_ceil(2), 0);
        crate::storage::twelve::pack12(&s, &mut h[DESC_LEN..]);
        h
    }

    fn period(h: &[u8], rate: u32) -> Option<f32> {
        let v = WholeRow::new(h).unwrap();
        of(&v, rate, v.frames(), |u| u).map(Period::get)
    }

    const TAU: f32 = core::f32::consts::TAU;

    /// A tone's period, or a whole multiple of it (which locks in phase
    /// as well), to a twentieth of a frame and 0.1 % a period, at 48, 32
    /// and 8 kHz.
    #[test]
    fn a_tone_finds_its_period() {
        for rate in [48_000u16, 32_000, 8_000] {
            for hz in [82.4f32, 261.63, 440.0, 1_318.5] {
                let h = held(|t| 0.5 * libm::sinf(TAU * hz * t), rate, 8_000);
                let want = f32::from(rate) / hz;
                let got = period(&h, rate.into()).unwrap_or(0.0);
                let m = libm::roundf(got / want).max(1.0);
                assert!(
                    (got - m * want).abs() < m * (0.05 + 0.001 * want),
                    "{hz} Hz at {rate}: {got} for {want}"
                );
            }
        }
    }

    /// A saw's period, not an octave of it; a fifth's common period.
    #[test]
    fn a_saw_and_a_fifth() {
        let saw = held(|t| 0.4 * (2.0 * (t * 261.63).fract() - 1.0), 48_000, 8_000);
        let got = period(&saw, 48_000).unwrap();
        assert!((got - 48_000.0 / 261.63).abs() < 0.1, "saw {got}");
        let fifth = held(
            |t| 0.25 * libm::sinf(TAU * 261.63 * t) + 0.25 * libm::sinf(TAU * 392.0 * t),
            48_000,
            8_000,
        );
        let got = period(&fifth, 48_000).unwrap();
        assert!((got - 2.0 * 48_000.0 / 261.63).abs() < 1.0, "fifth {got}");
    }

    /// Noise and silence don't repeat: no lock.
    #[test]
    fn noise_and_silence_have_none() {
        let mut r = 0x1234_5679u32;
        let noise: Vec<f32> = (0..8_000)
            .map(|_| {
                r ^= r << 13;
                r ^= r >> 17;
                r ^= r << 5;
                r as i32 as f32 / 2_147_483_648.0
            })
            .collect();
        let h = held(|t| 0.5 * noise[(t * 48_000.0) as usize], 48_000, 8_000);
        assert_eq!(period(&h, 48_000), None);
        assert_eq!(period(&held(|_| 0.0, 48_000, 8_000), 48_000), None);
    }

    /// A window too short for two periods of the lowest pitch looks no
    /// lower than two fit; one of 64 frames reads no further.
    #[test]
    fn a_short_window_stays_inside() {
        let h = held(|t| 0.5 * libm::sinf(TAU * 100.0 * t), 48_000, 64);
        assert_eq!(period(&h, 48_000), None);
    }
}
