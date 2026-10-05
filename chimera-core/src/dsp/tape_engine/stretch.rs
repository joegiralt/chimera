//! STRETCH (spec § 2 STRETCH, Why period-locking): the Akai S950's
//! time-stretch (S950 Operator's Manual p. 32: TIMESTRETCH, D-TIME, MONO
//! and POLY) run in real time, as the JUCE forum's Akai-style two-grain
//! stretch (2024) does. WSOLA (Verhelst and Roelands, 1993) is the costed
//! follow-up TYPE (ADR 0093).
//!
//! A time pointer `T` runs through the window at MOTION's speed (−1 to +2
//! times the source's own), from POSITION. Two grains read at the note's
//! `inc`, so the pitch is the note's whatever the speed. Every D-TIME/2
//! output samples (SIZE: 10–250 ms) a splice starts a grain at `T` under
//! one linear fade, D-TIME/2 long, latched as it starts; the next splice
//! waits for it to end, so the old grain's gain is always exactly 1 − the
//! new one's. MONO starts the new grain a whole number of the source's YIN
//! periods from the old grain's head (`lock`), so a held tone's splices
//! line up; a period that doesn't fit twice in the window and the D-TIME
//! (`locks`), or none, plays POLY.
//!
//! A grain starts at `T` wherever it is, so time always moves on; one that
//! reaches the window's end wraps: MONO's back by the fewest whole periods
//! that are a wrap's travel (`WRAP_SAMPLES` at the block's fastest `inc`),
//! re-reading its last cycles, the nearest in kind; POLY's by the
//! window. That jump's value, at the grain's share, rides on the
//! output as a lift fading over `LIFT_S`, as PING-PONG's retargeted clamp
//! carries its ghost's share: the output never steps there. A lift still
//! running carries on under the next.
//!
//! Events: a splice comes once in D-TIME/2 (at least `MIN_PASS_SAMPLES`).
//! The window is at least `2 · WRAP_SAMPLES · inc` frames where the source
//! has them, and MONO's wrap is at least `WRAP_SAMPLES` of travel, so a
//! grain wraps at most once in two blocks at the speed it started at, a
//! call each (a pitch risen since shortens that for the rest of its life;
//! the bill holds the floor's case). A window shorter than `WRAP_SAMPLES ·
//! inc` (a source shorter than the floor), or a wrap shorter than `INC_MAX`,
//! is a waveform loop: its grains wrap inline in the loop, a comparison
//! and at most one subtraction a sample, with no call and no lift. A wrap
//! shorter than `INC_MAX` frames (a window of a few frames) is strided to
//! a whole multiple at least that long, so one subtraction still does:
//! such a grain reads up to `INC_MAX` frames past its window, clamped in
//! the row.

use chimera_hal::BLOCK_SIZE;

use super::heads::{at, shortest, window_end_min, window_start};
use super::params::{StretchType, TapeEngineParams};
use super::read::{Pos, SourceView};
use super::{INC_MAX, MIN_PASS_SAMPLES, Macros};
use crate::dsp::algo::math::{clampf, exp2};

/// A wrap's lift fades over 5 ms, as PING-PONG's clamp does.
pub const LIFT_S: f32 = 0.005;
/// A grain wraps at most once in this many samples (two blocks): the
/// window is at least twice this many samples of travel at the block's
/// fastest `inc` (POLY wraps by it), and MONO's wrap is the fewest whole
/// periods at least this long. The two grains' wraps then cost one call a block, on average
/// (spec § 5): at `inc` 16 the window is at least 4 096 frames, at the
/// source's own speed 256 (5.3 ms at 48 kHz).
pub const WRAP_SAMPLES: u32 = 2 * BLOCK_SIZE as u32;
/// `log2(25)`: D-TIME runs 10 ms to 250 ms.
const LOG2_25: f32 = 4.643_856;
/// 2³²: a 32.32 position's whole frame.
const ONE: f32 = 4_294_967_296.0;

/// D-TIME in seconds for SIZE `size`: `0.01 · 25^size`, 10–250 ms.
pub fn d_time_s(size: f32) -> f32 {
    0.01 * exp2(LOG2_25 * clampf(size, 0.0, 1.0))
}

/// Time's speed for MOTION `motion`: `2 · motion` from 0 up, `motion`
/// below, −1 to +2 times the source's own.
pub fn time_speed(motion: f32) -> f32 {
    let m = clampf(motion, -1.0, 1.0);
    if m >= 0.0 { 2.0 * m } else { m }
}

/// MONO's splice: as near `t` as can be, a whole number of `period`s from
/// the old grain's head `h_old`: `h_old − P · round((h_old − t) / P)`. The
/// engine runs it in 32.32 with the period's reciprocal (`lock_at`).
pub fn lock(h_old: f32, t: f32, period: f32) -> f32 {
    h_old - period * libm::roundf((h_old - t) / period)
}

/// Whether a splice locks: MONO, pitched, and two periods inside both the
/// window and the D-TIME (both in source frames). Otherwise it plays POLY.
pub fn locks(kind: StretchType, period: f32, window: u32, d_time_frames: u32) -> bool {
    kind == StretchType::Mono && period > 0.0 && 2.0 * period <= window.min(d_time_frames) as f32
}

/// The grains' clock: the time pointer, and the fade in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrainClock {
    /// `T`, source frames: where the next grain starts (before MONO's lock),
    /// as of the last block's end. It wraps in the window.
    pub t: Pos,
    /// The fade's length in samples, latched as it starts.
    pub len: u32,
    /// Its samples played.
    pub pos: u32,
}

impl GrainClock {
    /// Samples to the next splice: it comes as the fade ends.
    pub fn left(&self) -> u32 {
        self.len - self.pos
    }

    /// The new grain's share.
    pub fn gain(&self) -> f32 {
        self.pos as f32 / self.len as f32
    }
}

/// What a block's splices take, worked out once a block.
#[derive(Clone, Copy)]
struct Block {
    /// The window's start and its end for grains, frames: its last two
    /// frames are left out, so no Hermite tap reads past it.
    ws: u32,
    hi: u32,
    /// Its length, 32.32.
    wq: u64,
    /// The window is a waveform loop: grains wrap inline.
    tiny: bool,
    /// A new grain's end and wrap: `hi` and `span`, but a span under
    /// `INC_MAX` is strided to the first whole multiple at or over it, so
    /// one wrap a sample does (the end moves on by as much: past the
    /// window by under `INC_MAX` frames, read clamped in the row).
    edge: u32,
    stride: u64,
    locked: bool,
    /// The fade's length and its reciprocal.
    half: u32,
    inv_half: f32,
    /// POSITION in the window, 32.32, and time's step a sample.
    off: u64,
    tstep: i64,
}

/// `inc` (in `(0, INC_MAX]`) in 32.32, from 32-bit conversions.
fn q(x: f32) -> u64 {
    let whole = x as u32;
    ((whole as u64) << 32) | ((x - whole as f32) * ONE) as u32 as u64
}

/// `x` (32.32, signed) wrapped into `[0, wq)`: a step or two, or a
/// remainder for a window shorter than a block of travel.
fn wrap_q(x: i64, wq: u64) -> u64 {
    let w = wq as i64;
    if (0..w).contains(&x) {
        x as u64
    } else if (-w..0).contains(&x) {
        (x + w) as u64
    } else if (w..w.saturating_mul(2)).contains(&x) {
        (x - w) as u64
    } else {
        x.rem_euclid(w.max(1)) as u64
    }
}

/// A STRETCH note's state.
#[derive(Clone, Copy, Debug)]
pub struct Stretch {
    clock: GrainClock,
    /// The old grain's head and the new one's, 32.32 source frames.
    heads: [Pos; 2],
    /// Each grain's window end (whole frames) and wrap, latched as it
    /// starts.
    hi: [u32; 2],
    span: [u64; 2],
    tiny: [bool; 2],
    /// The new grain's share at the last sample, and its step a sample.
    g: f32,
    dg: f32,
    /// Time since the note-on in the window, 32.32: `T` is `ws` + POSITION's
    /// offset + this, wrapped.
    tau: u64,
    ws: u32,
    kind: StretchType,
    /// The source's period, 32.32 and frames (0 unpitched), and its
    /// reciprocal.
    period: u64,
    pf: f32,
    inv_p: f32,
    /// The first splice is to come: the fade follows each block's D-TIME.
    runup: bool,
    /// Each grain's last sample read.
    last: [f32; 2],
    /// What wraps moved, riding on the output; its fall a sample and the
    /// samples left; `LIFT_S` in samples, and its reciprocal.
    lift: f32,
    lift_dl: f32,
    lift_left: u32,
    lift_len: u32,
    inv_lift: f32,
    /// Splices and wraps: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) events: u32,
}

impl Stretch {
    pub const IDLE: Stretch = Stretch {
        clock: GrainClock {
            t: Pos(0),
            len: 1,
            pos: 0,
        },
        heads: [Pos(0); 2],
        hi: [1; 2],
        span: [1 << 32; 2],
        tiny: [true; 2],
        g: 0.0,
        dg: 0.0,
        tau: 0,
        ws: 0,
        kind: StretchType::Mono,
        period: 0,
        pf: 0.0,
        inv_p: 0.0,
        runup: false,
        last: [0.0; 2],
        lift: 0.0,
        lift_dl: 0.0,
        lift_left: 0,
        lift_len: 1,
        inv_lift: 1.0,
        #[cfg(any(test, feature = "test-support"))]
        events: 0,
    };

    /// A note on `v` at `inc` frames a sample, `m` its macros: both grains
    /// at `T`, START + POSITION, the first fade's length following each
    /// block until it ends. TYPE and PERIOD latch here.
    pub fn on(p: &TapeEngineParams, v: &SourceView<'_>, inc: f32, m: &Macros, sr: u32) -> Stretch {
        let pf = if p.period.is_finite() && p.period >= 1.0 && p.period < 16_777_216.0 {
            p.period
        } else {
            0.0
        };
        let lift_len = ((LIFT_S * sr as f32) as u32).max(1);
        let mut s = Stretch {
            ws: window_start(p.start, v.frames()),
            kind: p.kind,
            period: Pos::from_f32(pf).0,
            pf,
            inv_p: if pf > 0.0 { 1.0 / pf } else { 0.0 },
            runup: true,
            lift_len,
            inv_lift: 1.0 / lift_len as f32,
            #[cfg(any(test, feature = "test-support"))]
            events: 0,
            ..Stretch::IDLE
        };
        let k = s.block(v, (inc, inc), (m.position, m.motion, m.size), p.end, sr);
        let t = s.time(k.off as i64, k);
        s.heads = [t; 2];
        s.clock = GrainClock {
            t,
            len: k.half,
            pos: 0,
        };
        s.dg = k.inv_half;
        s
    }

    /// The clock: for the tests.
    pub fn clock(&self) -> GrainClock {
        self.clock
    }

    /// The old grain's frame, the new one's, and the new one's share.
    pub fn probe(&self) -> (u32, u32, f32) {
        (self.heads[0].whole(), self.heads[1].whole(), self.g)
    }

    /// This block's window, D-TIME, lock and time step, from its `inc` ramp
    /// and eased macros: one division and one `exp2`.
    fn block(
        &self,
        v: &SourceView<'_>,
        (from, to): (f32, f32),
        (position, motion, size): (f32, f32, f32),
        end: f32,
        sr: u32,
    ) -> Block {
        let (frames, rate) = (v.frames(), v.desc().rate);
        let ws = self.ws.min(frames - 1);
        let floor = shortest(
            rate,
            (2 * WRAP_SAMPLES / MIN_PASS_SAMPLES) as f32 * from.max(to),
        );
        let we = window_end_min(ws, end, frames, floor);
        let hi = we.saturating_sub(2).max(ws + 1);
        let w = hi - ws;
        let wq = (w as u64) << 32;
        let d = d_time_s(size);
        let half = ((d * sr as f32 * 0.5 + 0.5) as u32).max(MIN_PASS_SAMPLES);
        let locked = locks(self.kind, self.pf, w, (d * rate as f32 + 0.5) as u32);
        let span = if locked {
            // The fewest whole periods that are a wrap's travel at the
            // block's fastest `inc`, so a grain re-reads its last cycles,
            // the nearest in kind; at most the window's.
            let most = (w as f32 * self.inv_p) as u32;
            let fewest = (WRAP_SAMPLES as f32 * from.max(to) * self.inv_p) as u32 + 1;
            let n = fewest.min(most).max(1);
            let s = (n as u64).wrapping_mul(self.period);
            if s > wq { s - self.period } else { s }
        } else {
            wq
        };
        // POSITION's offset in the window and time's step a sample, 32.32,
        // built from 32-bit conversions.
        let most = (INC_MAX as u64) << 32;
        let tiny = (w as f32) < WRAP_SAMPLES as f32 * from.max(to) || span < most;
        let (edge, stride) = if span < most {
            // At most 16 steps: the span is at least a frame.
            let mut st = span.max(1 << 32);
            while st < most {
                st += span.max(1 << 32);
            }
            ((at(hi) + Pos(st - span)).whole(), st)
        } else {
            (hi, span)
        };
        let x = clampf(position, 0.0, 1.0) * w as f32;
        let off = q(x).min(wq - 1);
        let speed = time_speed(motion) * rate as f32 / sr as f32;
        Block {
            ws,
            hi,
            wq,
            tiny,
            edge,
            stride,
            locked,
            half,
            inv_half: 1.0 / half as f32,
            off,
            tstep: ((speed * (1u32 << 30) as f32) as i32 as i64) << 2,
        }
    }

    /// `T` for `x`, POSITION's offset plus time (32.32, signed).
    fn time(&self, x: i64, k: Block) -> Pos {
        at(k.ws) + Pos(wrap_q(x, k.wq))
    }

    /// A block: the grains read at `inc` ramped `(from, to)`, POSITION,
    /// MOTION and SIZE eased; END is read here, and latches into each grain
    /// as it starts.
    pub fn render(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        (from, to): (f32, f32),
        macros: (f32, f32, f32),
        end: f32,
        sr: u32,
    ) {
        let k = self.block(v, (from, to), macros, end, sr);
        if self.runup {
            // Nothing is in play yet: the first grain takes the block's
            // window, and its fade the block's D-TIME.
            (self.hi, self.span, self.tiny) = ([k.edge; 2], [k.stride; 2], [k.tiny; 2]);
            self.clock.len = k.half.max(self.clock.pos + 1);
            // Re-aimed, so the share lands on 1 as the fade ends.
            self.dg = (1.0 - self.g) / self.clock.left() as f32;
            if self
                .heads
                .iter()
                .zip(self.hi)
                .any(|(h, hi)| h.whole() >= hi)
            {
                self.last = self.heads.map(|h| v.read(h));
                self.wrap(v, 0);
            }
        }
        let (from, to) = (q(from), q(to));
        let step = (to.wrapping_sub(from) as i64 / BLOCK_SIZE as i64) as u64;
        let tau = self.tau as i64;
        if self.tiny[0] || self.tiny[1] {
            self.play::<true>(out, v, &k, (from, step), tau);
        } else {
            self.play::<false>(out, v, &k, (from, step), tau);
        }
        self.tau = wrap_q(tau + BLOCK_SIZE as i64 * k.tstep, k.wq);
        self.clock.t = self.time((k.off + self.tau) as i64, k);
    }

    /// The block's samples: two grains, the fade's gain rising `dg` a
    /// sample, `inc` ramped by `step`, and the lift riding on them (0 with
    /// none), each run to the next splice, a grain's wrap, the lift's end or
    /// the block's end. `TINY`: a waveform loop, its grains wrapped inline.
    #[inline(never)]
    fn play<const TINY: bool>(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        k: &Block,
        (mut cur, step): (u64, u64),
        tau: i64,
    ) {
        let mut i = 0;
        while i < BLOCK_SIZE {
            let [mut h0, mut h1] = self.heads;
            let [e0, e1] = self.hi;
            let [s0, s1] = self.span.map(Pos);
            let (mut g, dg) = (self.g, self.dg);
            let (mut l, dl) = (self.lift, self.lift_dl);
            let (mut a, mut b) = (0.0, 0.0);
            let s = i;
            let mut stop = BLOCK_SIZE.min(i + self.clock.left() as usize);
            if self.lift_left > 0 {
                stop = stop.min(i + self.lift_left as usize);
            }
            for y in out.iter_mut().take(stop).skip(s) {
                a = v.read(h0);
                b = v.read(h1);
                g += dg;
                *y = a + g * (b - a) + l;
                l -= dl;
                cur = cur.wrapping_add(step);
                h0 = h0 + Pos(cur);
                h1 = h1 + Pos(cur);
                i += 1;
                if TINY {
                    // A stride is at least a step: one wrap does.
                    if h0.whole() >= e0 {
                        h0 = h0 - s0;
                    }
                    if h1.whole() >= e1 {
                        h1 = h1 - s1;
                    }
                } else if h0.whole() >= e0 || h1.whole() >= e1 {
                    break;
                }
            }
            self.heads = [h0, h1];
            self.g = g;
            self.clock.pos += (i - s) as u32;
            if self.lift_left > 0 {
                self.lift_left -= (i - s) as u32;
                (self.lift, self.lift_dl) = if self.lift_left == 0 {
                    (0.0, 0.0)
                } else {
                    (l, dl)
                };
            }
            if !TINY && (h0.whole() >= e0 || h1.whole() >= e1) {
                self.last = [a, b];
                self.wrap(v, cur);
            }
            if self.clock.left() == 0 {
                let t = self.time(k.off as i64 + tau + i as i64 * k.tstep, *k);
                self.splice(t, *k);
            }
        }
    }

    /// A grain at its window's end wraps back by its span: at least
    /// `INC_MAX`, so one wrap does it (`tiny` takes any shorter). It last
    /// read `last` one step (`cur`) back: what that sample read, against the same place wrapped, at the
    /// share the grain plays next, rides on the output from here, and a lift
    /// still running with it, falling to 0 over `LIFT_S`.
    #[cold]
    #[inline(never)]
    fn wrap(&mut self, v: &SourceView<'_>, cur: u64) {
        let g = self.g + self.dg;
        for x in 0..2 {
            let (u, hi, span) = (self.heads[x], self.hi[x], self.span[x]);
            if u.whole() < hi {
                continue;
            }
            let mut w = u - Pos(span);
            if w.whole() >= hi {
                w = at(hi) - Pos(span);
            }
            let share = if x == 1 { g } else { 1.0 - g };
            self.lift += share * (self.last[x] - v.read(w - Pos(cur)));
            self.heads[x] = w;
            #[cfg(any(test, feature = "test-support"))]
            {
                self.events += 1;
            }
        }
        self.lift_left = self.lift_len;
        self.lift_dl = self.lift * self.inv_lift;
    }

    /// The fade has ended: the new grain plays on as the old, and a grain
    /// starts at `t` (MONO: locked to the old one's phase) under a fade of
    /// the block's D-TIME/2.
    #[cold]
    #[inline(never)]
    fn splice(&mut self, t: Pos, k: Block) {
        self.heads[0] = self.heads[1];
        self.hi[0] = self.hi[1];
        self.span[0] = self.span[1];
        self.tiny[0] = self.tiny[1];
        self.heads[1] = if k.locked {
            self.lock_at(self.heads[0], t, k)
        } else {
            t
        };
        self.hi[1] = k.edge;
        self.span[1] = k.stride;
        self.tiny[1] = k.tiny;
        self.clock.len = k.half;
        self.clock.pos = 0;
        self.g = 0.0;
        self.dg = k.inv_half;
        self.runup = false;
        #[cfg(any(test, feature = "test-support"))]
        {
            self.events += 1;
        }
    }

    /// `lock` in 32.32 with the period's reciprocal, no division, and the
    /// grain moved a period into the window if the lock took it out (two
    /// periods fit, so one move does it).
    fn lock_at(&self, h: Pos, t: Pos, k: Block) -> Pos {
        let d = h.0.wrapping_sub(t.0);
        let x = ((d >> 32) as i32 as f32 + (d as u32) as f32 * (1.0 / ONE)) * self.inv_p;
        let n = (x + 0.5f32.copysign(x)) as i32 as i64;
        let mut s = h.0.wrapping_sub(n.wrapping_mul(self.period as i64) as u64);
        let (lo, hi) = (at(k.ws).0, at(k.hi).0);
        if (s as i64) < lo as i64 {
            s = s.wrapping_add(self.period);
        } else if s >= hi {
            s = s.wrapping_sub(self.period);
        }
        if (lo..hi).contains(&s) { Pos(s) } else { t }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_q_wraps_both_ways() {
        let w = 10u64 << 32;
        assert_eq!(wrap_q(3 << 32, w), 3 << 32);
        assert_eq!(wrap_q(-(3 << 32), w), 7 << 32);
        assert_eq!(wrap_q(13 << 32, w), 3 << 32);
        assert_eq!(wrap_q(43 << 32, w), 3 << 32);
        assert_eq!(wrap_q(-(43 << 32), w), 7 << 32);
    }

    /// The engine's fixed-point lock agrees with `lock`.
    #[test]
    fn lock_at_is_lock() {
        let mut s = Stretch::IDLE;
        let p = 109.3f32;
        (s.period, s.pf, s.inv_p) = (Pos::from_f32(p).0, p, 1.0 / p);
        let k = Block {
            ws: 0,
            hi: 100_000,
            wq: 100_000 << 32,
            tiny: false,
            edge: 100_000,
            stride: 0,
            locked: true,
            half: 240,
            inv_half: 1.0 / 240.0,
            off: 0,
            tstep: 0,
        };
        for (h, t) in [
            (50_000.25, 1_234.5),
            (1_000.0, 90_000.75),
            (777.7, 777.0),
            (1_000.0, 940.0),
            (940.0, 1_000.0),
        ] {
            let got = s.lock_at(Pos::from_f32(h), Pos::from_f32(t), k);
            let want = lock(h, t, p);
            let got = got.whole() as f32 + got.frac();
            assert!((got - want).abs() < 0.02, "{h}, {t}: {got} against {want}");
        }
    }

    /// `held` of `frames` (≤ 224) frames, sample `i` at `i · 16` (12 bits)
    /// after a `Plain` descriptor at 48 kHz, and its length.
    fn ramp(frames: usize) -> ([u8; 16 + 336], usize) {
        use super::super::read::{DESC_LEN, Descriptor, Shape};
        let d = Descriptor {
            shape: Shape::Plain,
            count: 1,
            rate: 48_000,
            head_frames: 0,
        };
        let mut x = [0i16; 224];
        for (i, y) in x.iter_mut().enumerate() {
            *y = (i * 16 % 2_048) as i16;
        }
        let mut held = [0u8; 16 + 336];
        held[..DESC_LEN].copy_from_slice(&d.to_bytes());
        crate::storage::twelve::pack12(&x[..frames], &mut held[DESC_LEN..]);
        (held, DESC_LEN + 3 * frames / 2)
    }

    /// A wrap carries the grain's value at the share it plays next: the
    /// new grain's `g + dg`, not the last sample's `g`.
    #[test]
    fn wrap_carries_the_next_share() {
        let (held, n) = ramp(64);
        let v = SourceView::new(&held[..n]).unwrap();
        let mut s = Stretch::IDLE;
        (s.heads, s.hi, s.span) = ([at(1), at(40)], [40; 2], [32 << 32; 2]);
        (s.g, s.dg, s.last) = (0.25, 0.25, [0.0, 0.5]);
        s.wrap(&v, 0);
        assert_eq!(s.heads[1], at(8));
        let want = 0.5 * (0.5 - v.read(at(8)));
        assert!((s.lift - want).abs() < 1e-6, "{} against {want}", s.lift);
    }

    /// MONO's wrap is whole periods inside the window, even where the
    /// window is a whole number of them and `w / P` rounds up in `f32`
    /// (P = 2.15, w = 86: a source of 88 frames at `inc` 16).
    #[test]
    fn mono_span_fits_the_window() {
        let (held, n) = ramp(88);
        let v = SourceView::new(&held[..n]).unwrap();
        let p = 2.15f32;
        let mut s = Stretch::IDLE;
        (s.period, s.pf, s.inv_p) = (Pos::from_f32(p).0, p, 1.0 / p);
        let k = s.block(&v, (16.0, 16.0), (0.0, 0.0, 0.0), 1.0, 48_000);
        assert_eq!(k.hi - k.ws, 86);
        assert!(k.locked);
        assert!(k.stride <= k.wq, "{} > {}", k.stride, k.wq);
        assert!(k.stride + s.period > k.wq);
    }
}
