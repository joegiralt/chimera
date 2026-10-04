//! BURST (reverb TYPEs spec § BURST engine): GATE and REVERSE on one code
//! path. The send, through TONE and a two-allpass f32 pre-diffuser, is
//! written to the burst line; 12 taps over [0, LENGTH] sum mono under
//! their curve's weights (tap 0 is the value written, tap 11 is REGEN's
//! read too); GATE's TAIL loop (delay → normalised allpass → ×g) adds back
//! into the sum; two f32 allpasses a side make the stereo, each side's
//! second smeared. GATE runs with REGEN at 0 and REVERSE with TAIL at 0:
//! both pay for both.
//!
//! Laid out for the chip's 24 kHz ring: at another bus rate every time
//! scales with it, as QUAD's lengths do. Values are in LSBs.

// Indexed loops over the taps read as the spec's sums do.
#![allow(clippy::needless_range_loop)]

use core::f32::consts::{LOG2_10, LOG2_E, TAU};

use crate::dsp::algo::math::{exp2, inv_sqrt};
use crate::dsp::clampf;
use crate::dsp::halfband::HALF;
use crate::dsp::reverb::EngineKnobs;
use crate::dsp::ring::{FULL_SCALE, RING_LEN};

use super::arena::ReverbArena;
use super::parts::{
    Lines, ap_f32, ap_norm, ease_k, heads, one_pole, pow2, prime_ms, segment, soft_clip, wrap,
};

/// The ring rate the lengths are laid out at.
const FS: f32 = 24_000.0;

/// The taps' jitter and signs are drawn from this (spec § BURST engine).
pub const TAP_SEED: u32 = 0x5EED_0343;

/// A textbook LCG step (Numerical Recipes' constants).
const fn lcg(x: u32) -> u32 {
    x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// Each tap's jitter j_k, within ±0.3 and 0 at taps 0 and 11, and its
/// sign: two LCG draws a tap from `seed`. The jitter breaks the comb a
/// regular spacing would ring.
pub const fn tap_table(seed: u32) -> [(f32, i8); 12] {
    let mut t = [(0.0f32, 1i8); 12];
    let mut x = seed;
    let mut k = 0;
    while k < 12 {
        x = lcg(x);
        let j = if k == 0 || k == 11 {
            0.0
        } else {
            ((x >> 8) as f32 / 16_777_216.0 * 2.0 - 1.0) * 0.3
        };
        x = lcg(x);
        t[k] = (j, if x >> 31 == 0 { 1 } else { -1 });
        k += 1;
    }
    t
}

const TABLE: [(f32, i8); 12] = tap_table(TAP_SEED);

/// Taps 1–11's places in the burst, (k + j_k)/11.
const PLACE: [f32; 11] = {
    let mut p = [0.0; 11];
    let mut k = 0;
    while k < 11 {
        p[k] = (k as f32 + 1.0 + TABLE[k + 1].0) / 11.0;
        k += 1;
    }
    p
};

/// The burst line: REVERSE's longest LENGTH, 800 ms.
pub const BURST_MAX: usize = 19_200;
/// TAIL's loop stage: a 37 ms delay, then an 11 ms allpass.
const TAIL_D: usize = prime_ms(37.0);
const TAIL_AP: usize = prime_ms(11.0);
const LINES: [usize; 3] = [BURST_MAX, TAIL_D, TAIL_AP];
/// The f32 lines: the pre-diffuser, 4.3 and 7.1 ms; the decorrelators,
/// a 3.1 ms allpass both sides share, then L 8.3 ms and R 6.1 ms.
const FLINES: [usize; 5] = [
    prime_ms(4.3),
    prime_ms(7.1),
    prime_ms(3.1),
    prime_ms(8.3),
    prime_ms(6.1),
];

const fn distinct() -> bool {
    let all = [
        TAIL_D, TAIL_AP, FLINES[0], FLINES[1], FLINES[2], FLINES[3], FLINES[4],
    ];
    let mut i = 0;
    while i < all.len() {
        let mut j = i + 1;
        while j < all.len() {
            if all[i] == all[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
const _: () = assert!(distinct());

const I: ([usize; 3], usize) = heads(LINES);
const F: ([usize; 5], usize) = heads(FLINES);
/// The i16 lines' slots; the f32 lines' f32s above them.
pub const SLOTS: usize = I.1;
pub const FSLOTS: usize = F.1;
const _: () = assert!(SLOTS + 2 * FSLOTS <= RING_LEN);
const _: () = assert!(SLOTS < u16::MAX as usize);

/// TAIL's delay and allpass reads.
const TAIL_RD: [usize; 2] = [I.0[1] - TAIL_D, I.0[2] - TAIL_AP];
/// The f32 fixed reads: the pre-diffuser's two, the shared decorrelator.
const F_RD: [usize; 3] = [F.0[0] - FLINES[0], F.0[1] - FLINES[1], F.0[2] - FLINES[2]];
/// Each side's smeared decorrelator, its nominal length.
const NOM: [f32; 2] = [FLINES[3] as f32, FLINES[4] as f32];
/// Ring samples a millisecond.
pub const PER_MS: f32 = FS / 1000.0;
/// SMEAR's deepest D, ring samples: GATE's and REVERSE's 1 ms. Its reads
/// only shorten, by 2D at most.
pub const SMEAR_MAX: f32 = crate::dsp::reverb::GATE.smear_ms * PER_MS;
const _: () = assert!(FLINES[3] as f32 - 2.0 * SMEAR_MAX >= 2.0);
const _: () = assert!(FLINES[4] as f32 - 2.0 * SMEAR_MAX >= 2.0);

/// The decorrelators' g (§ BURST engine), and TAIL's allpass's.
const DEC_G: f32 = 0.6;
const TAIL_G: f32 = 0.5;
/// The new engines' stage gain cap (§ Shared machinery).
const MAX_GAIN: f32 = 0.985;
/// The send's injection, ½ (§ Shared machinery), in LSBs.
const IN_GAIN: f32 = 0.5 * FULL_SCALE;
const INV_FS: f32 = 1.0 / FULL_SCALE;
/// REGEN's darkening one-pole, about 6 kHz: its coefficient.
const REGEN_A: f32 = 1.0 - exp2(-LOG2_E * TAU * 6_000.0 / FS);

/// Ring samples a tap crossfade takes (4 blocks), the most one moves a
/// tap (5 ms, QUAD's step), and the most that move at once.
pub const XF: u8 = 128;
pub const STEP: u16 = 120;
pub const MOVERS: usize = 4;
const _: () = assert!((XF as usize).is_multiple_of(HALF));

/// DENSITY keeps these six taps, and fades the other six in this order:
/// the span stays LENGTH at any DENSITY.
const ALWAYS: [usize; 6] = [0, 2, 4, 7, 9, 11];
const FADE: [usize; 6] = [5, 6, 3, 8, 1, 10];
const _: () = {
    let mut seen = [false; 12];
    let mut i = 0;
    while i < 6 {
        seen[ALWAYS[i]] = true;
        seen[FADE[i]] = true;
        i += 1;
    }
    let mut k = 0;
    while k < 12 {
        assert!(seen[k]);
        k += 1;
    }
};

/// Taps 1–11's read lengths for `length_ms`, ring samples, each in
/// 2..=`BURST_MAX`.
#[inline(never)]
pub fn tap_targets(length_ms: f32) -> [u16; 11] {
    let len = length_ms * (FS / 1000.0);
    PLACE.map(|p| clampf(len * p + 0.5, 2.0, BURST_MAX as f32) as u16)
}

/// The 12 signed weights for `k`'s curve and DENSITY, Σw² = 1.
pub fn tap_weights(k: &EngineKnobs) -> [f32; 12] {
    weights(k.shape_db, k.rising, k.taps)
}

/// The curve falls from tap 0 (or rises to tap 11) by |`shape_db`| end to
/// end; DENSITY's six faded taps come in one after another over the
/// knob, each across a third of it (`taps` 6 → 12).
#[inline(never)]
fn weights(shape_db: f32, rising: bool, taps: f32) -> [f32; 12] {
    let n = clampf(taps - 6.0, 0.0, 6.0);
    // Tap to tap, the curve's 1/11.
    let step = pow2(-libm::fabsf(shape_db) * (LOG2_10 / 20.0 / 11.0));
    let mut w = [0.0f32; 12];
    let mut g = 1.0;
    for i in 0..12 {
        w[if rising { 11 - i } else { i }] = g;
        g *= step;
    }
    for (i, &k) in FADE.iter().enumerate() {
        w[k] *= clampf((n - 0.8 * i as f32) * 0.5, 0.0, 1.0);
    }
    let s = inv_sqrt(w.iter().map(|x| x * x).sum());
    for (x, t) in w.iter_mut().zip(&TABLE) {
        *x *= s * t.1 as f32;
    }
    w
}

/// 1/Σ|w|: TAIL's input is the sum over it, so it stays ≤ 2.0 (§ Level
/// plan).
fn inv_abs_sum(w: &[f32; 12]) -> f32 {
    1.0 / w.iter().map(|x| libm::fabsf(*x)).sum::<f32>()
}

/// TAIL's loop gain for an RT60 of `rt60_s`, capped.
fn tail_gain(rt60_s: f32) -> f32 {
    let trip = (TAIL_D + TAIL_AP) as f32;
    pow2(-3.0 * LOG2_10 * trip / (rt60_s.max(1e-3) * FS)).min(MAX_GAIN)
}

/// One block's one-pole toward `t`, `k` the share kept (0 lands). It
/// settles on a fixed point a step from `t` at most, and stays.
fn ease(x: &mut f32, t: f32, k: f32) {
    *x = t + k * (*x - t);
}

#[inline(always)]
fn clip<const C: bool>(x: f32) -> f32 {
    if C {
        soft_clip(x * INV_FS) * FULL_SCALE
    } else {
        x
    }
}

/// SMEAR's moving read on the f32 lines: `parts::pair`'s form, `d` (≥ 2)
/// behind `base`.
#[inline(always)]
fn pair_f32(f: &[f32], base: usize, d: f32) -> f32 {
    let n = FSLOTS;
    let base = base.min(n - 1);
    let di = (d as usize).min(n - 1);
    let fr = d - di as f32;
    let i0 = wrap(base + n - di, n);
    let i1 = wrap(i0 + n - 1, n);
    // SAFETY: `f` holds `FSLOTS` f32s (asserted each block), and `wrap`
    // keeps both indices below it: `base` and `di` are below n.
    let (x0, x1) = unsafe { (*f.get_unchecked(i0), *f.get_unchecked(i1)) };
    x0 + fr * (x1 - x0)
}

/// GATE's and REVERSE's state beside the arena. Zero bytes are a fresh
/// engine, primed by its first block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BurstEngine {
    /// The shared write heads of the i16 and the f32 lines.
    h: u16,
    hf: u16,
    primed: bool,
    /// Ring samples left in the tap crossfade; 0 between.
    left: u8,
    /// How many of `mv` move this crossfade.
    moving: u8,
    /// The taps (1–11) crossfading, and to where; tap 11, when it moves,
    /// sits in slot 0.
    mv: [u8; MOVERS],
    to: [u16; MOVERS],
    /// Taps 1–11's read lengths, ring samples.
    at: [u16; 11],
    tone: f32,
    /// The eased TONE coefficient, pre-diffuser g, REGEN and TAIL gains,
    /// curve and DENSITY: each block ramps from last block's.
    ta: f32,
    gp: f32,
    gr: f32,
    gt: f32,
    shape: f32,
    taps: f32,
    regen: f32,
    w: [f32; 12],
    inv_sw: f32,
    /// This block's SMEAR offsets (start, end), ring samples: `D·(1 + x)`.
    smear: [(f32, f32); 2],
}

/// The i16 accesses, as offsets from the shared head: the three writes,
/// TAIL's two reads, taps 1–11, then each crossfade slot's new and old
/// read (set each block).
const W_BURST: usize = 0;
const W_TAIL_D: usize = 1;
const W_TAIL_AP: usize = 2;
const R_TAIL: usize = 3;
const R_TAP: usize = 5;
const R_TO: usize = R_TAP + 11;
const R_FROM: usize = R_TO + MOVERS;
const ACCESS: usize = R_FROM + MOVERS;
/// The f32 accesses: the five writes, then the three fixed reads.
const F_ACCESS: usize = 8;
const F_OFF: [usize; F_ACCESS] = [
    F.0[0], F.0[1], F.0[2], F.0[3], F.0[4], F_RD[0], F_RD[1], F_RD[2],
];

impl BurstEngine {
    /// Land on `k` at once: a fresh engine's first block, which no one
    /// hears move.
    #[inline(never)]
    pub fn prime(&mut self, k: &EngineKnobs) {
        self.at = tap_targets(k.length_ms);
        (self.left, self.moving, self.primed) = (0, 0, true);
        self.ease_to(k, 0.0, 0.0, true);
    }

    /// One block of each knob's one-pole toward `k` (`k20`, `k50` the
    /// shares kept: 0 lands), and the curve's weights from the eased SHAPE
    /// and DENSITY when they moved (or `always`).
    #[inline(never)]
    fn ease_to(&mut self, k: &EngineKnobs, k20: f32, k50: f32, always: bool) {
        ease(&mut self.ta, one_pole(k.tone_hz, FS), k20);
        ease(&mut self.gp, k.diffuse_g, k20);
        ease(&mut self.gr, k.regen, k50);
        ease(&mut self.gt, k.tail * tail_gain(k.rt60_s), k50);
        let curve = (self.shape, self.taps);
        ease(&mut self.shape, k.shape_db, k20);
        ease(&mut self.taps, k.taps, k20);
        if always || (self.shape, self.taps) != curve {
            self.w = weights(self.shape, k.rising, self.taps);
            self.inv_sw = inv_abs_sum(&self.w);
        }
    }

    /// The SMEAR offsets the next block reads its two smeared
    /// decorrelators short by, (start, end) in ring samples, each in
    /// 0..=2·`SMEAR_MAX`.
    pub fn set_smear(&mut self, s: [(f32, f32); 2]) {
        let c = |x: f32| clampf(x, 0.0, 2.0 * SMEAR_MAX);
        self.smear = s.map(|(a, b)| (c(a), c(b)));
    }

    /// Taps 1–11's read lengths.
    pub fn taps_at(&self) -> [u16; 11] {
        self.at
    }

    /// The crossfade running: each moving tap (1–11), from and to.
    pub fn crossfades(&self) -> impl Iterator<Item = (u8, u16, u16)> + '_ {
        (0..self.moving as usize).map(|m| {
            let t = self.mv[m];
            (t, self.at[t as usize - 1], self.to[m])
        })
    }

    /// One ring block of the decimated send `u` (1.0 full scale); `l` and
    /// `r` get the return in LSBs.
    #[inline(never)]
    pub fn run(
        &mut self,
        arena: &mut ReverbArena,
        u: &[f32; HALF],
        k: &EngineKnobs,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        let (lines, f, _) = arena.split(SLOTS, FSLOTS);
        self.block::<[i16], true>(lines, f, u, k, l, r);
    }

    /// `run` with its i16 lines as `lines`' floats (unquantised), and the
    /// re-entry clips off: the level plan's references.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(clippy::too_many_arguments)]
    pub fn run_reference(
        &mut self,
        arena: &mut ReverbArena,
        lines: Option<&mut [f32]>,
        clipped: bool,
        u: &[f32; HALF],
        k: &EngineKnobs,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        let (lo, f, _) = arena.split(SLOTS, FSLOTS);
        match (lines, clipped) {
            (Some(x), true) => self.block::<[f32], true>(x, f, u, k, l, r),
            (Some(x), false) => self.block::<[f32], false>(x, f, u, k, l, r),
            (None, true) => self.block::<[i16], true>(lo, f, u, k, l, r),
            (None, false) => self.block::<[i16], false>(lo, f, u, k, l, r),
        }
    }

    /// Up to `MOVERS` taps, the farthest from their targets, start a
    /// crossfade of at most `STEP` toward them; tap 11, REGEN's read, sits
    /// in slot 0 when it moves.
    #[inline(never)]
    fn schedule(&mut self, length_ms: f32) {
        let goal = tap_targets(length_ms);
        let mut n = 0;
        let mut taken = 0u16;
        while n < MOVERS {
            let (mut far, mut tap) = (0, 0);
            for t in 0..11 {
                let d = goal[t].abs_diff(self.at[t]);
                if taken & 1 << t == 0 && d > far {
                    (far, tap) = (d, t);
                }
            }
            if far == 0 {
                break;
            }
            taken |= 1 << tap;
            let step = far.min(STEP);
            let at = self.at[tap];
            self.to[n] = if goal[tap] > at { at + step } else { at - step };
            self.mv[n] = tap as u8 + 1;
            n += 1;
        }
        if let Some(m) = (0..n).find(|&m| self.mv[m] == 11) {
            self.mv.swap(0, m);
            self.to.swap(0, m);
        }
        for m in n..MOVERS {
            (self.mv[m], self.to[m]) = (1, self.at[0]);
        }
        self.moving = n as u8;
        self.left = if n > 0 { XF } else { 0 };
    }

    fn block<L: Lines + ?Sized, const CLIP: bool>(
        &mut self,
        lines: &mut L,
        f: &mut [f32],
        u: &[f32; HALF],
        k: &EngineKnobs,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        // Every unchecked access below leans on these two.
        assert!(lines.len() >= SLOTS && f.len() >= FSLOTS);
        if !self.primed {
            self.prime(k);
        }
        // Each scalar's value at the block's start, then its end: TONE's
        // coefficient, the pre-diffuser's g, REGEN, TAIL's return
        // (g·Σ|w|), 1/Σ|w|, the crossfade's blend, each side's SMEAR. The
        // eases: τ 50 ms for the gains, as QUAD's TIME; 20 ms the rest. The
        // weights step a block at a time, eased.
        let ends = |e: &Self| [e.ta, e.gp, e.gr, e.gt / e.inv_sw, e.inv_sw];
        let start = ends(self);
        const K20: f32 = ease_k(0.02);
        const K50: f32 = ease_k(0.05);
        self.ease_to(k, K20, K50, false);
        if self.left == 0 {
            self.schedule(k.length_ms);
        }
        let end = ends(self);
        let n = 1.0 / HALF as f32;
        let mut g = [
            start[0],
            start[1],
            start[2],
            start[3],
            start[4],
            // The crossfade's blend, once `schedule` has run.
            (XF - self.left) as f32 / XF as f32,
            self.smear[0].0,
            self.smear[1].0,
        ];
        let dg = [
            (end[0] - start[0]) * n,
            (end[1] - start[1]) * n,
            (end[2] - start[2]) * n,
            (end[3] - start[3]) * n,
            (end[4] - start[4]) * n,
            if self.left > 0 { 1.0 / XF as f32 } else { 0.0 },
            (self.smear[0].1 - self.smear[0].0) * n,
            (self.smear[1].1 - self.smear[1].0) * n,
        ];
        let c = self.w;
        let movers = self.moving as usize;
        let slot = |m: usize| (self.mv[m] as usize).clamp(1, 11);
        let cm: [f32; MOVERS] = core::array::from_fn(|m| c[slot(m)]);
        // Tap 11's blend reaches REGEN only when it moves.
        let e0 = (movers > 0 && self.mv[0] == 11) as u8 as f32;
        let ra = REGEN_A;
        // Each read's offset from the head.
        let w = I.0[0];
        let mut off = [0usize; ACCESS];
        off[..R_TAP].copy_from_slice(&[I.0[0], I.0[1], I.0[2], TAIL_RD[0], TAIL_RD[1]]);
        for t in 0..11 {
            off[R_TAP + t] = w - self.at[t] as usize;
        }
        for m in 0..MOVERS {
            off[R_TO + m] = w - self.to[m] as usize;
            off[R_FROM + m] = off[R_TAP + slot(m) - 1];
        }
        let (mut tone, mut regen) = (self.tone, self.regen);
        let (mut h, mut hf) = (self.h as usize % SLOTS, self.hf as usize % FSLOTS);
        let (mut b, mut fb) = ([0usize; ACCESS], [0usize; F_ACCESS]);
        let mut i = 0;
        while i < HALF {
            let len = segment(h, SLOTS, &off, &mut b, HALF - i);
            let len = segment(hf, FSLOTS, &F_OFF, &mut fb, len);
            for t in 0..len {
                macro_rules! rd {
                    ($b:expr) => {
                        // SAFETY: `segment` put each base below its
                        // region's length m (≤ the slice's, asserted
                        // above) and `len` at most m − base, and `t` <
                        // `len`. The writes likewise.
                        unsafe { lines.rd($b + t) }
                    };
                }
                macro_rules! fget {
                    ($b:expr) => {
                        // SAFETY: as `rd`'s, on the f32 region.
                        unsafe { *f.get_unchecked($b + t) }
                    };
                }
                macro_rules! fset {
                    ($b:expr, $v:expr) => {
                        // SAFETY: as `rd`'s, on the f32 region.
                        unsafe { *f.get_unchecked_mut($b + t) = $v }
                    };
                }
                let [ta, gp, gr, gts, isw, fr, ol, or] = g;
                tone += ta * (u[i + t] * IN_GAIN - tone);
                let (y, v) = ap_f32(tone, fget!(fb[5]), gp);
                fset!(fb[0], v);
                let (p, v) = ap_f32(y, fget!(fb[6]), gp);
                fset!(fb[1], v);
                // Taps 1–11, then the crossfading taps' blends,
                // o + f·(v − o): o is in the fixed sum.
                let mut sum = 0.0;
                for j in 0..11 {
                    sum += c[j + 1] * rd!(b[R_TAP + j]);
                }
                let mut t11 = rd!(b[R_TAP + 10]);
                for m in 0..movers {
                    let d = fr * (rd!(b[R_TO + m]) - rd!(b[R_FROM + m]));
                    sum += cm[m] * d;
                    if m == 0 {
                        t11 += e0 * d;
                    }
                }
                // REGEN: tap 11 through its one-pole into the line's
                // clipped, truncating write; tap 0 is that value.
                regen += ra * (t11 - regen);
                let x0 = clip::<CLIP>(p + gr * regen);
                // SAFETY: as `rd`'s.
                unsafe { lines.trunc(b[W_BURST] + t, x0) };
                sum += c[0] * x0;
                // TAIL: delay → normalised allpass → ×g, back into the sum.
                let (ty, tw) = ap_norm(
                    rd!(b[R_TAIL]),
                    rd!(b[R_TAIL + 1]),
                    TAIL_G,
                    1.0 / (1.0 - TAIL_G),
                );
                // SAFETY: as `rd`'s.
                unsafe { lines.trunc(b[W_TAIL_AP] + t, tw) };
                sum += gts * ty;
                // SAFETY: as `rd`'s.
                unsafe { lines.round(b[W_TAIL_D] + t, clip::<CLIP>(sum * isw)) };
                // The decorrelators: the shared allpass, then each side's
                // own, smeared. Two independent chains would leave the sides
                // correlated by about g⁴ (0.13), under the spike's 0.4–0.65;
                // the shared first puts them near g² (0.36), and each side
                // stays an allpass of the sum: no echo of it in either.
                let (a, v) = ap_f32(sum, fget!(fb[7]), DEC_G);
                fset!(fb[2], v);
                let (yl, v) = ap_f32(a, pair_f32(f, fb[3] + t, NOM[0] - ol), DEC_G);
                fset!(fb[3], v);
                let (yr, v) = ap_f32(a, pair_f32(f, fb[4] + t, NOM[1] - or), DEC_G);
                fset!(fb[4], v);
                (l[i + t], r[i + t]) = (yl, yr);
                for (x, d) in g.iter_mut().zip(&dg) {
                    *x += d;
                }
            }
            i += len;
            h = wrap(h + len, SLOTS);
            hf = wrap(hf + len, FSLOTS);
        }
        (self.tone, self.regen, self.h, self.hf) = (tone, regen, h as u16, hf as u16);
        if self.left > 0 {
            self.left -= HALF as u8;
            if self.left == 0 {
                for m in 0..movers {
                    self.at[slot(m) - 1] = self.to[m];
                }
                self.moving = 0;
            }
        }
    }
}
