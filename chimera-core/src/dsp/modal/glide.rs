//! A glide in log-period, as an analogue portamento: a one-pole, stepped
//! once a block, so equal intervals take equal times (ADR 0062).

use chimera_hal::BLOCK_SIZE;

use crate::hw::SAMPLE_RATE;

/// Nearer than this to its target, in octaves (0.12 cent), a glide lands:
/// a step's move is then near an `f32`'s rounding of the log.
const LANDED: f32 = 1e-4;

/// A period gliding to its target: its log₂ now and at the target, and the
/// share of the way it moves each block.
#[derive(Clone, Copy, Debug)]
pub struct Glide {
    now: f32,
    to: f32,
    step: f32,
    /// The period now, samples: the target's own value once landed.
    period: f32,
    target: f32,
}

impl Glide {
    /// Resting on `period` samples; a move takes `tau_s` to go 63 %.
    pub fn new(period: f32, tau_s: f32) -> Self {
        let l = libm::log2f(period);
        Self {
            now: l,
            to: l,
            step: 1.0 - libm::expf(-(BLOCK_SIZE as f32) / (tau_s * SAMPLE_RATE as f32)),
            period,
            target: period,
        }
    }

    /// The period now, samples.
    pub fn period(&self) -> f32 {
        self.period
    }

    /// Where it is going, samples.
    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn gliding(&self) -> bool {
        self.now != self.to
    }

    /// Glides to `period` from where it is.
    pub fn toward(&mut self, period: f32) {
        self.target = period;
        self.to = libm::log2f(period);
        if self.to == self.now {
            self.period = period;
        }
    }

    /// There at once.
    pub fn snap(&mut self) {
        (self.now, self.period) = (self.to, self.target);
    }

    /// A block on.
    pub fn tick(&mut self) {
        let d = self.to - self.now;
        if d.abs() < LANDED {
            self.snap();
        } else {
            self.now += self.step * d;
            self.period = libm::exp2f(self.now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fifth down takes τ to go 63 %, lands exactly, and then holds.
    #[test]
    fn a_glide_is_a_one_pole_in_log_period() {
        let tau = 0.08;
        let mut g = Glide::new(100.0, tau);
        g.toward(150.0);
        let blocks = (tau * SAMPLE_RATE as f32 / BLOCK_SIZE as f32).round() as usize;
        for _ in 0..blocks {
            g.tick();
        }
        let share = (g.period() / 100.0).log2() / 1.5f32.log2();
        assert!((share - 0.632).abs() < 0.01, "{share}");
        for _ in 0..2000 {
            g.tick();
        }
        assert!(!g.gliding());
        assert_eq!(g.period(), 150.0);
    }
}
