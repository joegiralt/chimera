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

/// What a source holds: one sample, a transwave's frames, or TRON's zones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Shape {
    Plain = 0,
    Transwave = 1,
    ZoneSet = 2,
}

impl Shape {
    pub fn from_code(c: u8) -> Option<Shape> {
        match c {
            0 => Some(Shape::Plain),
            1 => Some(Shape::Transwave),
            2 => Some(Shape::ZoneSet),
            _ => None,
        }
    }

    /// The largest `count` this shape takes.
    fn max_count(self) -> u8 {
        match self {
            Shape::Plain => 1,
            Shape::Transwave => 32,
            Shape::ZoneSet => 24,
        }
    }
}

/// The descriptor's bytes at the start of `held`.
pub const DESC_LEN: usize = 16;
/// One zone's entry in a `ZoneSet`'s layout, after the descriptor.
pub const ZONE_LAYOUT_LEN: usize = 9;

/// `held`'s first 16 B: `shape`, `count`, `rate` (LE), `head_frames` (LE,
/// 0 when held whole), then 8 reserved zero bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub shape: Shape,
    pub count: u8,
    pub rate: u16,
    pub head_frames: u32,
}

impl Descriptor {
    pub fn to_bytes(&self) -> [u8; DESC_LEN] {
        let mut b = [0; DESC_LEN];
        (b[0], b[1]) = (self.shape as u8, self.count);
        b[2..4].copy_from_slice(&self.rate.to_le_bytes());
        b[4..8].copy_from_slice(&self.head_frames.to_le_bytes());
        b
    }

    /// `None` for a short slice, an unknown shape, a count outside its
    /// shape's `1..=max` (a `Plain` holds exactly 1), rate 0, or a set
    /// reserved byte.
    pub fn from_bytes(b: &[u8]) -> Option<Descriptor> {
        let b: &[u8; DESC_LEN] = b.get(..DESC_LEN)?.try_into().ok()?;
        let shape = Shape::from_code(b[0])?;
        let d = Descriptor {
            shape,
            count: b[1],
            rate: u16::from_le_bytes([b[2], b[3]]),
            head_frames: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        };
        let ok = (1..=shape.max_count()).contains(&d.count)
            && d.rate != 0
            && b[8..].iter().all(|&x| x == 0);
        ok.then_some(d)
    }

    /// Where the samples start in `held`: after a `ZoneSet`'s layout.
    fn data_start(&self) -> usize {
        match self.shape {
            Shape::ZoneSet => DESC_LEN + ZONE_LAYOUT_LEN * self.count as usize,
            _ => DESC_LEN,
        }
    }
}

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
    /// Exactly `held`.
    bytes: &'a [u8],
    desc: Descriptor,
    /// Where the samples start in `bytes`.
    data: usize,
    frames: u32,
    /// Panic on a tap outside `[0, frames)` rather than clamp it.
    #[cfg(any(test, feature = "test-support"))]
    strict: bool,
}

impl<'a> SourceView<'a> {
    /// `None` if the descriptor is bad, or `held` holds no whole pair of
    /// samples after it. The samples are `2 · (data_len / 3)` frames: an
    /// odd source reads one silent frame at its end.
    pub fn new(held: &'a [u8]) -> Option<SourceView<'a>> {
        let desc = Descriptor::from_bytes(held)?;
        let data = desc.data_start();
        let pairs = held.len().checked_sub(data)? / 3;
        let frames = u32::try_from(2 * pairs).ok().filter(|&f| f > 0)?;
        Some(SourceView {
            bytes: held,
            desc,
            data,
            frames,
            #[cfg(any(test, feature = "test-support"))]
            strict: false,
        })
    }

    /// The same view, panicking on any tap outside `[0, frames)` before the
    /// decode's own clamp would hide it: the models' bounds, checked.
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

    /// Sample `i` (clamped to the last frame), sign-extended, unscaled. A
    /// sample is 12 bits at bit `12·i` of the stream: one halfword load at
    /// byte `3·(i >> 1) + (i & 1)`, shifted by 0 or 4 for its parity. The
    /// pair's three bytes hold both halfwords, so it never reads past them.
    #[inline]
    pub fn sample(&self, i: u32) -> i32 {
        let i = i.min(self.frames - 1) as usize;
        let o = self.data + 3 * (i >> 1) + (i & 1);
        let h = u16::from_le_bytes([self.bytes[o], self.bytes[o + 1]]) as i32;
        ((h >> (4 * (i & 1))) << 20) >> 20
    }

    #[inline]
    fn tap(&self, j: u32) -> i32 {
        #[cfg(any(test, feature = "test-support"))]
        assert!(
            !self.strict || j < self.frames,
            "tap {j} outside {} frames",
            self.frames
        );
        self.sample(j)
    }

    /// The source at `p`: taps `p.whole() − 1 ..= p.whole() + 2`, each
    /// clamped to `[0, frames − 1]` (a whole past 2³¹ reads as below 0).
    #[inline]
    pub fn read(&self, p: Pos) -> f32 {
        let (w, last) = (p.whole() as i32, self.frames as i32 - 1);
        let tap = |k: i32| self.tap(w.saturating_add(k).max(0).min(last) as u32);
        hermite([tap(-1), tap(0), tap(1), tap(2)], p.frac())
    }

    /// A transwave frame of `N = 2^log2_n` samples from `base`, at `phase`:
    /// the taps wrap `& (N − 1)` inside `[base, base + N)`.
    #[inline]
    pub fn read_cycle(&self, base: u32, log2_n: u32, phase: Pos) -> f32 {
        let (w, mask) = (phase.whole(), (1u32 << (log2_n & 31)).wrapping_sub(1));
        let tap = |k: u32| self.tap(base.wrapping_add(w.wrapping_add(k) & mask));
        hermite([tap(u32::MAX), tap(0), tap(1), tap(2)], phase.frac())
    }
}
