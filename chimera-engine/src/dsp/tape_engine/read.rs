//! Reading a source (spec § 1 What the arena holds, § 2 Reading a source):
//! the 16 B descriptor at the start of `held`, the 12-bit decode, a 32.32
//! head position and the 4-point, 3rd-order Hermite in Niemitalo's x-form
//! (Niemitalo 2001, "Polynomial Interpolators for High-Quality Resampling
//! of Oversampled Audio"). 12-bit storage at a rate per source is the
//! S950's.
//!
//! Every tap is clamped to `[0, frames − 1]`, or wrapped inside a
//! transwave frame, so no read leaves the bytes the row publishes.

use core::num::NonZeroU32;
use core::ops::{Add, Sub};

pub use super::desc::{
    DESC_LEN, Descriptor, HEAD_ENTRY, HeadSpec, HeadTable, MAX_ZONES, MIN_SOURCE_FRAMES, Shape,
    ZONE_LAYOUT_LEN, head_table, streamed_held_len, write_head_table,
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

/// What a voice reads a source through: one block's copy of its row, or
/// of a span of it (`span`): source frames `first..end`.
#[derive(Clone, Copy, Debug)]
pub struct SourceView<'a> {
    /// Whole pairs of packed samples from `first`: exactly
    /// `3 · (end − first) / 2` B.
    samples: &'a [u8],
    desc: Descriptor,
    /// The source's frames: even, at least 2.
    frames: NonZeroU32,
    /// The frames held, `first..end`: even, `first < end ≤ frames`. A
    /// whole row's are `0..frames`.
    first: u32,
    end: u32,
    /// Panic on a tap outside the frames held (but at the source's own
    /// ends), or a frame past the row.
    #[cfg(any(test, feature = "test-support"))]
    strict: bool,
}

impl<'a> SourceView<'a> {
    /// `None` if the descriptor is bad or streamed (its heads aren't the
    /// source), or `held` holds fewer than `MIN_SOURCE_FRAMES` after it.
    /// The samples are `2 · (data_len / 3)` frames: an odd source reads one
    /// silent frame at its end. Always inlined: bank 1's
    /// `Pool::source_view` (the UI's) calls it, and `read` is bank 2's
    /// (door-check).
    #[inline(always)]
    pub fn new(held: &'a [u8]) -> Option<SourceView<'a>> {
        Self::of(held, Descriptor::from_bytes(held)?)
    }

    /// `new` under `desc`, `held`'s descriptor already read.
    #[inline(always)]
    pub fn of(held: &'a [u8], desc: Descriptor) -> Option<SourceView<'a>> {
        if desc.is_streamed() {
            return None;
        }
        let data = held.get(desc.data_start()..)?;
        let pairs = data.len() / 3;
        let frames = u32::try_from(2 * pairs)
            .ok()
            .filter(|&f| f >= MIN_SOURCE_FRAMES)
            .and_then(NonZeroU32::new)?;
        Some(SourceView {
            samples: &data[..3 * pairs],
            desc,
            frames,
            first: 0,
            end: frames.get(),
            #[cfg(any(test, feature = "test-support"))]
            strict: false,
        })
    }

    /// A span of a source of `frames` (even, at least `MIN_SOURCE_FRAMES`):
    /// `bytes`' whole pairs from source frame `first` (even), cut at
    /// `frames`. Its taps clamp at its own ends, and read as a whole row's
    /// inside them. `None` for an odd `first`, or no pair held.
    pub fn span(
        bytes: &'a [u8],
        desc: Descriptor,
        first: u32,
        frames: u32,
    ) -> Option<SourceView<'a>> {
        let ok = first.is_multiple_of(2)
            && frames.is_multiple_of(2)
            && frames >= MIN_SOURCE_FRAMES
            && first < frames;
        let pairs = u32::try_from(bytes.len() / 3)
            .unwrap_or(u32::MAX)
            .min((frames - first.min(frames)) / 2);
        if !ok || pairs == 0 {
            return None;
        }
        Some(SourceView {
            samples: bytes.get(..3 * pairs as usize)?,
            desc,
            frames: NonZeroU32::new(frames)?,
            first,
            end: first + 2 * pairs,
            #[cfg(any(test, feature = "test-support"))]
            strict: false,
        })
    }

    /// The same span, checking every tap lands inside it (`strict`).
    #[cfg(any(test, feature = "test-support"))]
    pub fn strict_span(
        bytes: &'a [u8],
        desc: Descriptor,
        first: u32,
        frames: u32,
    ) -> Option<SourceView<'a>> {
        Self::span(bytes, desc, first, frames).map(|v| SourceView { strict: true, ..v })
    }

    /// Silence over a source of `frames`: what a streamed voice reads
    /// where nothing is held (its gain is 0 there).
    pub fn silent(desc: Descriptor, frames: NonZeroU32) -> SourceView<'static> {
        SourceView {
            samples: &[0; 3],
            desc,
            frames,
            first: 0,
            end: 2,
            #[cfg(any(test, feature = "test-support"))]
            strict: false,
        }
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

    /// The source's frames (a span's too).
    pub fn frames(&self) -> u32 {
        self.frames.get()
    }

    /// `frames`, never 0.
    pub fn file_frames(&self) -> NonZeroU32 {
        self.frames
    }

    /// The first frame held: 0 for a whole row.
    pub fn first(&self) -> u32 {
        self.first
    }

    /// One past the last frame held: `frames` for a whole row.
    pub fn end(&self) -> u32 {
        self.end
    }

    /// Sample `i` (clamped to the frames held), sign-extended, unscaled.
    #[inline(always)]
    pub fn sample(&self, i: u32) -> i32 {
        self.decode(i.max(self.first).min(self.end - 1))
    }

    /// Samples `i − 1 ..= i + 2` of a whole row (no span: STRETCH holds
    /// its source in RAM), unscaled, `i` held to `1 ..= end − 4` (the last
    /// frame reads as the one before it). The four are 48 bits of the
    /// stream from bit `12·(i − 1)`: two unaligned words, not four
    /// halfwords, each sample a shift up and one back.
    #[inline(always)]
    pub fn row_taps(&self, i: u32) -> [i32; 4] {
        debug_assert!(self.first == 0 && self.end >= 64);
        let j = (i.max(1).min(self.end - 4) - 1) as usize;
        #[cfg(any(test, feature = "test-support"))]
        assert!(!self.strict || (j + 3) < self.end as usize);
        let o = (3 * j) >> 1;
        let s = 4 * (j as u32 & 1);
        // SAFETY: `j ≤ end − 5` and `samples` is `3 · end / 2` bytes (a
        // whole row, `first` 0, `end` even), so `o + 6 ≤ (3 · end − 15) / 2
        // + 6 < 3 · end / 2`: both words, bytes `o ..= o + 6`, are inside
        // it. `read_unaligned` takes any alignment.
        let (a, b) = unsafe {
            let p = self.samples.as_ptr().add(o);
            (
                u32::from_le(core::ptr::read_unaligned(p.cast::<u32>())),
                u32::from_le(core::ptr::read_unaligned(p.add(3).cast::<u32>())),
            )
        };
        [
            ((a << (20 - s)) as i32) >> 20,
            ((a << (8 - s)) as i32) >> 20,
            ((b << (20 - s)) as i32) >> 20,
            ((b << (8 - s)) as i32) >> 20,
        ]
    }

    /// Sample `first ≤ i < end`. It is 12 bits at bit `12·(i − first)` of
    /// the stream: one unaligned halfword load at byte `3·(j >> 1) +
    /// (j & 1)`, `j = i − first`, shifted by 0 or 4 for its parity (`first`
    /// is even). The pair's three bytes hold both halfwords.
    #[inline(always)]
    fn decode(&self, i: u32) -> i32 {
        #[cfg(any(test, feature = "test-support"))]
        assert!(
            !self.strict || (self.first <= i && i < self.end),
            "tap {i} outside frames {}..{}",
            self.first,
            self.end
        );
        debug_assert!(self.first <= i && i < self.end);
        let j = (i - self.first) as usize;
        let o = (3 * j) >> 1; // 3·(j >> 1) + (j & 1)
        // SAFETY: every caller holds `first ≤ i ≤ end − 1` (`sample`,
        // `read` and `read_cycle` clamp to it), and `samples` is
        // `3 · (end − first) / 2` bytes, so `o + 1 ≤ 3 · (end − first) / 2
        // − 1`: both bytes are inside it. `read_unaligned` takes any
        // alignment.
        let h = unsafe { core::ptr::read_unaligned(self.samples.as_ptr().add(o).cast::<u16>()) };
        ((u16::from_le(h) as i32) << (20 - 4 * (j & 1))) >> 20
    }

    /// The source at `p`: taps `p.whole() − 1 ..= p.whole() + 2`, each
    /// clamped to the frames held (a whole past 2³¹ reads as below 0). A
    /// strict span checks them before the clamp, which only the source's
    /// own ends may take. Inside the frames held, one compare and no clamp
    /// (`read_edge` takes the rest, out of line).
    #[inline(always)]
    pub fn read(&self, p: Pos) -> f32 {
        let (w, lo, last) = (p.whole() as i32, self.first as i32, self.end as i32 - 1);
        #[cfg(any(test, feature = "test-support"))]
        assert!(
            !self.strict
                || ((lo == 0 || w.saturating_sub(1) >= lo)
                    && (self.end == self.frames.get() || w.saturating_add(2) <= last)),
            "read at {w} outside frames {lo}..{}",
            self.end
        );
        // `w − 1 − lo` wraps when `w` is under `lo + 1`; a span of under
        // four frames has no inside.
        if (w.wrapping_sub(lo + 1) as u32) < (last - lo - 2).max(0) as u32 {
            let at = |k: i32| self.decode((w + k) as u32);
            return hermite([at(-1), at(0), at(1), at(2)], p.frac());
        }
        self.read_edge(p)
    }

    /// `read` within two frames of the frames' ends: every tap clamped.
    #[cold]
    #[inline(never)]
    fn read_edge(&self, p: Pos) -> f32 {
        let (w, lo, last) = (p.whole() as i32, self.first as i32, self.end as i32 - 1);
        let tap = |k: i32| self.decode(w.saturating_add(k).max(lo).min(last) as u32);
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
            !self.strict || base as u64 + n as u64 <= u64::from(self.frames.get()),
            "frame {base} + {n} past {} frames",
            self.frames
        );
        let (w, mask, lo, last) = (phase.whole(), n.wrapping_sub(1), self.first, self.end - 1);
        let tap = |k: u32| {
            self.decode(
                base.wrapping_add(w.wrapping_add(k) & mask)
                    .max(lo)
                    .min(last),
            )
        };
        hermite([tap(u32::MAX), tap(0), tap(1), tap(2)], phase.frac())
    }
}
