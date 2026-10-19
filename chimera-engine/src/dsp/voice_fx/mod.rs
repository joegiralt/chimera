//! The voice's one FX slot, PRE or POST the filter (ADR 0155): its type,
//! place and pots (`params`).

pub mod fold;
pub mod params;
pub mod soft;

use params::pot_level;

/// A block's ramp ends, `(from, to)`: P1-P3, MIX and the stage gate. The
/// slot steps its ramps and hands the kernels these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ends {
    pub pots: [(f32, f32); 3],
    pub mix: (f32, f32),
    pub gate: (f32, f32),
}

impl Ends {
    /// P3's LEVEL gain at the block's ends: the wet's, exactly 1 at 1/2.
    #[inline]
    pub(crate) fn level(&self) -> (f32, f32) {
        let (a, b) = self.pots[2];
        (pot_level(a), pot_level(b))
    }

    /// Nothing moves and the gate is open: the steady loop's case.
    #[inline]
    pub(crate) fn steady(&self) -> bool {
        self.pots.iter().all(|p| p.0 == p.1) && self.mix.0 == self.mix.1 && self.gate == (1.0, 1.0)
    }
}

/// What the slot does around a kernel, for the kernels' tests: steps the
/// ramps and gate into `Ends`.
#[cfg(test)]
pub(crate) mod rig {
    use super::Ends;
    use crate::dsp::ease::{Gate, Ramp};
    use crate::hw::BLOCK_SIZE;

    #[derive(Default)]
    pub struct Rig(pub [Ramp; 3], pub Ramp, pub Gate);

    impl Rig {
        pub fn ends(&mut self, pots: [f32; 3], mix: f32, on: bool) -> Ends {
            Ends {
                pots: [0, 1, 2].map(|i| self.0[i].step(pots[i])),
                mix: self.1.step(mix),
                gate: self.2.step(on),
            }
        }
    }

    pub fn sine(b: usize, amp: f32, hz: f32) -> [f32; BLOCK_SIZE] {
        core::array::from_fn(|i| {
            let t = (b * BLOCK_SIZE + i) as f32 / 48_000.0;
            amp * libm::sinf(core::f32::consts::TAU * hz * t)
        })
    }
}
