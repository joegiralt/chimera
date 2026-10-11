//! The reverb: four stages in a ring, after Sean Costello's description of
//! the Alesis Quadraverb (Valhalla DSP, KVR thread 349039): "4 parallel
//! loops (2 x AP + 1 delay), outputs from delay taps", each loop feeding the
//! next. Half rate, stored as i16 (FX diet spec § Reverb, ADR 0028).

use crate::dsp::algo::math::exp2;
use crate::dsp::clampf;
use crate::dsp::halfband::HALF;
use crate::dsp::rev::arena::ReverbArena;
use crate::dsp::rev::parts::pair;
use chimera_hal::BLOCK_SIZE;
use core::f32::consts::{LOG2_10, LOG2_E, PI};

pub const STAGES: usize = 4;
/// Per stage: allpass 1, allpass 2, delay.
pub const LINES: usize = 3 * STAGES;
pub const SIZE_STEPS: usize = 32;
/// Every line at SIZE 1, in ring samples.
pub const BASE: [u16; LINES] = [
    151, 313, 4903, 181, 353, 5147, 211, 397, 5393, 241, 277, 5641,
];
/// Per stage: the shorter allpass +c, the longer −c.
pub const AP_COEF: [[f32; 2]; STAGES] = [[0.6, -0.6], [0.5, -0.5], [0.6, -0.6], [0.5, -0.5]];
/// (stage, fraction of its delay, sign); left, then right.
pub const TAPS: [[(usize, f32, f32); 3]; 2] = [
    [(0, 0.07, 1.0), (1, 0.41, -1.0), (2, 0.73, 1.0)],
    [(2, 0.11, 1.0), (3, 0.47, -1.0), (0, 0.79, 1.0)],
];
/// The send's gain into each stage's input: +½ at S1, −½ at S3.
pub const INJECT: [f32; STAGES] = [0.5, 0.0, -0.5, 0.0];
/// 1.0 in i16 LSBs: ±2.0 fits.
pub const FULL_SCALE: f32 = 16_384.0;
/// Ring samples a SIZE step's crossfade lasts (30 ms at 24 kHz).
pub const XFADE: u16 = 720;
const XFADE_INV: f32 = 1.0 / XFADE as f32;
pub const MAX_GAIN: f32 = 0.97;

/// The return's gain: its RMS at default settings matches the old plate's
/// at the same MIX, within 1 dB (tested against the plate's recorded RMS).
pub const WET_GAIN: f32 = 4.54;

const fn offsets() -> [usize; LINES] {
    let mut o = [0; LINES];
    let mut j = 1;
    while j < LINES {
        o[j] = o[j - 1] + BASE[j - 1] as usize + 1;
        j += 1;
    }
    o
}
const OFFSET: [usize; LINES] = offsets();
pub const RING_LEN: usize = OFFSET[LINES - 1] + BASE[LINES - 1] as usize + 1;
const _: () = assert!(RING_LEN == 23_220);
const _: () = assert!(core::mem::size_of::<[i16; RING_LEN]>() == 46_440);

/// The lines share one write head: line j writes at head + `HEAD[j]` and
/// reads d back at head + `HEAD[j]` − d, both mod `RING_LEN`. Each line
/// owns the `BASE[j] + 1` slots behind its write point, and they all
/// rotate together, so a read d ≤ `BASE[j]` back is what line j wrote d
/// samples ago.
const HEAD: [usize; LINES] = {
    let mut h = [0; LINES];
    let mut j = 0;
    while j < LINES {
        h[j] = OFFSET[j] + BASE[j] as usize;
        j += 1;
    }
    h
};

/// One block per `K` in the list, with `K` a const: the stage loops
/// unroll, and every table lookup in them folds to an immediate.
macro_rules! unroll {
    ($k:ident in [$($n:literal),*] $body:block) => {
        $({
            const $k: usize = $n;
            $body
        })*
    };
}

pub(crate) const fn is_prime(n: u32) -> bool {
    if n < 2 {
        return false;
    }
    let mut d = 2;
    while d * d <= n {
        if n.is_multiple_of(d) {
            return false;
        }
        d += 1;
    }
    true
}

const fn taken(row: &[u16; LINES], upto: usize, n: u32) -> bool {
    let mut j = 0;
    while j < upto {
        if row[j] as u32 == n {
            return true;
        }
        j += 1;
    }
    false
}

/// Step i scales every SIZE-1 length by 0.35 + 0.65·i/31, to the nearest
/// prime not already used in the step (the lower on a tie).
const fn size_table() -> [[u16; LINES]; SIZE_STEPS] {
    let mut t = [[0u16; LINES]; SIZE_STEPS];
    let mut i = 0;
    while i < SIZE_STEPS {
        let s = 0.35 + 0.65 * i as f32 / 31.0;
        let mut j = 0;
        while j < LINES {
            let want = (BASE[j] as f32 * s + 0.5) as u32;
            let mut d = 0;
            loop {
                if is_prime(want - d) && !taken(&t[i], j, want - d) {
                    t[i][j] = (want - d) as u16;
                    break;
                }
                if is_prime(want + d) && !taken(&t[i], j, want + d) {
                    t[i][j] = (want + d) as u16;
                    break;
                }
                d += 1;
            }
            j += 1;
        }
        i += 1;
    }
    t
}
pub const SIZE_TABLE: [[u16; LINES]; SIZE_STEPS] = size_table();

const fn fits() -> bool {
    let mut i = 0;
    while i < SIZE_STEPS {
        let mut j = 0;
        while j < LINES {
            if SIZE_TABLE[i][j] > BASE[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
const _: () = assert!(fits());

/// round(SIZE·31).
pub fn size_step(size: f32) -> u8 {
    (clampf(size, 0.0, 1.0) * 31.0 + 0.5) as u8
}

/// Stage k's share of the round trip at `step`: D + M1 + M2.
pub fn stage_len(step: u8, k: usize) -> u16 {
    let t = &SIZE_TABLE[step as usize % SIZE_STEPS];
    t[3 * k] + t[3 * k + 1] + t[3 * k + 2]
}

/// The shortest RT60 at `step`: the longest allpass rings about
/// 13.5·M / fs_ring on its own.
pub fn t_min(step: u8, fs_ring: f32) -> f32 {
    let t = &SIZE_TABLE[step as usize % SIZE_STEPS];
    let m = (0..STAGES).fold(0, |m, k| m.max(t[3 * k]).max(t[3 * k + 1]));
    (1.5 * 13.5 * m as f32 / fs_ring).max(0.3)
}

/// Target RT60 in seconds: 0.3·40^TIME, no shorter than `t_min`.
pub fn rt60(time: f32, step: u8, fs_ring: f32) -> f32 {
    const LOG2_40: f32 = 5.321_928;
    (0.3 * exp2(clampf(time, 0.0, 1.0) * LOG2_40)).max(t_min(step, fs_ring))
}

/// g_k = 10^(−3·L_k / (RT60·fs_ring)), capped at `MAX_GAIN`.
pub fn stage_gains(rt60: f32, step: u8, fs_ring: f32) -> [f32; STAGES] {
    core::array::from_fn(|k| {
        let l = stage_len(step, k) as f32;
        exp2(-3.0 * LOG2_10 * l / (rt60 * fs_ring)).min(MAX_GAIN)
    })
}

/// DAMP's one-pole coefficient, 1 − exp(−2π·f_c / fs_ring), with
/// f_c = 11,000·(1.5/11)^DAMP Hz.
pub fn damp_coef(damp: f32, fs_ring: f32) -> f32 {
    const LOG2_RATIO: f32 = -2.874_469; // log2(1.5 / 11)
    let fc = 11_000.0 * exp2(clampf(damp, 0.0, 1.0) * LOG2_RATIO);
    1.0 - exp2(-2.0 * PI * fc / fs_ring * LOG2_E)
}

/// The earliest a send can reach the return at `step`, in output samples:
/// the shortest tap at twice the ring's rate (the FIRs only add to it).
pub fn first_reflection(step: u8) -> usize {
    let t = &SIZE_TABLE[step as usize % SIZE_STEPS];
    TAPS.iter()
        .flatten()
        .map(|&(k, frac, _)| 2 * (frac * t[3 * k + 2] as f32) as usize)
        .min()
        .unwrap_or(0)
}

/// A delay or damper write: `x` (in LSBs) rounded to the nearest LSB,
/// ties away from zero; saturates at the i16 range. The lines store plain
/// i16 (Q3 over ±2.0) at a fixed 1 LSB.
#[inline(always)]
pub fn q_round_fixed(x: f32) -> i16 {
    rnd(x).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

/// An allpass state write: `x` (in LSBs) truncated toward zero onto the
/// LSB; saturates at the i16 range. Rounding there sustains a deadband
/// tail.
#[inline(always)]
pub fn q_trunc_fixed(x: f32) -> i16 {
    (x as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

/// SMEAR 1's depth D, a share of the ring's round trip (ADR 0121): the
/// four stage delays together move the loop by at most 1.2 %, and a held
/// tone's pitch by at most 15 cents (P95) at RATE's top.
pub const SMEAR_SHARE: f32 = 0.0015;

/// SMEAR's depth D at `step`, in ring samples: SMEAR × `SMEAR_SHARE` of
/// the round trip, so SMEAR 1 is the cap at every SIZE and the whole knob
/// always does something.
pub fn smear_depth(smear: f32, step: u8) -> f32 {
    let trip: u32 = (0..STAGES).map(|k| stage_len(step, k) as u32).sum();
    clampf(smear, 0.0, 1.0) * SMEAR_SHARE * trip as f32
}

/// The SMEAR reads each stage delay made on this thread since the last
/// reset: (shortest, longest), in ring samples back. Its own feature, which
/// only this crate's tests turn on: the simulator's audio path, built with
/// `test-support`, never pays for it.
#[cfg(any(test, feature = "smear-probe"))]
pub mod smear_probe {
    extern crate std;
    use super::STAGES;
    use core::cell::Cell;

    const NONE: [(f32, f32); STAGES] = [(f32::INFINITY, f32::NEG_INFINITY); STAGES];

    std::thread_local! {
        static READS: Cell<[(f32, f32); STAGES]> = const { Cell::new(NONE) };
    }

    pub(super) fn note(k: usize, back: f32) {
        READS.with(|r| {
            let mut v = r.get();
            v[k] = (v[k].0.min(back), v[k].1.max(back));
            r.set(v);
        });
    }

    pub fn reads() -> [(f32, f32); STAGES] {
        READS.with(|r| r.get())
    }

    pub fn reset() {
        READS.with(|r| r.set(NONE));
    }
}

/// RATE's wander speed: 0.1 Hz at 0 to 2 Hz at 1, exponential, every
/// TYPE's law (ADR 0121).
pub fn smear_hz(rate: f32) -> f32 {
    const LOG2_20: f32 = 4.321_928;
    0.1 * exp2(clampf(rate, 0.0, 1.0) * LOG2_20)
}

/// Nearest integer, ties away from zero, saturating, NaN to 0: VCVTA on
/// the M7 (`core` has no `f32::round`).
#[inline(always)]
pub fn rnd(x: f32) -> i32 {
    #[cfg(all(target_arch = "arm", target_feature = "vfp2"))]
    {
        // FPv5's VCVTA; an FPU without it fails to assemble here.
        let r: f32;
        // SAFETY: one register-to-register convert; no memory or stack
        // touched. May set FPSCR's exception flags (IXC on an inexact
        // result, IOC on NaN); nothing reads them.
        unsafe {
            core::arch::asm!("vcvta.s32.f32 {r}, {x}", r = lateout(sreg) r, x = in(sreg) x,
                options(pure, nomem, nostack));
        }
        r.to_bits() as i32
    }
    #[cfg(not(all(target_arch = "arm", target_feature = "vfp2")))]
    {
        rnd_portable(x)
    }
}

/// `rnd` as a float, `x` itself when already integral (so ±inf and NaN
/// too): VRINTA on the M7. Scaled and passed through `rnd`, it gives what
/// `rnd(x) as f32` would.
#[inline(always)]
pub fn rint(x: f32) -> f32 {
    #[cfg(all(target_arch = "arm", target_feature = "vfp2"))]
    {
        let r: f32;
        // SAFETY: as `rnd`'s. (VRINTA raises only IOC, on a signalling
        // NaN.)
        unsafe {
            core::arch::asm!("vrinta.f32 {r}, {x}", r = lateout(sreg) r, x = in(sreg) x,
                options(pure, nomem, nostack));
        }
        r
    }
    #[cfg(not(all(target_arch = "arm", target_feature = "vfp2")))]
    {
        rint_portable(x)
    }
}

/// `rnd` without the FPU instruction: the host's path.
pub fn rnd_portable(x: f32) -> i32 {
    // Past 2^31 `as` saturates, as VCVTA does, and NaN goes to 0.
    rint_portable(x) as i32
}

/// `rint` without the FPU instruction: the host's path.
pub fn rint_portable(x: f32) -> f32 {
    // From 2^23 up every float is an integer, as are ±inf, and NaN stays
    // NaN; below it the fraction is exact.
    if x.is_nan() || x.abs() >= 8_388_608.0 {
        return x;
    }
    let t = x as i32 as f32;
    let f = x - t;
    let r = if f >= 0.5 {
        t + 1.0
    } else if f <= -0.5 {
        t - 1.0
    } else {
        t
    };
    // VRINTA keeps the sign of a zero result.
    if r == 0.0 { 0.0f32.copysign(x) } else { r }
}

/// One block's settings for the ring; the gains, DAMP and each stage
/// delay's SMEAR offset ramp from `*_from` to `*_to` across it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingBlock {
    pub gains_from: [f32; STAGES],
    pub gains_to: [f32; STAGES],
    pub damp_from: f32,
    pub damp_to: f32,
    /// Each stage delay read `D·(1 + x)` samples short, as (from, to);
    /// `None` reads them fixed, bit for bit as before SMEAR.
    pub smear: Option<[(f32, f32); STAGES]>,
}

/// The ring's state; its lines are the `ReverbArena`'s slots. Zero bytes
/// are `new()`'s state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ring {
    /// The shared write head, below `RING_LEN`.
    head: u16,
    lp: [f32; STAGES],
    step: u8,
    from: u8,
    xf: u16,
}

crate::in_place::field_list!(Ring => Ring { head, lp, step, from, xf });

/// `i` below 2·`RING_LEN`, wrapped under it.
#[inline(always)]
fn wrap(i: usize) -> usize {
    if i >= RING_LEN { i - RING_LEN } else { i }
}

impl Ring {
    pub const fn new() -> Self {
        Self {
            head: 0,
            lp: [0.0; STAGES],
            step: 0,
            from: 0,
            xf: 0,
        }
    }

    /// The SIZE step the lines read at (a crossfade's target).
    pub fn step(&self) -> u8 {
        self.step
    }

    pub fn crossfading(&self) -> bool {
        self.xf > 0
    }

    /// Ask for SIZE `step`: a crossfade starts unless one is running; a
    /// request during one waits for the first call after it ends.
    pub fn request(&mut self, step: u8) {
        let step = step.min(SIZE_STEPS as u8 - 1);
        if self.xf == 0 && step != self.step {
            (self.from, self.step, self.xf) = (self.step, step, XFADE);
        }
    }

    /// Jump to `step` with no crossfade: only before anything sounds.
    pub fn snap(&mut self, step: u8) {
        let step = step.min(SIZE_STEPS as u8 - 1);
        (self.from, self.step, self.xf) = (step, step, 0);
    }

    pub fn damp_state(&self) -> [f32; STAGES] {
        self.lp
    }

    /// Every line and state is exactly zero. At high DAMP (a < ½) the
    /// rounded DAMP state can hold ±Δ for good while the return is already
    /// exactly zero, so this may never become true: gate nothing on it.
    pub fn is_silent(&self, arena: &ReverbArena) -> bool {
        self.lp.iter().all(|&s| s == 0.0) && arena.lines().iter().all(|&s| s == 0)
    }

    /// `u` is the decimated send (1.0 full scale); `l` and `r` get the tap
    /// sums in LSBs. The lines span the whole arena.
    pub fn process(
        &mut self,
        arena: &mut ReverbArena,
        u: &[f32; HALF],
        blk: &RingBlock,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        let lines = arena.lines_mut();
        // A SIZE crossfade never runs under SMEAR: `QuadState` asks for
        // one only with SMEAR at rest.
        if self.xf > 0 {
            debug_assert!(blk.smear.is_none());
            self.run::<true>(lines, u, blk, l, r);
        } else {
            self.run::<false>(lines, u, blk, l, r);
        }
    }

    /// SMEAR is a runtime branch at the four stage-delay reads, not a
    /// third copy of this loop: a monomorph costs about 4 KB of flash.
    fn run<const XF: bool>(
        &mut self,
        lines: &mut [i16; RING_LEN],
        u: &[f32; HALF],
        blk: &RingBlock,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        // Each read's offset from the head: `HEAD[j]` − d, d ≤ `BASE[j]`
        // (the table fits, and a tap is a fraction of its line), so at
        // least `OFFSET[j]`; the clamp keeps the bound below local.
        let back = |j: usize, d: u16| (HEAD[j] - d as usize).min(RING_LEN - 1);
        let reads =
            |t: &[u16; LINES]| -> [usize; LINES] { core::array::from_fn(|j| back(j, t[j])) };
        let taps = |t: &[u16; LINES]| -> [[usize; 3]; 2] {
            core::array::from_fn(|s| {
                core::array::from_fn(|i| {
                    let (k, frac, _) = TAPS[s][i];
                    back(3 * k + 2, (frac * t[3 * k + 2] as f32) as u16)
                })
            })
        };
        let (new, old) = (
            &SIZE_TABLE[self.step as usize],
            &SIZE_TABLE[self.from as usize],
        );
        let (kn, ko) = (reads(new), reads(old));
        let (tn, to) = (taps(new), taps(old));
        let n = HALF as f32;
        let dg: [f32; STAGES] = core::array::from_fn(|k| (blk.gains_to[k] - blk.gains_from[k]) / n);
        let da = (blk.damp_to - blk.damp_from) / n;
        let mut g = blk.gains_from;
        let mut a = blk.damp_from;
        // SMEAR: each stage delay's nominal length, and its offset ramp.
        let smear = !XF && blk.smear.is_some();
        let sm = blk.smear.unwrap_or_default();
        let nominal: [f32; STAGES] = core::array::from_fn(|k| new[3 * k + 2] as f32);
        let doff: [f32; STAGES] = core::array::from_fn(|k| (sm[k].1 - sm[k].0) / n);
        let mut off: [f32; STAGES] = core::array::from_fn(|k| sm[k].0);
        let mut lp = self.lp;
        let mut xf = self.xf;
        let mut h = (self.head as usize).min(RING_LEN - 1);
        let mut i = 0;
        // In segments that no access wraps in: each access's slot at the
        // segment's start, plus the sample's index in it.
        while i < HALF {
            let at = |k: usize| wrap(h + k);
            let (bn, bo, bw) = (kn.map(at), ko.map(at), HEAD.map(at));
            let (sn, so) = (tn.map(|s| s.map(at)), to.map(|s| s.map(at)));
            let mut len = HALF - i;
            let slots = bn.iter().chain(&bw).chain(sn.as_flattened());
            for &b in slots {
                len = len.min(RING_LEN - b);
            }
            if XF {
                for &b in bo.iter().chain(so.as_flattened()) {
                    len = len.min(RING_LEN - b);
                }
            }
            for t in 0..len {
                macro_rules! rd {
                    ($b:expr) => {
                        // SAFETY: `h` and each offset are below `RING_LEN`,
                        // so `wrap` puts each slot b under it; `len` ≤
                        // `RING_LEN` − b and `t` < `len`, so b + t is in
                        // range. The writes below likewise.
                        (unsafe { *lines.get_unchecked($b + t) }) as f32
                    };
                }
                let w = if XF {
                    let w = 1.0 - xf as f32 * XFADE_INV;
                    xf = xf.saturating_sub(1);
                    w
                } else {
                    1.0
                };
                macro_rules! blend {
                    ($nb:expr, $ob:expr) => {{
                        let v = rd!($nb);
                        if XF {
                            let o = rd!($ob);
                            o + w * (v - o)
                        } else {
                            v
                        }
                    }};
                }
                let mut out = [0.0f32; STAGES];
                unroll!(K in [0, 1, 2, 3] {
                    let j = 3 * K + 2;
                    let d = if smear {
                        // `bw[j] + t` is line j's write slot, below
                        // `RING_LEN`; the read is 2 ≤ d ≤ `BASE[j]` back.
                        let back = nominal[K] - off[K];
                        #[cfg(any(test, feature = "smear-probe"))]
                        smear_probe::note(K, back);
                        pair(lines, bw[j] + t, back)
                    } else {
                        blend!(bn[j], bo[j])
                    };
                    lp[K] = q_round_fixed(lp[K] + a * (d - lp[K])) as f32;
                    out[K] = g[K] * lp[K];
                });
                let mut side = [0.0f32; 2];
                unroll!(S in [0, 1] {
                    unroll!(T in [0, 1, 2] {
                        side[S] += TAPS[S][T].2 * blend!(sn[S][T], so[S][T]);
                    });
                });
                (l[i + t], r[i + t]) = (side[0], side[1]);
                let inj = u[i + t] * FULL_SCALE;
                unroll!(K in [0, 1, 2, 3] {
                    let mut x = out[(K + STAGES - 1) % STAGES];
                    if INJECT[K] != 0.0 {
                        x += INJECT[K] * inj;
                    }
                    unroll!(M in [0, 1] {
                        let (j, c) = (3 * K + M, AP_COEF[K][M]);
                        let vd = blend!(bn[j], bo[j]);
                        let vq = q_trunc_fixed(x + c * vd);
                        // SAFETY: as the reads.
                        unsafe { *lines.get_unchecked_mut(bw[j] + t) = vq };
                        x = vd - c * vq as f32;
                    });
                    // SAFETY: as the reads.
                    unsafe { *lines.get_unchecked_mut(bw[3 * K + 2] + t) = q_round_fixed(x) };
                });
                unroll!(K in [0, 1, 2, 3] {
                    g[K] += dg[K];
                    if smear {
                        off[K] += doff[K];
                    }
                });
                a += da;
            }
            i += len;
            h = wrap(h + len);
        }
        self.lp = lp;
        self.xf = xf;
        self.head = h as u16;
    }
}

/// QUAD's ring controls, each 0..1. SMEAR and RATE are `Reverb`'s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingControls {
    pub time: f32,
    pub damp: f32,
    pub size: f32,
}

impl Default for RingControls {
    fn default() -> Self {
        Self {
            time: 0.5,
            damp: 0.3,
            size: 0.5,
        }
    }
}

impl RingControls {
    /// Each control clamped to 0..1; NaN reads as its default.
    pub fn sanitised(self) -> Self {
        let d = Self::default();
        let f = |v: f32, d: f32| if v.is_nan() { d } else { clampf(v, 0.0, 1.0) };
        Self {
            time: f(self.time, d.time),
            damp: f(self.damp, d.damp),
            size: f(self.size, d.size),
        }
    }
}

/// One-pole smoothing time constants, in seconds.
const TIME_TAU: f32 = 0.050;
const DAMP_TAU: f32 = 0.020;

/// A once-per-block one-pole's coefficient for time constant `tau`.
fn smoothing(tau: f32, sample_rate: u32) -> f32 {
    1.0 - exp2(-(BLOCK_SIZE as f32) / (tau * sample_rate as f32) * LOG2_E)
}

/// QUAD's state: the ring and its smoothed controls, at the ring's rate.
/// The rate change, SMEAR's source and depth, and MIX are `Reverb`'s.
/// Zero bytes are the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadState {
    ring: Ring,
    /// Smoothed TIME and DAMP.
    time: f32,
    damp: f32,
    /// Last block's gains and DAMP coefficient: this block's ramp start.
    gains: [f32; STAGES],
    damp_coef: f32,
    primed: bool,
}

crate::in_place::field_list!(QuadState => QuadState { ring, time, damp, gains, damp_coef, primed });

impl QuadState {
    pub fn ring(&self) -> &Ring {
        &self.ring
    }

    /// Whether SIZE asks for another step than the lines read, or a
    /// crossfade runs: SMEAR must rest first.
    pub fn wants_size(&self, size: f32) -> bool {
        let size = RingControls {
            size,
            ..RingControls::default()
        }
        .sanitised()
        .size;
        self.primed && (size_step(size) != self.ring.step() || self.ring.crossfading())
    }

    /// One block of the decimated send `u` through the ring in `arena`;
    /// `out` gets the L and R tap sums in LSBs. With `smear`, each stage
    /// delay reads its offset short and SIZE waits.
    pub fn process(
        &mut self,
        arena: &mut ReverbArena,
        u: &[f32; HALF],
        ctl: &RingControls,
        smear: Option<[(f32, f32); STAGES]>,
        sample_rate: u32,
        out: &mut [[f32; HALF]; 2],
    ) {
        let ctl = ctl.sanitised();
        let fs = sample_rate as f32 * 0.5;
        let step = size_step(ctl.size);
        if self.primed {
            let k = |tau| smoothing(tau, sample_rate);
            self.time += k(TIME_TAU) * (ctl.time - self.time);
            self.damp += k(DAMP_TAU) * (ctl.damp - self.damp);
            if smear.is_none() {
                self.ring.request(step);
            }
        } else {
            (self.time, self.damp) = (ctl.time, ctl.damp);
            self.ring.snap(step);
        }
        let now = self.ring.step();
        let gains = stage_gains(rt60(self.time, now, fs), now, fs);
        let damp = damp_coef(self.damp, fs);
        if !self.primed {
            (self.gains, self.damp_coef, self.primed) = (gains, damp, true);
        }
        let blk = RingBlock {
            gains_from: self.gains,
            gains_to: gains,
            damp_from: self.damp_coef,
            damp_to: damp,
            smear,
        };
        (self.gains, self.damp_coef) = (gains, damp);
        let [l, r] = out;
        self.ring.process(arena, u, &blk, l, r);
    }
}
