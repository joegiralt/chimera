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
//! locks in phase as well. The window's mean is taken off first, so a
//! level (DC) doesn't repeat.
//!
//! At 48 kHz the search is, at worst, about 950 decodes and 5,400
//! multiply-adds coarse, then up to 11 fine lags (the climb's `d` = 8
//! steps and the parabola's three) of 256 frames: about 3,800 decodes and
//! 11,000 multiply-adds in all, some 50 k M7 cycles. So it runs once a
//! source and window (`of_window`, its answer kept in `CACHE`), not once a
//! note: a chord's notes after the first, and every later note on the
//! same window, pay a lookup of eight decodes.

use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use super::super::read::WholeRow;
use super::units::Period;

/// Windows whose search is kept: more than the voices.
const SLOTS: usize = 8;

/// One kept search: its window's key, and the answer tagged with the
/// key's low half (a slot written by another key never reads as this
/// one's). In RAM; only the audio path writes it.
struct Slot {
    key: AtomicU32,
    /// `key`'s low 16 bits over the period in 1/64 frame, 0 for none.
    val: AtomicU32,
}

#[allow(clippy::declare_interior_mutable_const)]
const EMPTY: Slot = Slot {
    key: AtomicU32::new(0),
    val: AtomicU32::new(0),
};

/// The kept searches, written round in turn.
static CACHE: [Slot; SLOTS] = [EMPTY; SLOTS];
static NEXT: AtomicU32 = AtomicU32::new(0);

/// Searches the audio block may still run (#777, ADR 0211): one. A new
/// window's search is some 50 k cycles, and eight in one block (a chord
/// on eight windows) overran it; past the first, a note plays unlocked
/// and searches in a later block (`Grains::render`).
/// The block's count, `GEN`: a GRAIN voice rendering a second time in it
/// means the next block has begun (`rendering`).
#[cfg(not(any(test, feature = "test-support")))]
static LEFT: AtomicU32 = AtomicU32::new(1);
#[cfg(not(any(test, feature = "test-support")))]
static GEN: AtomicU32 = AtomicU32::new(1);

#[cfg(not(any(test, feature = "test-support")))]
mod left {
    use super::{GEN, LEFT, Relaxed};
    pub fn get() -> u32 {
        LEFT.load(Relaxed)
    }
    pub fn set(n: u32) {
        LEFT.store(n, Relaxed);
    }
    pub fn generation() -> u32 {
        GEN.load(Relaxed)
    }
    pub fn set_generation(g: u32) {
        GEN.store(g, Relaxed);
    }
    pub fn ran() {}
}

/// The allowance, the block's count and the searches run, per thread:
/// parallel tests don't spend each other's.
#[cfg(any(test, feature = "test-support"))]
mod left {
    extern crate std;
    use core::cell::Cell;
    std::thread_local!(static LEFT: Cell<(u32, u32, u32)> = const { Cell::new((1, 1, 0)) });
    pub fn get() -> u32 {
        LEFT.with(|c| c.get().0)
    }
    pub fn set(n: u32) {
        LEFT.with(|c| {
            let (_, g, r) = c.get();
            c.set((n, g, r))
        });
    }
    pub fn generation() -> u32 {
        LEFT.with(|c| c.get().1)
    }
    pub fn set_generation(g: u32) {
        LEFT.with(|c| {
            let (n, _, r) = c.get();
            c.set((n, g, r))
        });
    }
    pub fn ran() {
        LEFT.with(|c| {
            let (n, g, r) = c.get();
            c.set((n, g, r + 1))
        });
    }
    pub fn runs() -> u32 {
        LEFT.with(|c| c.get().2)
    }
}

/// A GRAIN voice's block begins, `seen` the block count it last rendered
/// in: a voice that rendered in this count already starts the next block,
/// and its one search comes back. At most one such a block: the others
/// that rendered last block take the new count as they render.
pub fn rendering(seen: &mut u32) {
    let g = left::generation();
    if *seen == g {
        let next = g.wrapping_add(1).max(1);
        left::set_generation(next);
        left::set(1);
        *seen = next;
    } else {
        *seen = g;
    }
}

/// The next block's search back now: for the tests' own blocks.
#[cfg(any(test, feature = "test-support"))]
pub fn new_block() {
    left::set(1);
}

/// Searches run so far on this thread: for the tests.
#[cfg(any(test, feature = "test-support"))]
pub fn searches() -> u32 {
    left::runs()
}

/// `of`, once a source and window, and at most one search a block (`LEFT`):
/// `None` while this block's is spent, so the caller asks again next block.
/// The row `v` (where it sits and its
/// frames), its `rate`, the window (`base`, `len`, `rev`) and eight of its
/// samples make the key, so a row re-stored where another was, or the
/// window moved, searches again. The answer is the kept one's, to 1/64
/// frame, even the first time, so every note of a window locks alike.
pub fn of_window(
    v: &WholeRow<'_>,
    rate: u32,
    (base, len, rev): (u32, u32, bool),
    frame: impl Fn(u32) -> u32,
) -> Option<Option<Period>> {
    let mut h: u32 = 0x811C_9DC5;
    let mut mix = |x: u32| {
        h = (h ^ x).wrapping_mul(0x0100_0193);
    };
    mix(v.addr() as u32);
    mix(v.frames());
    mix(rate);
    mix(base);
    mix(len);
    mix(u32::from(rev));
    for k in 0..8 {
        mix(v.sample(frame(k * (len / 8))) as u32);
    }
    let key = h | 1;
    let tag = (key & 0xFFFF) << 16;
    for s in &CACHE {
        if s.key.load(Relaxed) == key {
            let val = s.val.load(Relaxed);
            if val & 0xFFFF_0000 == tag {
                return Some(decode(val));
            }
        }
    }
    if left::get() == 0 {
        return None;
    }
    left::set(0);
    left::ran();
    let val = tag | encode(of(v, rate, len, frame));
    let i = NEXT.load(Relaxed) as usize % SLOTS;
    NEXT.store((i + 1) as u32, Relaxed);
    let s = &CACHE[i];
    s.key.store(0, Relaxed);
    s.val.store(val, Relaxed);
    s.key.store(key, Relaxed);
    Some(decode(val))
}

/// A period in 1/64 frame, 1 to 0xFFFF (1,023 frames: past any the search
/// finds); 0, none.
fn encode(p: Option<Period>) -> u32 {
    p.map_or(0, |p| {
        crate::dsp::clampf(libm::roundf(p.get() * 64.0), 1.0, 65_535.0) as u32
    })
}

fn decode(val: u32) -> Option<Period> {
    Period::new((val & 0xFFFF) as f32 * (1.0 / 64.0))
}

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
// Index loops, not iterator adapters (an adapter's instance is core's and
// links into bank 1), and every read and write through `get`: no bounds
// check's panic location in bank 1. The bounds above hold every index.
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
    for k in 0..N + hi + 2 {
        let at = k as u32 * d;
        let mut sum = 0.0;
        for j in 0..d {
            sum += s(at + j);
        }
        if let Some(x) = x.get_mut(k) {
            *x = sum;
        }
    }
    // The level off: a constant repeats at every lag.
    let m = N + hi + 2;
    let mut mean = 0.0;
    for k in 0..m {
        mean += at(&x, k);
    }
    mean /= m as f32;
    for k in 0..m {
        if let Some(x) = x.get_mut(k) {
            *x -= mean;
        }
    }
    let xi = |i: usize| at(&x, i);
    let mut e0 = 0.0f32;
    for k in 0..N {
        e0 += xi(k) * xi(k);
    }
    if e0 <= 0.0 {
        return None;
    }
    let corr = |t: usize| {
        let (mut c, mut e) = (0.0f32, 0.0f32);
        for i in 0..N {
            c += xi(i) * xi(i + t);
            e += xi(i + t) * xi(i + t);
        }
        if e > 0.0 {
            c / libm::sqrtf(e0 * e)
        } else {
            0.0
        }
    };
    let mut r = [0.0f32; LAGS + 2];
    let mut best = 0.0f32;
    for t in lo - 1..hi + 2 {
        let c = corr(t);
        if let Some(r) = r.get_mut(t) {
            *r = c;
        }
        if (lo..=hi).contains(&t) {
            best = best.max(c);
        }
    }
    if best < LOCK {
        return None;
    }
    let ri = |t: usize| at(&r, t);
    let mut t = lo;
    while !(ri(t) >= NEAR_BEST * best && ri(t) >= ri(t - 1) && ri(t) >= ri(t + 1)) {
        t += 1;
        if t > hi {
            return None;
        }
    }
    let tc = t as f32 + vertex(ri(t - 1), ri(t), ri(t + 1));
    refine(&s, libm::roundf(tc * d as f32) as u32, d)
}

/// The fine period about `p0` (within about a coarse frame, `d`): from it
/// the climb to the nearest peak of the normalised correlation over about
/// a period, at most `d` steps, and the parabola there.
fn refine(s: &impl Fn(u32) -> f32, p0: u32, d: u32) -> Option<Period> {
    let n = (p0 as usize).clamp(FINE / 4, FINE);
    let mut a = [0.0f32; FINE];
    let mut mean = 0.0;
    for i in 0..n {
        let y = s(i as u32);
        if let Some(a) = a.get_mut(i) {
            *a = y;
        }
        mean += y;
    }
    mean /= n as f32;
    let mut e0 = 0.0f32;
    for i in 0..n {
        if let Some(a) = a.get_mut(i) {
            *a -= mean;
            e0 += *a * *a;
        }
    }
    let corr = |l: u32| {
        let (mut c, mut e1) = (0.0f32, 0.0f32);
        for i in 0..n {
            let y = s(i as u32 + l) - mean;
            c += at(&a, i) * y;
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

/// `x[i]`, or 0 past its end (never, under the callers' bounds): no
/// bounds check's panic.
#[inline(always)]
fn at(x: &[f32], i: usize) -> f32 {
    match x.get(i) {
        Some(&y) => y,
        None => 0.0,
    }
}

/// The offset of the parabola's top through `(−1, a)`, `(0, b)` and
/// `(1, c)`, held within half a step: 0 where it doesn't open down.
fn vertex(a: f32, b: f32, c: f32) -> f32 {
    let den = a - 2.0 * b + c;
    if den < 0.0 {
        crate::dsp::clampf(0.5 * (a - c) / den, -0.5, 0.5)
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

    /// A level alone (DC) and a level under a tone: none for the one, the
    /// tone's period for the other (the mean comes off first).
    #[test]
    fn a_level_has_no_period() {
        assert_eq!(period(&held(|_| 0.4, 48_000, 8_000), 48_000), None);
        let h = held(|t| 0.4 + 0.3 * libm::sinf(TAU * 440.0 * t), 48_000, 8_000);
        let got = period(&h, 48_000).unwrap();
        let m = libm::roundf(got / (48_000.0 / 440.0)).max(1.0);
        assert!((got - m * 48_000.0 / 440.0).abs() < 0.2 * m, "{got}");
    }

    /// The kept search answers as the search does, to 1/64 frame, the
    /// same on every note; a window moved, or another row, is its own.
    #[test]
    fn a_window_is_searched_once() {
        let h = held(|t| 0.5 * libm::sinf(TAU * 261.63 * t), 48_000, 8_000);
        let v = WholeRow::new(&h).unwrap();
        let n = v.frames();
        let once = |base: u32| {
            new_block();
            of_window(&v, 48_000, (base, n - base, false), |u| base + u).unwrap()
        };
        let first = once(0).unwrap().get();
        let want = of(&v, 48_000, n, |u| u).unwrap().get();
        assert!((first - want).abs() <= 1.0 / 128.0, "{first} {want}");
        for _ in 0..4 {
            assert_eq!(once(0).map(Period::get), Some(first));
        }
        let moved = once(1_000).unwrap().get();
        let want = of(&v, 48_000, n - 1_000, |u| 1_000 + u).unwrap().get();
        assert!((moved - want).abs() <= 1.0 / 128.0, "{moved} {want}");
        let saw = held(|t| 0.4 * (2.0 * (t * 261.63).fract() - 1.0), 48_000, 8_000);
        let w = WholeRow::new(&saw).unwrap();
        new_block();
        let other = of_window(&w, 48_000, (0, n, false), |u| u).unwrap();
        assert_eq!(
            other.map(Period::get),
            of(&w, 48_000, n, |u| u).map(|p| { libm::roundf(p.get() * 64.0) / 64.0 })
        );
    }

    /// One search a block: a second new window waits for the next block,
    /// and a kept one answers whatever the block has spent.
    #[test]
    fn one_search_a_block() {
        let h = held(|t| 0.5 * libm::sinf(TAU * 220.0 * t), 48_000, 8_000);
        let v = WholeRow::new(&h).unwrap();
        let n = v.frames();
        let at = |base: u32| of_window(&v, 48_000, (base, n - base, false), |u| base + u);
        new_block();
        let first = at(10).expect("the block's search");
        assert_eq!(at(20), None, "a second new window waits");
        assert_eq!(at(10), Some(first), "a kept one answers");
        new_block();
        assert!(at(20).is_some(), "next block it searches");
    }

    /// A window too short for two periods of the lowest pitch looks no
    /// lower than two fit; one of 64 frames reads no further.
    #[test]
    fn a_short_window_stays_inside() {
        let h = held(|t| 0.5 * libm::sinf(TAU * 100.0 * t), 48_000, 64);
        assert_eq!(period(&h, 48_000), None);
    }
}
