//! ROOM (ADR 0095 § ROOM v2): the send, through TONE, PRE-DLY and five
//! fixed f32 allpasses, enters four loops of two stages at +1. A stage is
//! a delay then an f32 allpass; each loop ends in a one-pole damper and the
//! stage gains carry the decay. The loops re-enter in pairs, (A, B) and
//! (C, D), through a reflection of angle θ (LOOPS): θ 0 is four 2-stage
//! loops, θ 90° two 4-stage ones. Eight taps a side, one a stage line,
//! make the return.
//!
//! SIZE scales the stage delays and the taps by one k, 0.4 → 1, and moves
//! them by pitch-free crossfades, LOOP's scheduler (ADR 0085, ROOM as
//! built): one stage delay a loop a crossfade, at most 5 ms, every loop
//! in the same one, each tap with its line. SMEAR moves one stage allpass
//! a loop. Its own module: HALL's engine runs a third of the accesses, and
//! sharing their tables would size HALL's to ROOM's. Values are in LSBs.

// Indexed loops over a segment read as the graph does, several arrays at
// once.
#![allow(clippy::needless_range_loop)]

use core::f32::consts::LOG2_10;
use core::mem::MaybeUninit;
#[cfg(any(test, feature = "test-support"))]
use core::ops::Range;

use crate::dsp::clampf;
use crate::dsp::halfband::HALF;
use crate::dsp::reverb::EngineKnobs;
use crate::dsp::ring::{FULL_SCALE, RING_LEN};

use super::arena::ReverbArena;
use super::parts::{Lines, XFADE, XFADE_INV, ap_f32, ease_k, one_pole, pair_in, pow2, wrap};

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
// ROOM programs (github.com/thement/midiverb_emulator's decompilation,
// GPL-3.0), used by the owner's approval of 2026-10-05. Only these
// tables carry them; nothing else here comes from the emulator.

/// The diffuser's five allpasses, fixed.
pub const DIFFUSER_MS: [f32; 5] = [0.81, 1.92, 3.59, 4.91, 6.24];
/// Each loop's (A–D) two stage delays at SIZE 1.
pub const DELAY_MS: [[f32; 2]; 4] = [[37.1, 57.5], [59.4, 42.3], [52.7, 41.9], [41.5, 43.6]];
/// Each loop's two stage allpasses, fixed.
pub const AP_MS: [[f32; 2]; 4] = [[29.0, 13.3], [28.9, 18.4], [30.4, 16.3], [27.1, 14.2]];
/// The taps at SIZE 1, L then R; tap j reads stage line j (A1, A2, B1,
/// B2, C1, C2, D1, D2). R's first stays put at every SIZE.
pub const TAP_MS: [[f32; 8]; 2] = [
    [5.5, 5.6, 36.7, 11.8, 24.1, 8.9, 18.0, 38.1],
    [0.51, 31.4, 10.8, 20.4, 15.8, 21.3, 30.9, 9.1],
];
/// The diffuser's g.
pub const DIFFUSER_G: f32 = 0.75;
/// Each tap's weight.
pub const TAP_W: f32 = 0.75;
// End of ADR 0095's tables.

/// SIZE's k at 0; 1 at SIZE 1.
pub const K_MIN: f32 = 0.4;
/// PRE-DLY's longest, ms.
pub const PRE_MS: f32 = 50.0;
/// The smeared stage allpass of each loop: A2, B1, C2, D1.
pub const SMEARED: [usize; 4] = [1, 0, 1, 0];

/// `ms` at 24 kHz, to the nearest sample.
pub const fn samples(ms: f32) -> usize {
    (ms * PER_MS + 0.5) as usize
}

const fn map5(ms: [f32; 5]) -> [usize; 5] {
    let mut o = [0; 5];
    let mut j = 0;
    while j < 5 {
        o[j] = samples(ms[j]);
        j += 1;
    }
    o
}

const fn map42(ms: [[f32; 2]; 4]) -> [[usize; 2]; 4] {
    let mut o = [[0; 2]; 4];
    let mut l = 0;
    while l < 4 {
        o[l] = [samples(ms[l][0]), samples(ms[l][1])];
        l += 1;
    }
    o
}

pub const DIFFUSER: [usize; 5] = map5(DIFFUSER_MS);
pub const DELAY: [[usize; 2]; 4] = map42(DELAY_MS);
pub const AP: [[usize; 2]; 4] = map42(AP_MS);
pub const PRE: usize = samples(PRE_MS);
/// R's first tap, ring samples: fixed.
pub const TAP_FIXED: u16 = samples(TAP_MS[1][0]) as u16;

/// The stage delays at `k`, ring samples: each table length × k, to the
/// nearest sample.
pub fn delays_at(k: f32) -> [[u16; 2]; 4] {
    DELAY_MS.map(|l| l.map(|ms| (ms * PER_MS * k + 0.5) as u16))
}

/// Tap `j` of `side` with its line at `d` ring samples: the tap's SIZE-1
/// offset scaled as its line is, to the nearest sample; R's first fixed.
#[inline(always)]
pub fn tap_at(side: usize, j: usize, d: u16) -> u16 {
    if side == 1 && j == 0 {
        TAP_FIXED
    } else {
        (TAP_MS[side][j] / DELAY_MS[j / 2][j % 2] * d as f32 + 0.5) as u16
    }
}

/// Every tap at the lines' lengths `d`, [side][j].
pub fn taps_at(d: &[[u16; 2]; 4]) -> [[u16; 8]; 2] {
    core::array::from_fn(|side| core::array::from_fn(|j| tap_at(side, j, d[j / 2][j % 2])))
}

/// LOOPS's reflection at `theta_deg`: (cos θ, sin θ), by their series in
/// f32 (to θ¹⁴, exact to a few ulps over 0–90°), scaled so c² + s² is 1
/// to the rounding: the reflection then keeps its energy.
pub fn coupling(theta_deg: f32) -> (f32, f32) {
    let t = clampf(theta_deg, 0.0, 90.0).to_radians();
    let x = t * t;
    let s = t
        * (1.0
            + x / -6.0
                * (1.0
                    + x / -20.0
                        * (1.0
                            + x / -42.0
                                * (1.0 + x / -72.0 * (1.0 + x / -110.0 * (1.0 + x / -156.0))))));
    let c = 1.0
        + x / -2.0
            * (1.0
                + x / -12.0
                    * (1.0
                        + x / -30.0
                            * (1.0
                                + x / -56.0
                                    * (1.0
                                        + x / -90.0 * (1.0 + x / -132.0 * (1.0 + x / -182.0))))));
    let n = 1.0 / libm::sqrtf(c * c + s * s);
    (c * n, s * n)
}

/// A pair's re-entries from its loop outputs (a, b): the Householder
/// reflection [[c, s], [s, −c]]. The engine and its tests share it.
#[inline(always)]
pub fn reflect(c: f32, s: f32, a: f32, b: f32) -> (f32, f32) {
    (c * a + s * b, s * a - c * b)
}

/// Slots each line keeps past its length and 2: one block. A line's write
/// lands on the slot the line above it read `len + 2 + GUARD − d` samples
/// earlier; run stage by stage over a segment (`block`), a writer may run
/// before that reader, so the gap must outlast a segment.
pub const GUARD: usize = HALF;

/// Where each line writes, from the shared heads: line j owns its length,
/// 2 slots more and `GUARD`, so a read d back, 2 ≤ d ≤ len + 1, is what it
/// wrote d samples ago. The i16 lines (PRE, the delays), then the f32 ones
/// (the diffuser, the allpasses).
struct Heads {
    pre: usize,
    delay: [[usize; 2]; 4],
    slots: usize,
    diffuser: [usize; 5],
    ap: [[usize; 2]; 4],
    fslots: usize,
}

const HEADS: Heads = {
    let mut o = PRE + 2 + GUARD;
    let pre = o - 1;
    let (mut delay, mut ap) = ([[0; 2]; 4], [[0; 2]; 4]);
    let mut l = 0;
    while l < 4 {
        let mut s = 0;
        while s < 2 {
            o += DELAY[l][s] + 2 + GUARD;
            delay[l][s] = o - 1;
            s += 1;
        }
        l += 1;
    }
    let slots = o + o % 2;
    let (mut diffuser, mut f) = ([0; 5], 0);
    let mut j = 0;
    while j < 5 {
        f += DIFFUSER[j] + 2 + GUARD;
        diffuser[j] = f - 1;
        j += 1;
    }
    l = 0;
    while l < 4 {
        let mut s = 0;
        while s < 2 {
            f += AP[l][s] + 2 + GUARD;
            ap[l][s] = f - 1;
            s += 1;
        }
        l += 1;
    }
    Heads {
        pre,
        delay,
        slots,
        diffuser,
        ap,
        fslots: f,
    }
};

/// The i16 slots, and the f32s above them.
pub const SLOTS: usize = HEADS.slots;
pub const FSLOTS: usize = HEADS.fslots;

/// The i16 accesses from the head: the writes (PRE, the eight delays),
/// PRE's new and old reads, the delays' new and old reads, the taps' new
/// and old reads (side-major).
const W_PRE: usize = 0;
const W_D: usize = 1;
const R_PRE: usize = 9;
const R_D_TO: usize = 11;
const R_D_FROM: usize = 19;
const R_TAP_TO: usize = 27;
const R_TAP_FROM: usize = 43;
const ACCESS: usize = 59;
/// The f32 accesses: the diffuser's writes and reads, the stage
/// allpasses' writes and fixed reads.
const W_DIF: usize = 0;
const R_DIF: usize = 5;
const W_AP: usize = 10;
const R_AP: usize = 18;
const F_ACCESS: usize = 26;

/// The writes' offsets from the head (each read's own entry unused).
const OFFSETS: [usize; ACCESS] = {
    let mut o = [0; ACCESS];
    o[W_PRE] = HEADS.pre;
    let mut j = 0;
    while j < 8 {
        o[W_D + j] = HEADS.delay[j / 2][j % 2];
        j += 1;
    }
    o
};
/// The f32 writes' offsets (each read's own entry unused).
const F_OFFSETS: [usize; F_ACCESS] = {
    let mut o = [0; F_ACCESS];
    let mut j = 0;
    while j < 5 {
        o[W_DIF + j] = HEADS.diffuser[j];
        j += 1;
    }
    j = 0;
    while j < 8 {
        o[W_AP + j] = HEADS.ap[j / 2][j % 2];
        j += 1;
    }
    o
};
/// The longest stage allpass, ring samples.
const AP_MAX: f32 = {
    let (mut m, mut j) = (0, 0);
    while j < 8 {
        if AP[j / 2][j % 2] > m {
            m = AP[j / 2][j % 2];
        }
        j += 1;
    }
    m as f32
};

/// Loop A's stage allpasses and stage delays at SIZE 1, ms: SIZE's k
/// from its round trip.
pub const A_AP_MS: f32 = AP_MS[0][0] + AP_MS[0][1];
pub const A_DELAY_MS: f32 = DELAY_MS[0][0] + DELAY_MS[0][1];

/// DAMP at its top: the dampers open.
pub const DAMP_OPEN_HZ: f32 = crate::dsp::reverb::ROOM.damp_hz.lo;

/// SMEAR's cap, a share of the loop (§ The SMEAR bound).
const SMEAR_CAP: f32 = crate::dsp::reverb::ROOM.smear_cap;

const FITS: () = assert!(
    SLOTS + 2 * FSLOTS <= RING_LEN
        && SLOTS < u16::MAX as usize
        && FSLOTS < u16::MAX as usize
        // Every tap inside its line at SIZE 1 (so at every k), 2 or more
        // behind its write.
        && {
            let (mut ok, mut j) = (true, 0);
            while j < 16 {
                let (side, t) = (j / 8, j % 8);
                ok &= TAP_MS[side][t] <= DELAY_MS[t / 2][t % 2]
                    && TAP_MS[side][t] * K_MIN * PER_MS >= 2.0;
                j += 1;
            }
            ok
        }
        // The smeared reads, `M − 2D` at the deepest D (twice the cap of
        // the loop's longest trip), stay 2 or more behind their write:
        // `pair_in`'s contract.
        && {
            let (mut ok, mut l) = (true, 0);
            while l < 4 {
                let trip = DELAY[l][0] + DELAY[l][1] + AP[l][0] + AP[l][1];
                ok &= AP[l][SMEARED[l]] as f32 - 2.0 * SMEAR_CAP * trip as f32 >= 2.0;
                l += 1;
            }
            ok
        }
);

/// The new engines' stage gain cap (§ Shared machinery).
const MAX_GAIN: f32 = 0.985;
/// The send, ½ (§ Shared machinery), in LSBs.
const IN_GAIN: f32 = 0.5 * FULL_SCALE;
const INV_FS: f32 = 1.0 / FULL_SCALE;
/// The most one crossfade moves PRE-DLY, and a stage delay.
pub const STEP: u16 = 120;

/// ROOM's state beside the arena. Zero bytes are a fresh engine, primed by
/// its first block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    /// The shared write heads of the i16 and the f32 lines.
    h: u16,
    hf: u16,
    primed: bool,
    retarget: bool,
    /// Ring samples left in the length crossfade; 0 between.
    left: u16,
    /// The stage delays and PRE-DLY, ring samples: landed, crossfading
    /// to, and the knobs' goal. The taps follow `at` and `to`.
    at: [[u16; 2]; 4],
    to: [[u16; 2]; 4],
    goal: [[u16; 2]; 4],
    pre_at: u16,
    pre_to: u16,
    pre_goal: u16,
    tone: f32,
    /// The eased TONE coefficient, the stage allpasses' g, the damper's
    /// coefficient, the reflection's (cos θ, sin θ) and the stage gains,
    /// one step a block (TONE ramps across it).
    ta: f32,
    gs: f32,
    da: f32,
    cs: (f32, f32),
    g: [[f32; 2]; 4],
    /// What the eases head for, worked out again only when RT60, TONE,
    /// DAMP, LOOPS (`key`'s bits) or the lengths (`retarget`) change.
    key: [u32; 5],
    t_ta: f32,
    t_da: f32,
    t_cap: f32,
    t_cs: (f32, f32),
    t_g: [[f32; 2]; 4],
    /// The dampers, one a loop. Unrounded (ADR 0085, HALL as built).
    lp: [f32; 4],
    /// This block's SMEAR offsets (start, end), ring samples: `D·(1 + x)`.
    smear: [(f32, f32); 4],
    /// A crossfade step other than `STEP`, when not 0 (the slap test's
    /// negative control): never in firmware.
    #[cfg(any(test, feature = "test-support"))]
    coarse: u16,
    /// 1 + the loop the send enters alone, past the diffuser, through
    /// stage allpasses of g 0 (the coupling and tap tests); 0 off: never
    /// in firmware.
    #[cfg(any(test, feature = "test-support"))]
    probe: u8,
}

impl Default for Room {
    fn default() -> Self {
        Self {
            h: 0,
            hf: 0,
            primed: false,
            retarget: false,
            left: 0,
            at: [[0; 2]; 4],
            to: [[0; 2]; 4],
            goal: [[0; 2]; 4],
            pre_at: 0,
            pre_to: 0,
            pre_goal: 0,
            tone: 0.0,
            ta: 0.0,
            gs: 0.0,
            da: 0.0,
            cs: (0.0, 0.0),
            g: [[0.0; 2]; 4],
            key: [0; 5],
            t_ta: 0.0,
            t_da: 0.0,
            t_cap: 0.0,
            t_cs: (0.0, 0.0),
            t_g: [[0.0; 2]; 4],
            lp: [0.0; 4],
            smear: [(0.0, 0.0); 4],
            #[cfg(any(test, feature = "test-support"))]
            coarse: 0,
            #[cfg(any(test, feature = "test-support"))]
            probe: 0,
        }
    }
}

/// A stage allpass's share of the decay's trip, in its own lengths: its
/// slow modes (group delay up to (1 + g)/(1 − g) of its length) hold the
/// tail, more so the more of the trip it is (small k). Fitted to the 1 kHz
/// octave's RT60 over DECAY ¼–1, SIZE 0–1, g 0.45–0.70 (`g4` = g⁴): 1.19 at
/// g 0.45, 2.2 at 0.70, × 1.22 at k 0.4.
#[inline(always)]
fn slow(g4: f32, k: f32) -> f32 {
    (1.0 + 4.8 * g4) * (1.37 - 0.37 * k)
}

/// One block's one-pole toward `t`, `k` the share kept (0 lands).
fn ease(x: &mut f32, t: f32, k: f32) {
    *x = t + k * (*x - t);
}

/// R1's clip (`parts::soft_clip`) on an LSB value: under its knee, as a
/// re-entry nearly always is, `x` itself, past one compare.
#[inline(always)]
fn clip<const C: bool>(x: f32) -> f32 {
    if C && x.abs() > FULL_SCALE {
        let a = x.abs();
        let u = clampf(a - FULL_SCALE, 0.0, 2.0 * FULL_SCALE);
        (a.min(FULL_SCALE) + u - (0.25 * INV_FS) * u * u).copysign(x)
    } else {
        x
    }
}

/// A block's scratch, written before it is read: never zeroed.
struct Scratch<T: Copy, const N: usize>([MaybeUninit<T>; N]);

impl<T: Copy, const N: usize> Scratch<T, N> {
    fn new() -> Self {
        Self([MaybeUninit::uninit(); N])
    }

    #[inline(always)]
    fn set(&mut self, i: usize, v: T) {
        self.0[i].write(v);
    }

    /// # Safety
    /// `set(i, _)` ran first.
    #[inline(always)]
    unsafe fn get(&self, i: usize) -> T {
        // SAFETY: the caller set slot `i`.
        unsafe { self.0[i].assume_init() }
    }
}

/// Line `w`'s write slot at head `h` (mod `m`), at offset `off`, and
/// its `reads`' (index, depth behind the write) slots, into `base`; `len`
/// shortened to the samples that run before any of them wraps. A read
/// wraps only while its line straddles the ring's end (`w` under `deep`,
/// the line's deepest read): one line at a time. Else each read is the
/// write's slot less its depth.
#[inline(always)]
fn line<const R: usize, const N: usize>(
    (h, m): (usize, usize),
    (w, off, deep): (usize, usize, usize),
    reads: [(usize, usize); R],
    base: &mut Scratch<usize, N>,
    len: &mut usize,
) {
    let b = wrap(h + off, m);
    *len = (*len).min(m - b);
    base.set(w, b);
    if b >= deep {
        for (r, d) in reads {
            base.set(r, b - d);
        }
    } else {
        for (r, d) in reads {
            let x = if b >= d {
                b - d
            } else {
                let x = b + m - d;
                *len = (*len).min(m - x);
                x
            };
            base.set(r, x);
        }
    }
}

impl Room {
    /// The knobs' goals: the stage delays at SIZE's k (from loop A's round
    /// trip, `length_ms`), and PRE-DLY, in ring samples.
    fn goals(k: &EngineKnobs) -> ([[u16; 2]; 4], u16) {
        let kk = clampf((k.length_ms - A_AP_MS) * (1.0 / A_DELAY_MS), K_MIN, 1.0);
        let pre = clampf(k.predelay_ms * PER_MS + 0.5, 2.0, PRE as f32) as u16;
        (delays_at(kk), pre)
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

    /// Each eased value one block toward `k`. The stage allpasses take
    /// DIFFUSE's g held to the longest one's stage gain, so their own ring
    /// never outlasts the RT60 (ADR 0085, ROOM as built). The targets'
    /// `exp2`s run only when what they read changes.
    fn ease_to(&mut self, k: &EngineKnobs, k20: f32, k50: f32) {
        let key = [
            k.rt60_s.to_bits(),
            k.tone_hz.to_bits(),
            k.damp_hz.to_bits(),
            k.shape_db.to_bits(),
            k.diffuse_g.to_bits(),
        ];
        if key != self.key || self.retarget {
            (self.key, self.retarget) = (key, false);
            self.t_ta = one_pole(k.tone_hz, FS);
            // DAMP's top end opens the damper (ADR 0095's first near
            // miss): a bypass, not 10 kHz.
            self.t_da = if k.damp_hz >= DAMP_OPEN_HZ {
                1.0
            } else {
                one_pole(k.damp_hz, FS)
            };
            self.t_cs = coupling(k.shape_db);
            let per = -3.0 * LOG2_10 / (k.rt60_s.max(1e-3) * FS);
            self.t_cap = pow2(per * AP_MAX);
            let g = k.diffuse_g.min(self.t_cap);
            let g4 = g * g * g * g;
            for l in 0..4 {
                for s in 0..2 {
                    let d = self.to[l][s] as f32;
                    let kk = d * (1.0 / (DELAY_MS[l][s] * PER_MS));
                    let len = d + AP[l][s] as f32 * slow(g4, kk);
                    self.t_g[l][s] = pow2(per * len).min(MAX_GAIN);
                }
            }
        }
        ease(&mut self.ta, self.t_ta, k20);
        ease(&mut self.da, self.t_da, k20);
        ease(&mut self.gs, k.diffuse_g.min(self.t_cap), k20);
        ease(&mut self.cs.0, self.t_cs.0, k20);
        ease(&mut self.cs.1, self.t_cs.1, k20);
        for l in 0..4 {
            for s in 0..2 {
                ease(&mut self.g[l][s], self.t_g[l][s], k50);
            }
        }
    }

    /// The SMEAR offsets the next block reads its four smeared allpasses
    /// short by, (start, end) in ring samples: `D·(1 + x)`, each held to
    /// twice its loop's cap.
    pub fn set_smear(&mut self, s: [(f32, f32); 4]) {
        self.smear = s;
    }

    /// The SMEAR offsets the last block read by, as held.
    pub fn smear(&self) -> [(f32, f32); 4] {
        self.smear
    }

    /// Step `step` ring samples a crossfade from now on (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn coarse_steps(&mut self, step: u16) {
        self.coarse = step;
    }

    /// The send into loop `l` (0–3) alone, past the diffuser, through
    /// stage allpasses of g 0, from now on (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn probe_loop(&mut self, l: u8) {
        self.probe = l + 1;
    }

    /// The cached targets: TONE's and the damper's coefficients, and each
    /// stage's gain (tests: a stale cache).
    #[cfg(any(test, feature = "test-support"))]
    #[allow(clippy::type_complexity)]
    pub fn targets(&self) -> (f32, f32, (f32, f32), [[f32; 2]; 4]) {
        (self.t_ta, self.t_da, self.t_cs, self.t_g)
    }

    /// The i16 lines' shared write head (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn head(&self) -> usize {
        self.h as usize
    }

    /// Line (`l`, `s`)'s slots from the head: what it wrote in its last
    /// `len + 2` samples (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn line(l: usize, s: usize) -> Range<usize> {
        let w = HEADS.delay[l][s];
        w + 1 - (DELAY[l][s] + 2)..w + 1
    }

    /// The i16 regions a level-plan test reads, from the head: PRE and the
    /// stage delays (PRE's own head slot left out).
    #[cfg(any(test, feature = "test-support"))]
    pub fn regions() -> [Range<usize>; 2] {
        [0..HEADS.pre, HEADS.pre + 1..HEADS.delay[3][1]]
    }

    /// The stage delays, landed.
    pub fn delays(&self) -> [[u16; 2]; 4] {
        self.at
    }

    /// The taps' reads, landed and crossfading to, [side][j].
    #[allow(clippy::type_complexity)]
    pub fn taps(&self) -> ([[u16; 8]; 2], [[u16; 8]; 2]) {
        (taps_at(&self.at), taps_at(&self.to))
    }

    /// PRE-DLY, landed.
    pub fn predelay(&self) -> u16 {
        self.pre_at
    }

    /// The crossfade running: the stage delays from and to, and PRE-DLY's.
    #[allow(clippy::type_complexity)]
    pub fn crossfade(&self) -> Option<([[u16; 2]; 4], [[u16; 2]; 4], (u16, u16))> {
        (self.left > 0).then_some((self.at, self.to, (self.pre_at, self.pre_to)))
    }

    /// Whether a length still moves toward the knobs.
    pub fn moving(&self) -> bool {
        self.left > 0 || self.at != self.goal || self.pre_at != self.pre_goal
    }

    /// The next crossfade toward the goals: in each loop the stage delay
    /// farthest from its goal moves `STEP` (or what is left), every loop
    /// in the same crossfade; PRE-DLY likewise (ADR 0085, ROOM as built).
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
        for l in 0..4 {
            let far = (self.goal[l][1].abs_diff(self.at[l][1])
                > self.goal[l][0].abs_diff(self.at[l][0])) as usize;
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
        let (lines, f, _) = arena.split(SLOTS, FSLOTS);
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
        let (lo, f, _) = arena.split(SLOTS, FSLOTS);
        match (lines, clipped) {
            (Some(x), true) => self.block::<[f32], true>(x, f, u, k, l, r),
            (Some(x), false) => self.block::<[f32], false>(x, f, u, k, l, r),
            (None, true) => self.block::<[i16], true>(lo, f, u, k, l, r),
            (None, false) => self.block::<[i16], false>(lo, f, u, k, l, r),
        }
    }

    /// Each i16 read's depth behind its line's write this block: PRE-DLY's,
    /// the stage delays', the taps' (the writes' entries unused).
    fn depths(&self) -> [usize; ACCESS] {
        let mut o = [0; ACCESS];
        o[R_PRE] = self.pre_to as usize;
        o[R_PRE + 1] = self.pre_at as usize;
        let (from, to) = self.taps();
        for j in 0..8 {
            let (l, s) = (j / 2, j % 2);
            o[R_D_TO + j] = self.to[l][s] as usize;
            o[R_D_FROM + j] = self.at[l][s] as usize;
            for side in 0..2 {
                o[R_TAP_TO + 8 * side + j] = to[side][j] as usize;
                o[R_TAP_FROM + 8 * side + j] = from[side][j] as usize;
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
        // TONE and SMEAR ramp from the block's start to its end; the stage
        // gains, the stage allpasses' g, the dampers and the reflection step
        // once a block on their eases (ADR 0085, BURST as built: its
        // weights; ADR 0095, ROOM v2 as built). The eases: τ 50 ms for the
        // gains, as QUAD's TIME; 20 ms the rest.
        let ta0 = self.ta;
        const K20: f32 = ease_k(0.02);
        const K50: f32 = ease_k(0.05);
        self.ease_to(k, K20, K50);
        #[cfg(any(test, feature = "test-support"))]
        let (probe, only) = (self.probe > 0, usize::from(self.probe).wrapping_sub(1));
        #[cfg(not(any(test, feature = "test-support")))]
        let (probe, only) = (false, 0);
        if probe {
            (self.gs, self.cs) = (0.0, self.t_cs);
        }
        let n = 1.0 / HALF as f32;
        let (mut ta, dta) = (ta0, (self.ta - ta0) * n);
        let (gs, da, (c, s)) = (self.gs, self.da, self.cs);
        let gb = self.g;
        // The crossfade's blend: the new read's share.
        let (mut w, dw) = if self.left > 0 {
            ((XFADE - self.left) as f32 * XFADE_INV, XFADE_INV)
        } else {
            (1.0, 0.0)
        };
        // SMEAR, held to twice each loop's cap at the shorter of its
        // lengths this block.
        let off: [(f32, f32); 4] = core::array::from_fn(|lp| {
            let trip = (0..2)
                .map(|s| (self.at[lp][s].min(self.to[lp][s]) as usize + AP[lp][s]) as f32)
                .sum::<f32>();
            let top = 2.0 * SMEAR_CAP * trip;
            let (a, b) = self.smear[lp];
            (clampf(a, 0.0, top), clampf(b, 0.0, top))
        });
        self.smear = off;
        let mut sm = off.map(|o| o.0);
        let dsm = off.map(|o| (o.1 - o.0) * n);
        let nom: [f32; 4] = core::array::from_fn(|l| AP[l][SMEARED[l]] as f32);
        let depth = self.depths();
        let (mut tone, mut lp) = (self.tone, self.lp);
        let (mut h, mut hf) = (self.h as usize % SLOTS, self.hf as usize % FSLOTS);
        // Stage by stage over each segment: every feedback read sits 319 or
        // more samples back (a stage delay or allpass at k 0.4), so no
        // stage reads what this block writes, and the taps, read last, see
        // every write before them. Each stage's loop keeps its few bases in
        // registers; one loop over every access spilled them. The
        // arithmetic is the sample loop's, in its order.
        // Scratch, each slot written before it is read: the bases every
        // segment (`line` sets every access), the blend, send and A's or
        // C's output for t < len.
        let (mut wv, mut dv, mut ov) = (
            Scratch::<f32, HALF>::new(),
            Scratch::<f32, HALF>::new(),
            Scratch::<f32, HALF>::new(),
        );
        let (mut b, mut fb) = (
            Scratch::<usize, ACCESS>::new(),
            Scratch::<usize, F_ACCESS>::new(),
        );
        let mut i = 0;
        while i < HALF {
            let mut len = HALF - i;
            line(
                (h, SLOTS),
                (W_PRE, OFFSETS[W_PRE], PRE + 1),
                [(R_PRE, depth[R_PRE]), (R_PRE + 1, depth[R_PRE + 1])],
                &mut b,
                &mut len,
            );
            unroll!(J in [0, 1, 2, 3, 4, 5, 6, 7] {
                let deep = DELAY[J / 2][J % 2] + 1;
                let reads = [R_D_TO + J, R_D_FROM + J, R_TAP_TO + J, R_TAP_FROM + J, R_TAP_TO + 8 + J, R_TAP_FROM + 8 + J]
                    .map(|r| (r, depth[r]));
                line((h, SLOTS), (W_D + J, OFFSETS[W_D + J], deep), reads, &mut b, &mut len);
            });
            unroll!(J in [0, 1, 2, 3, 4] {
                line((hf, FSLOTS), (W_DIF + J, F_OFFSETS[W_DIF + J], DIFFUSER[J]), [(R_DIF + J, DIFFUSER[J])], &mut fb, &mut len);
            });
            unroll!(J in [0, 1, 2, 3, 4, 5, 6, 7] {
                let d = AP[J / 2][J % 2];
                line((hf, FSLOTS), (W_AP + J, F_OFFSETS[W_AP + J], d), [(R_AP + J, d)], &mut fb, &mut len);
            });
            let len = len;
            // SAFETY (each): `line` above set every base, and each `t` <
            // `len` of the blend, the send and A's or C's output is set
            // before its loop reads it.
            macro_rules! bi {
                ($k:expr) => {
                    unsafe { b.get($k) }
                };
            }
            macro_rules! fbi {
                ($k:expr) => {
                    unsafe { fb.get($k) }
                };
            }
            macro_rules! wvi {
                ($t:expr) => {
                    unsafe { wv.get($t) }
                };
            }
            macro_rules! dvi {
                ($t:expr) => {
                    unsafe { dv.get($t) }
                };
            }
            macro_rules! ovi {
                ($t:expr) => {
                    unsafe { ov.get($t) }
                };
            }
            // This segment's send and return: `t` < `len` bounds all three.
            let (us, ls, rs) = (&u[i..i + len], &mut l[i..i + len], &mut r[i..i + len]);
            macro_rules! rd {
                ($base:expr, $t:expr) => {
                    // SAFETY: `segment` put each base below its region's
                    // length m (≤ the slice's, asserted above) and `len` at
                    // most m − base, and every `t` here is < `len`. The
                    // writes and the f32 accesses likewise.
                    unsafe { lines.rd($base + $t) }
                };
            }
            // The crossfade's blend, sample by sample.
            for t in 0..len {
                wv.set(t, w);
                w = (w + dw).min(1.0);
            }
            // TONE, then PRE-DLY.
            {
                let (wp, to, from) = (bi!(W_PRE), bi!(R_PRE), bi!(R_PRE + 1));
                macro_rules! pre {
                    ($mv:expr) => {
                        for t in 0..len {
                            tone += ta * (us[t] * IN_GAIN - tone);
                            // SAFETY: as `rd`'s.
                            unsafe { lines.round(wp + t, tone) };
                            let x = if $mv {
                                let o = rd!(from, t);
                                o + wvi!(t) * (rd!(to, t) - o)
                            } else {
                                rd!(from, t)
                            };
                            dv.set(t, x);
                            ta += dta;
                        }
                    };
                }
                if to != from { pre!(true) } else { pre!(false) }
            }
            if !probe {
                let (w, r): ([usize; 5], [usize; 5]) = (
                    core::array::from_fn(|j| fbi!(W_DIF + j)),
                    core::array::from_fn(|j| fbi!(R_DIF + j)),
                );
                for t in 0..len {
                    let mut y = dvi!(t);
                    unroll!(J in [0, 1, 2, 3, 4] {
                        // SAFETY: as `rd`'s, on the f32 region.
                        let (o, v) = ap_f32(y, unsafe { *f.get_unchecked(r[J] + t) }, DIFFUSER_G);
                        // SAFETY: as `rd`'s, on the f32 region.
                        unsafe { *f.get_unchecked_mut(w[J] + t) = v };
                        y = o;
                    });
                    dv.set(t, y);
                }
            }
            // Each loop's stages, unrolled: `LO` and `ST` are literals. A
            // read whose length holds this crossfade reads once. Each
            // pair's reflection, send and re-entries run in its second
            // loop's last stage, beside A's or C's output.
            unroll!(LO in [0, 1, 2, 3] {
                let sm_at = sm[LO];
                unroll!(ST in [0, 1] {
                    let j = LO * 2 + ST;
                    let (to, from) = (bi!(R_D_TO + j), bi!(R_D_FROM + j));
                    let moving = to != from;
                    let (wa, ra) = (fbi!(W_AP + j), fbi!(R_AP + j));
                    let wd = bi!(W_D + (j | 1));
                    // The pair's re-entries: its first loop's and its own.
                    let (wr0, wr1) = (bi!(W_D + 4 * (LO / 2)), bi!(W_D + 4 * (LO / 2) + 2));
                    let (mut sm_, g_) = (sm_at, gb[LO][ST]);
                    let mut lp_ = lp[LO];
                    macro_rules! stage { ($mv:expr, $fast:expr) => {
                    for t in 0..len {
                        let x = if $mv {
                            let o = rd!(from, t);
                            o + wvi!(t) * (rd!(to, t) - o)
                        } else {
                            rd!(from, t)
                        };
                        let z = if ST == SMEARED[LO] {
                            // SAFETY (`pair_in`): the write slot is below
                            // `FSLOTS` (as `rd`'s); `sm` is clamped to [0,
                            // 2·cap·trip] above, and `FITS` keeps `nom − sm`
                            // in 2..`FSLOTS`.
                            if $fast {
                                let d = nom[LO] - sm_;
                                let di = d as usize;
                                let fr = d - di as f32;
                                // SAFETY: `wa` > the deepest read (`fast`),
                                // so both slots sit in `wa − di − 1..wa + t`.
                                let (x0, x1) = unsafe {
                                    (*f.get_unchecked(wa + t - di), *f.get_unchecked(wa + t - di - 1))
                                };
                                x0 + fr * (x1 - x0)
                            } else {
                                unsafe { pair_in(&*f, FSLOTS, wa + t, nom[LO] - sm_) }
                            }
                        } else {
                            // SAFETY: as `rd`'s, on the f32 region.
                            unsafe { *f.get_unchecked(ra + t) }
                        };
                        let (mut y, v) = ap_f32(x, z, gs);
                        // SAFETY: as `rd`'s, on the f32 region.
                        unsafe { *f.get_unchecked_mut(wa + t) = v };
                        if ST == 1 {
                            lp_ += da * (y - lp_);
                            y = lp_;
                        }
                        y *= g_;
                        if ST == 0 {
                            // SAFETY: as `rd`'s.
                            unsafe { lines.round(wd + t, y) };
                        } else if LO.is_multiple_of(2) {
                            ov.set(t, y);
                        } else {
                            let (a, bb) = reflect(c, s, ovi!(t), y);
                            // Under a probe, only its loop hears the send.
                            let a_send = if probe && only + 1 != LO { 0.0 } else { dvi!(t) };
                            let send = if probe && only != LO { 0.0 } else { dvi!(t) };
                            // SAFETY: as `rd`'s.
                            unsafe {
                                let (ea, eb) = (a + a_send, bb + send);
                                // Both under the knee, as nearly always: as
                                // they are, past one compare.
                                if CLIP && ea.abs().max(eb.abs()) > FULL_SCALE {
                                    lines.settle(wr0 + t, clip::<CLIP>(ea));
                                    lines.settle(wr1 + t, clip::<CLIP>(eb));
                                } else {
                                    lines.settle(wr0 + t, ea);
                                    lines.settle(wr1 + t, eb);
                                }
                            }
                        }
                        if ST == SMEARED[LO] {
                            sm_ += dsm[LO];
                        }
                    }
                    }}
                    let fast = ST == SMEARED[LO] && wa > AP[LO][ST];
                    match (moving, fast) {
                        (true, true) => stage!(true, true),
                        (true, false) => stage!(true, false),
                        (false, true) => stage!(false, true),
                        (false, false) => stage!(false, false),
                    }
                    lp[LO] = lp_;
                    if ST == SMEARED[LO] {
                        sm[LO] = sm_;
                    }
                });
            });
            // The taps, after every write they may read. At most one line a
            // loop moves; a loop with none stands in its first line, to ==
            // from, whose difference is 0. Each side: the four still lines'
            // and the four movers' reads summed exactly, then in a crossfade
            // w × Σ(to − from) over the movers, from 12 reads.
            let moving: [bool; 8] = core::array::from_fn(|j| bi!(R_D_TO + j) != bi!(R_D_FROM + j));
            let any = moving.iter().any(|&m| m);
            let mv: [usize; 4] = core::array::from_fn(|lo| 2 * lo + moving[2 * lo + 1] as usize);
            unroll!(SD in [0, 1] {
                let out: &mut [f32] = if SD == 0 { &mut *ls } else { &mut *rs };
                let st: [usize; 4] = core::array::from_fn(|k| bi!(R_TAP_FROM + 8 * SD + (mv[k] ^ 1)));
                let fr: [usize; 4] = core::array::from_fn(|k| bi!(R_TAP_FROM + 8 * SD + mv[k]));
                if any {
                    let tt: [usize; 4] = core::array::from_fn(|k| bi!(R_TAP_TO + 8 * SD + mv[k]));
                    for t in 0..len {
                        // SAFETY: as `rd`'s.
                        let (s0, f0, t0) = unsafe {
                            (
                                lines.rd_sum(st[0] + t) + lines.rd_sum(st[1] + t)
                                    + lines.rd_sum(st[2] + t) + lines.rd_sum(st[3] + t),
                                lines.rd_sum(fr[0] + t) + lines.rd_sum(fr[1] + t)
                                    + lines.rd_sum(fr[2] + t) + lines.rd_sum(fr[3] + t),
                                lines.rd_sum(tt[0] + t) + lines.rd_sum(tt[1] + t)
                                    + lines.rd_sum(tt[2] + t) + lines.rd_sum(tt[3] + t),
                            )
                        };
                        out[t] = TAP_W * (Ln::sum_f32(s0 + f0) + wvi!(t) * Ln::sum_f32(t0 - f0));
                    }
                } else {
                    for t in 0..len {
                        // SAFETY: as `rd`'s.
                        let s0 = unsafe {
                            lines.rd_sum(st[0] + t) + lines.rd_sum(st[1] + t)
                                + lines.rd_sum(st[2] + t) + lines.rd_sum(st[3] + t)
                                + lines.rd_sum(fr[0] + t) + lines.rd_sum(fr[1] + t)
                                + lines.rd_sum(fr[2] + t) + lines.rd_sum(fr[3] + t)
                        };
                        out[t] = TAP_W * Ln::sum_f32(s0);
                    }
                }
            });
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
