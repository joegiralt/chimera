//! A Sound's stereo (ADR 0294): OUT PAN places each note of ALGO, MODAL,
//! VA and TAPE in its Part's stereo (`VoicePan`, a per-voice destination,
//! split into the Part's mid and side as a DRUM pad's PAN is), and WIDTH
//! scales each Part's side (`Width`, eased a block at a time in
//! `PartWidths`). Both run in bank 2 (`kern`, an S4 group) behind
//! `door::StereoDoor`; a closed door plays every note centred and every
//! Part as panned.

pub mod door;
pub mod kern;

use crate::dsp::clampf;
use crate::hw::MAX_PARTS;
use crate::project::PartId;

/// A note's place, −1 (left) ..= 1 (right), 0 the centre: OUT PAN as
/// stored (`OutParams::pan`) and as the voice plays it, its routes on. NaN
/// is the centre. Transparent: the Part's knobs read it as the `f32` it is.
#[derive(Clone, Copy, Default, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct VoicePan(f32);

/// Its value alone, as the other OUT values print.
impl core::fmt::Debug for VoicePan {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&self.0, f)
    }
}

impl VoicePan {
    pub const CENTRE: VoicePan = VoicePan(0.0);

    #[inline(always)]
    pub const fn new(v: f32) -> VoicePan {
        VoicePan(if v.is_nan() {
            0.0
        } else {
            clampf(v, -1.0, 1.0)
        })
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    /// Off centre: a split to pay for.
    #[inline(always)]
    pub fn off_centre(self) -> bool {
        self.0 != 0.0
    }

    /// Its gains on the Part's mid and side: `(cos θ, sin θ)`, θ = pan ·
    /// π/4, the constant-power law of the Part's own PAN and a pad's
    /// (`MidSide::of`): left `mid − side` is `√2·sin((1 − pan)·π/4)`,
    /// right `√2·sin((1 + pan)·π/4)`. Polynomials, not `sinf` (soft-float
    /// f64 on this FPU): within 4e-7 of both over the range, and exactly
    /// (1, 0) at the centre.
    #[inline(always)]
    pub fn gains(self) -> MidSideGains {
        let t = self.0 * core::f32::consts::FRAC_PI_4;
        let t2 = t * t;
        // Taylor to t⁷ and t⁸: |t| ≤ π/4, so the next terms are under
        // 4e-7 and 3e-8.
        let sin = t * (1.0 - t2 / 6.0 * (1.0 - t2 / 20.0 * (1.0 - t2 / 42.0)));
        let cos = 1.0 - t2 / 2.0 * (1.0 - t2 / 12.0 * (1.0 - t2 / 30.0 * (1.0 - t2 / 56.0)));
        MidSideGains {
            mid: cos,
            side: sin,
        }
    }
}

/// A note's gains on its Part's mid (its mono bus) and side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidSideGains {
    pub mid: f32,
    pub side: f32,
}

/// A Part's WIDTH, its side's gain: 0 (mono) ..= 2, 1 (as panned, the
/// default) at the knob's middle. Above 1 the side passes the pans that
/// made it; the mono sum (left + right, the mid twice) never moves.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Width(f32);

impl Default for Width {
    fn default() -> Self {
        Self::UNITY
    }
}

/// Its value alone, as the other OUT values print.
impl core::fmt::Debug for Width {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&self.get(), f)
    }
}

impl Width {
    pub const MONO: Width = Width(0.0);
    pub const UNITY: Width = Width(1.0);
    pub const MAX: Width = Width(2.0);

    /// `v` held to 0..=2; NaN is unity.
    pub const fn new(v: f32) -> Width {
        Width(if v.is_nan() { 1.0 } else { clampf(v, 0.0, 2.0) })
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Each Part's WIDTH as its side last played it, eased toward the stored
/// one a block at a time (`kern::widths`): unity at rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartWidths([Width; MAX_PARTS]);

impl PartWidths {
    pub const REST: PartWidths = PartWidths([Width::UNITY; MAX_PARTS]);

    /// Every Part's side as panned, last block and this (`to`, the
    /// stored WIDTHs): nothing to do, every side bit for bit.
    #[inline(always)]
    pub fn at_rest(&self, to: impl Fn(PartId) -> Width) -> bool {
        PartId::ALL
            .iter()
            .zip(&self.0)
            .all(|(&p, &w)| w == Width::UNITY && to(p) == Width::UNITY)
    }

    /// Part `p`'s, as its side last played it.
    pub fn at(&self, p: PartId) -> Width {
        self.0[p.index()]
    }
}

impl Default for PartWidths {
    fn default() -> Self {
        Self::REST
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The polynomials against the Part PAN's `sinf` law, across the knob.
    #[test]
    fn the_law_is_the_parts_constant_power_one() {
        for i in -64..=64 {
            let pan = i as f32 / 64.0;
            let g = VoicePan::new(pan).gains();
            let q = core::f32::consts::FRAC_PI_4;
            let k = core::f32::consts::SQRT_2;
            let (l, r) = (
                k * libm::sinf((1.0 - pan) * q),
                k * libm::sinf((1.0 + pan) * q),
            );
            assert!((g.mid - (l + r) / 2.0).abs() < 1e-6, "{pan}: mid {}", g.mid);
            assert!(
                (g.side - (r - l) / 2.0).abs() < 1e-6,
                "{pan}: side {}",
                g.side
            );
            // Constant power: L² + R² = 2 (mid² + side²) = 2.
            assert!((g.mid * g.mid + g.side * g.side - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn the_centre_is_exactly_the_bus() {
        let g = VoicePan::CENTRE.gains();
        assert_eq!((g.mid.to_bits(), g.side.to_bits()), (1.0f32.to_bits(), 0));
        assert!(!VoicePan::new(f32::NAN).off_centre());
        assert_eq!(VoicePan::new(3.0).get(), 1.0);
        assert_eq!(VoicePan::new(-3.0).get(), -1.0);
    }

    /// Hard right: left is silent, right the bus's √2.
    #[test]
    fn hard_right_leaves_the_left_silent() {
        let g = VoicePan::new(1.0).gains();
        assert!((g.mid - g.side).abs() < 1e-6);
        assert!((g.mid + g.side - core::f32::consts::SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn width_holds_its_range() {
        assert_eq!(Width::new(-1.0), Width::MONO);
        assert_eq!(Width::new(9.0), Width::MAX);
        assert_eq!(Width::new(f32::NAN), Width::UNITY);
        assert_eq!(Width::default(), Width::UNITY);
        assert!(PartWidths::REST.at_rest(|_| Width::UNITY));
        let p4 = PartId::ALL[3];
        assert!(!PartWidths::REST.at_rest(|p| if p == p4 { Width::MAX } else { Width::UNITY }));
        assert_eq!(PartWidths::REST.at(p4), Width::UNITY);
        let mut w = PartWidths::REST;
        w.0[2] = Width::MONO;
        assert!(!w.at_rest(|_| Width::UNITY), "still easing");
    }
}
