//! Reading a source (spec § 1 What the arena holds, § 2 Reading a source):
//! the 16 B descriptor at the start of `held`, the 12-bit decode, a 32.32
//! head position and the 4-point, 3rd-order Hermite in Niemitalo's x-form
//! (Niemitalo 2001, "Polynomial Interpolators for High-Quality Resampling
//! of Oversampled Audio"). 12-bit storage at a rate per source is the
//! S950's.
//!
//! Every tap is clamped to `[0, frames − 1]`, or wrapped inside a
//! transwave frame, so no read leaves the bytes the row publishes.

use core::ops::{Add, Sub};

pub use super::desc::{
    DESC_LEN, Descriptor, MAX_ZONES, MIN_SOURCE_FRAMES, Shape, ZONE_LAYOUT_LEN, head_table,
};

/// A head's position: 32.32 fixed-point frames, exact over any source the
/// arena holds. Adds and subtracts wrap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos(pub u64);

impl Pos {
    /// `x` frames; a negative `x` wraps below 0.
    pub fn from_f32(x: f32) -> Pos {
        let whole = libm::floorf(x);
        let frac = ((x - whole) * 4_294_967_296.0) as u64;
        Pos(((whole as i32 as i64 as u64) << 32).wrapping_add(frac))
    }

    pub fn whole(self) -> u32 {
        (self.0 >> 32) as u32
    }

    /// In [0, 1): the top 24 bits, exact in an `f32`.
    pub fn frac(self) -> f32 {
        ((self.0 as u32) >> 8) as f32 * (1.0 / 16_777_216.0)
    }
}

impl Add for Pos {
    type Output = Pos;
    fn add(self, o: Pos) -> Pos {
        Pos(self.0.wrapping_add(o.0))
    }
}

impl Sub for Pos {
    type Output = Pos;
    fn sub(self, o: Pos) -> Pos {
        Pos(self.0.wrapping_sub(o.0))
    }
}

/// The 4-point, 3rd-order Hermite between `y[1]` and `y[2]` at `t` in
/// [0, 1), Niemitalo's x-form, on raw 12-bit samples: the `1/2048` that
/// scales them to [−1, 1) is applied once at the end.
#[inline]
pub fn hermite(y: [i32; 4], t: f32) -> f32 {
    let [y0, y1, y2, y3] = y.map(|v| v as f32);
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    (((c3 * t + c2) * t + c1) * t + y1) * (1.0 / 2048.0)
}

/// What a voice reads a source through: one block's copy of its row.
#[derive(Clone, Copy, Debug)]
pub struct SourceView<'a> {
    /// `held`'s whole pairs of packed samples: exactly `3 · frames / 2` B.
    samples: &'a [u8],
    desc: Descriptor,
    /// Even, at least 2.
    frames: u32,
    /// Panic on a tap outside `[0, frames)`, or a frame past the row.
    #[cfg(any(test, feature = "test-support"))]
    strict: bool,
}

impl<'a> SourceView<'a> {
    /// `None` if the descriptor is bad, or `held` holds fewer than
    /// `MIN_SOURCE_FRAMES` after it. The samples are `2 · (data_len / 3)`
    /// frames: an odd source reads one silent frame at its end. Always
    /// inlined: bank 1's `Pool::source_view` (the UI's) calls it, and
    /// `read` is bank 2's (door-check).
    #[inline(always)]
    pub fn new(held: &'a [u8]) -> Option<SourceView<'a>> {
        let desc = Descriptor::from_bytes(held)?;
        let data = held.get(desc.data_start()..)?;
        let pairs = data.len() / 3;
        let frames = u32::try_from(2 * pairs)
            .ok()
            .filter(|&f| f >= MIN_SOURCE_FRAMES)?;
        Some(SourceView {
            samples: &data[..3 * pairs],
            desc,
            frames,
            #[cfg(any(test, feature = "test-support"))]
            strict: false,
        })
    }

    /// The same view, checking the models' bounds: `read_cycle` panics on a
    /// frame past the row (`base + N > frames`), and any tap outside
    /// `[0, frames)` panics. `read`'s taps are clamped before the check
    /// sees them, so in `read` it guards the clamp itself.
    #[cfg(any(test, feature = "test-support"))]
    pub fn strict(held: &'a [u8]) -> Option<SourceView<'a>> {
        Self::new(held).map(|v| SourceView { strict: true, ..v })
    }

    pub fn desc(&self) -> Descriptor {
        self.desc
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// Sample `i` (clamped to the last frame), sign-extended, unscaled.
    #[inline]
    pub fn sample(&self, i: u32) -> i32 {
        self.decode(i.min(self.frames - 1))
    }

    /// Sample `i < frames`. It is 12 bits at bit `12·i` of the stream: one
    /// unaligned halfword load at byte `3·(i >> 1) + (i & 1)`, shifted by 0
    /// or 4 for its parity. The pair's three bytes hold both halfwords.
    #[inline(always)]
    fn decode(&self, i: u32) -> i32 {
        #[cfg(any(test, feature = "test-support"))]
        assert!(
            !self.strict || i < self.frames,
            "tap {i} outside {} frames",
            self.frames
        );
        let i = i as usize;
        debug_assert!(i < self.frames as usize);
        let o = (3 * i) >> 1; // 3·(i >> 1) + (i & 1)
        // SAFETY: every caller holds `i ≤ frames − 1` (`sample`, `read` and
        // `read_cycle` clamp with `min(frames − 1)`), and `samples` is
        // `3 · frames / 2` bytes, so `o + 1 ≤ 3 · frames / 2 − 1`: both
        // bytes are inside it. `read_unaligned` takes any alignment.
        let h = unsafe { core::ptr::read_unaligned(self.samples.as_ptr().add(o).cast::<u16>()) };
        ((u16::from_le(h) as i32) << (20 - 4 * (i & 1))) >> 20
    }

    /// The source at `p`: taps `p.whole() − 1 ..= p.whole() + 2`, each
    /// clamped to `[0, frames − 1]` (a whole past 2³¹ reads as below 0).
    #[inline]
    pub fn read(&self, p: Pos) -> f32 {
        let (w, last) = (p.whole() as i32, self.frames as i32 - 1);
        let tap = |k: i32| self.decode(w.saturating_add(k).max(0).min(last) as u32);
        hermite([tap(-1), tap(0), tap(1), tap(2)], p.frac())
    }

    /// A transwave frame of `N = 2^log2_n` samples from `base`, at `phase`:
    /// the taps wrap `& (N − 1)` inside `[base, base + N)`. A frame past
    /// the row reads its last sample there (and panics when `strict`).
    #[inline]
    pub fn read_cycle(&self, base: u32, log2_n: u32, phase: Pos) -> f32 {
        let n = 1u32 << (log2_n & 31);
        #[cfg(any(test, feature = "test-support"))]
        assert!(
            !self.strict || base as u64 + n as u64 <= self.frames as u64,
            "frame {base} + {n} past {} frames",
            self.frames
        );
        let (w, mask, last) = (phase.whole(), n.wrapping_sub(1), self.frames - 1);
        let tap = |k: u32| self.decode(base.wrapping_add(w.wrapping_add(k) & mask).min(last));
        hermite([tap(u32::MAX), tap(0), tap(1), tap(2)], phase.frac())
    }
}
