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

pub mod smart;

use super::smart::{PartKind, Role, SmartEq};
use super::{ChannelEq, CoeffStep, Coeffs, Live, LpShare, OnePole, PartEq, PartEqs, Taps};
use crate::dsp::ease::ease_coeff;
use crate::dsp::{Db, Gain, Hz, Unit};
use crate::hw::MAX_PARTS;
use crate::instrument::{PartAudio, Sides};

/// TILT's pivot (the Jellymix's).
pub const PIVOT: Hz = Hz::new(810.0);
/// LO CUT's corner (the Jellymix's).
pub const LO_CUT: Hz = Hz::new(150.0);
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

/// The rate's warps: TILT's `tan(π·810/fs)` and LO CUT's low-pass.
#[derive(Clone, Copy)]
struct Warp {
    tilt: f32,
    cut: OnePole,
}

/// `tan x`'s series, x³ to x⁷: 1/3, 2/15, 17/315.
const TAN_X3: f32 = 1.0 / 3.0;
const TAN_X5: f32 = 2.0 / 15.0;
const TAN_X7: f32 = 17.0 / 315.0;

/// `tan x` for the small angles a corner well under Nyquist gives (810 Hz
/// at 48 kHz is 0.053 rad): its series to x⁷, inlined here, in bank 2,
/// not libm's `tanf` in bank 1. At 0.053 rad the next term is 1e-13 of
/// it, under f32's ulp; the tests check the shapes against `f64::tan`.
#[inline(always)]
fn tan_small(x: f32) -> f32 {
    let x2 = x * x;
    x * (1.0 + x2 * (TAN_X3 + x2 * (TAN_X5 + x2 * TAN_X7)))
}

impl Warp {
    #[inline(always)]
    fn at(sample_rate: u32) -> Warp {
        let t = |f: Hz| tan_small(core::f32::consts::PI * f.get() / sample_rate as f32);
        Warp {
            tilt: t(PIVOT),
            cut: OnePole::of_tan(t(LO_CUT)),
        }
    }
}

impl Coeffs {
    /// TILT at `db`, LO CUT crossfaded in by `a`.
    #[inline(always)]
    fn of(db: Db, a: Unit, w: Warp) -> Coeffs {
        let k = Gain::new(libm::exp2f(db.get() * DB_TO_LOG2));
        Coeffs {
            k,
            c: LpShare::of(k),
            g: OnePole::of_tan(k.get() * w.tilt),
            a,
        }
    }

    /// A sample's move from here to `to` across a block.
    #[inline(always)]
    fn step_to(self, to: Coeffs) -> CoeffStep {
        let n = BLOCK_SIZE as f32;
        CoeffStep {
            k: (to.k.get() - self.k.get()) / n,
            c: (to.c.get() - self.c.get()) / n,
            g: (to.g.get() - self.g.get()) / n,
            a: (to.a.get() - self.a.get()) / n,
        }
    }
}

/// A settled block's step: none.
const STILL: CoeffStep = CoeffStep {
    k: 0.0,
    c: 0.0,
    g: 0.0,
    a: 0.0,
};

/// `x` through LO CUT then TILT, the coefficients `from + step · (i + 1)`
/// at sample `i`: the block ends on the eased values. `MOVING` false is
/// the settled block's (`STILL`): the same sums, `from` throughout.
#[inline(always)]
fn filter<const MOVING: bool>(
    x: &mut [f32; BLOCK_SIZE],
    t: &mut Taps,
    from: Coeffs,
    step: CoeffStep,
    cut: OnePole,
) {
    let (k0, c0, g0, a0) = (from.k.get(), from.c.get(), from.g.get(), from.a.get());
    let cut_g = cut.get();
    // `n` counts in whole floats, exactly `(i + 1) as f32`.
    let mut n = 0.0f32;
    let (mut lo, mut tl) = (t.cut, t.tilt);
    for s in x.iter_mut() {
        let (k, c, g, a) = if MOVING {
            n += 1.0;
            (
                k0 + step.k * n,
                c0 + step.c * n,
                g0 + step.g * n,
                a0 + step.a * n,
            )
        } else {
            (k0, c0, g0, a0)
        };
        let v = (*s - lo) * cut_g;
        let lp = v + lo;
        lo = lp + v;
        let y = *s - a * lp;
        let v = (y - tl) * g;
        let lp = v + tl;
        tl = lp + v;
        *s = k * y - c * lp;
    }
    (t.cut, t.tilt) = (lo, tl);
}

/// One Part's block: its bus, its side (a DRUM pad panned), whether its
/// voices wrote it, and its setting.
struct Lane<'a> {
    bus: &'a mut [f32; BLOCK_SIZE],
    side: &'a mut Option<[f32; BLOCK_SIZE]>,
    written: bool,
    eq: PartEq,
}

impl ChannelEq {
    /// Out of bypass: at flat, the filters empty, the glides there.
    #[inline(always)]
    fn wake(&mut self, w: Warp) {
        self.tilt.land(Db::ZERO);
        self.cut.land(Unit::ZERO);
        self.at = Coeffs::of(Db::ZERO, Unit::ZERO, w);
        (self.mid, self.side) = (Taps::ZERO, Taps::ZERO);
        self.live = Live::Running;
    }

    /// Back to bypass once flat and settled.
    #[inline(always)]
    fn settle(&mut self, e: PartEq) {
        if e.is_flat() && self.tilt.at() == Db::ZERO && self.cut.at() == Unit::ZERO {
            self.live = Live::Bypass;
        }
    }

    /// The Part's block: written, filtered in place; unwritten, the
    /// glides landed unheard (as the mix's gains); flat and settled, left
    /// alone. A NaN or inf in leaves the filters' memory not finite: the
    /// next block starts from rest (as CLEAN's filter, `filter::clean`).
    #[inline(always)]
    fn block(&mut self, l: Lane<'_>, w: Warp, k: f32) {
        if self.live == Live::Bypass {
            if l.eq.is_flat() {
                return;
            }
            self.wake(w);
        }
        let (db, a) = (l.eq.tilt().db(), l.eq.lo_cut().amount());
        if !l.written {
            self.tilt.land(db);
            self.cut.land(a);
            self.at = Coeffs::of(db, a, w);
            (self.mid, self.side) = (Taps::ZERO, Taps::ZERO);
            self.settle(l.eq);
            return;
        }
        let (db, a) = (self.tilt.step(db, k), self.cut.step(a, k));
        let (from, to) = (self.at, Coeffs::of(db, a, w));
        self.at = to;
        if from == to {
            filter::<false>(l.bus, &mut self.mid, to, STILL, w.cut);
            if let Some(side) = l.side {
                filter::<false>(side, &mut self.side, to, STILL, w.cut);
            }
        } else {
            let step = from.step_to(to);
            filter::<true>(l.bus, &mut self.mid, from, step, w.cut);
            if let Some(side) = l.side {
                filter::<true>(side, &mut self.side, from, step, w.cut);
            }
        }
        if l.side.is_none() {
            // No pad panned this block: its side is silence, and so its memory.
            self.side = Taps::ZERO;
        }
        if !(self.mid.finite() && self.side.finite()) {
            (self.mid, self.side) = (Taps::ZERO, Taps::ZERO);
        }
        self.settle(l.eq);
    }
}

/// The Parts' buses as the Instrument hands them over, the settings in
/// its snapshot's Parts: the door's entry (`EqDoor`).
pub struct PartsBlock<'a> {
    pub buses: &'a mut [[f32; BLOCK_SIZE]; MAX_PARTS],
    pub sides: &'a mut Sides,
    pub written: &'a [bool; MAX_PARTS],
    pub parts: &'a [PartAudio; MAX_PARTS],
    /// The master's SMART EQ (ADR 0296), from the snapshot's `CompParams`,
    /// and the reverb return's role in it.
    pub smart: SmartEq,
    pub wash: Role,
    pub sample_rate: u32,
}

/// The door's entry (`EqDoor`): what bank 1 asked, answered here in the
/// EQ's S4 group.
#[inline(never)]
pub fn answer(ask: super::door::Ask<'_, '_>) {
    use super::door::Ask;
    match ask {
        Ask::Parts(eqs, b) => run_parts(eqs, b),
        Ask::Wash {
            eqs,
            wet,
            sounding,
            sample_rate,
        } => smart::run_wash(&mut eqs.smart, wet, sounding, sample_rate),
    }
}

/// `run` on the Parts' own settings (`PartParams::eq`), then SMART EQ's
/// cuts on their roles (`PartParams::role`).
#[inline(always)]
pub fn run_parts(eqs: &mut PartEqs, b: PartsBlock<'_>) {
    let settings: [PartEq; MAX_PARTS] = core::array::from_fn(|p| b.parts[p].mix.eq);
    run(
        eqs,
        EqBlock {
            buses: b.buses,
            sides: b.sides,
            written: b.written,
            settings: &settings,
            sample_rate: b.sample_rate,
        },
    );
    let roles: [Role; MAX_PARTS] = core::array::from_fn(|p| b.parts[p].mix.role);
    let kinds: [PartKind; MAX_PARTS] =
        core::array::from_fn(|p| PartKind::of(b.parts[p].params.engine()));
    smart::run(
        &mut eqs.smart,
        smart::SmartBlock {
            buses: b.buses,
            sides: b.sides,
            written: b.written,
            roles: &roles,
            kinds: &kinds,
            wash: b.wash,
            smart: b.smart,
            sample_rate: b.sample_rate,
        },
    );
}

/// Every Part's EQ for one block (`ChannelEq::block`).
#[inline(never)]
pub fn run(eqs: &mut PartEqs, b: EqBlock<'_>) {
    let w = Warp::at(b.sample_rate);
    let k = ease_coeff(b.sample_rate);
    let lanes = b
        .buses
        .iter_mut()
        .zip(b.sides.iter_mut())
        .zip(b.written.iter().zip(b.settings));
    for (ch, ((bus, side), (&written, &eq))) in eqs.parts.iter_mut().zip(lanes) {
        ch.block(
            Lane {
                bus,
                side,
                written,
                eq,
            },
            w,
            k,
        );
    }
}
