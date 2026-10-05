//! LOOP (reverb TYPEs spec § LOOP engine): ROOM's 2 loops × 2 stages and
//! HALL's 1 × 4 on one code path. The send, through TONE, PRE-DLY and a
//! four-allpass f32 diffuser, enters each loop at its own sign; a stage is
//! a delay then a normalised allpass, a damper follows every second
//! stage, and the stage gains carry the decay. ROOM's two loops re-enter
//! through an energy-preserving rotation, each past R1's clip. Three fixed
//! taps a side, from different stage delays, make the return.
//!
//! SIZE scales the stage delays only, by one k for all, and moves them by
//! pitch-free crossfades: one 5 ms slice a loop a crossfade, every loop in
//! the same one (§ Moving a length); PRE-DLY moves in the same crossfades.
//! SMEAR moves two stage allpasses, which never crossfade. The re-entries
//! and the allpass states write by `store_settle`. Laid out for the chip's
//! 24 kHz ring, as BURST is. Values are in LSBs.

// Indexed loops over loops and stages read as the spec's graph does.
#![allow(clippy::needless_range_loop)]

use core::f32::consts::LOG2_10;
#[cfg(any(test, feature = "test-support"))]
use core::ops::Range;

use crate::dsp::clampf;
use crate::dsp::halfband::HALF;
use crate::dsp::reverb::EngineKnobs;
use crate::dsp::ring::{FULL_SCALE, RING_LEN};

use super::arena::ReverbArena;
pub use super::parts::XFADE;
use super::parts::{
    Lines, XFADE_INV, ap_f32, ap_norm, ease_k, one_pole, pair_in, pow2, prime_ms, segment, wrap,
};

/// `$body` once per literal, `$k` that literal as a `const`: `ring.rs`'s.
macro_rules! unroll {
    ($k:ident in [$($n:literal),*] $body:block) => {
        $({
            const $k: usize = $n;
            $body
        })*
    };
}

/// The ring rate the lengths are laid out at.
const FS: f32 = 24_000.0;
/// Ring samples a millisecond.
pub const PER_MS: f32 = FS / 1000.0;

/// ROOM's coupling angle, rad: a light one (§ LOOP engine).
pub const THETA: f32 = 0.25;

/// (cos θ, sin θ), each by its series to θ¹⁹: exact in f32 for |θ| ≤ 1,
/// at compile time.
pub const fn rotation(theta: f32) -> (f32, f32) {
    let x = theta as f64;
    let (mut c, mut s, mut tc, mut ts) = (1.0, x, 1.0, x);
    let mut n = 1;
    while n < 10 {
        let k = 2.0 * n as f64;
        tc *= -x * x / ((k - 1.0) * k);
        ts *= -x * x / (k * (k + 1.0));
        c += tc;
        s += ts;
        n += 1;
    }
    (c as f32, s as f32)
}

const ROT: (f32, f32) = rotation(THETA);

/// ROOM's coupling: loop outputs (a, b) to the re-entries (c·a + s·b,
/// c·b − s·a). The engine and its energy test share it.
#[inline(always)]
pub fn rotate(a: f32, b: f32) -> (f32, f32) {
    let (c, s) = ROT;
    (c * a + s * b, c * b - s * a)
}

/// A LOOP TYPE's lines at SIZE 1, ring samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lens<const L: usize, const S: usize> {
    /// PRE-DLY's longest.
    pub pre: usize,
    /// The diffuser's four allpasses (f32).
    pub diffuser: [usize; 4],
    /// Each loop's stage delays, then its stage allpasses.
    pub delay: [[usize; S]; L],
    pub ap: [[usize; S]; L],
}

/// Where each line writes, from the shared heads: line j owns its length
/// and 2 slots more (a pair's second read, and the guard), so a read d
/// back, 2 ≤ d ≤ len + 1, is what it wrote d samples ago. The i16 lines
/// in `Lens`' order (PRE, delays, allpasses), then the f32 diffuser.
#[derive(Clone, Copy, Debug)]
pub struct Heads<const L: usize, const S: usize> {
    pre: usize,
    delay: [[usize; S]; L],
    ap: [[usize; S]; L],
    /// The i16 slots, even: the f32 region follows.
    slots: usize,
    diffuser: [usize; 4],
    fslots: usize,
}

const fn place<const L: usize, const S: usize>(n: &Lens<L, S>) -> Heads<L, S> {
    let mut o = n.pre + 2;
    let pre = o - 1;
    let (mut delay, mut ap) = ([[0; S]; L], [[0; S]; L]);
    let mut l = 0;
    while l < L {
        let mut s = 0;
        while s < S {
            o += n.delay[l][s] + 2;
            delay[l][s] = o - 1;
            s += 1;
        }
        l += 1;
    }
    l = 0;
    while l < L {
        let mut s = 0;
        while s < S {
            o += n.ap[l][s] + 2;
            ap[l][s] = o - 1;
            s += 1;
        }
        l += 1;
    }
    let slots = o + o % 2;
    let (mut diffuser, mut f) = ([0; 4], 0);
    let mut j = 0;
    while j < 4 {
        f += n.diffuser[j] + 2;
        diffuser[j] = f - 1;
        j += 1;
    }
    Heads {
        pre,
        delay,
        ap,
        slots,
        diffuser,
        fslots: f,
    }
}

/// A LOOP TYPE's graph: its lines, taps, smeared allpasses and send.
pub trait Layout<const L: usize, const S: usize> {
    const LENS: Lens<L, S>;
    /// Each side's three output taps: (loop, stage, ring samples behind
    /// that stage delay's write, sign). Fixed reads.
    const TAPS: [[(usize, usize, usize, f32); 3]; 2];
    /// The two smeared stage allpasses, (loop, stage).
    const SMEARED: [(usize, usize); 2];
    /// The send into each loop's first delay.
    const INJECT: [f32; L];
    /// SMEAR's cap, a share of the loop (§ The SMEAR bound).
    const SMEAR_CAP: f32;

    const HEADS: Heads<L, S> = place(&Self::LENS);
    /// The i16 lines' slots, and the f32 diffuser's f32s above them.
    const SLOTS: usize = Self::HEADS.slots;
    const FSLOTS: usize = Self::HEADS.fslots;
    /// The longest stage allpass, ring samples.
    const AP_MAX: f32 = {
        let (mut m, mut l) = (0, 0);
        while l < L {
            let mut s = 0;
            while s < S {
                if Self::LENS.ap[l][s] > m {
                    m = Self::LENS.ap[l][s];
                }
                s += 1;
            }
            l += 1;
        }
        m as f32
    };
    /// Loop 0's stage allpasses, and 1/its stage delays at SIZE 1: SIZE's
    /// k from its round trip.
    const AP0: f32 = {
        let (mut a, mut s) = (0, 0);
        while s < S {
            a += Self::LENS.ap[0][s];
            s += 1;
        }
        a as f32
    };
    const INV_D0: f32 = {
        let (mut d, mut s) = (0, 0);
        while s < S {
            d += Self::LENS.delay[0][s];
            s += 1;
        }
        1.0 / d as f32
    };
    /// Every access's offset from the head that no length moves: the
    /// writes, the stage allpasses' reads, the taps (`Access`). The ten
    /// PRE-DLY and delay reads are filled in each block.
    const OFFSETS: [usize; MAX_ACCESS] = fixed_offsets(&Self::HEADS, &Self::LENS, &Self::TAPS);
    /// The diffuser's four writes, then its four reads.
    const F_OFFSETS: [usize; 8] = {
        let mut o = [0; 8];
        let mut j = 0;
        while j < 4 {
            o[j] = Self::HEADS.diffuser[j];
            o[4 + j] = Self::HEADS.diffuser[j] - Self::LENS.diffuser[j];
            j += 1;
        }
        o
    };
    /// The i16 accesses (`Access`), every one in its segment.
    const ACCESS: usize = 9 + 5 * L * S;
    /// The smeared reads, `M − 2D` at the deepest D (twice the cap of the
    /// longest loop), stay 2 or more behind their write: `pair_in`'s
    /// contract.
    const SMEAR_FITS: bool = {
        let (mut m, mut ok) = (0, true);
        while m < 2 {
            let (l, st) = Self::SMEARED[m];
            let (mut trip, mut s) = (0, 0);
            while s < S {
                trip += Self::LENS.delay[l][s] + Self::LENS.ap[l][s];
                s += 1;
            }
            let d = Self::LENS.ap[l][st] as f32 - 2.0 * Self::SMEAR_CAP * trip as f32;
            ok &= d >= 2.0 && Self::LENS.ap[l][st] < Self::SLOTS;
            m += 1;
        }
        ok
    };
    const FITS: () = assert!(
        Self::SLOTS + 2 * Self::FSLOTS <= RING_LEN
            && Self::SLOTS < u16::MAX as usize
            && Self::ACCESS == MAX_ACCESS
            && S.is_multiple_of(2)
            && L * S == 2 * DAMPERS
            && Self::SMEAR_FITS
    );

    /// The i16 regions a level-plan test reads: PRE, the stage delays and
    /// the stage allpasses.
    #[cfg(any(test, feature = "test-support"))]
    fn regions() -> [Range<usize>; 3] {
        let h = Self::HEADS;
        let (d, a) = (h.delay[L - 1][S - 1] + 1, h.ap[L - 1][S - 1] + 1);
        [0..h.pre + 1, h.pre + 1..d, d..a]
    }
}

/// ROOM's and HALL's i16 accesses: both run 4 stages in all.
const MAX_ACCESS: usize = 29;
/// A damper after every second stage: 4 stages, 2 dampers.
const DAMPERS: usize = 2;

/// `Layout::OFFSETS`: the writes at their heads, each stage allpass read
/// its length back, each tap its own; the rest 0.
const fn fixed_offsets<const L: usize, const S: usize>(
    h: &Heads<L, S>,
    n: &Lens<L, S>,
    taps: &[[(usize, usize, usize, f32); 3]; 2],
) -> [usize; MAX_ACCESS] {
    let mut o = [0; MAX_ACCESS];
    o[Access::<L, S>::W_PRE] = h.pre;
    let mut l = 0;
    while l < L {
        let mut s = 0;
        while s < S {
            let j = l * S + s;
            o[Access::<L, S>::W_D + j] = h.delay[l][s];
            o[Access::<L, S>::W_AP + j] = h.ap[l][s];
            o[Access::<L, S>::R_AP + j] = h.ap[l][s] - n.ap[l][s];
            s += 1;
        }
        l += 1;
    }
    let mut side = 0;
    while side < 2 {
        let mut i = 0;
        while i < 3 {
            let (l, s, d, _) = taps[side][i];
            o[Access::<L, S>::R_TAP + 3 * side + i] = h.delay[l][s] - d;
            i += 1;
        }
        side += 1;
    }
    o
}

/// ROOM: loops A and B, two stages each (§ LOOP engine's table), SIZE's
/// k from 0.375 to 1 (A's round trip 100 → 200 ms).
pub type Room = LoopEngine<2, 2>;
/// HALL: one loop, four stages (Task 10).
pub type Hall = LoopEngine<1, 4>;

const ROOM_LENS: Lens<2, 2> = {
    let d = [prime_ms(2.3), prime_ms(3.7), prime_ms(5.9), prime_ms(8.9)];
    Lens {
        pre: (50.0 * PER_MS) as usize,
        diffuser: d,
        delay: [
            [prime_ms(59.0), prime_ms(101.0)],
            [prime_ms(73.0), prime_ms(95.0)],
        ],
        ap: [
            [prime_ms(17.0), prime_ms(23.0)],
            [prime_ms(13.0), prime_ms(19.0)],
        ],
    }
};

/// A tap `frac` of loop `l` stage `s`'s delay at SIZE 0 (k 0.375), or at
/// its longest.
const fn tap(
    l: usize,
    s: usize,
    frac: f32,
    of_size_0: bool,
    sign: f32,
) -> (usize, usize, usize, f32) {
    let len = ROOM_LENS.delay[l][s] as f32 * if of_size_0 { 0.375 } else { 1.0 };
    (l, s, (len * frac) as usize, sign)
}

impl Layout<2, 2> for Room {
    const LENS: Lens<2, 2> = ROOM_LENS;
    /// L: (A1, 0.10 of SIZE 0, +), (B2, 0.57, −), (A2, 0.83, +); R: (B1,
    /// 0.12 of SIZE 0, +), (A2, 0.53, −), (B2, 0.79, +).
    const TAPS: [[(usize, usize, usize, f32); 3]; 2] = [
        [
            tap(0, 0, 0.10, true, 1.0),
            tap(1, 1, 0.57, false, -1.0),
            tap(0, 1, 0.83, false, 1.0),
        ],
        [
            tap(1, 0, 0.12, true, 1.0),
            tap(0, 1, 0.53, false, -1.0),
            tap(1, 1, 0.79, false, 1.0),
        ],
    ];
    /// A2's and B1's: one a loop.
    const SMEARED: [(usize, usize); 2] = [(0, 1), (1, 0)];
    /// +½ into A, −½ into B, as QUAD injects into S1 and S3.
    const INJECT: [f32; 2] = [0.5, -0.5];
    const SMEAR_CAP: f32 = crate::dsp::reverb::ROOM.smear_cap;
}

/// The new engines' stage gain cap (§ Shared machinery).
const MAX_GAIN: f32 = 0.985;
/// The send's injection, ½ (§ Shared machinery), in LSBs.
const IN_GAIN: f32 = 0.5 * FULL_SCALE;
const INV_FS: f32 = 1.0 / FULL_SCALE;
/// The most one crossfade moves PRE-DLY, and a loop's round trip.
pub const STEP: u16 = 120;

/// A LOOP TYPE's state beside the arena. Zero bytes are a fresh engine,
/// primed by its first block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopEngine<const L: usize, const S: usize> {
    /// The shared write heads of the i16 and the f32 lines.
    h: u16,
    hf: u16,
    primed: bool,
    /// Ring samples left in the length crossfade; 0 between.
    left: u16,
    /// The stage delays and PRE-DLY, ring samples: landed, crossfading
    /// to, and the knobs' goal.
    at: [[u16; S]; L],
    to: [[u16; S]; L],
    goal: [[u16; S]; L],
    pre_at: u16,
    pre_to: u16,
    pre_goal: u16,
    tone: f32,
    /// The eased TONE coefficient, the diffuser's and the stage allpasses'
    /// g, the damper's coefficient and the stage gains: each block ramps
    /// from last block's.
    ta: f32,
    gd: f32,
    gs: f32,
    da: f32,
    g: [[f32; S]; L],
    /// 1/(1 − `gs`), as the last block ended.
    inv: f32,
    /// What the eases head for, worked out again only when RT60, TONE,
    /// the damper (`key`'s bits) or the lengths (`retarget`) change: the
    /// coefficients, the stage allpasses' cap and the stage gains.
    key: [u32; 3],
    retarget: bool,
    t_ta: f32,
    t_da: f32,
    t_cap: f32,
    t_g: [[f32; S]; L],
    /// The dampers, one after every second stage, rounded to the LSB.
    lp: [f32; DAMPERS],
    /// This block's SMEAR offsets (start, end), ring samples: `D·(1 + x)`.
    smear: [(f32, f32); 2],
    /// A crossfade step other than `STEP`, when not 0 (the slap test's
    /// negative control): never in firmware.
    #[cfg(any(test, feature = "test-support"))]
    coarse: u16,
}

impl<const L: usize, const S: usize> Default for LoopEngine<L, S> {
    fn default() -> Self {
        Self {
            h: 0,
            hf: 0,
            primed: false,
            left: 0,
            at: [[0; S]; L],
            to: [[0; S]; L],
            goal: [[0; S]; L],
            pre_at: 0,
            pre_to: 0,
            pre_goal: 0,
            tone: 0.0,
            ta: 0.0,
            gd: 0.0,
            gs: 0.0,
            da: 0.0,
            g: [[0.0; S]; L],
            inv: 0.0,
            key: [0; 3],
            retarget: false,
            t_ta: 0.0,
            t_da: 0.0,
            t_cap: 0.0,
            t_g: [[0.0; S]; L],
            lp: [0.0; DAMPERS],
            smear: [(0.0, 0.0); 2],
            #[cfg(any(test, feature = "test-support"))]
            coarse: 0,
        }
    }
}

/// One block's one-pole toward `t`, `k` the share kept (0 lands).
fn ease(x: &mut f32, t: f32, k: f32) {
    *x = t + k * (*x - t);
}

/// R1's clip (`parts::soft_clip`) on an LSB value, its knee and ceiling
/// in LSBs: no scaling in and out.
#[inline(always)]
fn clip<const C: bool>(x: f32) -> f32 {
    if C {
        let a = x.abs();
        let u = clampf(a - FULL_SCALE, 0.0, 2.0 * FULL_SCALE);
        (a.min(FULL_SCALE) + u - (0.25 * INV_FS) * u * u).copysign(x)
    } else {
        x
    }
}

/// The i16 accesses, as offsets from the shared head: the writes (PRE,
/// the delays, the allpasses), PRE's new and old read, the delays' new and
/// old reads, the allpasses' fixed reads, the six taps. Each per-line
/// group is `L·S` long, loop-major.
struct Access<const L: usize, const S: usize>;

impl<const L: usize, const S: usize> Access<L, S> {
    const W_PRE: usize = 0;
    const W_D: usize = 1;
    const W_AP: usize = 1 + L * S;
    const R_PRE: usize = 1 + 2 * L * S;
    const R_D_TO: usize = 3 + 2 * L * S;
    const R_D_FROM: usize = 3 + 3 * L * S;
    const R_AP: usize = 3 + 4 * L * S;
    const R_TAP: usize = 3 + 5 * L * S;
}

impl<const L: usize, const S: usize> LoopEngine<L, S>
where
    Self: Layout<L, S>,
{
    /// The stage delays at `k`, ring samples.
    fn delays_at(k: f32) -> [[u16; S]; L] {
        Self::LENS
            .delay
            .map(|l| l.map(|d| ((d as f32 * k + 0.5) as u16).clamp(2, d as u16)))
    }

    /// The knobs' goals: the stage delays at SIZE's k (loop 0's round
    /// trip is `length_ms`), and PRE-DLY, in ring samples.
    fn goals(k: &EngineKnobs) -> ([[u16; S]; L], u16) {
        let size = clampf((k.length_ms * PER_MS - Self::AP0) * Self::INV_D0, 0.0, 1.0);
        let pre = clampf(k.predelay_ms * PER_MS + 0.5, 2.0, Self::LENS.pre as f32) as u16;
        (Self::delays_at(size), pre)
    }

    /// Land on `k` at once: a fresh engine's first block, which no one
    /// hears move.
    #[inline(never)]
    pub fn prime(&mut self, k: &EngineKnobs) {
        let (d, pre) = Self::goals(k);
        (self.at, self.to, self.goal) = (d, d, d);
        (self.pre_at, self.pre_to, self.pre_goal) = (pre, pre, pre);
        (self.left, self.primed, self.retarget) = (0, true, true);
        self.ease_to(k, 0.0, 0.0);
        self.inv = 1.0 / (1.0 - self.gs);
    }

    /// Each eased value one block toward `k`: TONE's and the damper's
    /// coefficients, the g, and each stage's gain for `k`'s RT60 at the
    /// lengths it heads for. The stage allpasses take DIFFUSE's g held to
    /// the longest one's stage gain, so its own ring never outlasts the
    /// RT60: no `t_min` floors DECAY (it binds under about 0.33 s, DIFFUSE
    /// 1 at 0.2 s rang 0.37 s without it). The targets' `exp2`s run only
    /// when what they read changes.
    fn ease_to(&mut self, k: &EngineKnobs, k20: f32, k50: f32) {
        let key = [k.rt60_s.to_bits(), k.tone_hz.to_bits(), k.damp_hz.to_bits()];
        if key != self.key || self.retarget {
            (self.key, self.retarget) = (key, false);
            self.t_ta = one_pole(k.tone_hz, FS);
            self.t_da = one_pole(k.damp_hz, FS);
            let per = -3.0 * LOG2_10 / (k.rt60_s.max(1e-3) * FS);
            self.t_cap = pow2(per * Self::AP_MAX);
            let d = self.to;
            for l in 0..L {
                for s in 0..S {
                    let len = (d[l][s] as usize + Self::LENS.ap[l][s]) as f32;
                    self.t_g[l][s] = pow2(per * len).min(MAX_GAIN);
                }
            }
        }
        ease(&mut self.ta, self.t_ta, k20);
        ease(&mut self.gd, k.diffuse_g, k20);
        ease(&mut self.da, self.t_da, k20);
        ease(&mut self.gs, k.diffuse_g.min(self.t_cap), k20);
        for l in 0..L {
            for s in 0..S {
                ease(&mut self.g[l][s], self.t_g[l][s], k50);
            }
        }
    }

    /// The SMEAR offsets the next block reads its two smeared allpasses
    /// short by, (start, end) in ring samples: `D·(1 + x)`, each held to
    /// twice its loop's cap.
    pub fn set_smear(&mut self, s: [(f32, f32); 2]) {
        self.smear = s;
    }

    /// Step `step` ring samples a crossfade from now on: the slap a 5 ms
    /// step avoids, for the no-slap test to hear.
    #[cfg(any(test, feature = "test-support"))]
    pub fn coarse_steps(&mut self, step: u16) {
        self.coarse = step;
    }

    /// The SMEAR offsets the last block read by, as held.
    pub fn smear(&self) -> [(f32, f32); 2] {
        self.smear
    }

    /// The stage delays, landed.
    pub fn delays(&self) -> [[u16; S]; L] {
        self.at
    }

    /// PRE-DLY, landed.
    pub fn predelay(&self) -> u16 {
        self.pre_at
    }

    /// The crossfade running: the stage delays from and to, and PRE-DLY's.
    #[allow(clippy::type_complexity)]
    pub fn crossfade(&self) -> Option<([[u16; S]; L], [[u16; S]; L], (u16, u16))> {
        (self.left > 0).then_some((self.at, self.to, (self.pre_at, self.pre_to)))
    }

    /// Whether a length still moves toward the knobs.
    pub fn moving(&self) -> bool {
        self.left > 0 || self.at != self.goal || self.pre_at != self.pre_goal
    }

    /// The next crossfade toward the goals: in each loop, the stage delay
    /// farthest from its goal moves `STEP` (or what is left), so a loop's
    /// round trip moves by one slice of at most 5 ms (§ Moving a length),
    /// every loop in the same crossfade; PRE-DLY likewise. Two delays of a
    /// loop stepping unrelated amounts in one crossfade (k's way) left two
    /// combs where the spec has one at 200 Hz spacing, and scrambled a
    /// held tone's phase: its tail's peak sat 28 Hz off at a 5 Hz spacing.
    #[inline(never)]
    fn schedule(&mut self) {
        #[cfg(any(test, feature = "test-support"))]
        let step = if self.coarse > 0 { self.coarse } else { STEP };
        #[cfg(not(any(test, feature = "test-support")))]
        let step = STEP;
        let toward = |at: u16, goal: u16| {
            let d = goal.abs_diff(at).min(step);
            if goal > at { at + d } else { at - d }
        };
        self.to = self.at;
        for l in 0..L {
            let mut far = 0;
            for s in 1..S {
                if self.goal[l][s].abs_diff(self.at[l][s])
                    > self.goal[l][far].abs_diff(self.at[l][far])
                {
                    far = s;
                }
            }
            self.to[l][far] = toward(self.at[l][far], self.goal[l][far]);
        }
        self.pre_to = toward(self.pre_at, self.pre_goal);
        if self.to != self.at || self.pre_to != self.pre_at {
            (self.left, self.retarget) = (XFADE, true);
        }
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
        let (lines, f, _) = arena.split(Self::SLOTS, Self::FSLOTS);
        self.block::<[i16], true>(lines, f, u, k, l, r);
    }

    /// `run` with its i16 lines as `lines`' floats (unquantised), and the
    /// re-entry clips off unless `clipped`: the level plan's references.
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
        let (lo, f, _) = arena.split(Self::SLOTS, Self::FSLOTS);
        match (lines, clipped) {
            (Some(x), true) => self.block::<[f32], true>(x, f, u, k, l, r),
            (Some(x), false) => self.block::<[f32], false>(x, f, u, k, l, r),
            (None, true) => self.block::<[i16], true>(lo, f, u, k, l, r),
            (None, false) => self.block::<[i16], false>(lo, f, u, k, l, r),
        }
    }

    /// Each access's offset from the head this block: `OFFSETS`, and the
    /// ten reads the lengths move.
    fn offsets(&self) -> [usize; MAX_ACCESS] {
        type A<const L: usize, const S: usize> = Access<L, S>;
        let h = &Self::HEADS;
        let mut o = Self::OFFSETS;
        o[A::<L, S>::R_PRE] = h.pre - self.pre_to as usize;
        o[A::<L, S>::R_PRE + 1] = h.pre - self.pre_at as usize;
        for l in 0..L {
            for s in 0..S {
                let j = l * S + s;
                o[A::<L, S>::R_D_TO + j] = h.delay[l][s] - self.to[l][s] as usize;
                o[A::<L, S>::R_D_FROM + j] = h.delay[l][s] - self.at[l][s] as usize;
            }
        }
        o
    }

    fn block<Ln: Lines + ?Sized, const CLIP: bool>(
        &mut self,
        lines: &mut Ln,
        f: &mut [f32],
        u: &[f32; HALF],
        k: &EngineKnobs,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        type A<const L: usize, const S: usize> = Access<L, S>;
        let () = Self::FITS;
        let (slots, fslots) = (Self::SLOTS, Self::FSLOTS);
        // Every unchecked access below leans on these two.
        assert!(lines.len() >= slots && f.len() >= fslots);
        if !self.primed {
            self.prime(k);
        }
        (self.goal, self.pre_goal) = Self::goals(k);
        if self.left == 0 && self.moving() {
            self.schedule();
        }
        // Each scalar at the block's start, then its end: TONE's
        // coefficient, the allpass g and 1/(1 − g), the damper's
        // coefficient, the stage gains. The eases: τ 50 ms for the gains,
        // as QUAD's TIME; 20 ms the rest.
        let (ta0, gd0, gs0, da0, g0) = (self.ta, self.gd, self.gs, self.da, self.g);
        const K20: f32 = ease_k(0.02);
        const K50: f32 = ease_k(0.05);
        self.ease_to(k, K20, K50);
        let n = 1.0 / HALF as f32;
        let (mut ta, dta) = (ta0, (self.ta - ta0) * n);
        let (mut gd, dgd) = (gd0, (self.gd - gd0) * n);
        let (mut gs, dgs) = (gs0, (self.gs - gs0) * n);
        let inv0 = self.inv;
        if self.gs != gs0 {
            self.inv = 1.0 / (1.0 - self.gs);
        }
        let inv1 = self.inv;
        let (mut inv, dinv) = (inv0, (inv1 - inv0) * n);
        let (mut da, dda) = (da0, (self.da - da0) * n);
        let mut g = g0;
        let dg: [[f32; S]; L] =
            core::array::from_fn(|l| core::array::from_fn(|s| (self.g[l][s] - g0[l][s]) * n));
        // The crossfade's blend: the new read's share.
        let (mut w, dw) = if self.left > 0 {
            ((XFADE - self.left) as f32 * XFADE_INV, XFADE_INV)
        } else {
            (1.0, 0.0)
        };
        // SMEAR, held to twice each loop's cap at the shorter of its
        // lengths this block.
        let short: [[u16; S]; L] =
            core::array::from_fn(|l| core::array::from_fn(|s| self.at[l][s].min(self.to[l][s])));
        let off: [(f32, f32); 2] = core::array::from_fn(|m| {
            let lp = Self::SMEARED[m].0;
            let trip = (0..S)
                .map(|s| (short[lp][s] as usize + Self::LENS.ap[lp][s]) as f32)
                .sum::<f32>();
            let top = 2.0 * Self::SMEAR_CAP * trip;
            let (a, b) = self.smear[m];
            (clampf(a, 0.0, top), clampf(b, 0.0, top))
        });
        self.smear = off;
        let mut sm = [off[0].0, off[1].0];
        let dsm = [(off[0].1 - off[0].0) * n, (off[1].1 - off[1].0) * n];
        let nom = Self::SMEARED.map(|(l, s)| Self::LENS.ap[l][s] as f32);
        const { assert!(L <= 2 && S <= 4) };
        let off_i = self.offsets();
        let off_f: [usize; 8] = Self::F_OFFSETS;
        let (mut tone, mut lp) = (self.tone, self.lp);
        let (mut h, mut hf) = (self.h as usize % slots, self.hf as usize % fslots);
        let mut i = 0;
        while i < HALF {
            let (b, len) = segment(h, slots, &off_i, HALF - i);
            let (fb, len) = segment(hf, fslots, &off_f, len);
            // This segment's send and return: `t` < `len` bounds all three.
            let (us, ls, rs) = (&u[i..i + len], &mut l[i..i + len], &mut r[i..i + len]);
            for t in 0..len {
                macro_rules! rd {
                    ($j:expr) => {
                        // SAFETY: `segment` put each base below its
                        // region's length m (≤ the slice's, asserted
                        // above) and `len` at most m − base, and `t` <
                        // `len`. The writes likewise.
                        unsafe { lines.rd(b[$j] + t) }
                    };
                }
                macro_rules! fget {
                    ($j:expr) => {
                        // SAFETY: as `rd`'s, on the f32 region.
                        unsafe { *f.get_unchecked(fb[$j] + t) }
                    };
                }
                macro_rules! fset {
                    ($j:expr, $v:expr) => {
                        // SAFETY: as `rd`'s, on the f32 region.
                        unsafe { *f.get_unchecked_mut(fb[$j] + t) = $v }
                    };
                }
                macro_rules! blend {
                    ($to:expr, $from:expr) => {{
                        let o = rd!($from);
                        o + w * (rd!($to) - o)
                    }};
                }
                tone += ta * (us[t] * IN_GAIN - tone);
                // SAFETY: as `rd`'s.
                unsafe { lines.round(b[A::<L, S>::W_PRE] + t, tone) };
                let mut d = blend!(A::<L, S>::R_PRE, A::<L, S>::R_PRE + 1);
                unroll!(J in [0, 1, 2, 3] {
                    let (y, v) = ap_f32(d, fget!(4 + J), gd);
                    fset!(J, v);
                    d = y;
                });
                // Each loop's stages, unrolled: `LO` and `ST` are literals,
                // past `L` and `S` dead code (a runtime loop kept the arrays
                // on the stack and tested for SMEAR every stage).
                let mut out = [0.0f32; L];
                unroll!(LO in [0, 1] {
                    if LO < L {
                        unroll!(ST in [0, 1, 2, 3] {
                            if ST < S {
                                let j = LO * S + ST;
                                // Stage `ST`'s delay, 2 or more behind its
                                // write: before or after it, the same.
                                let x = blend!(A::<L, S>::R_D_TO + j, A::<L, S>::R_D_FROM + j);
                                // SAFETY (`pair_in`): the write slot is below
                                // `slots` (as `rd`'s); `sm` is clamped to
                                // [0, 2·cap·trip] above, and `SMEAR_FITS`
                                // keeps `nom − sm` in 2..`slots`.
                                let wd = if (LO, ST) == Self::SMEARED[0] {
                                    unsafe { pair_in(&*lines, slots, b[A::<L, S>::W_AP + j] + t, nom[0] - sm[0]) }
                                } else if (LO, ST) == Self::SMEARED[1] {
                                    unsafe { pair_in(&*lines, slots, b[A::<L, S>::W_AP + j] + t, nom[1] - sm[1]) }
                                } else {
                                    rd!(A::<L, S>::R_AP + j)
                                };
                                let (mut y, v) = ap_norm(x, wd, gs, inv);
                                // SAFETY: as `rd`'s.
                                unsafe { lines.settle(b[A::<L, S>::W_AP + j] + t, v) };
                                if ST % 2 == 1 {
                                    let q = j / 2;
                                    lp[q] = Ln::held(lp[q] + da * (y - lp[q]));
                                    y = lp[q];
                                }
                                y *= g[LO][ST];
                                if ST + 1 < S {
                                    // SAFETY: as `rd`'s.
                                    unsafe { lines.round(b[A::<L, S>::W_D + j + 1] + t, y) };
                                } else {
                                    out[LO] = y;
                                }
                            }
                        });
                    }
                });
                // The rotation (ROOM), the send, R1's clip at re-entry.
                let mut re = out;
                if L == 2 {
                    (re[0], re[1]) = rotate(out[0], out[1]);
                }
                unroll!(LO in [0, 1] {
                    if LO < L {
                        let x = clip::<CLIP>(re[LO] + Self::INJECT[LO] * d);
                        // SAFETY: as `rd`'s.
                        unsafe { lines.settle(b[A::<L, S>::W_D + LO * S] + t, x) };
                    }
                });
                let mut side = [0.0f32; 2];
                unroll!(SD in [0, 1] {
                    unroll!(Q in [0, 1, 2] {
                        side[SD] += Self::TAPS[SD][Q].3 * rd!(A::<L, S>::R_TAP + 3 * SD + Q);
                    });
                });
                (ls[t], rs[t]) = (side[0], side[1]);
                ta += dta;
                gd += dgd;
                gs += dgs;
                inv += dinv;
                da += dda;
                unroll!(LO in [0, 1] {
                    unroll!(ST in [0, 1, 2, 3] {
                        if LO < L && ST < S {
                            g[LO][ST] += dg[LO][ST];
                        }
                    });
                });
                w = (w + dw).min(1.0);
                sm = [sm[0] + dsm[0], sm[1] + dsm[1]];
            }
            i += len;
            h = wrap(h + len, slots);
            hf = wrap(hf + len, fslots);
        }
        (self.tone, self.lp, self.h, self.hf) = (tone, lp, h as u16, hf as u16);
        if self.left > 0 {
            self.left = self.left.saturating_sub(HALF as u16);
            if self.left == 0 {
                (self.at, self.pre_at) = (self.to, self.pre_to);
            }
        }
    }
}
