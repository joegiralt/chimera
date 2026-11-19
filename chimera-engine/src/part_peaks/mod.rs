//! Each Part's live peak for MIXER › PRT's meters (ADR 0293): the most
//! its bus reached since the UI last looked, after its EQ and before its
//! LEVEL, PAN and sends, a DRUM pad's side counted (`|mid| + |side|`, the
//! louder of L and R). The audio raises it a block (`kern::run`, bank 2's
//! behind `door::PeakDoor`), only while MIXER › PRT is up (`watched`); the
//! UI takes it once a frame.

pub mod door;
pub mod kern;

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::hw::MAX_PARTS;

/// A bus's peak, linear full scale (1.0 is 0 dBFS; a Part's bus may pass
/// it before LEVEL): never negative, −0 or NaN, so its bits order as its
/// values (`PeakMeter`). An infinity is the largest finite value.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Peak(f32);

impl Peak {
    pub const ZERO: Peak = Peak(0.0);

    /// `v` held to 0..=`f32::MAX`; NaN and −0 are 0.
    #[inline(always)]
    pub fn new(v: f32) -> Peak {
        Peak(if v > 0.0 { v.min(f32::MAX) } else { 0.0 })
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

/// The most a Part's bus reached since the UI last looked: audio raises
/// it each block (`fetch_max`), the UI takes it once a frame (`swap(0)`),
/// so no one-block hit is missed between frames.
pub struct PeakMeter(AtomicU32);

impl PeakMeter {
    pub const fn new() -> Self {
        Self(AtomicU32::new(0))
    }

    /// Audio: at least `p` until the next `take`. One atomic op.
    #[inline(always)]
    pub fn raise(&self, p: Peak) {
        self.0.fetch_max(p.0.to_bits(), Ordering::Relaxed);
    }

    /// The UI: the most since the last `take`, and back to 0.
    #[inline(always)]
    pub fn take(&self) -> Peak {
        Peak(f32::from_bits(self.0.swap(0, Ordering::Relaxed)))
    }
}

impl Default for PeakMeter {
    fn default() -> Self {
        Self::new()
    }
}

/// Each Part's peak. Global, as `meter::MASTER_GR`: a test binary that
/// renders owns them.
pub static PART_PEAKS: [PeakMeter; MAX_PARTS] = [const { PeakMeter::new() }; MAX_PARTS];

static WATCHED: AtomicBool = AtomicBool::new(false);

/// The UI, each frame: whether MIXER › PRT's meters are up.
#[inline(always)]
pub fn watch(on: bool) {
    WATCHED.store(on, Ordering::Relaxed);
}

/// The audio: whether to measure this block.
#[inline(always)]
pub fn watched() -> bool {
    WATCHED.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_order_as_values() {
        let vals = [0.0, 1e-30, 0.001, 0.5, 1.0, 2.0, 1e30, f32::MAX];
        for w in vals.windows(2) {
            assert!(Peak::new(w[0]).0.to_bits() < Peak::new(w[1]).0.to_bits());
        }
        for v in [-0.0, -1.0, f32::NAN] {
            assert_eq!(Peak::new(v), Peak::ZERO);
        }
        assert_eq!(Peak::new(f32::INFINITY).get(), f32::MAX);
    }

    /// A side counts as `|mid| + |side|`, the louder of L and R; a Part
    /// with nothing written raises nothing. The one test of `PART_PEAKS`.
    #[test]
    fn the_pass_reads_mid_and_side() {
        use crate::hw::{BLOCK_SIZE, MAX_PARTS};
        let mut buses = [[0.0f32; BLOCK_SIZE]; MAX_PARTS];
        let mut sides: crate::instrument::Sides = [None; MAX_PARTS];
        buses[0][5] = -0.5;
        buses[1][7] = 0.5;
        let mut side = [0.0f32; BLOCK_SIZE];
        side[7] = -0.25;
        sides[1] = Some(side);
        buses[2][0] = 0.9;
        let written = [true, true, false, false, false, false];
        kern::run(&buses, &sides, &written);
        let got: [f32; MAX_PARTS] = core::array::from_fn(|p| PART_PEAKS[p].take().get());
        assert_eq!(got, [0.5, 0.75, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_meter_holds_its_most_until_taken() {
        let m = PeakMeter::new();
        m.raise(Peak::new(0.25));
        m.raise(Peak::new(0.75));
        m.raise(Peak::new(0.5));
        assert_eq!(m.take().get(), 0.75);
        assert_eq!(m.take(), Peak::ZERO);
    }
}
