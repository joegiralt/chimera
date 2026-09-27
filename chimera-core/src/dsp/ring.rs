//! The reverb: four stages in a ring, after Sean Costello's description of
//! the Alesis Quadraverb (Valhalla DSP, KVR thread 349039): "4 parallel
//! loops (2 x AP + 1 delay), outputs from delay taps", each loop feeding the
//! next. Half rate, stored as i16 (FX diet spec § Reverb, ADR 0028).

use crate::dsp::Stereo;
use crate::dsp::algo::math::exp2;
use crate::dsp::halfband::{Decimator, HALF, Interpolator};
use chimera_hal::BLOCK_SIZE;
use core::f32::consts::{LOG2_10, LOG2_E, PI};
use core::mem::MaybeUninit;

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
/// 1.0 in i16 LSBs: ±2.0 fits.
pub const FULL_SCALE: f32 = 16_384.0;
/// Ring samples a SIZE step's crossfade lasts (30 ms at 24 kHz).
pub const XFADE: u16 = 720;
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

const fn is_prime(n: u32) -> bool {
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
    (size.clamp(0.0, 1.0) * 31.0 + 0.5) as u8
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
    (0.3 * exp2(time.clamp(0.0, 1.0) * LOG2_40)).max(t_min(step, fs_ring))
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
    let fc = 11_000.0 * exp2(damp.clamp(0.0, 1.0) * LOG2_RATIO);
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

/// GRIT's grid: a step of Δ = 2^(6·GRIT) i16 LSBs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    delta: f32,
    inv: f32,
}

impl Grid {
    pub fn new(grit: f32) -> Self {
        let delta = exp2(6.0 * grit.clamp(0.0, 1.0));
        Self {
            delta,
            inv: 1.0 / delta,
        }
    }

    pub fn delta(self) -> f32 {
        self.delta
    }

    /// `x` (in LSBs) rounded to the nearest grid point, and that to the
    /// nearest LSB, both ties away from zero; saturates at the i16 range.
    #[inline(always)]
    pub fn q_round(self, x: f32) -> i16 {
        rnd(rnd(x * self.inv) as f32 * self.delta).clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// `x` (in LSBs) truncated toward zero onto the grid, then onto the
    /// LSB; saturates at the i16 range. The allpass states use it: rounding
    /// there sustains a deadband tail.
    #[inline(always)]
    pub fn q_trunc(self, x: f32) -> i16 {
        (((x * self.inv) as i32 as f32 * self.delta) as i32).clamp(i16::MIN as i32, i16::MAX as i32)
            as i16
    }
}

/// Nearest integer, ties away from zero (`core` has no `f32::round`); `as`
/// saturates.
#[inline(always)]
fn rnd(x: f32) -> i32 {
    (x + if x < 0.0 { -0.5 } else { 0.5 }) as i32
}

/// One block's settings for the ring; the gains and DAMP ramp from
/// `*_from` to `*_to` across it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingBlock {
    pub gains_from: [f32; STAGES],
    pub gains_to: [f32; STAGES],
    pub damp_from: f32,
    pub damp_to: f32,
    pub grid: Grid,
}

pub struct Ring {
    lines: [i16; RING_LEN],
    pos: [u16; LINES],
    lp: [f32; STAGES],
    step: u8,
    from: u8,
    xf: u16,
}

crate::in_place::field_list!(Ring => Ring { lines, pos, lp, step, from, xf });

impl Ring {
    pub fn new() -> Self {
        Self {
            lines: [0; RING_LEN],
            pos: [0; LINES],
            lp: [0.0; STAGES],
            step: 0,
            from: 0,
            xf: 0,
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: every field is an integer or float array or an integer,
        // valid as zero bytes; zero is exactly `new()`'s state.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
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

    pub fn lines(&self) -> &[i16; RING_LEN] {
        &self.lines
    }

    pub fn damp_state(&self) -> [f32; STAGES] {
        self.lp
    }

    /// Nothing stored anywhere: the ring stays silent without input.
    pub fn is_silent(&self) -> bool {
        self.lp.iter().all(|&s| s == 0.0) && self.lines.iter().all(|&s| s == 0)
    }

    /// `u` is the decimated send (1.0 full scale); `l` and `r` get the tap
    /// sums in LSBs.
    pub fn process(
        &mut self,
        u: &[f32; HALF],
        blk: &RingBlock,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        if self.xf > 0 {
            self.run::<true>(u, blk, l, r);
        } else {
            self.run::<false>(u, blk, l, r);
        }
    }

    #[inline(always)]
    fn at(&self, j: usize, d: u16) -> f32 {
        let (p, d, cap) = (self.pos[j] as usize, d as usize, BASE[j] as usize + 1);
        let i = if p >= d { p - d } else { p + cap - d };
        self.lines[OFFSET[j] + i] as f32
    }

    /// Line `j` at delay `new`, or mid-crossfade blended from `old` by `w`.
    #[inline(always)]
    fn read<const XF: bool>(&self, j: usize, new: u16, old: u16, w: f32) -> f32 {
        let b = self.at(j, new);
        if XF {
            let a = self.at(j, old);
            a + w * (b - a)
        } else {
            b
        }
    }

    #[inline(always)]
    fn write(&mut self, j: usize, v: i16) {
        let p = self.pos[j] as usize;
        self.lines[OFFSET[j] + p] = v;
        self.pos[j] = if p == BASE[j] as usize {
            0
        } else {
            p as u16 + 1
        };
    }

    fn run<const XF: bool>(
        &mut self,
        u: &[f32; HALF],
        blk: &RingBlock,
        l: &mut [f32; HALF],
        r: &mut [f32; HALF],
    ) {
        let new = SIZE_TABLE[self.step as usize];
        let old = SIZE_TABLE[self.from as usize];
        let tap = |t: &[u16; LINES], s: usize, i: usize| -> u16 {
            let (k, frac, _) = TAPS[s][i];
            (frac * t[3 * k + 2] as f32) as u16
        };
        let taps_new: [[u16; 3]; 2] =
            core::array::from_fn(|s| core::array::from_fn(|i| tap(&new, s, i)));
        let taps_old: [[u16; 3]; 2] =
            core::array::from_fn(|s| core::array::from_fn(|i| tap(&old, s, i)));
        let n = HALF as f32;
        let dg: [f32; STAGES] = core::array::from_fn(|k| (blk.gains_to[k] - blk.gains_from[k]) / n);
        let da = (blk.damp_to - blk.damp_from) / n;
        let mut g = blk.gains_from;
        let mut a = blk.damp_from;
        let grid = blk.grid;
        for i in 0..HALF {
            let w = if XF {
                let w = 1.0 - self.xf as f32 / XFADE as f32;
                self.xf = self.xf.saturating_sub(1);
                w
            } else {
                1.0
            };
            let mut out = [0.0f32; STAGES];
            for k in 0..STAGES {
                let j = 3 * k + 2;
                let d = self.read::<XF>(j, new[j], old[j], w);
                self.lp[k] = grid.q_round(self.lp[k] + a * (d - self.lp[k])) as f32;
                out[k] = g[k] * self.lp[k];
            }
            let mut side = [0.0f32; 2];
            for (s, acc) in side.iter_mut().enumerate() {
                for (t, &(k, _, sign)) in TAPS[s].iter().enumerate() {
                    *acc += sign * self.read::<XF>(3 * k + 2, taps_new[s][t], taps_old[s][t], w);
                }
            }
            (l[i], r[i]) = (side[0], side[1]);
            let inj = u[i] * (0.5 * FULL_SCALE);
            for k in 0..STAGES {
                let mut x = out[(k + STAGES - 1) % STAGES];
                if k == 0 {
                    x += inj;
                } else if k == 2 {
                    x -= inj;
                }
                for (m, &c) in AP_COEF[k].iter().enumerate() {
                    let j = 3 * k + m;
                    let vd = self.read::<XF>(j, new[j], old[j], w);
                    let vq = grid.q_trunc(x + c * vd);
                    self.write(j, vq);
                    x = vd - c * vq as f32;
                }
                self.write(3 * k + 2, grid.q_round(x));
            }
            for k in 0..STAGES {
                g[k] += dg[k];
            }
            a += da;
        }
    }
}

impl Default for Ring {
    fn default() -> Self {
        Self::new()
    }
}

/// The reverb's four controls, each 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingControls {
    pub grit: f32,
    pub time: f32,
    pub damp: f32,
    pub size: f32,
}

impl Default for RingControls {
    fn default() -> Self {
        Self {
            grit: 0.3,
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
        let f = |v: f32, d: f32| if v.is_nan() { d } else { v.clamp(0.0, 1.0) };
        Self {
            grit: f(self.grit, d.grit),
            time: f(self.time, d.time),
            damp: f(self.damp, d.damp),
            size: f(self.size, d.size),
        }
    }
}

/// One-pole smoothing time constants, in seconds.
const TIME_TAU: f32 = 0.050;
const DAMP_TAU: f32 = 0.020;
const GRIT_TAU: f32 = 0.020;

/// A once-per-block one-pole's coefficient for time constant `tau`.
fn smoothing(tau: f32, sample_rate: u32) -> f32 {
    1.0 - exp2(-(BLOCK_SIZE as f32) / (tau * sample_rate as f32) * LOG2_E)
}

/// The ring with its rate change and smoothing: what the FX bus runs.
pub struct RingReverb {
    ring: Ring,
    dec: Decimator,
    up_l: Interpolator,
    up_r: Interpolator,
    /// Smoothed TIME, DAMP and GRIT.
    time: f32,
    damp: f32,
    grit: f32,
    /// Last block's gains and DAMP coefficient: this block's ramp start.
    gains: [f32; STAGES],
    damp_coef: f32,
    primed: bool,
}

crate::in_place::field_list!(RingReverb => RingReverb { ring, dec, up_l, up_r, time, damp, grit, gains, damp_coef, primed });

impl RingReverb {
    pub fn new() -> Self {
        Self {
            ring: Ring::new(),
            dec: Decimator::new(),
            up_l: Interpolator::new(),
            up_r: Interpolator::new(),
            time: 0.0,
            damp: 0.0,
            grit: 0.0,
            gains: [0.0; STAGES],
            damp_coef: 0.0,
            primed: false,
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: every field is an integer or float array, a float, an
        // integer or a `bool` (`false`), all valid as zero bytes; zero is
        // exactly `new()`'s state.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }

    pub fn ring(&self) -> &Ring {
        &self.ring
    }

    /// Wet only: the ring's return × `WET_GAIN` × `mix`, into `out`.
    pub fn process(
        &mut self,
        send: &[f32; BLOCK_SIZE],
        ctl: &RingControls,
        mix: f32,
        sample_rate: u32,
        out: &mut Stereo,
    ) {
        let ctl = ctl.sanitised();
        let fs = sample_rate as f32 * 0.5;
        let step = size_step(ctl.size);
        if self.primed {
            let k = |tau| smoothing(tau, sample_rate);
            self.time += k(TIME_TAU) * (ctl.time - self.time);
            self.damp += k(DAMP_TAU) * (ctl.damp - self.damp);
            self.grit += k(GRIT_TAU) * (ctl.grit - self.grit);
            self.ring.request(step);
        } else {
            (self.time, self.damp, self.grit) = (ctl.time, ctl.damp, ctl.grit);
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
            grid: Grid::new(self.grit),
        };
        (self.gains, self.damp_coef) = (gains, damp);
        let mut u = [0.0f32; HALF];
        self.dec.process(send, &mut u);
        let (mut l, mut r) = ([0.0f32; HALF], [0.0f32; HALF]);
        self.ring.process(&u, &blk, &mut l, &mut r);
        self.up_l.process(&l, &mut out.l);
        self.up_r.process(&r, &mut out.r);
        let gain = WET_GAIN * mix / FULL_SCALE;
        for s in out.l.iter_mut().chain(out.r.iter_mut()) {
            *s *= gain;
        }
    }
}

impl Default for RingReverb {
    fn default() -> Self {
        Self::new()
    }
}
