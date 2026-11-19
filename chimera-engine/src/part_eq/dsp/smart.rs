//! SMART EQ's block (ADR 0296): bank 2's, inside the Part EQ's S4 group
//! (`*7part_eq3dsp*`), run by the EQ's door entry after LO CUT and TILT.
//!
//! Each Part has four fixed band-passes (`Band`), TPT state-variable
//! filters in parallel on its post-TILT bus. A band's unity-peak band-pass
//! `bp` is both what SMART EQ hears (its block mean square) and what the
//! band cuts: `y = x + Σ (G − 1) · bp`. The gains are decided once a block
//! from every Part's levels and roles (`target`), follow their targets
//! slowly in dB, and are lerped sample by sample across the next block.
//! A Part whose four gains are unity at both ends of a block is read, never
//! written.

use chimera_hal::BLOCK_SIZE;

use super::tan_small;
use crate::dsp::algo::math::{exp2, log2};
use crate::dsp::{Unit, clampf};
use crate::hw::MAX_PARTS;
use crate::instrument::Sides;
use crate::part_eq::Live;
use crate::part_eq::smart::{
    BANDS, Band, BandTaps, CutDb, CutView, Dip, PartSmart, Power, Role, SMART_VIEW, SmartEq,
    SmartState,
};

/// Each band's Q: a two-octave bandwidth, neighbours crossing at −3 dB.
const Q: f32 = 2.0 / 3.0;
/// The level follower rises on power with this time constant, seconds...
const LEVEL_RISE_S: f32 = 0.05;
/// ...and falls with this one: a long MODAL tail or a pad's release is
/// heard as the sustained energy it is, not as peaks gone by.
const LEVEL_FALL_S: f32 = 0.5;
/// A cut deepens with this time constant, seconds.
const ATTACK_S: f32 = 0.5;
/// A cut lets go with this one, seconds.
const RELEASE_S: f32 = 2.0;
/// Under this a band is silent (dBFS).
const FLOOR_DB: f32 = -66.0;
/// Over `FLOOR_DB` by this a band is fully active (dB).
const ACTIVE_SPAN_DB: f32 = 24.0;
/// How far under a higher Part a lower one masks it, dB: not at all from
/// `none` under, fully from `full` under. Above the mud a lower Part
/// masks only near the higher one's level.
const CLEAR_THREAT: Threat = Threat {
    none: 12.0,
    full: 0.0,
};
/// In LOW and LOW MID a quiet ringing tail still clouds a louder Part
/// (owner, 2026-11-19: "muddy", MODAL's tails the worst): it yields fully
/// up to 12 dB under, and some to 30 dB under.
const MUD_THREAT: Threat = Threat {
    none: 30.0,
    full: 12.0,
};

/// A band's masking reach (`threat`): dB under the higher Part.
#[derive(Clone, Copy)]
struct Threat {
    none: f32,
    full: f32,
}
/// Above the mud a lower Part yields this share of its LOW and LOW MID
/// cut: the mud is where yielding matters most.
const CLEAR_WEIGHT: f32 = 0.75;
/// A band's skirt at the centre of the band `n` away (two octaves each),
/// dB: `−10 log10(1 + Q² (4ⁿ − 4⁻ⁿ)²)` at Q 2/3, for n = 1, 2, 3 (index 0
/// unused). What a tone at a neighbour's centre puts in this band.
const SKIRT_DB: [f32; BANDS] = [0.0, -8.60, -20.56, -32.60];
/// A higher Part's band is its own sound by this much excess over what its
/// other bands' skirts put there (dB): none at 0, all from here.
const OWN_SPAN_DB: f32 = 6.0;
/// A cut under this lands on 0 (dB).
const LAND_DB: f32 = 0.01;
/// A filter's or level's memory under this is silence: flushed to 0 before
/// it can fall subnormal (−300 dB).
const FLUSH: f32 = 1e-15;
/// 10·log10(2): dB of power per octave.
const DB_PER_LOG2_POWER: f32 = 3.010_3;
/// dB of amplitude to a gain's log2: `log2(10) / 20`.
const DB_TO_LOG2: f32 = core::f32::consts::LOG2_10 / 20.0;

/// One block of the Parts' buses (after LO CUT and TILT), as the door hands
/// them over.
pub struct SmartBlock<'a> {
    pub buses: &'a mut [[f32; BLOCK_SIZE]; MAX_PARTS],
    pub sides: &'a mut Sides,
    pub written: &'a [bool; MAX_PARTS],
    pub roles: &'a [Role; MAX_PARTS],
    pub smart: SmartEq,
    pub sample_rate: u32,
}

/// A band filter's coefficients at the rate (Zavalishin's TPT SVF): `a1 =
/// 1 / (1 + g (g + k))`, `a2 = g a1`, `a3 = g a2`, `k = 1/Q`.
#[derive(Clone, Copy)]
struct Svf {
    a1: f32,
    a2: f32,
    a3: f32,
}

/// The four bands' filters and the block's time constants at the rate.
#[derive(Clone, Copy)]
struct Rate {
    svf: [Svf; BANDS],
    /// One-pole coefficients a block: the level's rise and fall, the
    /// cut's attack and release.
    rise: f32,
    fall: f32,
    attack: f32,
    release: f32,
}

/// `exp(−block / (τ fs))`, a block's one-pole pole.
#[inline(always)]
fn pole(tau_s: f32, sample_rate: u32) -> f32 {
    exp2(-core::f32::consts::LOG2_E * BLOCK_SIZE as f32 / (tau_s * sample_rate as f32))
}

impl Rate {
    #[inline(always)]
    fn at(sample_rate: u32) -> Rate {
        let k = 1.0 / Q;
        let svf = Band::ALL.map(|b| {
            let g = tan_small(core::f32::consts::PI * b.centre().get() / sample_rate as f32);
            let a1 = 1.0 / (1.0 + g * (g + k));
            Svf {
                a1,
                a2: g * a1,
                a3: g * g * a1,
            }
        });
        Rate {
            svf,
            rise: pole(LEVEL_RISE_S, sample_rate),
            fall: pole(LEVEL_FALL_S, sample_rate),
            attack: pole(ATTACK_S, sample_rate),
            release: pole(RELEASE_S, sample_rate),
        }
    }
}

/// `G − 1` of a cut: 0 at 0 dB exactly.
#[inline(always)]
fn dip_of(c: CutDb) -> Dip {
    if c == CutDb::ZERO {
        Dip::UNITY
    } else {
        Dip(exp2(-c.get() * DB_TO_LOG2) - 1.0)
    }
}

/// Part `x`'s bus through its four bands: each band's `k · v1` (the
/// unity-peak band-pass) summed into the output by its dip, lerped from
/// `from` to `to` across the block (`WRITE`), and each band's energy
/// (`Σ v1²`, `k²` applied by the caller) added to `energy` (`LISTEN`).
/// Two bands at a time over the block: each pair's memory and
/// coefficients fit the M7's 32 float registers (all four interleaved
/// spilled to the stack, 75 instructions a sample), and two independent
/// recurrences hide each other's FPU latency (one band alone waits on
/// its own). Every band hears the block as it came in (`input`); the
/// bands add to the output low to high, as a sample at a time would.
#[inline(always)]
fn bands<const WRITE: bool, const LISTEN: bool>(
    x: &mut [f32; BLOCK_SIZE],
    taps: &mut [BandTaps; BANDS],
    r: &Rate,
    from: &[Dip; BANDS],
    to: &[Dip; BANDS],
    energy: &mut [f32; BANDS],
) {
    let k = 1.0 / Q;
    let n = BLOCK_SIZE as f32;
    let input = *x;
    for pair in [[0, 1], [2, 3]] {
        let f = pair.map(|b| r.svf[b]);
        let mut s = pair.map(|b| taps[b]);
        // Each dip's share of `k`, at the block's start and its step a sample.
        let mut d = pair.map(|b| k * from[b].0);
        let dd = pair.map(|b| k * (to[b].0 - from[b].0) / n);
        let mut e = [0.0f32; 2];
        for (o, &v) in x.iter_mut().zip(input.iter()) {
            let mut add = 0.0f32;
            for i in 0..2 {
                let v3 = v - s[i].s2;
                let v1 = f[i].a1 * s[i].s1 + f[i].a2 * v3;
                let v2 = s[i].s2 + f[i].a2 * s[i].s1 + f[i].a3 * v3;
                s[i].s1 = 2.0 * v1 - s[i].s1;
                s[i].s2 = 2.0 * v2 - s[i].s2;
                if LISTEN {
                    e[i] += v1 * v1;
                }
                if WRITE {
                    d[i] += dd[i];
                    add += d[i] * v1;
                }
            }
            if WRITE {
                *o += add;
            }
        }
        for (i, &b) in pair.iter().enumerate() {
            taps[b] = s[i];
            if LISTEN {
                energy[b] += e[i];
            }
        }
    }
}

/// Memory under `FLUSH` to 0, a NaN or inf to rest.
#[inline(always)]
fn tidy(taps: &mut [BandTaps; BANDS]) {
    for t in taps.iter_mut() {
        if !(t.s1 + t.s2).is_finite() {
            *t = BandTaps::ZERO;
        }
        if t.s1.abs() < FLUSH {
            t.s1 = 0.0;
        }
        if t.s2.abs() < FLUSH {
            t.s2 = 0.0;
        }
    }
}

/// A level in dBFS (power).
#[inline(always)]
fn db(p: Power) -> f32 {
    DB_PER_LOG2_POWER * log2(p.0)
}

/// How active a band at `l` dBFS is: 0 silent, 1 playing.
#[inline(always)]
fn active(l: f32) -> f32 {
    clampf((l - FLOOR_DB) / ACTIVE_SPAN_DB, 0.0, 1.0)
}

/// How much a Part at `lower` dB can mask one at `higher` dB in a band
/// with reach `t`.
#[inline(always)]
fn threat(lower: f32, higher: f32, t: Threat) -> f32 {
    clampf((lower - higher + t.none) / (t.none - t.full), 0.0, 1.0)
}

/// How much of band `b` is a Part's own sound, not its other bands'
/// energy through `b`'s skirt (`SKIRT_DB`): a pure 100 Hz bass is LOW's
/// alone, though LOW MID hears it 8.6 dB down.
#[inline(always)]
fn own(levels: &[f32; BANDS], b: usize) -> f32 {
    let mut leak = f32::MIN;
    for (o, &l) in levels.iter().enumerate() {
        if o != b {
            leak = leak.max(l + SKIRT_DB[o.abs_diff(b)]);
        }
    }
    clampf((levels[b] - leak) / OWN_SPAN_DB, 0.0, 1.0)
}

/// Each Part's cut target in each band: the deepest any higher-role Part
/// asks, scaled by AMOUNT (`share`). The rule (ADR 0296 § 3): `q` presses
/// on `p` in a band where `q` is active and the band is `q`'s own sound,
/// `p` is active, and `p` is loud enough there to mask `q`.
#[inline(always)]
fn targets(
    levels: &[[f32; BANDS]; MAX_PARTS],
    roles: &[Role; MAX_PARTS],
    share: Unit,
) -> [[CutDb; BANDS]; MAX_PARTS] {
    core::array::from_fn(|p| {
        core::array::from_fn(|b| {
            let (reach, weight) = if Band::ALL[b].muddy() {
                (MUD_THREAT, 1.0)
            } else {
                (CLEAR_THREAT, CLEAR_WEIGHT)
            };
            let lp = levels[p][b];
            let ap = active(lp);
            let mut press = 0.0f32;
            for q in 0..MAX_PARTS {
                let depth = roles[q].depth_over(roles[p]).get();
                if depth > 0.0 {
                    let lq = levels[q][b];
                    let heard = active(lq) * own(&levels[q], b);
                    press = press.max(depth * heard * ap * threat(lp, lq, reach));
                }
            }
            CutDb::of(Unit::new(share.get() * weight * press))
        })
    })
}

/// A cut toward its target: deeper at ATTACK's pace, shallower at
/// RELEASE's, landing on 0 under `LAND_DB`.
#[inline(always)]
fn follow(c: CutDb, target: CutDb, r: &Rate) -> CutDb {
    let pole = if target > c { r.attack } else { r.release };
    let next = target.get() + pole * (c.get() - target.get());
    if target == CutDb::ZERO && next < LAND_DB {
        CutDb::ZERO
    } else {
        CutDb::new(next)
    }
}

impl PartSmart {
    /// Any band cut or not yet back at unity.
    #[inline(always)]
    fn cutting(&self) -> bool {
        self.cut.iter().any(|c| *c != CutDb::ZERO) || self.dip.iter().any(|d| *d != Dip::UNITY)
    }
}

/// Every Part's SMART EQ for one block.
#[inline(always)]
pub fn run(st: &mut SmartState, b: SmartBlock<'_>) {
    let listens = b.smart.listens();
    if st.live == Live::Bypass {
        if !listens {
            return;
        }
        *st = SmartState::REST;
        st.live = Live::Running;
    }
    let r = Rate::at(b.sample_rate);
    let k2 = 1.0 / (Q * Q);
    let mut levels = [[FLOOR_DB - ACTIVE_SPAN_DB; BANDS]; MAX_PARTS];
    let lanes = b.buses.iter_mut().zip(b.sides.iter_mut()).zip(b.written);
    for ((p, heard), ((bus, side), &written)) in st.parts.iter_mut().zip(&mut levels).zip(lanes) {
        let from = p.dip;
        let to = p.cut.map(dip_of);
        let mut energy = [0.0f32; BANDS];
        if written {
            if from == to && to == [Dip::UNITY; BANDS] {
                bands::<false, true>(bus, &mut p.mid, &r, &from, &to, &mut energy);
            } else {
                bands::<true, true>(bus, &mut p.mid, &r, &from, &to, &mut energy);
                if let Some(side) = side {
                    let mut none = [0.0f32; BANDS];
                    bands::<true, false>(side, &mut p.side, &r, &from, &to, &mut none);
                }
            }
            if side.is_none() {
                p.side = [BandTaps::ZERO; BANDS];
            }
            tidy(&mut p.mid);
            tidy(&mut p.side);
        } else {
            // Nothing on the bus: the filters empty, the levels fall.
            (p.mid, p.side) = ([BandTaps::ZERO; BANDS], [BandTaps::ZERO; BANDS]);
        }
        p.dip = to;
        for ((l, e), h) in p.level.iter_mut().zip(energy).zip(heard.iter_mut()) {
            let ms = e * k2 / BLOCK_SIZE as f32;
            let pole = if ms > l.0 { r.rise } else { r.fall };
            let v = ms + pole * (l.0 - ms);
            *l = Power(if v.is_finite() && v >= FLUSH { v } else { 0.0 });
            *h = db(*l);
        }
    }
    let want = targets(&levels, b.roles, b.smart.share());
    let mut any = false;
    for (p, t) in st.parts.iter_mut().zip(want.iter()) {
        for (c, &tc) in p.cut.iter_mut().zip(t) {
            *c = follow(*c, tc, &r);
        }
        any |= p.cutting();
    }
    for (i, p) in st.parts.iter().enumerate() {
        SMART_VIEW.publish(i, CutView::of(&p.cut));
    }
    if !listens && !any {
        *st = SmartState::REST;
        for i in 0..MAX_PARTS {
            SMART_VIEW.publish(i, CutView::NONE);
        }
    }
}
