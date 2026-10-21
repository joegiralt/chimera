//! The 292B in a firmware's shape: f32 at 48 kHz. The LED driver and the
//! vactrol (`crate::vtl5c3::fw`) once every 16 samples; the audio core as
//! Parker & D'Angelo's topology-preserving trapezoidal solve (DAFx-13
//! §2.2.2), five coefficients ramped across the 16; the follower
//! linearised (0.957), two one-pole 7 Hz couplings and a cubic rail at IC3.

use super::VREF;
use super::core::Core;
use super::driver::{CC1, CR2, CR3, CR4, CR5, CR13, Driver, IZT, VB, VOUT_MIN, VZ, VZK};
use super::parts::{C2IN, C5OUT, IC3_GAIN, Mode, Parts, R10, R17, RAIL};
use crate::FS;
use crate::vtl5c3::fw::{FwLight, Vac32, exp2f};
use crate::vtl5c3::{Light, R_DARK};

/// Updates every this many samples.
pub const SUB: usize = 16;

/// The firmware-shaped unit.
#[derive(Clone, Debug)]
pub struct Fw {
    light: FwLight,
    in_gain: f32,
    out_gain: f32,
    // driver
    v: f32,
    /// The LED current, the last sub-block's (A).
    pub i: f32,
    f: f32,
    fk: f32,
    tk: f32,
    cv_last: f64,
    fs_last: f32,
    is_last: f32,
    vac: Vac32,
    // core
    s1: f32,
    s2: f32,
    c: [f32; 5],
    dc: [f32; 5],
    // the two 7 Hz couplings, one-pole HPs
    hp_in: f32,
    hp_out: f32,
    k_in: f32,
    k_out: f32,
    // constants of the mode
    wx: f32,
    wo: f32,
    ga: f32,
    k: f32,
    /// The LDR's conductance, the last sub-block's (S).
    pub g: f32,
}

impl Fw {
    pub fn new(parts: &Parts, mode: Mode, light: &Light) -> Self {
        let a = Core::follower_gain(parts) as f32;
        let h = 1.0 / FS as f32;
        let cfb = mode.c_fb(parts) as f32;
        let cx = (parts.c_mid as f32) + cfb;
        let c1 = parts.c_out as f32;
        let (k_in, k_out) = if parts.stages {
            let (_, rth) = mode.thevenin();
            let t_in = (C2IN * (rth + R10)) as f32;
            let t_out = (C5OUT * R17) as f32; // source impedance ≈ 1/gm, small
            ((-h / t_in).exp(), (-h / t_out).exp())
        } else {
            (1.0, 1.0)
        };
        let mut f = Fw {
            light: FwLight::new(light),
            in_gain: if parts.stages {
                mode.in_gain() as f32
            } else {
                1.0
            },
            out_gain: if parts.stages { a * IC3_GAIN as f32 } else { a },
            v: 0.0,
            i: 0.0,
            f: 0.0,
            fk: 0.0,
            tk: 0.0,
            cv_last: 0.0,
            fs_last: 0.0,
            is_last: 0.0,
            vac: Vac32::dark(&light.cell),
            s1: 0.0,
            s2: 0.0,
            c: [0.0; 5],
            dc: [0.0; 5],
            hp_in: 0.0,
            hp_out: 0.0,
            k_in,
            k_out,
            wx: h / (2.0 * cx),
            wo: h / (2.0 * c1),
            ga: mode.shunt(parts) as f32,
            k: if mode == Mode::Lowpass {
                a * cfb / c1
            } else {
                0.0
            },
            g: 1.0 / R_DARK as f32,
        };
        f.v = Driver::new(0.0).v as f32;
        f.is_last = (-f.v / CR5 as f32 + (VB as f32 - f.v) / CR4 as f32).min(0.0);
        f.c = f.coefs(f.g);
        f
    }

    /// The TPT solve's five coefficients at LDR conductance g (P&D §2.2.2):
    /// vx = c0 s1 + c1 s2 + c2 u; vo = c3 s2 + c4 vx.
    fn coefs(&self, g: f32) -> [f32; 5] {
        let (wx, wo, ga, k) = (self.wx, self.wo, self.ga, self.k);
        let d = 1.0 + wo * (g + ga);
        let pc = g - k * (g + ga);
        let den = d * (1.0 - wx * (k - 2.0) * g) - wx * wo * pc * g;
        let rden = 1.0 / den;
        let rd = 1.0 / d;
        [d * rden, wx * pc * rden, d * wx * g * rden, rd, wo * g * rd]
    }

    /// The LED driver, once a sub-block of `t` seconds, CV and OFFSET volts.
    /// `slope` is the CV's steepest rise in the block (V/s): C1's charge
    /// goes out at C1·(R4/R2)·slope (at most IC1's rail) until it is moved,
    /// the steady current the rest. Sets the block's current and the two
    /// pieces' drive.
    fn drive(&mut self, cv: f32, cv_mid: f32, off: f32, t: f32, slope: f32) {
        let vreq = |c: f32| {
            VB as f32
                - (CR4 / CR2) as f32 * (c - VB as f32)
                - (CR4 / CR3) as f32 * (off - VB as f32)
        };
        let vr = vreq(cv);
        let zen = |v: f32| {
            (IZT as f32) * exp2f(((-v) - VZ as f32) * (std::f32::consts::LOG2_E / VZK as f32))
        };
        let load = |v: f32| -v * (1.0 / CR5 as f32) + zen(v) + (VB as f32 - v) * (1.0 / CR4 as f32);
        let cc = CC1 as f32 / t;
        let is = load(vr);
        let fl = &self.light;
        // The steady light over the block: light goes as If^4.5 at low
        // current, so the end alone over-lights a rising block.
        let fs_end = fl.f(is.max(0.0));
        let is0 = self.is_last.max(0.0);
        // Simpson over the block when the CV moved (light is convex in If).
        let moved = (is - self.is_last).abs() > 1e-6;
        let im = if moved {
            load(vreq(cv_mid)).max(0.0)
        } else {
            is.max(0.0)
        };
        let fs = if moved {
            (self.fs_last + 4.0 * fl.f(im) + fs_end) * (1.0 / 6.0)
        } else {
            fs_end
        };
        self.fs_last = fs_end;
        self.is_last = is;
        let q = CC1 as f32 * (self.v - vr); // C1's charge to move, > 0 rising
        // IC1's rail: two LEDs at ~1.65 V, R13.
        let irail = ((vr - VOUT_MIN as f32 - 3.3) * (1.0 / CR13 as f32)).max(0.0);
        self.tk = 0.0;
        if is + q / t < 0.0 {
            // LEDs off: C1 relaxes through R5, R4 and D1 (semi-implicit).
            let gl = 1.0 / CR5 as f32 + 1.0 / CR4 as f32 + zen(self.v) * (1.0 / VZK as f32);
            let v = self.v + load(self.v) / (cc + gl);
            self.v = v.min(vr.max(self.v));
            self.i = 0.0;
            self.f = 0.0;
        } else if q > 0.0 && irail > is {
            // The kick: C1's charge over the rising time, capped at the rail;
            // it rides on a steady part that moved over the block.
            let ik = (is + (CC1 * CR4 / CR2) as f32 * slope)
                .min(irail)
                .max(is + q / t);
            let k = ik - is;
            let fk = (fl.f((is0 + k).min(irail)) + 4.0 * fl.f((im + k).min(irail)) + fl.f(ik))
                * (1.0 / 6.0);
            let tk = q / k;
            if tk < t {
                self.v = vr;
                self.i = is + q / t;
                self.fk = fk;
                self.tk = tk;
                self.f = fs;
            } else {
                self.v -= k * t / CC1 as f32;
                self.i = ik;
                self.f = fk;
            }
        } else {
            self.v = vr;
            self.i = (is + q / t).min(irail).max(0.0);
            // Falling slower than C1 empties: the string carries less than
            // steady; scale the light by f(i)/f(is).
            self.f = if is > 0.0 {
                fs * fl.f(self.i) / fs_end.max(1e-30)
            } else {
                0.0
            };
        }
    }

    /// The vactrol over a sub-block: on an edge, the rail's piece first,
    /// then the rest; otherwise one piece.
    fn vactrol(&mut self, t: f32) {
        if self.tk > 0.0 {
            self.vac.piece(self.fk, self.tk);
            self.vac.piece(self.f, t - self.tk);
            self.tk = 0.0;
        } else {
            self.vac.piece(self.f, t);
        }
    }

    /// One voice: input (digital) × gain, CV and OFFSET (volts, a sample
    /// each; OFFSET read at each sub-block's end).
    pub fn run(&mut self, x: &[f32], cv: &[f64], off: &[f64], gain: f32) -> Vec<f32> {
        let mut y = vec![0.0f32; x.len()];
        let mut i0 = 0;
        while i0 < x.len() {
            let end = (i0 + SUB).min(x.len());
            let t = (end - i0) as f32 / FS as f32;
            // The CV's steepest rise in the block (the envelope stage could
            // hand this over; here we measure).
            let mut max = 0.0f64;
            for &c in &cv[i0..end] {
                max = max.max(c - self.cv_last);
                self.cv_last = c;
            }
            let mid = cv[(i0 + end) / 2] as f32;
            self.drive(
                cv[end - 1] as f32,
                mid,
                off[end - 1] as f32,
                t,
                (max * FS) as f32,
            );
            self.vactrol(t);
            self.g = self.vac.g();
            let to = self.coefs(self.g);
            let m = 1.0 / (end - i0) as f32;
            for k in 0..5 {
                self.dc[k] = (to[k] - self.c[k]) * m;
            }
            for (yi, &xi) in y[i0..end].iter_mut().zip(&x[i0..end]) {
                for k in 0..5 {
                    self.c[k] += self.dc[k];
                }
                let c = &self.c;
                // The 7 Hz input coupling, IC2.
                let xv = xi * gain * VREF as f32;
                let hp = xv - self.hp_in;
                self.hp_in += (1.0 - self.k_in) * hp;
                let u = hp * self.in_gain;
                let vx = c[0] * self.s1 + c[1] * self.s2 + c[2] * u;
                let vo = c[3] * self.s2 + c[4] * vx;
                self.s1 = 2.0 * vx - self.s1;
                self.s2 = 2.0 * vo - self.s2;
                // The output coupling, the follower and IC3, its rails.
                let ho = vo - self.hp_out;
                self.hp_out += (1.0 - self.k_out) * ho;
                *yi = soft_rail(ho * self.out_gain) * (1.0 / VREF as f32);
            }
            self.c = to;
            i0 = end;
        }
        y
    }
}

/// The firmware's rail: a cubic knee from 0.8 of the rail to it.
fn soft_rail(x: f32) -> f32 {
    let r = RAIL as f32;
    let a = x.abs();
    let k = 0.8 * r;
    if a <= k {
        x
    } else {
        // Cubic from (k, slope 1) to (r, slope 0) over [k, k + 2(r − k)].
        let w = 2.0 * (r - k);
        let t = ((a - k) / w).min(1.0);
        let y = k + w * (t - 0.5 * t * t);
        y.copysign(x)
    }
}
