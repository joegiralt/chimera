//! MST > LVL: the master VOL and PAN, the last gain on the DAC pairs. They
//! sit after the limiter (ADR 0109): the limiter sees the same signal at
//! any VOL, so its knee, its GR meter and the loudness reference do not
//! move with the fader, and both gains are at most 1, so nothing passes
//! the ceiling. At VOL 1 and PAN 0 the stage does nothing at all.
//!
//! The gains ease as a bus setting does (20 ms, a ramp across each block),
//! never snap.

use chimera_hal::BLOCK_SIZE;
use core::f32::consts::FRAC_PI_2;

use crate::dsp::clampf;
use crate::dsp::comp::CompParams;
use crate::dsp::ease::{Ease, ease_coeff, step_of};
use crate::hw::DAC_PAIRS;

/// VOL's gain: a square, so the fader is roughly even in loudness (half
/// travel is -12 dB) and 0 is exactly silent.
pub fn vol_gain(vol: f32) -> f32 {
    let v = if vol.is_nan() {
        1.0
    } else {
        clampf(vol, 0.0, 1.0)
    };
    v * v
}

/// PAN's (left, right) gains: a balance for an already-stereo bus. The
/// near side stays at unity, the far side falls along a quarter cosine, so
/// the centre is exact unity, the ends are exactly 0, and the sum of
/// squares falls smoothly between (a constant-power pan, centre -3 dB per
/// side, would dim the centre and exceed 1 off it).
pub fn balance(pan: f32) -> (f32, f32) {
    let p = if pan.is_nan() {
        0.0
    } else {
        clampf(pan, -1.0, 1.0)
    };
    let far = if p.abs() >= 1.0 {
        0.0
    } else {
        libm::cosf(p.abs() * FRAC_PI_2)
    };
    if p > 0.0 { (far, 1.0) } else { (1.0, far) }
}

#[derive(Clone, Copy, Debug)]
pub struct MasterOut {
    l: Ease,
    r: Ease,
    /// The pan `gains` was last made for; zero bits are centre.
    pan: u32,
    gains: (f32, f32),
}

impl Default for MasterOut {
    fn default() -> Self {
        Self::new()
    }
}

impl MasterOut {
    pub fn new() -> Self {
        Self {
            l: Ease::default(),
            r: Ease::default(),
            pan: 0,
            gains: (1.0, 1.0),
        }
    }

    /// Scale the block the DACs play by `p`'s VOL and PAN.
    pub fn apply(
        &mut self,
        (out, zero): (
            &mut [[f32; 2 * BLOCK_SIZE]; DAC_PAIRS],
            &mut [bool; DAC_PAIRS],
        ),
        p: &CompParams,
        sample_rate: u32,
    ) {
        // `balance` is two libm calls: only when the pan moves.
        if p.pan.to_bits() != self.pan {
            (self.pan, self.gains) = (p.pan.to_bits(), balance(p.pan));
        }
        let v = vol_gain(p.vol);
        let k = ease_coeff(sample_rate);
        let l = self.l.step(v * self.gains.0, k);
        let r = self.r.step(v * self.gains.1, k);
        if l == (1.0, 1.0) && r == (1.0, 1.0) {
            return;
        }
        let (sl, sr) = (step_of(l, BLOCK_SIZE), step_of(r, BLOCK_SIZE));
        // Steady, every sample's gain is `l.0 + 0`: of + sign, a +0 pair
        // stays +0 and is skipped (#793). Ramping, a gain can be −0's
        // maker: a pair scaled is no longer known +0.
        let keep = sl == 0.0
            && sr == 0.0
            && [l.0 + sl, r.0 + sr]
                .iter()
                .all(|g| g.is_finite() && g.is_sign_positive());
        for (pair, z) in out.iter_mut().zip(zero.iter_mut()) {
            if *z && keep {
                continue;
            }
            *z = false;
            let (mut gl, mut gr) = (l.0, r.0);
            for s in pair.as_chunks_mut::<2>().0 {
                gl += sl;
                gr += sr;
                s[0] *= gl;
                s[1] *= gr;
            }
        }
    }
}
