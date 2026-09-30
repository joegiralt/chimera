//! Parameter easing (CLAUDE.md: never snap), two ways the codebase
//! already used: a bus setting (level, pan, send, an effect's MIX) by a
//! 20 ms one-pole once a block, as REV SEND; a voice setting, which a
//! route may move every block, by a ramp across the block, as CUTOFF's
//! `g` (#53). Either way the block ramps linearly from `from` to `to`.

use chimera_hal::BLOCK_SIZE;
use core::f32::consts::LOG2_E;

use crate::dsp::algo::math::exp2;

/// A bus setting's time constant, seconds.
pub const EASE_S: f32 = 0.02;

/// The one-pole's coefficient once a block at `EASE_S`.
pub fn ease_coeff(sample_rate: u32) -> f32 {
    exp2(-LOG2_E * BLOCK_SIZE as f32 / (EASE_S * sample_rate as f32))
}

/// A bus setting eased toward its target, a block at a time. Zero bytes
/// are the unprimed state: its first block lands on the target.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ease {
    v: f32,
    primed: bool,
}

/// Closer than this to the target, an ease lands on it: −100 dB of a
/// unit gain.
const LAND: f32 = 1e-5;

impl Ease {
    /// This block's `(from, to)` toward `target`, `k` from `ease_coeff`.
    #[inline]
    pub fn step(&mut self, target: f32, k: f32) -> (f32, f32) {
        let from = if self.primed { self.v } else { target };
        let mut to = target + k * (from - target);
        if (to - target).abs() < LAND {
            to = target;
        }
        (self.v, self.primed) = (to, true);
        (from, to)
    }

    /// Lands on `target` now: for a setting nothing can hear move.
    pub fn land(&mut self, target: f32) {
        (self.v, self.primed) = (target, true);
    }
}

/// A voice setting's last block value: the next block ramps from it. A
/// fresh note (`hold`) starts on its value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ramp(Option<f32>);

impl Ramp {
    /// This block's `(from, to)`.
    #[inline]
    pub fn step(&mut self, v: f32) -> (f32, f32) {
        (self.0.replace(v).unwrap_or(v), v)
    }

    pub fn hold(&mut self) {
        self.0 = None;
    }
}

/// Blocks a stage's on/off gate takes: 20 ms at 48 kHz, as `EASE_S`.
pub const GATE_BLOCKS: u8 = 15;

/// A stage's on/off, faded linearly over `GATE_BLOCKS` blocks so a fixed
/// 20 ms is all it runs past its off (and is billed off). Zero bytes are
/// unprimed: the first block lands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gate {
    /// Blocks of the fade open, `0..=GATE_BLOCKS`.
    open: u8,
    primed: bool,
}

impl Gate {
    /// This block's `(from, to)` of the wet's share.
    #[inline]
    pub fn step(&mut self, on: bool) -> (f32, f32) {
        let from = if self.primed {
            self.open
        } else if on {
            GATE_BLOCKS
        } else {
            0
        };
        self.open = if on {
            (from + 1).min(GATE_BLOCKS)
        } else {
            from.saturating_sub(1)
        };
        self.primed = true;
        let share = |n: u8| f32::from(n) / f32::from(GATE_BLOCKS);
        (share(from), share(self.open))
    }
}

/// Sample `i` of a block's ramp from `from` by `step` a sample: the last
/// lands on `to`.
#[inline(always)]
pub fn at(from: f32, step: f32, i: usize) -> f32 {
    from + step * (i + 1) as f32
}

/// The per-sample step of a ramp over `len` samples (a block).
#[inline(always)]
pub fn step_of((from, to): (f32, f32), len: usize) -> f32 {
    (to - from) / len as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ease_lands_first_then_glides() {
        let k = ease_coeff(48_000);
        let mut e = Ease::default();
        assert_eq!(e.step(0.5, k), (0.5, 0.5));
        let (from, to) = e.step(1.0, k);
        assert_eq!(from, 0.5);
        assert!(to > 0.5 && to < 1.0);
        // 20 ms is 15 blocks: within 1 % after 5 time constants.
        let mut last = to;
        for _ in 0..75 {
            last = e.step(1.0, k).1;
        }
        assert!((1.0 - last).abs() < 0.01, "{last}");
        for _ in 0..200 {
            last = e.step(1.0, k).1;
        }
        assert_eq!(last, 1.0);
    }

    #[test]
    fn a_gate_lands_first_then_fades_in_15_blocks() {
        let mut g = Gate::default();
        assert_eq!(g.step(true), (1.0, 1.0));
        let mut last = 1.0;
        for _ in 0..GATE_BLOCKS {
            last = g.step(false).1;
        }
        assert_eq!(last, 0.0);
        assert_eq!(g.step(false), (0.0, 0.0));
        assert_eq!(g.step(true).1, 1.0 / f32::from(GATE_BLOCKS));
    }

    #[test]
    fn a_ramp_starts_on_a_fresh_value() {
        let mut r = Ramp::default();
        assert_eq!(r.step(0.3), (0.3, 0.3));
        assert_eq!(r.step(0.7), (0.3, 0.7));
        r.hold();
        assert_eq!(r.step(0.1), (0.1, 0.1));
        assert_eq!(
            at(0.3, step_of((0.3, 0.7), BLOCK_SIZE), BLOCK_SIZE - 1),
            0.7
        );
    }
}
