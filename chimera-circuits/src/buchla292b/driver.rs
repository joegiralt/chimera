//! The 292B's LED driver: CV via R2 62k and the OFFSET pot via R3 62k into
//! IC1's − input; + input at Vb = 15·R7/(R6 + R7), R6 1 MΩ (rev. 7-76),
//! R7 10k. Feedback R4 15k from − to the LED string's top node, which has
//! R5 4.7k, C1 1 µF and D1 (1N5228B, 3.9 V) to ground; IC1 (741, 0/−24 V)
//! drives the string (two LEDs in series) through R13 1k. C1 across R5
//! differentiates the CV: a fast rise kicks the LEDs up to IC1's rail.

use crate::vtl5c3::{LED_IS, LED_VS, led_v};
use std::sync::OnceLock;

pub(crate) const CR2: f64 = 62e3;
pub(crate) const CR3: f64 = 62e3;
pub(crate) const CR4: f64 = 15e3;
pub(crate) const CR5: f64 = 4.7e3;
pub(crate) const CC1: f64 = 1e-6;
pub(crate) const CR13: f64 = 1e3;
pub(crate) const VB: f64 = 15.0 * 10e3 / (1e6 + 10e3);
/// IC1's lowest output (a 741 on 0/−24 V).
pub(crate) const VOUT_MIN: f64 = -22.5;
/// 1N5228B: 3.9 V at 20 mA; an exponential knee with V0 0.47 V fits both
/// sheet impedances (23 Ω at 20 mA, 1.9 kΩ at 0.25 mA).
pub(crate) const VZ: f64 = 3.9;
pub(crate) const IZT: f64 = 20e-3;
pub(crate) const VZK: f64 = 0.47;
/// LEDs in the string.
const N_LED: f64 = 2.0;

/// Zener current into the top node at top voltage v (≤ 0 conducts).
pub fn zener(v: f64) -> f64 {
    IZT * (((-v) - VZ) / VZK).exp() - IZT * (-VZ / VZK).exp()
}

fn zener_d(v: f64) -> f64 {
    IZT / VZK * (((-v) - VZ) / VZK).exp()
}

/// The top node voltage the loop sets for CV and OFFSET (volts).
pub fn v_req(cv: f64, off: f64) -> f64 {
    VB - CR4 * ((cv - VB) / CR2 + (off - VB) / CR3)
}

/// Current the passive load (R5, D1, R4 to node 2 at v2) feeds into the top.
fn load(v: f64, v2: f64) -> f64 {
    -v / CR5 + zener(v) + (v2 - v) / CR4
}

fn load_d(v: f64) -> f64 {
    -1.0 / CR5 - zener_d(v) - 1.0 / CR4
}

/// The most current IC1 can pull through the string with the top at v.
pub fn i_rail(v: f64) -> f64 {
    let mut i: f64 = 10e-3;
    for _ in 0..40 {
        let e = v - VOUT_MIN - N_LED * led_v(i) - i * CR13;
        let d = -N_LED * LED_VS / LED_IS - CR13; // rough slope, damped
        let ni = (i - e / d).max(0.0);
        if (ni - i).abs() < 1e-12 {
            break;
        }
        i = ni;
    }
    i.max(0.0)
}

/// Node 2's voltage with the loop open (IC1 saturated).
fn v2_open(cv: f64, off: f64, v: f64) -> f64 {
    (cv / CR2 + off / CR3 + v / CR4) / (1.0 / CR2 + 1.0 / CR3 + 1.0 / CR4)
}

/// The LED driver's state: the top node (C1's voltage) and the LED current.
#[derive(Clone, Copy, Debug)]
pub struct Driver {
    pub v: f64,
    pub i: f64,
}

impl Driver {
    /// Settled at OFFSET `off`, no CV.
    pub fn new(off: f64) -> Self {
        let mut d = Driver { v: 0.0, i: 0.0 };
        for _ in 0..2000 {
            d.step(0.0, off, 1e-4);
        }
        d
    }

    /// Backward Euler, h seconds, CV and OFFSET in volts. Sets the LED
    /// current, in its three regimes: closed loop, LEDs off, IC1 railed.
    pub fn step(&mut self, cv: f64, off: f64, h: f64) {
        let vr = v_req(cv, off);
        // Closed loop: the top sits at vr, the string carries the rest.
        let i = load(vr, VB) - CC1 * (vr - self.v) / h;
        if i >= 0.0 && i <= i_rail(vr) {
            (self.v, self.i) = (vr, i);
            return;
        }
        // Open: the LEDs off (i < 0) or IC1 at its rail.
        let railed = i > 0.0;
        let mut v = self.v;
        for _ in 0..60 {
            let il = if railed { i_rail(v) } else { 0.0 };
            let v2 = v2_open(cv, off, v);
            let r = CC1 * (v - self.v) / h - load(v, v2) + il;
            let dr = CC1 / h - load_d(v) + if railed { 1.0 / (CR13 + 50.0) } else { 0.0 };
            let nv = v - r / dr;
            if (nv - v).abs() < 1e-12 {
                v = nv;
                break;
            }
            v = nv;
        }
        // Railed, the top can't pass where the loop would hold it.
        if railed && v < vr {
            v = vr;
        }
        self.v = v;
        self.i = if railed { i_rail(v) } else { 0.0 };
    }
}

/// The steady LED current at CV and OFFSET (the driver settled).
pub fn static_current(cv: f64, off: f64) -> f64 {
    let mut d = Driver::new(off);
    for _ in 0..400 {
        d.step(cv, off, 1e-4);
    }
    d.i
}

/// The CV where the LEDs start (validation.md: ≈ 0.77 V).
pub fn threshold() -> f64 {
    let (mut lo, mut hi) = (0.0, 5.0);
    for _ in 0..40 {
        let m = 0.5 * (lo + hi);
        if static_current(m, 0.0) > 1e-7 {
            hi = m
        } else {
            lo = m
        }
    }
    hi
}

/// Volts a step of `static_table`.
pub const TABLE_DV: f64 = 0.25;
/// `static_table`'s top: the OFFSET pot's 15 V.
pub const TABLE_TOP: f64 = 15.0;

/// The steady LED current at OFFSET 0, 0.25, … 15 V, no CV (R2 = R3, so
/// the same at that CV, no OFFSET). Built once.
pub fn static_table() -> &'static [f64] {
    static T: OnceLock<Vec<f64>> = OnceLock::new();
    T.get_or_init(|| {
        let n = (TABLE_TOP / TABLE_DV) as usize;
        (0..=n)
            .map(|k| Driver::new(k as f64 * TABLE_DV).i)
            .collect()
    })
}
