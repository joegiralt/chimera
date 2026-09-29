//! MORPH between ALG A and ALG B (ADR 0024).

use crate::dsp::algo::math::inv_sqrt;
use crate::dsp::algo::plan::{EvalPlan, OPS, blend};

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Morph(f32);

impl Morph {
    pub const A: Morph = Morph(0.0);
    pub const B: Morph = Morph(1.0);
    pub const STORED_MAX: f32 = 127.0;

    /// A stored MORPH, or a modulated one (fractional, maybe out of range).
    pub fn from_param(v: f32) -> Self {
        Morph((v / Self::STORED_MAX).clamp(0.0, 1.0))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

/// The operators with a gain above 0: bit `i`, operator `i + 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sounding(u8);

impl Sounding {
    pub const ALL: Sounding = Sounding((1 << OPS) - 1);

    pub fn from_gains(gain: &[f32; OPS]) -> Self {
        Sounding((0..OPS).fold(0, |m, i| m | ((gain[i] > 0.0) as u8) << i))
    }

    pub fn has(self, op: usize) -> bool {
        self.0 & (1 << op) != 0
    }
}

/// The blended carrier gains of the sounding operators (#189: a silent
/// carrier adds nothing, so it takes no share of the output).
pub fn carrier_sum(plan: &EvalPlan, m: Morph, on: Sounding) -> f32 {
    (0..OPS)
        .filter(|&i| on.has(i))
        .map(|i| blend(plan.carrier_a[i], plan.carrier_b[i], m.get()))
        .sum()
}

/// Equal loudness for uncorrelated carriers, whatever their number.
pub fn carrier_norm(plan: &EvalPlan, m: Morph, on: Sounding) -> f32 {
    let sum = carrier_sum(plan, m, on);
    if sum <= 1.0 { 1.0 } else { inv_sqrt(sum) }
}

/// The modulation depth into `op` the mip choice allows for.
pub fn incoming(plan: &EvalPlan, m: Morph, op: usize, gain: &[f32; OPS]) -> f32 {
    plan.edges[..plan.edge_count()]
        .iter()
        .filter(|e| e.dst as usize == op)
        .map(|e| blend(e.a, e.b, m.get()) * gain[e.src as usize])
        .sum()
}
