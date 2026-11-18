//! The Parts' EQ block (ADR 0284): bank 2's, behind its door
//! (`part_eq::door`). `run` takes each written Part's bus (and a DRUM
//! pad's side) through LO CUT, then TILT, both one-pole and both gliding:
//! the settings eased a block at a time, the coefficients lerped across
//! the block from the last one's. Everything here inlines into `run`, so
//! the module links into its S4 group whole (`*7part_eq3dsp*`).
//!
//! LO CUT: `y = x − a · lp₁₅₀(x)`, a TPT one-pole high-pass at 150 Hz
//! crossfaded in by `a` (0 off, 1 on).
//! TILT: `H(s) = (k s + ω)/(s + k ω)` about ω = 2π·810 Hz, k = 10^(dB/20):
//! 1/k below, k above, exactly 1 at the pivot. As a one-pole low-pass at
//! `k ω`: `y = k x − (k − 1/k) · lp(x)`. Bilinear, warped at the pivot, so
//! the low-pass's `g` is `k · tan(π · 810 / fs)`.

use chimera_hal::BLOCK_SIZE;

use super::{ChannelEq, Coeffs, Live, LoCut, PartEq, PartEqs, Taps};
use crate::dsp::ease::ease_coeff;
use crate::hw::MAX_PARTS;
use crate::instrument::Sides;

/// TILT's pivot (the Jellymix's).
pub const PIVOT_HZ: f32 = 810.0;
/// LO CUT's corner (the Jellymix's).
pub const LO_CUT_HZ: f32 = 150.0;
/// dB to a gain's log2: `log2(10) / 20`.
const DB_TO_LOG2: f32 = core::f32::consts::LOG2_10 / 20.0;

/// One block of the Parts' buses, as the Instrument hands them over.
pub struct EqBlock<'a> {
    pub buses: &'a mut [[f32; BLOCK_SIZE]; MAX_PARTS],
    pub sides: &'a mut Sides,
    pub written: &'a [bool; MAX_PARTS],
    pub settings: &'a [PartEq; MAX_PARTS],
    pub sample_rate: u32,
}

/// The rate's warps: TILT's `tan(π·810/fs)` and LO CUT's low-pass gain.
#[derive(Clone, Copy)]
struct Warp {
    tilt: f32,
    cut_g: f32,
}

impl Warp {
    #[inline(always)]
    fn at(sample_rate: u32) -> Warp {
        let t = |hz: f32| libm::tanf(core::f32::consts::PI * hz / sample_rate as f32);
        let c = t(LO_CUT_HZ);
        Warp {
            tilt: t(PIVOT_HZ),
            cut_g: c / (1.0 + c),
        }
    }
}

impl Coeffs {
    /// TILT at `db`, LO CUT at amount `a`.
    #[inline(always)]
    fn of(db: f32, a: f32, w: Warp) -> Coeffs {
        let k = libm::exp2f(db * DB_TO_LOG2);
        let g = k * w.tilt;
        Coeffs {
            k,
            c: k - 1.0 / k,
            g: g / (1.0 + g),
            a,
        }
    }

    #[inline(always)]
    fn step_to(self, to: Coeffs) -> Coeffs {
        let n = BLOCK_SIZE as f32;
        Coeffs {
            k: (to.k - self.k) / n,
            c: (to.c - self.c) / n,
            g: (to.g - self.g) / n,
            a: (to.a - self.a) / n,
        }
    }
}

/// LO CUT's amount for its setting.
#[inline(always)]
fn amount(c: LoCut) -> f32 {
    match c {
        LoCut::Off => 0.0,
        LoCut::On => 1.0,
    }
}

/// `x` through LO CUT then TILT, the coefficients `from + step · (i + 1)`
/// at sample `i`: the block ends on the eased values.
#[inline(always)]
fn filter(x: &mut [f32; BLOCK_SIZE], t: &mut Taps, from: Coeffs, step: Coeffs, cut_g: f32) {
    // `n` counts in whole floats, exactly `(i + 1) as f32`.
    let mut n = 0.0f32;
    let (mut cut, mut tilt) = (t.cut, t.tilt);
    for s in x.iter_mut() {
        n += 1.0;
        let (k, c) = (from.k + step.k * n, from.c + step.c * n);
        let (g, a) = (from.g + step.g * n, from.a + step.a * n);
        let v = (*s - cut) * cut_g;
        let lp = v + cut;
        cut = lp + v;
        let y = *s - a * lp;
        let v = (y - tilt) * g;
        let lp = v + tilt;
        tilt = lp + v;
        *s = k * y - c * lp;
    }
    (t.cut, t.tilt) = (cut, tilt);
}

impl ChannelEq {
    /// Out of bypass: at flat, the filters empty, the eases there.
    #[inline(always)]
    fn wake(&mut self, w: Warp) {
        self.tilt.land(0.0);
        self.cut.land(0.0);
        self.at = Coeffs::of(0.0, 0.0, w);
        (self.mid, self.side) = (Taps::ZERO, Taps::ZERO);
        self.live = Live::Running;
    }

    /// Back to bypass once flat and settled.
    #[inline(always)]
    fn settle(&mut self, e: PartEq) {
        if e.is_flat() && self.tilt.at() == 0.0 && self.cut.at() == 0.0 {
            self.live = Live::Bypass;
        }
    }
}

/// Every Part's EQ for one block: the written Parts' buses (and sides)
/// filtered in place, an unwritten Part's eases landed unheard (as the
/// mix's gains), a Part flat and settled left alone.
#[inline(never)]
pub fn run(eqs: &mut PartEqs, b: EqBlock<'_>) {
    let w = Warp::at(b.sample_rate);
    let k = ease_coeff(b.sample_rate);
    for (p, ch) in eqs.parts.iter_mut().enumerate() {
        let e = b.settings[p];
        if ch.live == Live::Bypass {
            if e.is_flat() {
                continue;
            }
            ch.wake(w);
        }
        let (db, a) = (e.tilt().db(), amount(e.lo_cut()));
        if !b.written[p] {
            ch.tilt.land(db);
            ch.cut.land(a);
            ch.at = Coeffs::of(db, a, w);
            (ch.mid, ch.side) = (Taps::ZERO, Taps::ZERO);
            ch.settle(e);
            continue;
        }
        let (_, db) = ch.tilt.step(db, k);
        let (_, a) = ch.cut.step(a, k);
        let (from, to) = (ch.at, Coeffs::of(db, a, w));
        let step = from.step_to(to);
        ch.at = to;
        filter(&mut b.buses[p], &mut ch.mid, from, step, w.cut_g);
        match &mut b.sides[p] {
            Some(side) => filter(side, &mut ch.side, from, step, w.cut_g),
            // No pad panned this block: its side is silence, and so its memory.
            None => ch.side = Taps::ZERO,
        }
        ch.settle(e);
    }
}
