//! What every reverb engine is built from (reverb TYPEs spec § Shared
//! machinery, § Level plan, § Moving a length, § Smoothing). Lines are
//! i16, full scale ±2.0 in line units; values passed in LSBs.

use core::f32::consts::{LOG2_E, TAU};
use core::mem::MaybeUninit;

use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

use crate::dsp::algo::math::exp2;
use crate::dsp::clampf;
use crate::dsp::halfband::HALF;
use crate::dsp::ring::{FULL_SCALE, RING_LEN, is_prime, rnd};
use crate::dsp::xorshift_noise;

/// R1's re-entry clip: `x` to the knee at 1.0, then `1 + u − u²/4` with
/// `u = min(|x| − 1, 2)`, reaching 2.0 with slope 0 at |x| = 3.
/// Branch-free; NaN comes out ±1.
#[inline(always)]
pub fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    let u = clampf(a - 1.0, 0.0, 2.0);
    (a.min(1.0) + u - 0.25 * u * u).copysign(x)
}

/// R1's clip (`soft_clip`) on an LSB value: under its knee, as a
/// re-entry nearly always is, `x` itself, past one compare. Off unless `C`
/// (the level plan's unclipped reference).
#[inline(always)]
pub(crate) fn clip<const C: bool>(x: f32) -> f32 {
    const INV_FS: f32 = 1.0 / FULL_SCALE;
    if C && x.abs() > FULL_SCALE {
        let a = x.abs();
        let u = clampf(a - FULL_SCALE, 0.0, 2.0 * FULL_SCALE);
        (a.min(FULL_SCALE) + u - (0.25 * INV_FS) * u * u).copysign(x)
    } else {
        x
    }
}

/// One block's one-pole toward `t`, `k` the share kept (0 lands).
pub(crate) fn ease(x: &mut f32, t: f32, k: f32) {
    *x = t + k * (*x - t);
}

/// A block's scratch, written before it is read: never zeroed.
pub(crate) struct Scratch<T: Copy, const N: usize>([MaybeUninit<T>; N]);

impl<T: Copy, const N: usize> Scratch<T, N> {
    pub(crate) fn new() -> Self {
        Self([MaybeUninit::uninit(); N])
    }

    #[inline(always)]
    pub(crate) fn set(&mut self, i: usize, v: T) {
        self.0[i].write(v);
    }

    /// # Safety
    /// `set(i, _)` ran first.
    #[inline(always)]
    pub(crate) unsafe fn get(&self, i: usize) -> T {
        // SAFETY: the caller set slot `i`.
        unsafe { self.0[i].assume_init() }
    }
}

/// R2's in-loop allpass on its normalised state: `w_d` is the stored
/// `(1 − g)·v` from M samples ago, `inv_1mg` = 1/(1 − g) once a block.
/// Returns `(y, w)`; the caller truncates `w` into the line.
#[inline(always)]
pub fn ap_norm(x: f32, w_d: f32, g: f32, inv_1mg: f32) -> (f32, f32) {
    let z = w_d * inv_1mg;
    let v = x - g * z;
    (z + g * v, (1.0 - g) * v)
}

/// The plain allpass on an f32 state `z` (R3's diffusers, R4's
/// decorrelators): `v = x − g·z`, `y = z + g·v`. Returns `(y, v)`.
#[inline(always)]
pub fn ap_f32(x: f32, z: f32, g: f32) -> (f32, f32) {
    let v = x - g * z;
    (z + g * v, v)
}

/// SMEAR's moving read: `d` (≥ 2) samples behind `base` in the arena's
/// `lines`, a linear pair `x0 + f·(x1 − x0)`, both slots wrapped. In LSBs.
/// The arena's fixed length makes the bounds below hold for any input.
#[inline(always)]
pub fn pair(lines: &[i16; RING_LEN], base: usize, d: f32) -> f32 {
    debug_assert!(d >= 2.0 && (d as usize) < RING_LEN && base < RING_LEN);
    const _: () = assert!(RING_LEN > 0);
    let n = RING_LEN;
    let base = base.min(n - 1);
    let di = (d as usize).min(n - 1);
    let f = d - di as f32;
    let wrap = |i: usize| if i >= n { i - n } else { i };
    let i0 = wrap(base + n - di);
    let i1 = wrap(i0 + n - 1);
    // SAFETY: `base` and `di` are below n, so `base + n − di` is below 2n
    // and `wrap` puts it below n; `i0 + n − 1` likewise.
    let (x0, x1) = unsafe {
        (
            *lines.get_unchecked(i0) as f32,
            *lines.get_unchecked(i1) as f32,
        )
    };
    x0 + f * (x1 - x0)
}

/// Ring samples a length crossfade takes: 30 ms, QUAD's.
pub const XFADE: u16 = 720;
pub(crate) const XFADE_INV: f32 = 1.0 / XFADE as f32;

/// A pitch-free length move: fixed reads at `from` and `to`, blended over
/// `XFADE` ring samples. Zero bytes are idle at length 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Xfade {
    pub from: u16,
    pub to: u16,
    left: u16,
}

impl Xfade {
    /// Head for `to`; a request during a crossfade waits (QUAD's rule).
    pub fn start(&mut self, to: u16) {
        if self.left == 0 && to != self.to {
            (self.to, self.left) = (to, XFADE);
        }
    }

    /// `to`'s share of the blend.
    #[inline(always)]
    pub fn weight(&self) -> f32 {
        (XFADE - self.left) as f32 * XFADE_INV
    }

    /// One ring sample on; the last lands `from` on `to`.
    #[inline(always)]
    pub fn tick(&mut self) {
        if self.left > 0 {
            self.left -= 1;
            if self.left == 0 {
                self.from = self.to;
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.left > 0
    }
}

/// SMEAR's source (§ SMEAR), one per smeared line: a new random target
/// in −1..1 every `period` ring samples, smoothed by two one-poles at the
/// rate, stepped once a block. The engine lerps `value()` to `block()`
/// across the block. Zero bytes hold still at 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wander {
    rng: u32,
    target: f32,
    lp1: f32,
    lp2: f32,
    /// Ring samples to the next target.
    left: u32,
    period: u32,
    a: f32,
}

/// `new`'s period until the first `set_rate`, which scales `left` with it.
const PHASE_UNIT: u32 = 1 << 16;

impl Wander {
    /// `seed` from `wander_seeds`; `phase` in 0..1, the share of a period
    /// before its first target (line k of n at k/n).
    pub const fn new(seed: u32, phase: f32) -> Self {
        Self {
            rng: seed,
            target: 0.0,
            lp1: 0.0,
            lp2: 0.0,
            left: (clampf(phase, 0.0, 1.0) * PHASE_UNIT as f32) as u32,
            period: PHASE_UNIT,
            a: 0.0,
        }
    }

    /// A target every 1/(2·`hz`) s and the smoothing at `hz`, at the ring
    /// rate `fs`. The wait in progress keeps its share of the period.
    pub fn set_rate(&mut self, hz: f32, fs: f32) {
        self.set(Self::rate(hz, fs));
    }

    /// `set_rate`'s (period, smoothing) for `hz` at `fs`: worked out once
    /// for every line that shares a rate.
    pub fn rate(hz: f32, fs: f32) -> (u32, f32) {
        (
            ((fs / (2.0 * hz)) as u32).max(HALF as u32),
            one_pole(hz, fs / HALF as f32),
        )
    }

    /// `rate`'s result on this line.
    pub fn set(&mut self, (period, a): (u32, f32)) {
        if period != self.period && self.period > 0 {
            self.left = (self.left as u64 * period as u64 / self.period as u64) as u32;
        }
        (self.period, self.a) = (period, a);
    }

    /// The drift at the last block's end.
    #[inline(always)]
    pub fn value(&self) -> f32 {
        self.lp2
    }

    /// One block on: the drift at its end, in −1..1. Not inlined: every
    /// line's copy would cost flash for nothing.
    #[inline(never)]
    pub fn block(&mut self) -> f32 {
        self.left = self.left.saturating_sub(HALF as u32);
        if self.left == 0 {
            self.target = xorshift_noise(&mut self.rng);
            self.left = self.period;
        }
        self.lp1 += self.a * (self.target - self.lp1);
        self.lp2 += self.a * (self.lp1 - self.lp2);
        self.lp2
    }
}

/// One xorshift32 seed per line from `engine_seed`, each a hash of the
/// seed and its line (murmur3's finaliser): far apart on the generator's
/// cycle, so no line replays another's targets late. Never 0.
pub const fn wander_seeds<const N: usize>(engine_seed: u32) -> [u32; N] {
    let mut out = [0; N];
    let mut k = 0;
    while k < N {
        let mut h = engine_seed ^ (k as u32 + 1).wrapping_mul(0x9e37_79b9);
        h ^= h >> 16;
        h = h.wrapping_mul(0x85eb_ca6b);
        h ^= h >> 13;
        h = h.wrapping_mul(0xc2b2_ae35);
        h ^= h >> 16;
        out[k] = if h == 0 { 1 } else { h };
        k += 1;
    }
    out
}

/// The per-block one-pole every continuous knob eases by, as
/// `dsp::ease::Ease::step`'s `k`: the share kept each 64-sample block at
/// 48 kHz, for a time constant of `tau_s`.
pub const fn ease_k(tau_s: f32) -> f32 {
    exp2(-LOG2_E * BLOCK_SIZE as f32 / (tau_s * SAMPLE_RATE as f32))
}

/// `y += a·(x − y)`'s `a` for a corner at `fc` and rate `fs`.
pub fn one_pole(fc: f32, fs: f32) -> f32 {
    1.0 - pow2(-LOG2_E * TAU * fc / fs)
}

/// `exp2`, out of line: one copy for the engines' per-block maths.
#[inline(never)]
pub fn pow2(x: f32) -> f32 {
    exp2(x)
}

/// `log2`, out of line: one copy for the knobs' laws and the TAIL.
#[inline(never)]
pub fn lg2(x: f32) -> f32 {
    crate::dsp::algo::math::log2(x)
}

/// `ms` at 24 kHz, to the nearest prime (the lower on a tie): the
/// engines' lengths, by `ring.rs`'s rule.
pub(crate) const fn prime_ms(ms: f32) -> usize {
    let want = (ms * 24.0 + 0.5) as u32;
    let mut d = 0;
    loop {
        if is_prime(want - d) {
            return (want - d) as usize;
        }
        if is_prime(want + d) {
            return (want + d) as usize;
        }
        d += 1;
    }
}

/// Each line's write offset from the shared head, and the slots taken:
/// line j owns `len[j] + 2` (a pair's second read, and the guard), so a
/// read d back, 2 ≤ d ≤ `len[j]` + 1, is what line j wrote d samples
/// ago. The total is even, so the f32 region may follow.
pub(crate) const fn heads<const N: usize>(len: [usize; N]) -> ([usize; N], usize) {
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

#[inline(always)]
pub(crate) fn wrap(i: usize, m: usize) -> usize {
    if i >= m { i - m } else { i }
}

/// Each access's slot at head `h` (mod `m`) into `base`, and how many
/// samples from `len` run before any of them wraps. Out of line: one copy
/// serves both regions.
#[inline(never)]
pub(crate) fn segment<const N: usize>(
    h: usize,
    m: usize,
    off: &[usize; N],
    len: usize,
) -> ([usize; N], usize) {
    let mut len = len;
    let base = core::array::from_fn(|j| {
        let b = wrap(h + off[j], m);
        len = len.min(m - b);
        b
    });
    (base, len)
}

/// A line store: the arena's i16 slots (quantised writes), or plain f32s,
/// which store as written: the LOOP engine's stage allpasses, and the
/// level plan's unquantised reference (tests).
pub(crate) trait Lines {
    /// Reads summed exactly: i32 on the i16 lines (16 of them stay under
    /// 2²⁴, so the f32 they convert to is the f32 sum), f32 on the f32
    /// ones.
    type Sum: Copy
        + Default
        + core::ops::Add<Output = Self::Sum>
        + core::ops::Sub<Output = Self::Sum>;
    fn len(&self) -> usize;
    /// # Safety
    /// `i` < `len()`.
    unsafe fn rd(&self, i: usize) -> f32;
    /// `rd`, unconverted, to sum.
    /// # Safety
    /// `i` < `len()`.
    unsafe fn rd_sum(&self, i: usize) -> Self::Sum;
    fn sum_f32(s: Self::Sum) -> f32;
    /// A truncating write (allpass states, a feedback line).
    /// # Safety
    /// `i` < `len()`.
    unsafe fn trunc(&mut self, i: usize, x: f32);
    /// A rounding write (delays).
    /// # Safety
    /// `i` < `len()`.
    unsafe fn round(&mut self, i: usize, x: f32);
    /// A re-entry write: rounding, but under 1 LSB to 0, so no ±1 LSB
    /// cycle can outlive the loop (`store_settle`).
    /// # Safety
    /// `i` < `len()`.
    unsafe fn settle(&mut self, i: usize, x: f32);
    /// A feed-forward allpass state: rounding, but truncated under 3 LSB
    /// (`store_settle_ap`).
    /// # Safety
    /// `i` < `len()`.
    unsafe fn settle_ap(&mut self, i: usize, x: f32);
}

impl Lines for [i16] {
    type Sum = i32;

    fn len(&self) -> usize {
        <[i16]>::len(self)
    }

    #[inline(always)]
    unsafe fn rd_sum(&self, i: usize) -> i32 {
        // SAFETY: the caller keeps `i` < `len()`.
        (unsafe { *self.get_unchecked(i) }) as i32
    }

    #[inline(always)]
    fn sum_f32(s: i32) -> f32 {
        s as f32
    }

    #[inline(always)]
    unsafe fn rd(&self, i: usize) -> f32 {
        // SAFETY: the caller keeps `i` < `len()`.
        (unsafe { *self.get_unchecked(i) }) as f32
    }

    #[inline(always)]
    unsafe fn trunc(&mut self, i: usize, x: f32) {
        // SAFETY: the caller keeps `i` < `len()`.
        unsafe { *self.get_unchecked_mut(i) = store_trunc(x) }
    }

    #[inline(always)]
    unsafe fn round(&mut self, i: usize, x: f32) {
        // SAFETY: the caller keeps `i` < `len()`.
        unsafe { *self.get_unchecked_mut(i) = store_round(x) }
    }

    #[inline(always)]
    unsafe fn settle(&mut self, i: usize, x: f32) {
        // SAFETY: the caller keeps `i` < `len()`.
        unsafe { *self.get_unchecked_mut(i) = store_settle(x) }
    }

    #[inline(always)]
    unsafe fn settle_ap(&mut self, i: usize, x: f32) {
        // SAFETY: the caller keeps `i` < `len()`.
        unsafe { *self.get_unchecked_mut(i) = store_settle_ap(x) }
    }
}

/// Unquantised lines: the LOOP engine's f32 stage allpasses, and (tests)
/// the level plan's reference.
impl Lines for [f32] {
    type Sum = f32;

    fn len(&self) -> usize {
        <[f32]>::len(self)
    }

    #[inline(always)]
    unsafe fn rd_sum(&self, i: usize) -> f32 {
        // SAFETY: the caller keeps `i` < `len()`.
        unsafe { *self.get_unchecked(i) }
    }

    #[inline(always)]
    fn sum_f32(s: f32) -> f32 {
        s
    }

    #[inline(always)]
    unsafe fn rd(&self, i: usize) -> f32 {
        // SAFETY: the caller keeps `i` < `len()`.
        unsafe { *self.get_unchecked(i) }
    }

    unsafe fn trunc(&mut self, i: usize, x: f32) {
        self[i] = x;
    }

    unsafe fn round(&mut self, i: usize, x: f32) {
        self[i] = x;
    }

    unsafe fn settle(&mut self, i: usize, x: f32) {
        self[i] = x;
    }

    unsafe fn settle_ap(&mut self, i: usize, x: f32) {
        self[i] = x;
    }
}

/// SMEAR's moving read on `lines`' first `n` slots: `pair`'s form, `d`
/// behind `base`.
///
/// # Safety
/// `n` ≤ `lines.len()`, `base` < `n`, and 2 ≤ `d` < `n`: no clamp here,
/// the caller's layout bounds them (`LoopEngine`'s const asserts).
#[inline(always)]
pub(crate) unsafe fn pair_in<L: Lines + ?Sized>(lines: &L, n: usize, base: usize, d: f32) -> f32 {
    debug_assert!(n <= lines.len() && base < n && (2.0..n as f32).contains(&d));
    let di = d as usize;
    let f = d - di as f32;
    let i0 = wrap(base + n - di, n);
    let i1 = wrap(i0 + n - 1, n);
    // SAFETY: `base` and `di` are below n ≤ `len()` (the caller's
    // contract), so `wrap` puts both indices below n.
    let (x0, x1) = unsafe { (lines.rd(i0), lines.rd(i1)) };
    x0 + f * (x1 - x0)
}

/// A delay or damper write: `x` (LSBs) rounded, ties away from zero, and
/// saturated to i16.
#[inline(always)]
pub fn store_round(x: f32) -> i16 {
    saturate(rnd(x))
}

/// A loop's re-entry write: `store_round`, but 0 under 1 LSB. At the
/// LSB a loop's gains hand a rounded ±1 back to itself (0.6 rounds up),
/// and its normalised allpasses truncate only their scaled state: ROOM
/// held a 1080 Hz cycle at −86 dB for good. Truncating the whole write
/// instead kills it too, but its error is then a square wave on a held
/// tone's sign (+1.4 dB on the floor).
#[inline(always)]
pub fn store_settle(x: f32) -> i16 {
    // `x as i32` truncates: 0 exactly when |x| < 1 (and for NaN), an
    // integer compare where `abs() < 1.0` read the FPU's flags.
    let r = store_round(x);
    if x as i32 == 0 { 0 } else { r }
}

/// A feed-forward allpass's normalised state (HALL's diffuser): rounded,
/// as a delay is, but truncated under 3 LSB. A rounding allpass at g 0.8
/// holds w = ±2 for good (0.8·2 rounds back to 2); its deadband is 0.5/(1 −
/// g) = 2.5 LSB, so truncating there kills it, and the signal keeps
/// rounding's error, a quarter of truncation's power.
#[inline(always)]
pub fn store_settle_ap(x: f32) -> i16 {
    let i = x as i32;
    if i.unsigned_abs() < 3 {
        saturate(i)
    } else {
        store_round(x)
    }
}

/// An allpass state or REGEN write: `x` (LSBs) truncated toward zero, and
/// saturated to i16. Truncation always decays.
#[inline(always)]
pub fn store_trunc(x: f32) -> i16 {
    saturate(x as i32)
}

#[inline(always)]
fn saturate(r: i32) -> i16 {
    let s = r.clamp(i16::MIN as i32, i16::MAX as i32);
    #[cfg(any(test, feature = "clamp-probe"))]
    probe::note(s != r);
    s as i16
}

/// Clamp hits and writes through `store_*` on this thread since the last
/// reset (§ Level plan's transients). Its own feature, which only this
/// crate's tests turn on: the simulator's audio path never pays for it.
#[cfg(any(test, feature = "clamp-probe"))]
pub fn clamp_counts() -> (u64, u64) {
    probe::COUNTS.with(|c| c.get())
}

#[cfg(any(test, feature = "clamp-probe"))]
pub fn reset_clamp_counts() {
    probe::COUNTS.with(|c| c.set((0, 0)));
}

#[cfg(any(test, feature = "clamp-probe"))]
mod probe {
    extern crate std;
    use core::cell::Cell;

    std::thread_local! {
        /// (hits, writes): per thread, so parallel tests don't mix.
        pub static COUNTS: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
    }

    pub fn note(hit: bool) {
        COUNTS.with(|c| {
            let (h, w) = c.get();
            c.set((h + hit as u64, w + 1));
        });
    }
}
