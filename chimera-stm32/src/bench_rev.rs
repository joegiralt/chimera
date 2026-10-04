//! Throwaway worst-case reverb kernels (reverb TYPEs plan Task 3, spec
//! § CPU budget): HALL's inner loop fixed at its worst, and
//! one moving pair against two fixed reads, each timed beside QUAD's
//! crossfading ring on the same bench. No knobs: constants only. Task 8
//! took BURST's out for the real engine (REV GATE, REV RVRS), Task 9
//! ROOM's (REV ROOM); Task 10 deletes this file.
//!
//! Each kernel lays its lines out as `ring.rs` does (one shared write
//! head, wrap-free segments for the fixed reads), its f32 lines likewise
//! in an f32 region above them. Values are in LSBs (1.0 = `FULL_SCALE`).

// Indexed loops over the lines' bases read as the engine's will.
#![allow(clippy::needless_range_loop)]

use core::hint::black_box;
use core::ptr::addr_of_mut;

use chimera_core::dsp::halfband::HALF;
use chimera_core::dsp::rev::arena::ReverbArena;
use chimera_core::dsp::rev::parts::{
    Wander, ap_f32, ap_norm, pair, soft_clip, store_round, store_trunc, wander_seeds,
};
use chimera_core::dsp::ring::{FULL_SCALE, QuadState, RING_LEN, RingControls, rint};
use chimera_core::hw::SAMPLE_RATE;

/// One ring block (`HALF` samples) of the decimated send `u` through the
/// kernel; `l` and `r` get its return.
pub trait RevKernel {
    fn run(
        &mut self,
        arena: &mut ReverbArena,
        u: &[f32; HALF],
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    );
}

/// The REVERB screen's kernel rows, in `with_kernel`'s order. K FIX is K
/// PAIR's 32 fixed reads: (K PAIR − K FIX) / 16 is a pair's marginal
/// cost. K CONV is the converter stubs alone, which every kernel but K
/// QUAD carries (GRIT has since left: the kernels are pessimistic by it).
pub const ROWS: [&str; 5] = ["K QUAD", "K HALL", "K PAIR", "K FIX", "K CONV"];

/// `f` on row `i`'s kernel, fresh.
pub fn with_kernel(i: usize, f: impl FnOnce(&mut dyn RevKernel) -> u32) -> u32 {
    match i {
        0 => f(&mut KQuad::default()),
        1 => f(&mut KHall::new()),
        2 => f(&mut KPair::<true>::new()),
        3 => f(&mut KPair::<false>::new()),
        _ => f(&mut KConv(Conv::new())),
    }
}

/// The kernels' own arena, beside `FxBus`'s (both in AXI SRAM).
static mut ARENA: ReverbArena = ReverbArena::new();

/// The kernels' arena, zeroed.
pub fn arena() -> &'static mut ReverbArena {
    // SAFETY: only the bench calls this, once per kernel row, from `run`
    // before any interrupt is unmasked, and drops the reference before the
    // next call; nothing else names `ARENA`.
    let a = unsafe { &mut *addr_of_mut!(ARENA) };
    a.clear();
    a
}

// ---- layout ---------------------------------------------------------------

/// Each line's write offset from the shared head, and the slots taken:
/// line j owns `len[j] + 2` slots (a pair's second read, and the guard),
/// so a read d back, 2 ≤ d ≤ `len[j]` + 1, is what line j wrote d samples
/// ago. The total is rounded up to even, so an f32 region may follow.
const fn heads<const N: usize>(len: [usize; N]) -> ([usize; N], usize) {
    let mut w = [0; N];
    let mut o = 0;
    let mut j = 0;
    while j < N {
        o += len[j] + 2;
        w[j] = o - 1;
        j += 1;
    }
    (w, o + o % 2)
}

/// `w`'s line read `d` back.
const fn back<const N: usize>(w: &[usize; N], j: usize, d: usize) -> usize {
    w[j] - d
}

#[inline(always)]
fn wrap(i: usize, m: usize) -> usize {
    if i >= m { i - m } else { i }
}

/// Each offset's slot at head `h` (mod `m`), and `len` cut so no access
/// wraps inside the segment.
#[inline(always)]
fn seg<const A: usize>(h: usize, m: usize, off: &[usize; A], len: &mut usize) -> [usize; A] {
    off.map(|o| {
        let b = wrap(h + o, m);
        *len = (*len).min(m - b);
        b
    })
}

/// SMEAR's moving read over a line region of `M` slots: `parts::pair`'s
/// form, `d` (≥ 2) behind `base`.
#[inline(always)]
fn pair_m<T: Copy + Into<f32>, const M: usize>(lines: &[T; M], base: usize, d: f32) -> f32 {
    let base = base.min(M - 1);
    let di = (d as usize).min(M - 1);
    let f = d - di as f32;
    let i0 = wrap(base + M - di, M);
    let i1 = wrap(i0 + M - 1, M);
    // SAFETY: `base` and `di` are below M, so `base + M − di` is below 2M
    // and `wrap` puts it below M; `i0 + M − 1` likewise.
    let (x0, x1) = unsafe {
        (
            (*lines.get_unchecked(i0)).into(),
            (*lines.get_unchecked(i1)).into(),
        )
    };
    x0 + f * (x1 - x0)
}

/// The i16 region below `m` and the `f` f32s above it.
fn regions<const M: usize, const F: usize>(
    arena: &mut ReverbArena,
) -> (&mut [i16; M], &mut [f32; F]) {
    let (lo, f, _) = arena.split(M, F);
    (
        lo.try_into().expect("the split is M slots"),
        f.try_into().expect("the region is F f32s"),
    )
}

// ---- shared pieces ---------------------------------------------------------

const INV_FS: f32 = 1.0 / FULL_SCALE;
const FS_RING: f32 = SAMPLE_RATE as f32 * 0.5;

/// R1's clip on an LSB value.
#[inline(always)]
fn clip(x: f32) -> f32 {
    soft_clip(x * INV_FS) * FULL_SCALE
}

#[inline(always)]
fn xs(s: &mut u32) -> f32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    (*s as i32) as f32 * (1.0 / i32::MAX as f32)
}

/// GRIT's converters as step 0 bills them: 3 quantisations, 3 one-poles
/// and 2 xorshift steps a ring sample (ADC in, DAC out).
#[derive(Clone, Copy)]
struct Conv {
    rng: [u32; 2],
    lp: [f32; 3],
    a: f32,
    step: f32,
    inv: f32,
}

impl Conv {
    fn new() -> Self {
        let step = black_box(16.0);
        Self {
            rng: [0x1234_5679, 0x9e37_79b9],
            lp: [0.0; 3],
            a: black_box(0.6),
            step,
            inv: 1.0 / step,
        }
    }

    #[inline(always)]
    fn q(&self, x: f32) -> f32 {
        rint(x * self.inv) * self.step
    }

    #[inline(always)]
    fn adc(&mut self, x: f32) -> f32 {
        let n = xs(&mut self.rng[0]);
        self.lp[0] += self.a * (x - self.lp[0]);
        self.q(self.lp[0] + n * self.step)
    }

    #[inline(always)]
    fn dac(&mut self, l: f32, r: f32) -> (f32, f32) {
        let n = xs(&mut self.rng[1]) * self.step;
        self.lp[1] += self.a * (l - self.lp[1]);
        self.lp[2] += self.a * (r - self.lp[2]);
        (self.q(self.lp[1] + n), self.q(self.lp[2] - n))
    }
}

/// A crossfade that always runs: it flips between two length sets every
/// `period` ring samples, starting the next at the first block after.
#[derive(Clone, Copy, Default)]
struct Flip {
    near: bool,
    left: u16,
}

impl Flip {
    /// At a block's start: (to the near set, samples left), restarted.
    fn block(&mut self, period: u16) -> (bool, u16) {
        if self.left == 0 {
            (self.near, self.left) = (!self.near, period);
        }
        (self.near, self.left)
    }
}

/// One smeared line: its wander, and its read offset's ramp this block.
#[derive(Clone, Copy)]
struct Smear {
    w: Wander,
    depth: f32,
}

impl Smear {
    fn new(seed: u32, phase: f32, depth: f32) -> Self {
        let mut w = Wander::new(seed, phase);
        w.set_rate(2.0, FS_RING);
        Self {
            w,
            depth: black_box(depth),
        }
    }

    /// (offset at the block's start, its step a sample).
    #[inline(always)]
    fn block(&mut self) -> (f32, f32) {
        let x0 = self.w.value();
        let x1 = self.w.block();
        let o0 = self.depth * (1.0 + x0);
        (o0, (self.depth * (1.0 + x1) - o0) * (1.0 / HALF as f32))
    }
}

// ---- K QUAD ------------------------------------------------------------------

/// QUAD's worst, the reference: `worst_reverb`'s TIME 1, DAMP 0.5, SIZE
/// alternating steps 31 and 30 so a crossfade always runs, SMEAR off.
#[derive(Default)]
pub struct KQuad {
    q: QuadState,
    block: u32,
    out: [[f32; HALF]; 2],
}

impl RevKernel for KQuad {
    fn run(
        &mut self,
        arena: &mut ReverbArena,
        u: &[f32; HALF],
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        let size = [1.0, 30.0 / 31.0][self.block as usize % 2];
        self.block += 1;
        let ctl = RingControls {
            time: 1.0,
            damp: 0.5,
            size,
        };
        self.q
            .process(arena, u, &ctl, None, SAMPLE_RATE, &mut self.out);
        (*l, *r) = (self.out[0], self.out[1]);
    }
}

// ---- K CONV ------------------------------------------------------------------

/// The converter stubs alone.
pub struct KConv(Conv);

impl RevKernel for KConv {
    fn run(
        &mut self,
        _arena: &mut ReverbArena,
        u: &[f32; HALF],
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        for i in 0..HALF {
            let x = self.0.adc(u[i] * FULL_SCALE);
            (l[i], r[i]) = self.0.dac(x, -x);
        }
    }
}

// ---- crossfading reads ------------------------------------------------------

/// The crossfading lines (PRE, 4 delays) at their far and near lengths:
/// one crossfade moves the round trip 120 samples (5 ms), PRE 120.
const XF_LINES: [usize; 5] = [1, 2, 3, 4, 0];
const fn xf_reads<const N: usize, const X: usize>(
    w: &[usize; N],
    len: &[usize; N],
    lines: [usize; X],
    near: usize,
) -> [usize; X] {
    let mut o = [0; X];
    let mut i = 0;
    while i < X {
        let j = lines[i];
        // PRE (line 0) moves 120, each loop delay 30.
        let step = if j == 0 { 120 } else { 30 };
        o[i] = back(w, j, len[j] + 1 - near * step);
        i += 1;
    }
    o
}

// ---- K HALL ------------------------------------------------------------------

/// HALL's lines at SIZE 1: PRE, delays S1–S4, stage APs 1–4; the
/// diffuser's four f32 APs.
const HALL_LEN: [usize; 9] = [3000, 2712, 3768, 2976, 3504, 264, 408, 312, 456];
const HALL_DIF: [usize; 4] = [41, 74, 127, 214];
const HALL: ([usize; 9], usize) = heads(HALL_LEN);
const HALL_F: ([usize; 4], usize) = heads(HALL_DIF);
const HALL_M: usize = HALL.1;
const HALL_FN: usize = HALL_F.1;
const _: () = assert!(HALL_M + 2 * HALL_FN <= RING_LEN);
const HALL_FAR: [usize; 5] = xf_reads(&HALL.0, &HALL_LEN, XF_LINES, 0);
const HALL_NEAR: [usize; 5] = xf_reads(&HALL.0, &HALL_LEN, XF_LINES, 1);
/// The fixed stage-AP reads (2, 4) and the taps: L (S1 0.30 of SIZE 0,
/// +), (S3 0.61, −), (S2 0.83, +); R (S2 0.25 of SIZE 0, +), (S4 0.49, −),
/// (S1 0.91, +).
const HALL_AP: [usize; 2] = [back(&HALL.0, 6, 409), back(&HALL.0, 8, 457)];
const HALL_TAP: [usize; 6] = [
    back(&HALL.0, 1, 90),
    back(&HALL.0, 3, 1815),
    back(&HALL.0, 2, 3127),
    back(&HALL.0, 2, 104),
    back(&HALL.0, 4, 1717),
    back(&HALL.0, 1, 2468),
];
const HALL_DIF_RD: [usize; 4] = [
    back(&HALL_F.0, 0, 42),
    back(&HALL_F.0, 1, 75),
    back(&HALL_F.0, 2, 128),
    back(&HALL_F.0, 3, 215),
];

/// HALL's worst: as ROOM's, one loop of 4 stages, APs 1 and 3 smeared,
/// dampers after stages 2 and 4, one soft clip at re-entry.
pub struct KHall {
    h: usize,
    hf: usize,
    flip: Flip,
    conv: Conv,
    tone: f32,
    ta: f32,
    lp: [f32; 2],
    damp: [f32; 2],
    gains: [f32; 4],
    g_dif: f32,
    g_ap: f32,
    smear: [Smear; 2],
}

impl KHall {
    fn new() -> Self {
        let seeds = wander_seeds::<2>(0x5EED_0002);
        Self {
            h: 0,
            hf: 0,
            flip: Flip::default(),
            conv: Conv::new(),
            tone: 0.0,
            ta: black_box(0.5),
            lp: [0.0; 2],
            damp: black_box([0.4, 0.4]),
            gains: black_box([0.985; 4]),
            g_dif: black_box(0.75),
            g_ap: black_box(0.75),
            smear: [
                Smear::new(seeds[0], 0.0, 10.0),
                Smear::new(seeds[1], 0.5, 10.0),
            ],
        }
    }
}

impl RevKernel for KHall {
    fn run(
        &mut self,
        arena: &mut ReverbArena,
        u: &[f32; HALF],
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        let (li, lf) = regions::<HALL_M, HALL_FN>(arena);
        let (near, mut xf) = self.flip.block(720);
        let (kn, ko) = if near {
            (HALL_NEAR, HALL_FAR)
        } else {
            (HALL_FAR, HALL_NEAR)
        };
        let n = HALF as f32;
        let mut g = self.gains;
        let dg = g.map(|g| (black_box(g) - g) / n);
        let mut a = self.damp[0];
        let da = (self.damp[1] - a) / n;
        let (ga, inv) = (self.g_ap, 1.0 / (1.0 - self.g_ap));
        let gd = self.g_dif;
        let (mut o1, do1) = self.smear[0].block();
        let (mut o3, do3) = self.smear[1].block();
        let nom = [HALL_LEN[5] as f32, HALL_LEN[7] as f32];
        let (mut lp, mut tone, ta) = (self.lp, self.tone, self.ta);
        let (mut h, mut hf) = (self.h, self.hf);
        let mut i = 0;
        while i < HALF {
            let mut len = HALF - i;
            let bw = seg(h, HALL_M, &HALL.0, &mut len);
            let bn = seg(h, HALL_M, &kn, &mut len);
            let bo = seg(h, HALL_M, &ko, &mut len);
            let bap = seg(h, HALL_M, &HALL_AP, &mut len);
            let bt = seg(h, HALL_M, &HALL_TAP, &mut len);
            let fw = seg(hf, HALL_FN, &HALL_F.0, &mut len);
            let fr = seg(hf, HALL_FN, &HALL_DIF_RD, &mut len);
            for t in 0..len {
                macro_rules! rd {
                    ($b:expr) => {
                        // SAFETY: each base b is below its region's length
                        // m, `len` ≤ m − b and `t` < `len`.
                        (unsafe { *li.get_unchecked($b + t) }) as f32
                    };
                }
                macro_rules! wr {
                    ($b:expr, $v:expr) => {
                        // SAFETY: each base b is below its region's length
                        // m, `len` ≤ m − b and `t` < `len`.
                        unsafe { *li.get_unchecked_mut($b + t) = $v }
                    };
                }
                macro_rules! frd {
                    ($b:expr) => {
                        // SAFETY: each base b is below its region's length
                        // m, `len` ≤ m − b and `t` < `len`.
                        unsafe { *lf.get_unchecked($b + t) }
                    };
                }
                macro_rules! fwr {
                    ($b:expr, $v:expr) => {
                        // SAFETY: each base b is below its region's length
                        // m, `len` ≤ m − b and `t` < `len`.
                        unsafe { *lf.get_unchecked_mut($b + t) = $v }
                    };
                }
                let w = 1.0 - xf as f32 * (1.0 / 720.0);
                xf = xf.saturating_sub(1);
                macro_rules! blend {
                    ($k:expr) => {{
                        let (v, o) = (rd!(bn[$k]), rd!(bo[$k]));
                        o + w * (v - o)
                    }};
                }
                let x = self.conv.adc(u[i + t] * FULL_SCALE);
                tone += ta * (x - tone);
                wr!(bw[0], store_round(tone));
                let mut d = blend!(4);
                for k in 0..4 {
                    let (y, v) = ap_f32(d, frd!(fr[k]), gd);
                    fwr!(fw[k], v);
                    d = y;
                }
                // S1 → AP1 (smeared) → S2 → AP2 → DAMP → S3 → AP3 (smeared) → S4 → AP4 → DAMP.
                let (y, wn) = ap_norm(blend!(0), pair_m(li, bw[5] + t, nom[0] - o1), ga, inv);
                wr!(bw[5], store_trunc(wn));
                wr!(bw[2], store_round(g[0] * y));
                let (y, wn) = ap_norm(blend!(1), rd!(bap[0]), ga, inv);
                wr!(bw[6], store_trunc(wn));
                lp[0] = store_round(lp[0] + a * (y - lp[0])) as f32;
                wr!(bw[3], store_round(g[1] * lp[0]));
                let (y, wn) = ap_norm(blend!(2), pair_m(li, bw[7] + t, nom[1] - o3), ga, inv);
                wr!(bw[7], store_trunc(wn));
                wr!(bw[4], store_round(g[2] * y));
                let (y, wn) = ap_norm(blend!(3), rd!(bap[1]), ga, inv);
                wr!(bw[8], store_trunc(wn));
                lp[1] = store_round(lp[1] + a * (y - lp[1])) as f32;
                wr!(bw[1], store_round(clip(g[3] * lp[1] + 0.5 * d)));
                let ol = rd!(bt[0]) - rd!(bt[1]) + rd!(bt[2]);
                let or = rd!(bt[3]) - rd!(bt[4]) + rd!(bt[5]);
                (l[i + t], r[i + t]) = self.conv.dac(ol, or);
                for k in 0..4 {
                    g[k] += dg[k];
                }
                a += da;
                o1 += do1;
                o3 += do3;
            }
            i += len;
            h = wrap(h + len, HALL_M);
            hf = wrap(hf + len, HALL_FN);
        }
        (self.lp, self.tone, self.h, self.hf) = (lp, tone, h, hf);
        self.flip.left = xf;
    }
}

// ---- K PAIR / K FIX ------------------------------------------------------------

/// 16 lines of 1,000 over the whole arena, each written once a sample and
/// read either by one moving pair (`MOVING`, `parts::pair`, its offset
/// ramping) or by two fixed reads a sample apart.
const PAIR_LEN: [usize; 16] = [1_000; 16];
const PAIR: ([usize; 16], usize) = heads(PAIR_LEN);
const _: () = assert!(PAIR.1 <= RING_LEN);
const fn pair_reads(d: usize) -> [usize; 16] {
    let mut o = [0; 16];
    let mut j = 0;
    while j < 16 {
        o[j] = back(&PAIR.0, j, d - j * 7);
        j += 1;
    }
    o
}
const PAIR_R0: [usize; 16] = pair_reads(900);
const PAIR_R1: [usize; 16] = pair_reads(901);

pub struct KPair<const MOVING: bool> {
    h: usize,
    smear: Smear,
}

impl<const MOVING: bool> KPair<MOVING> {
    fn new() -> Self {
        Self {
            h: 0,
            smear: Smear::new(0x5EED_0004, 0.0, 40.0),
        }
    }
}

impl<const MOVING: bool> RevKernel for KPair<MOVING> {
    fn run(
        &mut self,
        arena: &mut ReverbArena,
        u: &[f32; HALF],
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        let li = arena.lines_mut();
        let (mut o, d_o) = self.smear.block();
        let mut h = self.h;
        let mut i = 0;
        while i < HALF {
            let mut len = HALF - i;
            let bw = seg(h, RING_LEN, &PAIR.0, &mut len);
            let (b0, b1) = if MOVING {
                (bw, bw)
            } else {
                (
                    seg(h, RING_LEN, &PAIR_R0, &mut len),
                    seg(h, RING_LEN, &PAIR_R1, &mut len),
                )
            };
            for t in 0..len {
                let x = u[i + t] * FULL_SCALE;
                let mut acc = 0.0f32;
                for j in 0..16 {
                    acc += if MOVING {
                        pair(li, bw[j] + t, (900 - 7 * j) as f32 - o)
                    } else {
                        // SAFETY: each base b is below `RING_LEN`, `len` ≤
                        // `RING_LEN` − b and `t` < `len`.
                        unsafe {
                            *li.get_unchecked(b0[j] + t) as f32
                                + *li.get_unchecked(b1[j] + t) as f32
                        }
                    };
                }
                for j in 0..16 {
                    // SAFETY: as the reads.
                    unsafe { *li.get_unchecked_mut(bw[j] + t) = store_round(x + 0.05 * acc) };
                }
                (l[i + t], r[i + t]) = (acc, -acc);
                o += d_o;
            }
            i += len;
            h = wrap(h + len, RING_LEN);
        }
        self.h = h;
    }
}
