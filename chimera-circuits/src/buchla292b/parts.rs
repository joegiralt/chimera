// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The 292's switch, its audio-path parts and the 292B's fixed stages.
//! Values from Buchla Associates drawing B21208 (10/12/70, rev. 7-76) and
//! Parker & D'Angelo, DAFx-13, Table 1; `docs/lpg/research.md` has each.

/// The 292's switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// FILT: C3 from the middle node to the buffer's output (Sallen-Key).
    Lowpass,
    /// The 292C's centre ("both"): C3 out, the gate shunt out. Not on the
    /// 292B; leaky (−20 dB at DC shut).
    Combo,
    /// GATE: C3 out, R14 15 kΩ across the output node, more input gain.
    Gate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Buffer {
    /// An op-amp follower, gain 1 (292C, Parker & D'Angelo).
    OpAmp,
    /// The 292B's Q1: an ITE4339 n-JFET source follower, R16 33 kΩ to −15 V.
    Jfet,
}

/// One unit's audio path.
#[derive(Clone, Copy, Debug)]
pub struct Parts {
    pub name: &'static str,
    /// Middle node to ground (292B C6, 292C C3; P&D's C2).
    pub c_mid: f64,
    /// Output node to ground (292B C4, 292C C5; P&D's C1).
    pub c_out: f64,
    /// FILT's feedback cap (292B C3, 292C C4; P&D's C3).
    pub c_fb: f64,
    /// Output node to ground, always in (292B R15; P&D's Rα).
    pub r_alpha: f64,
    /// GATE's shunt (292B R14).
    pub r_gate: f64,
    pub buffer: Buffer,
    /// The 292B's input attenuator + IC2 and the C5/IC3 output stage.
    pub stages: bool,
}

/// The 292B as drawn, with the 292C's COMBO position.
pub const B292: Parts = Parts {
    name: "292B",
    c_mid: 220e-12,
    c_out: 910e-12,
    c_fb: 4.7e-9,
    r_alpha: 2.2e6,
    r_gate: 15e3,
    buffer: Buffer::Jfet,
    stages: true,
};

/// Parker & D'Angelo's canonical circuit (Table 1), ideal buffer, no stages.
pub const PD13: Parts = Parts {
    name: "P&D Table 1",
    c_mid: 220e-12,
    c_out: 1e-9,
    c_fb: 4.7e-9,
    r_alpha: 5e6,
    r_gate: 5e3 * 5e6 / (5e6 - 5e3),
    buffer: Buffer::OpAmp,
    stages: false,
};

// 292B input: R8 15k in, R9 12k to ground in FILT (and COMBO, as the 292C
// raises gain for GATE only), C2 0.47 µF, R10 47k, IC2 × (1 + R11/R12).
pub(crate) const R8: f64 = 15e3;
pub(crate) const R9: f64 = 12e3;
pub(crate) const C2IN: f64 = 0.47e-6;
pub(crate) const R10: f64 = 47e3;
pub(crate) const IC2_GAIN: f64 = 1.0 + 33e3 / 68e3;
// Output: R16 33k to −15 V, C5 0.47 µF, R17 47k, IC3 × (1 + R19/R18) = 2.
pub(crate) const R16: f64 = 33e3;
pub(crate) const VNEG: f64 = -15.0;
pub(crate) const C5OUT: f64 = 0.47e-6;
pub(crate) const R17: f64 = 47e3;
pub(crate) const IC3_GAIN: f64 = 2.0;
/// Op-amp output swing on ±15 V (5558).
pub(crate) const RAIL: f64 = 13.5;
/// ITE4339 (≈ 2N4339) typicals: IDSS 1 mA, VP −1.2 V (sheet: 0.5–1.5 mA,
/// −0.6…−1.8 V).
pub(crate) const IDSS: f64 = 1.0e-3;
pub(crate) const VP: f64 = -1.2;

impl Mode {
    /// Conductance across the output node.
    pub fn shunt(self, p: &Parts) -> f64 {
        let ga = 1.0 / p.r_alpha;
        match self {
            Mode::Gate => ga + 1.0 / p.r_gate,
            _ => ga,
        }
    }

    /// The feedback cap in this mode (0 out).
    pub fn c_fb(self, p: &Parts) -> f64 {
        if self == Mode::Lowpass { p.c_fb } else { 0.0 }
    }

    /// The 292B input divider's Thevenin: (gain, resistance).
    pub fn thevenin(self) -> (f64, f64) {
        match self {
            Mode::Gate => (1.0, R8),
            _ => (R9 / (R8 + R9), R8 * R9 / (R8 + R9)),
        }
    }

    /// Input stage's high-frequency gain (the HP's passband).
    pub fn in_gain(self) -> f64 {
        let (k, rth) = self.thevenin();
        k * R10 / (R10 + rth) * IC2_GAIN
    }
}

/// An op-amp's output: a smooth limiter with a hard knee at the rail.
pub(crate) fn rail(x: f64) -> f64 {
    let r = x / RAIL;
    x / (1.0 + r.powi(8)).powf(0.125)
}
