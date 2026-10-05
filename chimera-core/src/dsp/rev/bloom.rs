//! BLOOM (ADR 0095 § BLOOM): the send, through TONE's one-pole and
//! PRE-DLY, enters a loop of 15 series unity allpasses with no plain
//! delay. The cascade's end comes back through a second one-pole on TONE
//! and DECAY's gain one sample later, as the unit's does, and leaves
//! through two 3-allpass decorrelators, L and R.
//!
//! SWELL scales the whole cascade, × 0.5–1.2, by pitch-free crossfades:
//! one allpass a crossfade, its read blended from the old length to the
//! new, at most 5 ms, with PRE-DLY in the same crossfades. SMEAR shortens
//! the three longest allpasses' reads, each on its own wander. The
//! cascade's states are normalised i16 (R2), settled as ROOM's re-entries
//! are; the decorrelators' are f32 (R4). Values are in LSBs.

// Indexed loops over a segment read as the graph does, several arrays at
// once.
#![allow(clippy::needless_range_loop)]

use core::f32::consts::LOG2_10;
#[cfg(any(test, feature = "test-support"))]
use core::ops::Range;

use crate::dsp::clampf;
use crate::dsp::halfband::HALF;
use crate::dsp::reverb::EngineKnobs;
use crate::dsp::ring::{FULL_SCALE, RING_LEN};

use super::arena::ReverbArena;
use super::parts::{
    Lines, Scratch, XFADE, XFADE_INV, ap_f32, ap_norm, clip, ease, ease_k, one_pole, pair_in, pow2,
    wrap,
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

// ADR 0095's tables, ms: structure numbers read from the MidiVerb II's
// Bloom programs (github.com/thement/midiverb_emulator's decompilation,
// GPL-3.0), used by the owner's approval of 2026-10-05. Only these
// tables carry them; nothing else here comes from the emulator.

/// The cascade's 15 allpasses, in signal order, at SWELL's × 1.
pub const CASCADE_MS: [f32; 15] = [
    0.56, 3.55, 4.96, 10.21, 14.49, 20.56, 23.72, 35.17, 42.69, 47.01, 57.56, 66.79, 78.68, 85.51,
    89.02,
];
/// The decorrelators' three allpasses, L then R, in signal order.
pub const DECORR_MS: [[f32; 3]; 2] = [[10.6, 2.1, 0.9], [6.3, 1.8, 2.4]];
/// The decorrelators' g.
pub const DECORR_G: f32 = 0.5;
// End of ADR 0095's tables.

/// SWELL's scale of the cascade at 0 and at 1.
pub const SWELL_LO: f32 = 0.5;
pub const SWELL_HI: f32 = 1.2;
/// PRE-DLY's longest, ms.
pub const PRE_MS: f32 = 125.0;
/// The plain allpasses, the cascade's first: the rest are smeared.
pub const PLAIN: usize = 12;
/// The smeared allpasses: the three longest, the cascade's last.
pub const SMEARED: [usize; 3] = [12, 13, 14];

/// The cascade at SWELL's × 1, ms.
pub const TOTAL_MS: f32 = {
    let (mut s, mut j) = (0.0, 0);
    while j < 15 {
        s += CASCADE_MS[j];
        j += 1;
    }
    s
};

/// `ms` at 24 kHz, to the nearest sample.
pub const fn samples(ms: f32) -> usize {
    (ms * PER_MS + 0.5) as usize
}

/// The cascade's lengths at SWELL's scale `s`, ring samples: each table
/// length × `s`, to the nearest sample.
pub fn lengths_at(s: f32) -> [u16; 15] {
    CASCADE_MS.map(|ms| (ms * PER_MS * s + 0.5) as u16)
}

/// Each allpass's longest, at × `SWELL_HI`.
const MAX: [usize; 15] = {
    let mut o = [0; 15];
    let mut j = 0;
    while j < 15 {
        o[j] = (CASCADE_MS[j] * PER_MS * SWELL_HI + 0.5) as usize;
        j += 1;
    }
    o
};
pub const DECORR: [[usize; 3]; 2] = {
    let mut o = [[0; 3]; 2];
    let mut j = 0;
    while j < 6 {
        o[j / 3][j % 3] = samples(DECORR_MS[j / 3][j % 3]);
        j += 1;
    }
    o
};
pub const PRE: usize = samples(PRE_MS);

/// Slots each decorrelator line keeps past its length and 2: one block.
/// They run stage by stage over a segment (`ROOM`'s `GUARD`).
const GUARD: usize = HALF;

/// Where each line writes, from the shared heads: line j owns its longest,
/// 2 slots more (a smeared pair's second read), so a read d back, 1 ≤ d ≤
/// longest + 1, is what it wrote d samples ago. The i16 lines (PRE, the
/// cascade) run sample by sample and need no guard; the f32 decorrelators
/// a block's each.
struct Heads {
    pre: usize,
    ap: [usize; 15],
    slots: usize,
    decorr: [[usize; 3]; 2],
    fslots: usize,
}

const HEADS: Heads = {
    let mut o = PRE + 2;
    let pre = o - 1;
    let mut ap = [0; 15];
    let mut j = 0;
    while j < 15 {
        o += MAX[j] + 2;
        ap[j] = o - 1;
        j += 1;
    }
    let slots = o + o % 2;
    let (mut decorr, mut f) = ([[0; 3]; 2], 0);
    j = 0;
    while j < 6 {
        f += DECORR[j / 3][j % 3] + 2 + GUARD;
        decorr[j / 3][j % 3] = f - 1;
        j += 1;
    }
    Heads {
        pre,
        ap,
        slots,
        decorr,
        fslots: f,
    }
};

/// The i16 slots, and the f32s above them.
pub const SLOTS: usize = HEADS.slots;
pub const FSLOTS: usize = HEADS.fslots;

/// SMEAR's cap, a share of the loop (§ The SMEAR bound).
const SMEAR_CAP: f32 = crate::dsp::reverb::BLOOM.smear_cap;

const FITS: () = assert!(
    SLOTS + 2 * FSLOTS <= RING_LEN
        && SLOTS < u16::MAX as usize
        && FSLOTS < u16::MAX as usize
        // The smeared reads, `M − 2D` at the deepest D (twice the cap of
        // the shortest cascade), stay 2 or more behind their write:
        // `pair_in`'s contract.
        && {
            let (mut ok, mut s) = (true, 0);
            while s < 3 {
                let m = CASCADE_MS[SMEARED[s]] * SWELL_LO;
                ok &= (m - 2.0 * SMEAR_CAP * TOTAL_MS * SWELL_LO) * PER_MS >= 3.0;
                s += 1;
            }
            ok
        }
);

/// The new engines' gain cap (§ Shared machinery).
const MAX_GAIN: f32 = 0.985;
/// The send, ½ (§ Shared machinery), in LSBs.
const IN_GAIN: f32 = 0.5 * FULL_SCALE;
/// The most one crossfade moves PRE-DLY, and the cascade.
pub const STEP: u16 = 120;

/// A cascade allpass's slow modes: around its poles its group delay
/// reaches (1 + g)/(1 − g) of its length, and those modes hold the tail.
/// The loop rings as if `slow(g)` times its length: fitted to the 1 kHz
/// octave's RT60 over DIFFUSE's g 0.45–0.65, DECAY ½ and 1, and SWELL.
#[inline(always)]
fn slow(g: f32) -> f32 {
    let g2 = g * g;
    1.07 + 1.9 * g2 * g2
}

/// The one-pole `y += a·(x − y)`'s gain at 1 kHz: DECAY holds the 1 kHz
/// RT60 at every TONE.
fn gain_1k(a: f32) -> f32 {
    // cos(2π · 1 kHz / 24 kHz), to f32.
    const C: f32 = 0.965_925_8;
    let p = 1.0 - a;
    a / libm::sqrtf(1.0 - 2.0 * p * C + p * p)
}

/// BLOOM's state beside the arena. Zero bytes are a fresh engine, primed by
/// its first block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bloom {
    /// The shared write heads of the i16 and the f32 lines.
    h: u16,
    hf: u16,
    primed: bool,
    retarget: bool,
    /// Ring samples left in the length crossfade; 0 between.
    left: u16,
    /// The cascade's lengths and PRE-DLY, ring samples: landed,
    /// crossfading to, and the knobs' goal.
    at: [u16; 15],
    to: [u16; 15],
    goal: [u16; 15],
    pre_at: u16,
    pre_to: u16,
    pre_goal: u16,
    /// TONE's input one-pole and the loop's: unrounded.
    tone: f32,
    lp: f32,
    /// The eased TONE coefficient (both one-poles), the cascade's g and
    /// the feedback gain, one step a block.
    ta: f32,
    g: f32,
    fb: f32,
    /// What the eases head for, worked out again only when RT60, TONE,
    /// DIFFUSE (`key`'s bits) or the lengths (`retarget`) change.
    key: [u32; 3],
    t_ta: f32,
    t_fb: f32,
    /// This block's SMEAR offsets (start, end), ring samples: `D·(1 + x)`.
    smear: [(f32, f32); 3],
    /// A crossfade step other than `STEP`, when not 0 (tests): never in
    /// firmware.
    #[cfg(any(test, feature = "test-support"))]
    coarse: u16,
    /// Both one-poles open and no feedback (tests): never in firmware.
    #[cfg(any(test, feature = "test-support"))]
    open: bool,
}

/// Line `off`'s slot `d` behind its write at head `h` (mod `m`); `len`
/// shortened to the samples that run before it wraps.
#[inline(always)]
fn slot(h: usize, m: usize, off: usize, d: usize, len: &mut usize) -> usize {
    let b = wrap(h + off, m);
    let x = if b >= d { b - d } else { b + m - d };
    *len = (*len).min(m - x);
    x
}

impl Bloom {
    /// The knobs' goals: the cascade at SWELL's scale (from its length,
    /// `length_ms`), and PRE-DLY, in ring samples.
    fn goals(k: &EngineKnobs) -> ([u16; 15], u16) {
        let s = clampf(k.length_ms / TOTAL_MS, SWELL_LO, SWELL_HI);
        let pre = clampf(k.predelay_ms * PER_MS + 0.5, 2.0, PRE as f32) as u16;
        (lengths_at(s), pre)
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
    }

    /// Each eased value one block toward `k`. The feedback gain sets the
    /// 1 kHz RT60 over the cascade's slow length and through the loop's
    /// one-pole; its `exp2` runs only when what it reads changes.
    fn ease_to(&mut self, k: &EngineKnobs, k20: f32, k50: f32) {
        let key = [
            k.rt60_s.to_bits(),
            k.tone_hz.to_bits(),
            k.diffuse_g.to_bits(),
        ];
        if key != self.key || self.retarget {
            (self.key, self.retarget) = (key, false);
            self.t_ta = one_pole(k.tone_hz, FS);
            // The trip: the cascade's lengths and the one sample back.
            let trip = self.to.iter().map(|&d| d as u32).sum::<u32>() as f32 + 1.0;
            let per = -3.0 * LOG2_10 / (k.rt60_s.max(1e-3) * FS);
            self.t_fb = (pow2(per * trip * slow(k.diffuse_g)) / gain_1k(self.t_ta)).min(MAX_GAIN);
        }
        ease(&mut self.ta, self.t_ta, k20);
        ease(&mut self.g, k.diffuse_g, k20);
        ease(&mut self.fb, self.t_fb, k50);
    }

    /// The SMEAR offsets the next block reads its three smeared allpasses
    /// short by, (start, end) in ring samples: `D·(1 + x)`, each held to
    /// twice the cap.
    pub fn set_smear(&mut self, s: [(f32, f32); 3]) {
        self.smear = s;
    }

    /// The SMEAR offsets the last block read by, as held.
    pub fn smear(&self) -> [(f32, f32); 3] {
        self.smear
    }

    /// Step `step` ring samples a crossfade from now on (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn coarse_steps(&mut self, step: u16) {
        self.coarse = step;
    }

    /// Both one-poles open and the feedback cut, from now on (tests): the
    /// cascade and the decorrelators alone.
    #[cfg(any(test, feature = "test-support"))]
    pub fn open_loop(&mut self) {
        self.open = true;
    }

    /// The cached targets: TONE's coefficient and the feedback gain (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn targets(&self) -> (f32, f32) {
        (self.t_ta, self.t_fb)
    }

    /// The i16 lines' shared write head (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn head(&self) -> usize {
        self.h as usize
    }

    /// The i16 regions a level-plan test reads, from the head: PRE and the
    /// cascade (PRE's own head slot left out).
    #[cfg(any(test, feature = "test-support"))]
    pub fn regions() -> [Range<usize>; 2] {
        [0..HEADS.pre, HEADS.pre + 1..HEADS.ap[14] + 1]
    }

    /// The cascade's lengths, landed.
    pub fn lengths(&self) -> [u16; 15] {
        self.at
    }

    /// PRE-DLY, landed.
    pub fn predelay(&self) -> u16 {
        self.pre_at
    }

    /// The crossfade running: the cascade from and to, and PRE-DLY's.
    #[allow(clippy::type_complexity)]
    pub fn crossfade(&self) -> Option<([u16; 15], [u16; 15], (u16, u16))> {
        (self.left > 0).then_some((self.at, self.to, (self.pre_at, self.pre_to)))
    }

    /// Whether a length still moves toward the knobs.
    pub fn moving(&self) -> bool {
        self.left > 0 || self.at != self.goal || self.pre_at != self.pre_goal
    }

    /// The next crossfade toward the goals: the allpass farthest from its
    /// goal moves `STEP` (or what is left), PRE-DLY likewise.
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
        let mut far = 0;
        for j in 1..15 {
            if self.goal[j].abs_diff(self.at[j]) > self.goal[far].abs_diff(self.at[far]) {
                far = j;
            }
        }
        self.to = self.at;
        self.to[far] = toward(self.at[far], self.goal[far]);
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
        let (lines, f, _) = arena.split(SLOTS, FSLOTS);
        self.block::<[i16], true>(lines, f, u, k, l, r);
    }

    /// `run` with its i16 lines as `lines`' floats (unquantised), and the
    /// re-entry clip off unless `clipped`: the level plan's references.
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

    fn block<Ln: Lines + ?Sized, const CLIP: bool>(
        &mut self,
        lines: &mut Ln,
        f: &mut [f32],
        u: &[f32; HALF],
        k: &EngineKnobs,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        let () = FITS;
        // Every unchecked access below leans on these two.
        assert!(lines.len() >= SLOTS && f.len() >= FSLOTS);
        if !self.primed {
            self.prime(k);
        }
        (self.goal, self.pre_goal) = Self::goals(k);
        if self.left == 0 && self.moving() {
            self.schedule();
        }
        // TONE, g and the feedback step once a block on their eases (ADR
        // 0095, ROOM v2 as built): τ 50 ms for the gain, as QUAD's TIME;
        // 20 ms the rest. SMEAR ramps across the block.
        const K20: f32 = ease_k(0.02);
        const K50: f32 = ease_k(0.05);
        self.ease_to(k, K20, K50);
        #[allow(unused_mut)]
        let (mut ta, mut fb, g) = (self.ta, self.fb, self.g);
        #[cfg(any(test, feature = "test-support"))]
        if self.open {
            (ta, fb) = (1.0, 0.0);
        }
        let inv = 1.0 / (1.0 - g);
        let n = 1.0 / HALF as f32;
        // The crossfade's blend: the new read's share.
        let (mut w, dw) = if self.left > 0 {
            ((XFADE - self.left) as f32 * XFADE_INV, XFADE_INV)
        } else {
            (1.0, 0.0)
        };
        let (at, to) = (self.at, self.to);
        // The one allpass moving, 15 for none.
        let mv = (0..15).find(|&j| at[j] != to[j]).unwrap_or(15);
        let pre_moving = self.pre_at != self.pre_to;
        let (pre_at, pre_to) = (self.pre_at as usize, self.pre_to as usize);
        // SMEAR, held to twice the cap of the cascade at the shorter of
        // its lengths this block.
        let short = (0..15).map(|j| at[j].min(to[j]) as u32).sum::<u32>() as f32;
        let top = 2.0 * SMEAR_CAP * short;
        let off = self
            .smear
            .map(|(a, b)| (clampf(a, 0.0, top), clampf(b, 0.0, top)));
        self.smear = off;
        let mut sm = off.map(|o| o.0);
        let dsm = off.map(|o| (o.1 - o.0) * n);
        let (mut tone, mut lp) = (self.tone, self.lp);
        let (mut h, mut hf) = (self.h as usize % SLOTS, self.hf as usize % FSLOTS);
        // The cascade's end, for the decorrelators: set for t < len before
        // it is read.
        let mut ov = Scratch::<f32, HALF>::new();
        let mut i = 0;
        while i < HALF {
            let mut len = HALF - i;
            let wp = slot(h, SLOTS, HEADS.pre, 0, &mut len);
            let (pf, pt) = (
                slot(h, SLOTS, HEADS.pre, pre_at, &mut len),
                slot(h, SLOTS, HEADS.pre, pre_to, &mut len),
            );
            let wb: [usize; 15] =
                core::array::from_fn(|j| slot(h, SLOTS, HEADS.ap[j], 0, &mut len));
            let rb: [usize; PLAIN] =
                core::array::from_fn(|j| slot(h, SLOTS, HEADS.ap[j], at[j] as usize, &mut len));
            let tb = if mv < PLAIN {
                slot(h, SLOTS, HEADS.ap[mv], to[mv] as usize, &mut len)
            } else {
                0
            };
            let fw: [[usize; 3]; 2] = core::array::from_fn(|s| {
                core::array::from_fn(|j| slot(hf, FSLOTS, HEADS.decorr[s][j], 0, &mut len))
            });
            let fr: [[usize; 3]; 2] = core::array::from_fn(|s| {
                core::array::from_fn(|j| {
                    slot(hf, FSLOTS, HEADS.decorr[s][j], DECORR[s][j], &mut len)
                })
            });
            // A smeared read's pair sits in `wb − d − 1..wb + t` when its
            // write is past its deepest: no wrap to check.
            let fast: [bool; 3] = core::array::from_fn(|s| {
                let j = SMEARED[s];
                wb[j] > at[j].max(to[j]) as usize
            });
            let len = len;
            // The smeared three's writes and lengths.
            let sw: [usize; 3] = core::array::from_fn(|s| wb[SMEARED[s]]);
            let sa: [f32; 3] = core::array::from_fn(|s| at[SMEARED[s]] as f32);
            let st: [f32; 3] = core::array::from_fn(|s| to[SMEARED[s]] as f32);
            // This segment's send and return: `t` < `len` bounds all three.
            let (us, ls, rs) = (&u[i..i + len], &mut l[i..i + len], &mut r[i..i + len]);
            macro_rules! rd {
                ($i:expr) => {
                    // SAFETY: `slot` put each base below its region's
                    // length m (≤ the slice's, asserted above) and `len` at
                    // most m − base, and every `t` here is < `len`. The
                    // writes and the f32 accesses likewise.
                    unsafe { lines.rd($i) }
                };
            }
            for t in 0..len {
                // TONE, then PRE-DLY.
                tone += ta * (us[t] * IN_GAIN - tone);
                // SAFETY: as `rd`'s.
                unsafe { lines.round(wp + t, tone) };
                let mut x = rd!(pf + t);
                if pre_moving {
                    x += w * (rd!(pt + t) - x);
                }
                // The re-entry: the cascade's 15 allpasses invert as the
                // unit's do (its own sign each), so its gain subtracts.
                let mut a = clip::<CLIP>(x - fb * lp);
                macro_rules! ap {
                    ($j:expr, $z:expr) => {{
                        let (y, s) = ap_norm(a, $z, g, inv);
                        // SAFETY: as `rd`'s.
                        unsafe { lines.settle(wb[$j] + t, s) };
                        a = y;
                    }};
                }
                // The plain twelve: unrolled while none of them moves,
                // else up to the mover, the mover's blend, and on.
                if mv >= PLAIN {
                    unroll!(J in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] {
                        ap!(J, rd!(rb[J] + t));
                    });
                } else {
                    for j in 0..mv {
                        ap!(j, rd!(rb[j] + t));
                    }
                    let o = rd!(rb[mv] + t);
                    ap!(mv, o + w * (rd!(tb + t) - o));
                    for j in mv + 1..PLAIN {
                        ap!(j, rd!(rb[j] + t));
                    }
                }
                // The smeared three, unrolled: `S` is a literal.
                unroll!(S in [0, 1, 2] {
                    let pair = |d: f32| {
                        if fast[S] {
                            let di = d as usize;
                            let fr = d - di as f32;
                            // SAFETY: the write slot is past the deepest
                            // read (`fast`), so both slots sit in
                            // `sw − di − 1..sw + t`.
                            let (x0, x1) =
                                unsafe { (lines.rd(sw[S] + t - di), lines.rd(sw[S] + t - di - 1)) };
                            x0 + fr * (x1 - x0)
                        } else {
                            // SAFETY (`pair_in`): the write slot is below
                            // `SLOTS` (as `rd`'s); `sm` is clamped to [0,
                            // 2·cap·trip] above, and `FITS` keeps `d` in
                            // 2..`SLOTS`.
                            unsafe { pair_in(&*lines, SLOTS, sw[S] + t, d) }
                        }
                    };
                    let mut z = pair(sa[S] - sm[S]);
                    if mv == PLAIN + S {
                        z += w * (pair(st[S] - sm[S]) - z);
                    }
                    ap!(PLAIN + S, z);
                    sm[S] += dsm[S];
                });
                lp += ta * (a - lp);
                ov.set(t, a);
                w = (w + dw).min(1.0);
            }
            // The decorrelators, stage by stage: each side from the
            // cascade's end, its three allpasses in turn.
            for s in 0..2 {
                let out: &mut [f32] = if s == 0 { &mut *ls } else { &mut *rs };
                for t in 0..len {
                    // SAFETY: the loop above set every `t` < `len`.
                    out[t] = unsafe { ov.get(t) };
                }
                for j in 0..3 {
                    let (wf, rf) = (fw[s][j], fr[s][j]);
                    for t in 0..len {
                        // SAFETY: as `rd`'s, on the f32 region.
                        let z = unsafe { *f.get_unchecked(rf + t) };
                        let (y, v) = ap_f32(out[t], z, DECORR_G);
                        // SAFETY: as `rd`'s, on the f32 region.
                        unsafe { *f.get_unchecked_mut(wf + t) = v };
                        out[t] = y;
                    }
                }
            }
            i += len;
            h = wrap(h + len, SLOTS);
            hf = wrap(hf + len, FSLOTS);
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
