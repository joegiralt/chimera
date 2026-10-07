//! BURST (reverb TYPEs spec § BURST engine; ADR 0095 § GATE and REVERSE,
//! as built): GATE and REVERSE, each its own loop over one store. The
//! send, through TONE, is written to the burst line; taps over it sum
//! under their curve's weights, then a diffuser of f32 allpasses, then two
//! f32 allpasses a side make the stereo, each side's second smeared.
//!
//! - **GATE:** 12 taps over [PRE-DLY, PRE-DLY + LENGTH], falling by SHAPE,
//!   DENSITY fading six in; TAIL's loop (delay → normalised allpass →
//!   DAMP's one-pole → ×g) adds back into the sum; five allpasses, their
//!   share against the dry sum DENSITY's.
//! - **REVERSE:** 16 taps from ONSET to 2.7 ms before LENGTH, rising by
//!   CURVE along (1 − x)^1.6; two allpasses; and at LENGTH the hit, an
//!   undiffused read 9 dB over the top tap, centred, which REGEN feeds
//!   back flat.
//!
//! Laid out for the chip's 24 kHz ring: at another bus rate every time
//! scales with it, as QUAD's lengths do. Values are in LSBs.

// Indexed loops over the taps read as the spec's sums do.
#![allow(clippy::needless_range_loop)]

use core::f32::consts::{LOG2_10, LOG2_E, TAU};

use crate::dsp::algo::math::{exp2, inv_sqrt};
use crate::dsp::clampf;
use crate::dsp::halfband::HALF;
use crate::dsp::reverb::{Burst, EngineKnobs};
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

/// Each of `N` taps' jitter j_k, within ±0.3 and 0 at the ends, and its
/// sign: two LCG draws a tap from `seed`. The jitter breaks the comb a
/// regular spacing would ring.
pub const fn tap_table<const N: usize>(seed: u32) -> [(f32, i8); N] {
    let mut t = [(0.0f32, 1i8); N];
    let mut x = seed;
    let mut k = 0;
    while k < N {
        x = lcg(x);
        let j = if k == 0 || k == N - 1 {
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

/// GATE's tap count, and REVERSE's ramp's.
pub const GATE_TAPS: usize = 12;
pub const REVERSE_TAPS: usize = 16;
/// The most taps a burst reads; the hit's (REGEN's) read follows them.
const MAX_TAPS: usize = REVERSE_TAPS;
pub const HIT: usize = MAX_TAPS;
/// Every read the scheduler moves: the taps, then the hit.
pub const READS: usize = MAX_TAPS + 1;

/// Each tap's place along its span, (k + j_k)/(N − 1): 0 at the first, 1
/// at the last.
const fn places<const N: usize>() -> [f32; N] {
    let t = tap_table::<N>(TAP_SEED);
    let mut p = [0.0; N];
    let mut k = 0;
    while k < N {
        p[k] = (k as f32 + t[k].0) / (N - 1) as f32;
        k += 1;
    }
    p
}

const GATE_TABLE: [(f32, i8); GATE_TAPS] = tap_table(TAP_SEED);
const REVERSE_TABLE: [(f32, i8); REVERSE_TAPS] = tap_table(TAP_SEED);
const GATE_PLACE: [f32; GATE_TAPS] = places();
const REVERSE_PLACE: [f32; REVERSE_TAPS] = places();

/// The burst line: REVERSE's longest LENGTH, 800 ms; GATE's 600 ms + its
/// 80 ms PRE-DLY fits inside.
pub const BURST_MAX: usize = 19_200;
/// REVERSE's ramp ends this before the hit (the unit's 2.7 ms gap).
const HIT_GAP: f32 = 2.7 * PER_MS;
/// The hit over the ramp's top tap: +9 dB (ADR 0095).
const HIT_GAIN: f32 = 2.818_383;
/// REVERSE's curve exponent: the ramp's last stretch louder.
const GAMMA: f32 = 1.6;

/// TAIL's loop stage: a 37 ms delay, then an 11 ms allpass.
const TAIL_D: usize = prime_ms(37.0);
const TAIL_AP: usize = prime_ms(11.0);
const LINES: [usize; 3] = [BURST_MAX, TAIL_D, TAIL_AP];

/// The diffuser's allpasses, ms: REVERSE runs [1] and [2] (the original
/// two), GATE all five.
const DIFFUSER_MS: [f32; 5] = [2.3, 4.3, 7.1, 11.3, 16.6];
/// The f32 lines: the diffuser's five; the decorrelators, a 3.1 ms
/// allpass both sides share, then L 8.3 ms and R 6.1 ms.
const FLINES: [usize; 8] = [
    prime_ms(DIFFUSER_MS[0]),
    prime_ms(DIFFUSER_MS[1]),
    prime_ms(DIFFUSER_MS[2]),
    prime_ms(DIFFUSER_MS[3]),
    prime_ms(DIFFUSER_MS[4]),
    prime_ms(3.1),
    prime_ms(8.3),
    prime_ms(6.1),
];
/// The diffuser's lines, and the decorrelators', in `FLINES`.
const DIFF: usize = 0;
const DEC: usize = 5;

/// Which diffuser lines a burst runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diffuser {
    /// All five, mixed with the dry sum by DENSITY.
    Five,
    /// The original two (4.3 and 7.1 ms), fully diffused.
    Two,
}

impl Diffuser {
    /// The `FLINES` indices it runs.
    const fn lines(self) -> core::ops::Range<usize> {
        match self {
            Diffuser::Five => DIFF..DIFF + 5,
            Diffuser::Two => DIFF + 1..DIFF + 3,
        }
    }
}

/// A burst's shape at compile time: its taps, its diffuser, and whether
/// it runs TAIL or REGEN and the hit. One `block` a variant.
trait Variant {
    const BURST: Burst;
    const TAPS: usize;
    const DIFFUSER: Diffuser;
}

struct GateV;
struct ReverseV;

impl Variant for GateV {
    const BURST: Burst = Burst::Gate;
    const TAPS: usize = GATE_TAPS;
    const DIFFUSER: Diffuser = Diffuser::Five;
}

impl Variant for ReverseV {
    const BURST: Burst = Burst::Reverse;
    const TAPS: usize = REVERSE_TAPS;
    const DIFFUSER: Diffuser = Diffuser::Two;
}

const fn distinct() -> bool {
    let all = [
        TAIL_D, TAIL_AP, FLINES[0], FLINES[1], FLINES[2], FLINES[3], FLINES[4], FLINES[5],
        FLINES[6], FLINES[7],
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
const F: ([usize; 8], usize) = heads(FLINES);
/// The i16 lines' slots; the f32 lines' f32s above them.
pub const SLOTS: usize = I.1;
pub const FSLOTS: usize = F.1;
const _: () = assert!(SLOTS + 2 * FSLOTS <= RING_LEN);
const _: () = assert!(SLOTS < u16::MAX as usize);

/// TAIL's delay and allpass reads.
const TAIL_RD: [usize; 2] = [I.0[1] - TAIL_D, I.0[2] - TAIL_AP];
/// Each side's smeared decorrelator, its nominal length.
const NOM: [f32; 2] = [FLINES[DEC + 1] as f32, FLINES[DEC + 2] as f32];
/// Ring samples a millisecond.
pub const PER_MS: f32 = FS / 1000.0;
/// SMEAR's deepest D, ring samples: GATE's and REVERSE's 1 ms. Its reads
/// only shorten, by 2D at most.
pub const SMEAR_MAX: f32 = crate::dsp::reverb::GATE.smear_ms * PER_MS;
const _: () = assert!(NOM[0] - 2.0 * SMEAR_MAX >= 2.0);
const _: () = assert!(NOM[1] - 2.0 * SMEAR_MAX >= 2.0);
/// A tap's shortest read: the line's floor (`parts::heads`).
const MIN_READ: f32 = 2.0;

/// The decorrelators' g (§ BURST engine), and TAIL's allpass's.
const DEC_G: f32 = 0.6;
const TAIL_G: f32 = 0.5;
/// The new engines' stage gain cap (§ Shared machinery).
const MAX_GAIN: f32 = 0.985;
/// The send's injection, ½ (§ Shared machinery), in LSBs.
const IN_GAIN: f32 = 0.5 * FULL_SCALE;
const INV_FS: f32 = 1.0 / FULL_SCALE;

/// Ring samples a tap crossfade takes (4 blocks), the most one moves a
/// tap (5 ms, QUAD's step), and the most that move at once.
pub const XF: u8 = 128;
pub const STEP: u16 = 120;
pub const MOVERS: usize = 4;
const _: () = assert!((XF as usize).is_multiple_of(HALF));

/// DENSITY keeps these six of GATE's taps, and fades the other six in
/// this order: the span stays LENGTH at any DENSITY.
const ALWAYS: [usize; 6] = [0, 2, 4, 7, 9, 11];
const FADE: [usize; 6] = [5, 6, 3, 8, 1, 10];
const _: () = {
    let mut seen = [false; GATE_TAPS];
    let mut i = 0;
    while i < 6 {
        seen[ALWAYS[i]] = true;
        seen[FADE[i]] = true;
        i += 1;
    }
    let mut k = 0;
    while k < GATE_TAPS {
        assert!(seen[k]);
        k += 1;
    }
};

/// Every read's length for `k`, ring samples, each in
/// `MIN_READ`..=`BURST_MAX`: the taps (GATE's 12 or REVERSE's 16, the
/// rest at the last tap's), then the hit at LENGTH.
#[inline(never)]
pub fn tap_targets(k: &EngineKnobs) -> [u16; READS] {
    let len = k.length_ms * PER_MS;
    let start = k.predelay_ms * PER_MS;
    let at = |x: f32| clampf(x + 0.5, MIN_READ, BURST_MAX as f32) as u16;
    let mut t = [0; READS];
    match k.burst {
        Burst::Gate => {
            for i in 0..MAX_TAPS {
                t[i] = at(start + len * GATE_PLACE[i.min(GATE_TAPS - 1)]);
            }
            t[HIT] = t[GATE_TAPS - 1];
        }
        Burst::Reverse => {
            let span = (len - HIT_GAP - start).max(0.0);
            for i in 0..MAX_TAPS {
                t[i] = at(start + span * REVERSE_PLACE[i]);
            }
            t[HIT] = at(len);
        }
    }
    t
}

/// The signed tap weights for `k`'s curve and DENSITY (Σw² = 1 over the
/// taps; GATE's 12 then zeros), and the hit's (REVERSE).
pub fn tap_weights(k: &EngineKnobs) -> ([f32; MAX_TAPS], f32) {
    weights(k.shape_db, k.burst, k.taps)
}

/// GATE falls from tap 0 by |`shape_db`| end to end, DENSITY's six faded
/// taps coming in one after another over the knob, each across a third
/// of it (`taps` 6 → 12). REVERSE rises to its last tap as
/// −|`shape_db`|·(1 − x)^γ dB, x its index along the ramp, and its hit
/// sits `HIT_GAIN` over that.
#[inline(never)]
fn weights(shape_db: f32, burst: Burst, taps: f32) -> ([f32; MAX_TAPS], f32) {
    let c = libm::fabsf(shape_db) * (LOG2_10 / 20.0);
    let mut w = [0.0f32; MAX_TAPS];
    let hit;
    match burst {
        Burst::Gate => {
            let n = clampf(taps - 6.0, 0.0, 6.0);
            // Tap to tap, the curve's 1/11.
            let step = pow2(-c / (GATE_TAPS - 1) as f32);
            let mut g = 1.0;
            for x in w.iter_mut().take(GATE_TAPS) {
                *x = g;
                g *= step;
            }
            for (i, &k) in FADE.iter().enumerate() {
                w[k] *= clampf((n - 0.8 * i as f32) * 0.5, 0.0, 1.0);
            }
            let s = inv_sqrt(w.iter().map(|x| x * x).sum());
            for (x, t) in w.iter_mut().zip(&GATE_TABLE) {
                *x *= s * t.1 as f32;
            }
            hit = 0.0;
        }
        Burst::Reverse => {
            let last = (REVERSE_TAPS - 1) as f32;
            for (i, x) in w.iter_mut().enumerate() {
                let rest = 1.0 - i as f32 / last;
                *x = pow2(-c * pow2(GAMMA * super::parts::lg2(rest.max(1e-6))));
            }
            let s = inv_sqrt(w.iter().map(|x| x * x).sum());
            hit = w[REVERSE_TAPS - 1] * s * HIT_GAIN;
            for (x, t) in w.iter_mut().zip(&REVERSE_TABLE) {
                *x *= s * t.1 as f32;
            }
        }
    }
    (w, hit)
}

/// 1/Σ|w|: TAIL's input is the sum over it, so it stays ≤ 2.0 (§ Level
/// plan).
fn inv_abs_sum(w: &[f32; MAX_TAPS]) -> f32 {
    1.0 / w.iter().map(|x| libm::fabsf(*x)).sum::<f32>()
}

/// TAIL's loop gain for an RT60 of `rt60_s`, capped.
fn tail_gain(rt60_s: f32) -> f32 {
    let trip = (TAIL_D + TAIL_AP) as f32;
    pow2(-3.0 * LOG2_10 * trip / (rt60_s.max(1e-3) * FS)).min(MAX_GAIN)
}

/// DAMP's coefficient: GATE's TAIL one-pole at `hz`; 1 (open) at 0.
fn damp_coef(hz: f32) -> f32 {
    if hz > 0.0 {
        1.0 - exp2(-LOG2_E * TAU * hz / FS)
    } else {
        1.0
    }
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
    /// The reads (`at`'s indices) crossfading, and to where; the hit,
    /// when it moves, sits in slot 0.
    mv: [u8; MOVERS],
    to: [u16; MOVERS],
    /// Every read's length, ring samples: the taps, then the hit.
    at: [u16; READS],
    tone: f32,
    /// TAIL's damper state.
    damp: f32,
    /// The eased TONE coefficient, diffuser g, REGEN and TAIL gains, the
    /// diffused share, DAMP's coefficient, curve and DENSITY: each block
    /// ramps from last block's.
    ta: f32,
    gp: f32,
    gr: f32,
    gt: f32,
    wet: f32,
    td: f32,
    shape: f32,
    taps: f32,
    w: [f32; MAX_TAPS],
    hit: f32,
    inv_sw: f32,
    /// This block's SMEAR offsets (start, end), ring samples: `D·(1 + x)`.
    smear: [(f32, f32); 2],
}

/// The i16 accesses, as offsets from the shared head: the three writes,
/// TAIL's two reads, every read in `at`, then each crossfade slot's new
/// and old read (set each block).
const W_BURST: usize = 0;
const W_TAIL_D: usize = 1;
const W_TAIL_AP: usize = 2;
const R_TAIL: usize = 3;
const R_TAP: usize = 5;
const R_HIT: usize = R_TAP + HIT;
const R_TO: usize = R_TAP + READS;
const R_FROM: usize = R_TO + MOVERS;
const ACCESS: usize = R_FROM + MOVERS;
/// The f32 accesses: the eight writes, then the six fixed reads (the
/// diffuser's five, the shared decorrelator).
const F_ACCESS: usize = 14;
const F_OFF: [usize; F_ACCESS] = [
    F.0[0],
    F.0[1],
    F.0[2],
    F.0[3],
    F.0[4],
    F.0[5],
    F.0[6],
    F.0[7],
    F.0[0] - FLINES[0],
    F.0[1] - FLINES[1],
    F.0[2] - FLINES[2],
    F.0[3] - FLINES[3],
    F.0[4] - FLINES[4],
    F.0[5] - FLINES[5],
];
/// `F_OFF`'s first fixed read.
const F_RD: usize = 8;

impl BurstEngine {
    /// Land on `k` at once: a fresh engine's first block, which no one
    /// hears move.
    #[inline(never)]
    pub fn prime(&mut self, k: &EngineKnobs) {
        self.at = tap_targets(k);
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
        ease(&mut self.wet, k.wet, k20);
        ease(&mut self.td, damp_coef(k.damp_hz), k20);
        let curve = (self.shape, self.taps);
        ease(&mut self.shape, k.shape_db, k20);
        ease(&mut self.taps, k.taps, k20);
        if always || (self.shape, self.taps) != curve {
            (self.w, self.hit) = weights(self.shape, k.burst, self.taps);
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

    /// Every read's length: the taps, then the hit.
    pub fn taps_at(&self) -> [u16; READS] {
        self.at
    }

    /// The crossfade running: each moving read (`taps_at`'s index), from
    /// and to.
    pub fn crossfades(&self) -> impl Iterator<Item = (u8, u16, u16)> + '_ {
        (0..self.moving as usize).map(|m| {
            let t = self.mv[m];
            (t, self.at[t as usize], self.to[m])
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
        match k.burst {
            Burst::Gate => self.block::<[i16], true, GateV>(lines, f, u, k, l, r),
            Burst::Reverse => self.block::<[i16], true, ReverseV>(lines, f, u, k, l, r),
        }
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
        macro_rules! go {
            ($v:ty) => {
                match (lines, clipped) {
                    (Some(x), true) => self.block::<[f32], true, $v>(x, f, u, k, l, r),
                    (Some(x), false) => self.block::<[f32], false, $v>(x, f, u, k, l, r),
                    (None, true) => self.block::<[i16], true, $v>(lo, f, u, k, l, r),
                    (None, false) => self.block::<[i16], false, $v>(lo, f, u, k, l, r),
                }
            };
        }
        match k.burst {
            Burst::Gate => go!(GateV),
            Burst::Reverse => go!(ReverseV),
        }
    }

    /// Up to `MOVERS` reads, the farthest from their targets, start a
    /// crossfade of at most `STEP` toward them; the hit, REGEN's read,
    /// sits in slot 0 when it moves.
    #[inline(never)]
    fn schedule(&mut self, k: &EngineKnobs) {
        let goal = tap_targets(k);
        // A read the burst doesn't play (GATE's past its 12 taps, and its
        // hit) lands at once: nothing hears it.
        let plays = |t: usize| match k.burst {
            Burst::Gate => t < GATE_TAPS,
            Burst::Reverse => true,
        };
        for t in 0..READS {
            if !plays(t) {
                self.at[t] = goal[t];
            }
        }
        let mut n = 0;
        let mut taken = 0u32;
        while n < MOVERS {
            let (mut far, mut tap) = (0, 0);
            for t in 0..READS {
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
            self.mv[n] = tap as u8;
            n += 1;
        }
        if let Some(m) = (0..n).find(|&m| self.mv[m] as usize == HIT) {
            self.mv.swap(0, m);
            self.to.swap(0, m);
        }
        for m in n..MOVERS {
            (self.mv[m], self.to[m]) = (0, self.at[0]);
        }
        self.moving = n as u8;
        self.left = if n > 0 { XF } else { 0 };
    }

    fn block<L: Lines + ?Sized, const CLIP: bool, V: Variant>(
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
        let gate = V::BURST == Burst::Gate;
        // Each scalar's value at the block's start, then its end: TONE's
        // coefficient, the diffuser's g, REGEN, TAIL's return (g·Σ|w|),
        // 1/Σ|w|, the diffused share, DAMP, the crossfade's blend, each
        // side's SMEAR. The eases: τ 50 ms for the gains, as QUAD's TIME;
        // 20 ms the rest. The weights step a block at a time, eased.
        let ends = |e: &Self| [e.ta, e.gp, e.gr, e.gt / e.inv_sw, e.inv_sw, e.wet, e.td];
        let start = ends(self);
        const K20: f32 = ease_k(0.02);
        const K50: f32 = ease_k(0.05);
        self.ease_to(k, K20, K50, false);
        if self.left == 0 {
            self.schedule(k);
        }
        let end = ends(self);
        let n = 1.0 / HALF as f32;
        let mut g: [f32; 10] = core::array::from_fn(|i| match i {
            0..7 => start[i],
            // The crossfade's blend, once `schedule` has run.
            7 => (XF - self.left) as f32 / XF as f32,
            8 => self.smear[0].0,
            _ => self.smear[1].0,
        });
        let dg: [f32; 10] = core::array::from_fn(|i| match i {
            0..7 => (end[i] - start[i]) * n,
            7 => {
                if self.left > 0 {
                    1.0 / XF as f32
                } else {
                    0.0
                }
            }
            8 => (self.smear[0].1 - self.smear[0].0) * n,
            _ => (self.smear[1].1 - self.smear[1].0) * n,
        });
        let c = self.w;
        let wh = self.hit;
        let movers = self.moving as usize;
        let slot = |m: usize| (self.mv[m] as usize).min(HIT);
        // The hit's blend goes to the hit alone: it is no tap of the sum.
        let cm: [f32; MOVERS] = core::array::from_fn(|m| {
            let s = slot(m);
            if s == HIT { 0.0 } else { c[s] }
        });
        // The hit's blend reaches REGEN only when it moves.
        let e0 = (movers > 0 && slot(0) == HIT) as u8 as f32;
        // Each read's offset from the head.
        let w = I.0[0];
        let mut off = [0usize; ACCESS];
        off[..R_TAP].copy_from_slice(&[I.0[0], I.0[1], I.0[2], TAIL_RD[0], TAIL_RD[1]]);
        for t in 0..READS {
            off[R_TAP + t] = w - self.at[t] as usize;
        }
        for m in 0..MOVERS {
            off[R_TO + m] = w - self.to[m] as usize;
            off[R_FROM + m] = off[R_TAP + slot(m)];
        }
        let dl = V::DIFFUSER.lines();
        let (mut tone, mut damp) = (self.tone, self.damp);
        let (mut h, mut hf) = (self.h as usize % SLOTS, self.hf as usize % FSLOTS);
        let mut i = 0;
        while i < HALF {
            let (b, len) = segment(h, SLOTS, &off, HALF - i);
            let (fb, len) = segment(hf, FSLOTS, &F_OFF, len);
            let len = len.min(HALF - i);
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
                let [ta, gp, gr, gts, isw, wet, td, fr, ol, or] = g;
                tone += ta * (u[i + t] * IN_GAIN - tone);
                // The taps, then the crossfading reads' blends,
                // o + f·(v − o): o is in the fixed sum.
                let mut sum = 0.0;
                for j in 0..V::TAPS {
                    sum += c[j] * rd!(b[R_TAP + j]);
                }
                let mut hit = if gate { 0.0 } else { rd!(b[R_HIT]) };
                for m in 0..movers {
                    let d = fr * (rd!(b[R_TO + m]) - rd!(b[R_FROM + m]));
                    sum += cm[m] * d;
                    if m == 0 {
                        hit += e0 * d;
                    }
                }
                // The line: the TONE'd send, and REVERSE's REGEN (the
                // hit fed back, flat) into its clipped, truncating write,
                // the send at (1 − g) so a held tone stays at its level.
                let x0 = if gate {
                    tone
                } else {
                    clip::<CLIP>(tone + gr * (hit - tone))
                };
                // SAFETY: as `rd`'s.
                unsafe { lines.trunc(b[W_BURST] + t, x0) };
                if gate {
                    // TAIL: delay → normalised allpass → DAMP → ×g, back
                    // into the sum.
                    let (ty, tw) = ap_norm(
                        rd!(b[R_TAIL]),
                        rd!(b[R_TAIL + 1]),
                        TAIL_G,
                        1.0 / (1.0 - TAIL_G),
                    );
                    // SAFETY: as `rd`'s.
                    unsafe { lines.trunc(b[W_TAIL_AP] + t, tw) };
                    damp += td * (ty - damp);
                    sum += gts * damp;
                    // SAFETY: as `rd`'s.
                    unsafe { lines.round(b[W_TAIL_D] + t, clip::<CLIP>(sum * isw)) };
                }
                // The diffuser, after the sum: GATE's share of it against
                // the dry sum, REVERSE's whole.
                let mut d = sum;
                for j in dl.clone() {
                    let (y, v) = ap_f32(d, fget!(fb[F_RD + j]), gp);
                    fset!(fb[j], v);
                    d = y;
                }
                let s = if gate { sum + wet * (d - sum) } else { d };
                // The decorrelators: the shared allpass, then each side's
                // own, smeared. Two independent chains would leave the sides
                // correlated by about g⁴ (0.13), under the spike's 0.4–0.65;
                // the shared first puts them near g² (0.36), and each side
                // stays an allpass of the sum: no echo of it in either.
                let (a, v) = ap_f32(s, fget!(fb[F_RD + DEC]), DEC_G);
                fset!(fb[DEC], v);
                let (yl, v) = ap_f32(a, pair_f32(f, fb[DEC + 1] + t, NOM[0] - ol), DEC_G);
                fset!(fb[DEC + 1], v);
                let (yr, v) = ap_f32(a, pair_f32(f, fb[DEC + 2] + t, NOM[1] - or), DEC_G);
                fset!(fb[DEC + 2], v);
                // REVERSE's hit, centred and undiffused, as the unit's.
                let ht = if gate { 0.0 } else { wh * hit };
                (l[i + t], r[i + t]) = (yl + ht, yr + ht);
                for (x, d) in g.iter_mut().zip(&dg) {
                    *x += d;
                }
            }
            i += len;
            h = wrap(h + len, SLOTS);
            hf = wrap(hf + len, FSLOTS);
        }
        (self.tone, self.damp, self.h, self.hf) = (tone, damp, h as u16, hf as u16);
        if self.left > 0 {
            self.left -= HALF as u8;
            if self.left == 0 {
                for m in 0..movers {
                    self.at[slot(m)] = self.to[m];
                }
                self.moving = 0;
            }
        }
    }
}
