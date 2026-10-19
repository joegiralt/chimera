//! What the audio thread shows the UI besides the scope: one atomic word
//! per meter, written once per block (it never blocks), read whenever the
//! UI draws. Each meter has its own type, so a dB never lands in a clip's.

use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use crate::project::PartSet;

use crate::dsp::rev::sync::{FoldState, Tempo};

/// A level in dB, held as its bits: the last block's, a relaxed store.
pub struct DbMeter(AtomicU32);

impl DbMeter {
    pub const fn new() -> Self {
        Self(AtomicU32::new(0))
    }

    pub fn publish(&self, db: f32) {
        self.0.store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn read(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

impl Default for DbMeter {
    fn default() -> Self {
        Self::new()
    }
}

/// How hard a clipper bit, 0..=1: 0 below its knee, 1 full. Never
/// negative, −0 or NaN, so its bits order as its values (`ClipMeter`).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Clip(f32);

impl Clip {
    pub const ZERO: Clip = Clip(0.0);

    /// `v` held to 0..=1; NaN and −0 are 0.
    #[inline(always)]
    pub fn new(v: f32) -> Clip {
        Clip(if v > 0.0 { v.min(1.0) } else { 0.0 })
    }

    pub fn get(self) -> f32 {
        self.0
    }

    #[inline(always)]
    pub fn max(self, o: Clip) -> Clip {
        if o.0 > self.0 { o } else { self }
    }
}

/// The most a clipper bit since the UI last looked: audio raises it each
/// block (`fetch_max`), the UI takes it once a frame (`swap(0)`), so no
/// one-block clip is missed between frames.
pub struct ClipMeter(AtomicU32);

impl ClipMeter {
    pub const fn new() -> Self {
        Self(AtomicU32::new(0))
    }

    /// Audio: at least `c` until the next `take`. One atomic op.
    pub fn raise(&self, c: Clip) {
        self.0.fetch_max(c.0.to_bits(), Ordering::Relaxed);
    }

    /// The UI: the most since the last `take`, and back to 0.
    pub fn take(&self) -> Clip {
        Clip(f32::from_bits(self.0.swap(0, Ordering::Relaxed)))
    }
}

impl Default for ClipMeter {
    fn default() -> Self {
        Self::new()
    }
}

/// The master compressor's gain reduction, dB (0 while it is bypassed).
///
/// Global: tests run in parallel threads, so at most one test per test
/// binary may publish a nonzero value or render MST expecting one.
pub static MASTER_GR: DbMeter = DbMeter::new();

/// Each Part's filter LEVEL clip: the most any of its voices clipped since
/// the UI last took it (`Voice::filter_clip`). Global, as `MASTER_GR`.
pub static FILTER_CLIP: [ClipMeter; crate::hw::MAX_PARTS] =
    [const { ClipMeter::new() }; crate::hw::MAX_PARTS];

/// The Parts with a voice sounding, as the last block left them: the
/// header's dots. Global, as `MASTER_GR`.
static SOUNDING_PARTS: AtomicU8 = AtomicU8::new(0);

pub fn publish_sounding_parts(s: PartSet) {
    SOUNDING_PARTS.store(s.bits(), Ordering::Relaxed);
}

pub fn sounding_parts() -> PartSet {
    PartSet::from_bits(SOUNDING_PARTS.load(Ordering::Relaxed))
}

/// The tempo the reverb follows and its synced time's fold, in one word:
/// tempo × 4 (its 0.25 BPM grid) in the low 16 bits, the fold above. What
/// a synced cell shows. Global as `MASTER_GR`: only a test binary whose
/// renders keep 120 BPM may draw synced cells.
static REV_CLOCK: AtomicU32 = AtomicU32::new(0);

pub fn publish_rev_clock(t: Tempo, f: FoldState) {
    REV_CLOCK.store(clock_word(t, f), Ordering::Relaxed);
}

/// `t` × 4 in bits 0–15, `f` in bits 16–23: the published word, and what
/// a synced cell's redraw is keyed on.
pub fn clock_word(t: Tempo, f: FoldState) -> u32 {
    (libm::roundf(t.0 * 4.0) as u32 & 0xFFFF) | (f.0 as u8 as u32) << 16
}

/// The last published clock; 120 BPM, unfolded, before any.
pub fn rev_clock() -> (Tempo, FoldState) {
    let w = REV_CLOCK.load(Ordering::Relaxed);
    let q = w & 0xFFFF;
    let t = if q == 0 {
        Tempo::DEFAULT
    } else {
        Tempo(q as f32 / 4.0)
    };
    (t, FoldState((w >> 16) as u8 as i8))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bits order as values over `Clip`'s range, so `fetch_max` keeps the
    /// most; `take` empties it.
    #[test]
    fn a_clip_meter_keeps_the_most_until_taken() {
        let m = ClipMeter::new();
        for v in [0.3, 0.7, 0.0, 0.5] {
            m.raise(Clip::new(v));
        }
        assert_eq!(m.take().get(), 0.7);
        assert_eq!(m.take(), Clip::ZERO);
        assert_eq!(Clip::new(-0.0).get().to_bits(), 0);
        assert_eq!(Clip::new(f32::NAN), Clip::ZERO);
        assert_eq!(Clip::new(3.0).get(), 1.0);
        let mut last = 0u32;
        for i in 0..=1000 {
            let b = Clip::new(i as f32 / 1000.0).get().to_bits();
            assert!(b >= last);
            last = b;
        }
    }
}
