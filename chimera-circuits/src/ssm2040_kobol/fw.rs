// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! EXP-C: the 2040 EXP voicing derived from the Kobol's own circuit
//! (`circuit`), in the firmware's 2040 structure: four TPT OTA stages, the
//! loop solved linearly in one step, f32, 1×. Every constant is a circuit
//! value or a quantity derived from one; none is fitted to a recording
//! (the one recorded value, CI5's offset, is `Offsets::UNIT`). Variables
//! are volts at each OUT pin with the 2040's inversions taken out
//! (`OUT_i = SGN_i·y_i`).
//!
//! Not shipped: the firmware's EXP kept its own structure and took ADR
//! 0191's make-up gain instead (`README.md`).

/// 2V_T at 27 °C.
const VT2: f32 = 0.051_73;
/// Node gains `(1/R)/ΣG`: stage 1 (R24 100 K, R27 10 K, R26 220 Ω) and
/// stages 2–4 (10 K, 10 K, 220 Ω).
const A1: f32 = 1.0e-4 / (1.0e-5 + 1.0e-4 + 1.0 / 220.0);
const A: f32 = 1.0e-4 / (2.0e-4 + 1.0 / 220.0);
/// Each cell's knee in volts of stage error, `2V_T/a`.
const VK: [f32; 4] = [VT2 / A1, VT2 / A, VT2 / A, VT2 / A];
/// Stage 1's pole over the others' (its own node).
pub const W1: f32 = A1 / A;
/// CI5's divider R38/(R37 + R38) and its knee at OUT 4, `2V_T/a₅`.
pub const L5: f32 = VT2 / (220.0 / 4920.0);
/// CI3's divider R40/(R39 + R40) and its knee, `2V_T/a₃`.
pub const L3: f32 = VT2 / (220.0 / 10_220.0);
/// The inversions: OUT 1 and OUT 3 are inverted.
const SGN: [f32; 4] = [-1.0, 1.0, -1.0, 1.0];
/// The swing, volts at an OUT pin, least-squares on the reference cell's
/// own DC curve at FREQ 1 kHz (`circuit::dc_cell`, 9–11 mV rms): above,
/// the cap clamp, bending from 0.70 over 0.38 (to 1.08); below, the pair's
/// collector saturation, from −0.80 over 0.31 (to −1.11). Both widen with
/// the cells' current, `SPREAD` a decade of FREQ (1.02/1.08/1.14 and
/// −1.06/−1.11/−1.17 at 100 Hz/1 kHz/10 kHz).
const HI_KNEE: f32 = 0.70;
const HI_BEND: f32 = 0.38;
const LO_KNEE: f32 = -0.80;
const LO_BEND: f32 = 0.31;
const SPREAD: f32 = 0.055;
/// C6 (4.7 µF) into R37 + R38 ∥ R39 + R40 (3.32 K): 10.2 Hz.
pub const F_C6: f32 = 10.2;
/// CI5's input bias current, `I_abc/2β` (β 150), into R37 ∥ R38 (210 Ω):
/// an offset that grows with `k`, volts a unit of `k`.
const BIAS5: f32 = (VT2 / (220.0 / 4920.0 * 1.0e4)) / 300.0 * 210.0;
/// RES at 1: CI5's I_abc at the Kobol's top (T3: 14.4 V/47 K + 3.4 V/10 K =
/// 646 µA) gives `k = I_abc·R27·a₅/2V_T` = 5.59.
pub const K_MAX: f32 = 5.59;
/// Stage 1's DC output for a full-scale input: the Kobol's full VCO1
/// volume, 2.8 V peak at R24 (calibrated from its own ring level), × R27/R24.
pub const E_FULL: f32 = 0.28;

/// The cells' and CI5's offsets, volts (data sheets: 2040 2 mV typical a
/// cell; 3080 0.4 mV plus its bias current, I_abc/2β, into R37 ∥ R38).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Offsets {
    pub cell: [f32; 4],
    pub ci5: f32,
}

impl Offsets {
    /// The same chip as the reference netlist (`Kobol::SCHEMATIC`).
    pub const REFERENCE: Offsets = Offsets {
        cell: [1.6e-3, -2.1e-3, 0.8e-3, 2.4e-3],
        ci5: 0.4e-3,
    };
    pub const NONE: Offsets = Offsets {
        cell: [0.0; 4],
        ci5: 0.0,
    };
    /// The test unit (`Kobol::UNIT`): CI5 at 1.5 mV (in spec: 0.4
    /// typical, 5 max), the one value read off the recording, from its
    /// ring's H2 (−39 dBc).
    pub const UNIT: Offsets = Offsets {
        cell: [1.6e-3, -2.1e-3, 0.8e-3, 2.4e-3],
        ci5: 1.5e-3,
    };
}

/// Divide-free tanh: odd, degree 7, tanh's own `u − u³/3` at low level
/// (so an OTA's H3 is right), C1 and flat from |u| = 2 at tanh(2);
/// monotonic, within 2.5 % to |u| = 1 and 3.5 % at 1.5 (3.8 % at its
/// worst, between). (The Prophet
/// lab's `ssm2040_prophet::fw::tanh_q` is the quintic, with 56 % of the
/// cubic term.)
#[inline(always)]
pub fn tanh_q(u: f32) -> f32 {
    const A5: f32 = 0.084_61;
    const A7: f32 = -0.008_413;
    let u = u.clamp(-2.0, 2.0);
    let u2 = u * u;
    u * (1.0 + u2 * (-1.0 / 3.0 + u2 * (A5 + u2 * A7)))
}

/// The C1 cubic, flat at 1 from 1.5: `c − 4c³/27`.
#[inline(always)]
fn cubic(c: f32) -> f32 {
    let c = c.min(1.5);
    c - (4.0 / 27.0) * c * c * c
}

/// One side's knee, bend and `1/bend`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Side {
    pub knee: f32,
    pub bend: f32,
    pub inv: f32,
}

/// The soft limit: `hi` above, `lo` below (knees signed as the side's).
#[inline(always)]
pub(crate) fn swing(y: f32, hi: Side, lo: Side) -> f32 {
    if y > hi.knee {
        hi.knee + hi.bend * cubic((y - hi.knee) * hi.inv)
    } else if y < lo.knee {
        lo.knee - lo.bend * cubic((lo.knee - y) * lo.inv)
    } else {
        y
    }
}

/// A non-inverting OUT pin's (upper, lower) sides at FREQ `fc`, physical
/// sign.
pub(crate) fn sides(fc: f32) -> (Side, Side) {
    let w = 1.0 + SPREAD * (fc.max(10.0) / 1000.0).log10();
    let side = |knee: f32, bend: f32| Side {
        knee: knee * w,
        bend: bend * w,
        inv: 1.0 / (bend * w),
    };
    (side(HI_KNEE, HI_BEND), side(LO_KNEE, LO_BEND))
}

#[derive(Clone, Debug)]
pub struct Exp2 {
    s: [f32; 4],
    /// C6's lowpass state: the DC the coupling holds back.
    z: f32,
    off: [f32; 4],
    o5: f32,
    /// Each stage's (upper, lower) side in its own (inversion-free) sign.
    lim: [(Side, Side); 4],
    /// FREQ the limits were last set for.
    lim_fc: f32,
    c6: f32,
    /// The last sample's stage outputs, physical sign (OUT pins).
    pub outs: [f32; 4],
}

impl Exp2 {
    pub fn new(o: Offsets, fs: f32) -> Self {
        Self {
            s: [0.0; 4],
            z: 0.0,
            off: std::array::from_fn(|i| SGN[i] * o.cell[i] / VT2),
            o5: o.ci5 / VT2,
            lim: Self::limits(1000.0),
            lim_fc: 1000.0,
            c6: 1.0 - (-2.0 * std::f32::consts::PI * F_C6 / fs).exp(),
            outs: [0.0; 4],
        }
    }

    /// The stages' sides at FREQ `fc`: OUT's clamp is the inverted stages'
    /// lower side.
    fn limits(fc: f32) -> [(Side, Side); 4] {
        let (up, down) = sides(fc);
        let flip = |s: Side| Side { knee: -s.knee, ..s };
        std::array::from_fn(|i| {
            if SGN[i] > 0.0 {
                (up, down)
            } else {
                (flip(down), flip(up))
            }
        })
    }

    /// Once a block: the limits follow FREQ.
    pub fn set_fc(&mut self, fc: f32) {
        if fc != self.lim_fc {
            self.lim = Self::limits(fc);
            self.lim_fc = fc;
        }
    }

    /// One sample: `x` the input (full scale 1), `g = tan(πf/fs)`, `k` the
    /// loop gain. Returns the output, unity at RES 0 in the passband.
    #[inline(always)]
    pub fn tick(&mut self, x: f32, g: f32, k: f32) -> f32 {
        let gs = [g * W1, g, g, g];
        let big: [f32; 4] = gs.map(|g| g / (1.0 + g));
        let [s1, s2, s3, s4] = self.s;
        let sum = (1.0 - big[3]) * s4
            + big[3]
                * ((1.0 - big[2]) * s3
                    + big[2] * ((1.0 - big[1]) * s2 + big[1] * (1.0 - big[0]) * s1));
        let gg = big[0] * big[1] * big[2] * big[3];
        let e = x * E_FULL;
        // The loop, linear, through C6's coupling, then CI5 on the
        // predicted OUT 4.
        let ul = (e - k * (sum - self.z)) / (1.0 + k * gg);
        let y4p = gg * ul + sum - self.z;
        let o5 = self.o5 + k * (BIAS5 / VT2);
        let mut u = e - k * L5 * tanh_q(y4p * (1.0 / L5) - o5);
        for i in 0..4 {
            let s = self.s[i];
            let v = (1.0 - big[i]) * (u - s);
            let th = tanh_q(v * (1.0 / VK[i]) + self.off[i]);
            let t = gs[i] * VK[i] * th;
            let (hi, lo) = self.lim[i];
            let y = swing(s + t, hi, lo);
            self.s[i] = y + t;
            self.outs[i] = SGN[i] * y;
            u = y;
        }
        let y4 = u - self.z;
        self.z += self.c6 * y4;
        // CI3: |x| ≤ 0.5 (OUT 4 ≤ 1.2 V), so tanh's cubic, no clamp.
        let x3 = y4 * (1.0 / L3);
        L3 * x3 * (1.0 - (1.0 / 3.0) * x3 * x3) * (1.0 / E_FULL)
    }

    /// `x` at 48 kHz, FREQ (Hz) and `k` per sample; the limits follow FREQ
    /// every 16 samples.
    pub fn render(
        &mut self,
        x: &[f64],
        fc: &dyn Fn(usize) -> f64,
        k: &dyn Fn(usize) -> f64,
    ) -> Vec<f64> {
        let fs = crate::FS;
        x.iter()
            .enumerate()
            .map(|(i, &v)| {
                if i % 16 == 0 {
                    self.set_fc(fc(i) as f32);
                }
                let g = (std::f64::consts::PI * fc(i).min(20_000.0) / fs).tan() as f32;
                f64::from(self.tick(v as f32, g, k(i) as f32))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tanh_q_is_tanh_where_the_otas_run() {
        // exp2.rs: "within 2.5 % to |u| = 1 and 3.5 % at 1.5" (3.84 % at
        // its worst between), tanh's own cubic, C1 and flat from 2 at
        // tanh(2), odd and monotonic.
        let mut prev = 0.0f32;
        for i in 1..=300 {
            let u = i as f32 / 100.0;
            let (q, t) = (tanh_q(u), u.tanh());
            assert_eq!(tanh_q(-u), -q, "odd at {u}");
            assert!(q >= prev, "monotonic at {u}");
            prev = q;
            let err = (q / t - 1.0).abs();
            if u <= 1.0 {
                assert!(err < 0.025, "{u}: {err}");
            } else if u < 1.5 {
                assert!(err < 0.0385, "{u}: {err}");
            } else if u == 1.5 {
                assert!(err < 0.035, "{u}: {err}");
            }
        }
        // u − u³/3 at low level: H3 of a small sine is tanh's.
        let u = 0.05f32;
        let cube = (u - tanh_q(u)) / (u * u * u);
        assert!((cube - 1.0 / 3.0).abs() < 0.01, "{cube}");
        // Flat from 2, at tanh(2) (0.964), and C1 there.
        assert_eq!(tanh_q(2.0), tanh_q(5.0));
        assert!((tanh_q(2.0) / 2f32.tanh() - 1.0).abs() < 0.005);
        let slope = (tanh_q(2.0) - tanh_q(1.99)) / 0.01;
        assert!(slope.abs() < 0.01, "{slope}");
    }

    #[test]
    fn the_swing_tops_out_where_the_cell_does() {
        // research.md § 3 A1 / exp2.rs: the cell tops out at +1.02/1.08/1.14
        // and −1.06/−1.11/−1.17 V at FREQ 100 Hz / 1 kHz / 10 kHz. The fit's
        // one SPREAD for both sides leaves the low side at 100 Hz 11 mV
        // short (−1.049).
        for (fc, hi, lo) in [
            (100.0, 1.02, -1.06),
            (1000.0, 1.08, -1.11),
            (10_000.0, 1.14, -1.17),
        ] {
            let (up, down) = sides(fc);
            let (a, b) = (swing(50.0, up, down), swing(-50.0, up, down));
            assert!((a - hi).abs() < 0.006, "{fc}: {a}");
            assert!((b - lo).abs() < 0.015, "{fc}: {b}");
            assert_eq!(swing(0.5, up, down), 0.5);
            assert_eq!(swing(-0.6, up, down), -0.6);
        }
    }
}
