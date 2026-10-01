//! A glide in log₂, as an analogue portamento: a one-pole, stepped once a
//! block, so equal intervals take equal times (ADR 0062). It glides a
//! period (SYMP's halo) or a pitch ratio (a steal's glide).

use chimera_hal::BLOCK_SIZE;

use crate::hw::SAMPLE_RATE;

/// Nearer than this to its target, in octaves (0.12 cent), a glide lands:
/// a step's move is then near an `f32`'s rounding of the log.
const LANDED: f32 = 1e-4;

/// A value gliding to its target: its log₂ now and at the target, and the
/// share of the way it moves each block.
#[derive(Clone, Copy, Debug)]
pub struct Glide {
    now: f32,
    to: f32,
    step: f32,
    /// The value now: the target's own once landed.
    value: f32,
    target: f32,
}

impl Glide {
    /// Resting on `value`; a move takes `tau_s` to go 63 %.
    pub fn new(value: f32, tau_s: f32) -> Self {
        let l = libm::log2f(value);
        Self {
            now: l,
            to: l,
            step: 1.0 - libm::expf(-(BLOCK_SIZE as f32) / (tau_s * SAMPLE_RATE as f32)),
            value,
            target: value,
        }
    }

    /// The value now.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Where it is going.
    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn gliding(&self) -> bool {
        self.now != self.to
    }

    /// Glides to `value` from where it is.
    pub fn toward(&mut self, value: f32) {
        self.target = value;
        self.to = libm::log2f(value);
        if self.to == self.now {
            self.value = value;
        }
    }

    /// There at once.
    pub fn snap(&mut self) {
        (self.now, self.value) = (self.to, self.target);
    }

    /// A block on.
    pub fn tick(&mut self) {
        let d = self.to - self.now;
        if d.abs() < LANDED {
            self.snap();
        } else {
            self.now += self.step * d;
            self.value = libm::exp2f(self.now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fifth down takes τ to go 63 %, lands exactly, and then holds.
    #[test]
    fn a_glide_is_a_one_pole_in_log2() {
        let tau = 0.08;
        let mut g = Glide::new(100.0, tau);
        g.toward(150.0);
        let blocks = (tau * SAMPLE_RATE as f32 / BLOCK_SIZE as f32).round() as usize;
        for _ in 0..blocks {
            g.tick();
        }
        let share = (g.value() / 100.0).log2() / 1.5f32.log2();
        assert!((share - 0.632).abs() < 0.01, "{share}");
        for _ in 0..2000 {
            g.tick();
        }
        assert!(!g.gliding());
        assert_eq!(g.value(), 150.0);
    }
}
