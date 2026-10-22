//! STRETCH's CLASSIC TYPE (ADR 0204; spec § 2 STRETCH, Why period-locking): the Akai S950's
//! time-stretch (S950 Operator's Manual p. 32: TIMESTRETCH, D-TIME, MONO
//! and POLY) run in real time, as the JUCE forum's Akai-style two-grain
//! stretch (2024) does. WSOLA (Verhelst and Roelands, 1993) is the costed
//! follow-up TYPE (ADR 0093).
//!
//! A time pointer `T` runs through the window at MOTION's speed (−1 to +2
//! times the source's own), from POSITION. Two grains read at the note's
//! `inc`, so the pitch is the note's whatever the speed. Every D-TIME/2
//! output samples (SIZE: 10–250 ms) a splice starts a grain at `T` under
//! one fade (a smoothstep, so splices have no corner), D-TIME/2 long, latched as it starts; the next splice
//! waits for it to end. Locked grains' gains sum to 1; unlocked (POLY)
//! ones' powers do (`heads::xgains`). MONO starts the new grain a whole number of the source's YIN
//! periods from the old grain's head (`lock`), so a held tone's splices
//! line up; a period that doesn't fit twice in the window and the D-TIME
//! (`locks`), or none, plays POLY.
//!
//! A grain starts at `T` wherever it is, so time always moves on; one that
//! reaches the window's end wraps, in the render loop: MONO's back by the
//! fewest whole periods that are a wrap's travel (`WRAP_SAMPLES` at the
//! block's fastest `inc`, and at least `INC_MAX` frames), re-reading its
//! last cycles, the nearest in kind, until the next splice (at most one
//! grain's life), under the lift: what the grain read the sample before,
//! against the same place wrapped, at its share, rides on the output, a
//! one-pole falling over `LIFT_S` (or an eighth of the wrap's interval, if
//! shorter: lifts again and again add at most an eighth of a jump as DC),
//! as PING-PONG's retargeted clamp carries its ghost's share. POLY's wrap
//! is by the window, not whole periods, so its jump can be twice the peak
//! and a lift would add the new grain's swing to it (+6 dBFS, #384): it
//! holds the output it left instead, the held value moving on at the
//! output's slope as it slows (`hold_from`), and crossfades from it to the
//! new grain under the same one-pole, an eighth past the peak at most (ADR
//! 0103). MONO lifts only where whole periods hold: the grain latched
//! locked, no hold in play, and the jump within an eighth of the level.
//! Any other MONO wrap (a grain started unlocked wrapping in a block that
//! locks, a run-up grain moved to its span's start as the window moves, a
//! jump read past the source's edge) holds, as POLY's does. Either way the
//! output never steps or bends there. A grain whose wrap comes more than once in
//! `MIN_PASS_SAMPLES` samples is a waveform loop, as an oscillator's cycle
//! is: it wraps bare.
//!
//! END before START plays reversed (spec § 2 Machines, #369): the
//! window is mirrored, as LOOP's is, so the grains read backward and time
//! walks from START toward END (MOTION below 0, back toward START). Each
//! grain keeps the way it started, so a flip lands at a splice, under its
//! fade; a reversed grain's step is negated, in the one render loop.
//!
//! The window is never under `MIN_LOOP_FRAMES` (no source is shorter), so
//! every wrap is at least `INC_MAX` frames and one subtraction does: no
//! grain reads past its window, and no path divides. Where the source has
//! them the window is at least `2 · WRAP_SAMPLES · inc` frames, so a grain
//! wraps at most once in two blocks at the speed it started at; a shorter
//! source's grains wrap more often, at most once in `MIN_PASS_SAMPLES`
//! samples with a lift (the bill holds that case).

use chimera_hal::BLOCK_SIZE;

use super::super::heads::{
    Dir, POWER, at, power_of, shortest, window_end_min, window_start, xgains,
};
use super::super::params::{StretchType, TapeEngineParams};
use super::super::read::{Pos, SourceView};
use super::super::{INC_MAX, MIN_LOOP_FRAMES, MIN_PASS_SAMPLES, Macros};
use crate::dsp::algo::math::{clampf, exp2};

/// A wrap's lift fades over 5 ms, as PING-PONG's clamp does.
pub const LIFT_S: f32 = 0.005;
/// A hold's weight under which a locked wrap lifts again: what's left of
/// it is then at most twice this of the peak.
const HOLD_LEFT: f32 = 0.01;
/// A POLY hold travels at most this many samples' move of the output's
/// slope as it slows, and spills at most this many past the output's and
/// the grains' level, and at most this share of it (`hold_from`).
const HOLD_MOVES: f32 = 12.0;
const HOLD_SPILL: f32 = 4.0;
const HOLD_OVER: f32 = 0.125;
/// A grain wraps at most once in this many samples (two blocks): the
/// window is at least twice this many samples of travel at the block's
/// fastest `inc` (POLY wraps by it), and MONO's wrap is the fewest whole
/// periods at least this long, so where the source allows it the two
/// grains' wraps come once a block, on average (spec § 5): at `inc` 16 the window is at least 4 096 frames, at the
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

/// Time's speed for MOTION `motion`, −1 to +2 times the source's own:
/// freeze at 0, 1× at ½, 2× at 1. Under 1× either way it is `slow`, an
/// exponential from 0, so the slow stretches (0.1–0.5×, the usual musical
/// ones) take 14 detents, not 8 (#698); 1× to 2× is linear.
pub fn time_speed(motion: f32) -> f32 {
    let m = clampf(motion, -1.0, 1.0);
    match m {
        m if m >= 0.5 => 2.0 * m,
        m if m >= 0.0 => slow(2.0 * m),
        m => -slow(-m),
    }
}

/// `(2^(4u) − 1) / 15`: 0 at 0, 1 at 1.
fn slow(u: f32) -> f32 {
    (exp2(4.0 * u) - 1.0) * (1.0 / 15.0)
}

/// MOTION 0.385 93: half speed under `time_speed`.
pub const HALF_SPEED: f32 = 0.385_932_85;

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
        smooth(self.pos as f32 / self.len as f32)
    }
}

/// What a block's splices take, worked out once a block.
#[derive(Clone, Copy)]
struct Block {
    /// The window's start and end for grains, frames: its first frame and
    /// last two are left out, so no Hermite tap reads past it.
    ws: u32,
    hi: u32,
    /// Its length, 32.32.
    wq: u64,
    /// A new grain's wrap: the window, or MONO's whole periods; at least
    /// `INC_MAX` frames.
    span: u64,
    /// It wraps more than once in `MIN_PASS_SAMPLES`: a waveform loop,
    /// wrapped with no lift.
    tiny: bool,
    /// The lift's one-pole fall a sample: its time constant `LIFT_S`, or
    /// an eighth of the shorter grain's wrap interval if shorter, so lifts
    /// that come again and again add at most an eighth of a jump as DC.
    /// Set in `render`, from the grains' latched spans at the block's speed.
    lift_k: f32,
    locked: bool,
    /// A new grain's carry kind: holds (1, POLY: unlocked) or lifts (0,
    /// MONO, where its jump allows: `carry_at_wrap`).
    hold: f32,
    /// The fade's length and its reciprocal.
    half: u32,
    inv_half: f32,
    /// POSITION in the window, 32.32, and time's step a sample.
    off: u64,
    tstep: i64,
    /// The window is mirrored: END is before START.
    rev: bool,
}

/// The new grain's share `x` of the way through its fade: a smoothstep,
/// so a splice has no corner where one fade ends and the next starts (the
/// old grain's is `1 −` it: they still sum to 1).
#[inline(always)]
pub fn smooth(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

/// A POLY wrap's hold, from the output `o` it leaves (`prev` the sample
/// before) at `level` (the grains' mix, and their last block's RMS): the
/// held value moves on at the output's slope, so it leaves the wave with no
/// corner, slowing a sample by the returned factor to rest at most
/// `HOLD_MOVES` samples' move on, and within `HOLD_SPILL` samples' move
/// (`HOLD_OVER` of the level at most) past the level, or `|o|` if further,
/// so the hold passes the peak by an eighth at most. The block's RMS keeps
/// a loud wave crossing 0 there from resting at once. Returns the next
/// sample's held value, its move and the move's fall.
fn hold_from(o: f32, prev: f32, level: f32) -> (f32, f32, f32) {
    let v = o - prev;
    if v == 0.0 {
        return (o, 0.0, 0.0);
    }
    // Past the level by `HOLD_SPILL` moves or `HOLD_OVER` of it, whichever is
    // less; never further out than `o` already is, so holds can't build.
    let most = libm::fabsf(o).max(level + (HOLD_SPILL * libm::fabsf(v)).min(HOLD_OVER * level));
    let rest = clampf(o + HOLD_MOVES * v, -most, most);
    let moves = (rest - o) / v;
    if moves <= 1.0 {
        // No room for a full move: one step to rest.
        return (rest, 0.0, 0.0);
    }
    let k = 1.0 - 1.0 / moves;
    (o + v, v * k, k)
}

/// A grain's wrap in the loop starts a carry (ADR 0103). A grain latched
/// locked (`latched` 0) lifts what the carry in play adds now (`(c, w, m)`
/// at the grains' mix `x`) plus the jump, the grain's read `r` against the
/// place wrapped (`back`) at its `share`, but only while the jump is what
/// whole periods promise, within `HOLD_OVER` of the `level`, and no hold
/// is still in play (`m · w` under `HOLD_LEFT`: a lift would freeze the
/// held value's gap to a wave that moves on). Anything else, POLY's window
/// wraps among them, holds the output `o` (`hold_from`), so no wrap passes
/// the peak by more than an eighth. Returns the carry, its move, the move's
/// fall and its kind (0 lift, 1 hold). Out of line: one copy for both
/// grains.
#[cold]
#[inline(never)]
fn carry_at_wrap(
    v: &SourceView<'_>,
    latched: f32,
    (c, w, m, x): (f32, f32, f32, f32),
    (o, prev, level): (f32, f32, f32),
    (r, back, share): (f32, Pos, f32),
) -> (f32, f32, f32, f32) {
    if latched == 0.0 && m * w < HOLD_LEFT {
        let jump = r - v.read(back);
        if libm::fabsf(jump) <= HOLD_OVER * level {
            return (w * (c - m * x) + share * jump, 0.0, 0.0, 0.0);
        }
    }
    let (h, vel, vk) = hold_from(o, prev, level);
    (h, vel, vk, 1.0)
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
    /// The old grain's head and the new one's, 32.32 source frames, where
    /// it reads: a reversed grain's moves down (`u` is `last − head`).
    heads: [Pos; 2],
    /// Each grain's window end (whole frames) and wrap, latched as it
    /// starts.
    hi: [u32; 2],
    span: [u64; 2],
    tiny: [bool; 2],
    /// Each grain's carry kind, latched with its span: 0 lifts (MONO's
    /// whole periods), 1 holds (POLY's window). A grain started unlocked
    /// keeps holding in a block that locks (SIZE or END moved mid-note):
    /// its wrap is still the window's, its jump up to twice the peak.
    holds: [f32; 2],
    /// How far through the fade the last sample was (the new grain's share
    /// is `smooth` of it), and its step a sample.
    g: f32,
    dg: f32,
    /// Time since the note-on in the window, 32.32: `T` is `ws` + POSITION's
    /// offset + this, wrapped.
    tau: u64,
    /// START, latched at note-on.
    start: f32,
    /// Each grain's frame, latched as it starts: mirrored, its `u` is
    /// `last − head`, `last` the source's last frame. Its window end and
    /// span are in `u`.
    rev: [bool; 2],
    last: Pos,
    kind: StretchType,
    /// The source's period, 32.32 and frames (0 unpitched), and its
    /// reciprocal.
    period: u64,
    pf: f32,
    inv_p: f32,
    /// The first splice is to come: the fade follows each block's D-TIME.
    runup: bool,
    /// The splice's `heads::xgains` power: equal-power between unlocked
    /// (POLY) grains, whose material is uncorrelated; a sum to 1 between
    /// locked ones and the note's first, which read alike (#698).
    pow: f32,
    /// The grain in play started locked.
    locked: bool,
    /// What wraps carry, riding on the output `x` as `w · (carry − hold ·
    /// x)`, `w` (`carry_w`) from 1 at a wrap falling by `lift_k` a sample.
    /// MONO's wrap is whole periods, its jump near 0: it lifts (`hold` 0),
    /// its carry the jump, so no corner, where that holds (`carry_at_wrap`;
    /// else it holds). POLY's window isn't whole periods,
    /// so its jump is up to twice the peak and a lift would add the new
    /// grain's swing to it (#384): it holds (`hold` 1) the output it left
    /// and crossfades from it to the new grain, an eighth past the peak at
    /// most; the held value moves on at the output's slope (`vel`, falling
    /// by `vel_k` a sample, `hold_from`), so there is no corner.
    carry: f32,
    carry_w: f32,
    hold: f32,
    vel: f32,
    vel_k: f32,
    /// The last block's last sample out (a hold's slope at a block's first)
    /// and the grains' RMS in it (a hold's level).
    prev: f32,
    peak: f32,
    /// Splices and wraps: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(in super::super) events: u32,
    /// Wraps in the loop, lifted and bare: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(in super::super) wraps: [u32; 2],
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
        holds: [0.0; 2],
        g: 0.0,
        dg: 0.0,
        tau: 0,
        start: 0.0,
        rev: [false; 2],
        last: Pos(0),
        kind: StretchType::Mono,
        period: 0,
        pf: 0.0,
        inv_p: 0.0,
        runup: false,
        pow: 0.0,
        locked: false,
        carry: 0.0,
        carry_w: 0.0,
        hold: 0.0,
        vel: 0.0,
        vel_k: 0.0,
        prev: 0.0,
        peak: 0.0,
        #[cfg(any(test, feature = "test-support"))]
        events: 0,
        #[cfg(any(test, feature = "test-support"))]
        wraps: [0; 2],
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
        let mut s = Stretch {
            start: p.start,
            last: at(v.frames() - 1),
            kind: p.kind,
            period: Pos::from_f32(pf).0,
            pf,
            inv_p: if pf > 0.0 { 1.0 / pf } else { 0.0 },
            runup: true,
            #[cfg(any(test, feature = "test-support"))]
            events: 0,
            #[cfg(any(test, feature = "test-support"))]
            wraps: [0; 2],
            ..Stretch::IDLE
        };
        let k = s.block(v, (inc, inc), (m.position, m.motion, m.size), p.end, sr);
        let t = s.time(k.off as i64, k);
        s.rev = [k.rev; 2];
        s.set_u(0, t);
        s.set_u(1, t);
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
        (self.heads[0].whole(), self.heads[1].whole(), smooth(self.g))
    }

    /// Grain `x`'s place in its frame.
    fn u(&self, x: usize) -> Pos {
        if self.rev[x] {
            self.last - self.heads[x]
        } else {
            self.heads[x]
        }
    }

    /// Grain `x` to `u` in its frame.
    fn set_u(&mut self, x: usize, u: Pos) {
        self.heads[x] = if self.rev[x] { self.last - u } else { u };
    }

    /// The old grain's way and the new one's.
    pub fn dirs(&self) -> [Dir; 2] {
        self.rev.map(|r| if r { Dir::Rev } else { Dir::Fwd })
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
        let (frames, rate) = (v.frames(), v.desc().rate());
        // END before START: the window mirrored, START its start there.
        let rev = Dir::of(self.start, end) == Dir::Rev;
        let (start, end) = if rev {
            (1.0 - self.start, 1.0 - end)
        } else {
            (self.start, end)
        };
        // Never under `MIN_LOOP_FRAMES` (64, and no source is shorter): a
        // START near the end moves the window's start back.
        let ws = window_start(start, v.file_frames()).min(frames.saturating_sub(MIN_LOOP_FRAMES));
        let floor = shortest(
            rate,
            (2 * WRAP_SAMPLES / MIN_PASS_SAMPLES) as f32 * from.max(to),
        );
        let we = window_end_min(ws, end, frames, floor);
        // The grains keep a tap inside: the window's first frame and last
        // two are left out.
        let ws = ws + 1;
        let hi = we.saturating_sub(2).max(ws + 1);
        let w = hi - ws;
        let wq = (w as u64) << 32;
        let d = d_time_s(size);
        let half = ((d * sr as f32 * 0.5 + 0.5) as u32).max(MIN_PASS_SAMPLES);
        let locked = locks(self.kind, self.pf, w, (d * f32::from(rate) + 0.5) as u32);
        let span = if locked {
            // The fewest whole periods that are a wrap's travel at the
            // block's fastest `inc` and at least `INC_MAX` frames, so a
            // grain re-reads its last cycles, the nearest in kind; at most
            // the window's (two at least, so at least half of it).
            let most = (w as f32 * self.inv_p) as u32;
            let travel = (WRAP_SAMPLES as f32 * from.max(to)).max(INC_MAX);
            let fewest = (travel * self.inv_p) as u32 + 1;
            let n = fewest.min(most).max(1);
            let s = (n as u64).wrapping_mul(self.period);
            if s > wq { s - self.period } else { s }
        } else {
            wq
        };
        // POSITION's offset in the window and time's step a sample, 32.32,
        // built from 32-bit conversions.
        let tiny = (span >> 32) < (MIN_PASS_SAMPLES as f32 * from.max(to)) as u64;
        let x = clampf(position, 0.0, 1.0) * w as f32;
        let off = q(x).min(wq - 1);
        let speed = time_speed(motion) * f32::from(rate) / sr as f32;
        Block {
            ws,
            hi,
            wq,
            span,
            tiny,
            lift_k: 0.0,
            locked,
            hold: if locked { 0.0 } else { 1.0 },
            half,
            inv_half: 1.0 / half as f32,
            off,
            tstep: ((speed * (1u32 << 30) as f32) as i32 as i64) << 2,
            rev,
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
        let mut k = self.block(v, (from, to), macros, end, sr);
        // A flip waits for the first splice.
        if self.runup && k.rev == self.rev[1] {
            // Nothing is in play yet: the first grain takes the block's
            // window, and its fade the block's D-TIME.
            (self.hi, self.span, self.tiny) = ([k.hi; 2], [k.span; 2], [k.tiny; 2]);
            self.holds = [k.hold; 2];
            self.clock.len = k.half.max(self.clock.pos + 1);
            // Re-aimed, so the share lands on 1 as the fade ends.
            self.dg = (1.0 - self.g) / self.clock.left() as f32;
            if (0..2).any(|x| self.u(x).whole() >= self.hi[x]) {
                self.wrap(v, k.hold);
            }
        }
        // Each grain is a waveform loop, or not, at this block's speed: a
        // pitch risen mid-grain makes its wraps bare, so lifted wraps come
        // at most once in `MIN_PASS_SAMPLES`. The lift falls as fast as the
        // shorter grain's wraps now come.
        let fast = from.max(to);
        let most = (MIN_PASS_SAMPLES as f32 * fast) as u64;
        self.tiny = self.span.map(|s| (s >> 32) < most);
        let every = (self.span[0].min(self.span[1]) >> 32) as f32 / fast.max(f32::EPSILON);
        let tc = (LIFT_S * sr as f32).min(0.125 * every).max(1.0);
        k.lift_k = libm::expf(-1.0 / tc);
        let (from, to) = (q(from), q(to));
        let step = (to.wrapping_sub(from) as i64 / BLOCK_SIZE as i64) as u64;
        let tau = self.tau as i64;
        self.play(out, v, &k, (from, step), tau);
        self.tau = wrap_q(tau + BLOCK_SIZE as i64 * k.tstep, k.wq);
        self.clock.t = self.time((k.off + self.tau) as i64, k);
    }

    /// The block's samples: two grains, the fade's gain rising `dg` a
    /// sample, `inc` ramped by `step`, and the lift riding on them, falling
    /// by `lift_k` a sample; each run to the next splice or the block's
    /// end. A grain at its window's end wraps here, in the loop: by its
    /// span (at least `INC_MAX`, so once does), what it read the sample
    /// before against the same place wrapped joining the lift at its share
    /// next, unless the grain is a waveform loop's.
    #[inline(never)]
    fn play(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        k: &Block,
        (mut cur, step): (u64, u64),
        tau: i64,
    ) {
        let mut i = 0;
        let lk = k.lift_k;
        // The last block's last sample: a hold's slope at the block's first.
        let (last, peak) = (self.prev, self.peak);
        // The grains' energy this block (never the carry's, so a hold's
        // spill can't build on the last's): a multiply-add a sample.
        let mut xe = 0f32;
        let top = self.last.whole() + 1;
        // The carry's kind is its wrap's: a block whose lock came or went
        // plays the one in play out as it was.
        let mut m = self.hold;
        while i < BLOCK_SIZE {
            // Each grain's way as a mask: 0 forward, all ones reversed. A
            // reversed grain's head moves down by the same step (its step
            // negated, `(x ^ d) − d`) and wraps up by its span below its
            // edge (`last − hi + 1`), so either way costs the same, in one
            // loop.
            let [d0, d1] = self.rev.map(|r| if r { u64::MAX } else { 0 });
            let neg = |x: u64, d: u64| (x ^ d).wrapping_sub(d);
            let [mut h0, mut h1] = self.heads;
            let (mut c0, mut c1) = (neg(cur, d0), neg(cur, d1));
            let (s0, s1) = (neg(step, d0), neg(step, d1));
            let edge = |e: u32, d: u64| if d != 0 { (top - e) as i32 } else { e as i32 };
            let [e0, e1] = self.hi;
            let (e0, e1) = (edge(e0, d0), edge(e1, d1));
            let (m0, m1) = (d0 as i32, d1 as i32);
            let (w0, w1) = (Pos(neg(self.span[0], d0)), Pos(neg(self.span[1], d1)));
            let [t0, t1] = self.tiny;
            let [k0, k1] = self.holds;
            let (mut g, dg, pw) = (self.g, self.dg, self.pow);
            let (mut c, mut w) = (self.carry, self.carry_w);
            let (mut vel, mut vk) = (self.vel, self.vel_k);
            let s = i;
            let stop = BLOCK_SIZE.min(i + self.clock.left() as usize);
            while i < stop {
                // The carry's terms are ready before the reads: the output
                // waits on one multiply-add after them.
                let (keep, add) = (1.0 - m * w, w * c);
                let a = v.read(h0);
                let b = v.read(h1);
                g += dg;
                let (ga, gb) = xgains(g, pw);
                let x = ga * a + gb * b;
                let o = add + keep * x;
                xe += x * x;
                out[i] = o;
                w *= lk;
                c += vel;
                vel *= vk;
                c0 = c0.wrapping_add(s0);
                c1 = c1.wrapping_add(s1);
                h0 = h0 + Pos(c0);
                h1 = h1 + Pos(c1);
                i += 1;
                // Past the edge: at or over it forward, under it reversed.
                if (h0.whole() as i32).wrapping_sub(e0) ^ m0 >= 0 {
                    h0 = h0 - w0;
                    if !t0 {
                        let prev = if i >= 2 { out[i - 2] } else { last };
                        let share = xgains(g + dg, pw).0;
                        let jump = (a, h0 - Pos(c0), share);
                        let now = (o, prev, libm::fabsf(x).max(peak));
                        (c, vel, vk, m) = carry_at_wrap(v, k0, (c, w, m, x), now, jump);
                        w = 1.0;
                    }
                    #[cfg(any(test, feature = "test-support"))]
                    {
                        self.events += 1;
                        self.wraps[usize::from(t0)] += 1;
                    }
                }
                if (h1.whole() as i32).wrapping_sub(e1) ^ m1 >= 0 {
                    h1 = h1 - w1;
                    if !t1 {
                        let prev = if i >= 2 { out[i - 2] } else { last };
                        let jump = (b, h1 - Pos(c1), xgains(g + dg, pw).1);
                        let now = (o, prev, libm::fabsf(x).max(peak));
                        (c, vel, vk, m) = carry_at_wrap(v, k1, (c, w, m, x), now, jump);
                        w = 1.0;
                    }
                    #[cfg(any(test, feature = "test-support"))]
                    {
                        self.events += 1;
                        self.wraps[usize::from(t1)] += 1;
                    }
                }
            }
            cur = neg(c0, d0);
            self.heads = [h0, h1];
            self.g = g;
            (self.carry, self.carry_w, self.hold) = (c, w, m);
            (self.vel, self.vel_k) = (vel, vk);
            self.clock.pos += (i - s) as u32;
            if self.clock.left() == 0 {
                let t = self.time(k.off as i64 + tau + i as i64 * k.tstep, *k);
                self.splice(t, *k);
                self.pow = self.splice_power(v, k.half);
            }
        }
        self.prev = out[BLOCK_SIZE - 1];
        // Its RMS: never over its peak, so a hold rests within an eighth of
        // the grains' own.
        self.peak = libm::sqrtf(xe / BLOCK_SIZE as f32);
    }

    /// A grain past its window's end before a block (the window moved under
    /// the run-up) wraps back by its span, or to the span's start if further
    /// out; what it reads there against where it was joins the carry, of
    /// the block's kind (`hold`, `carry`). A move to the span's start isn't
    /// whole periods, so it holds whatever the kind, as does a lock with a
    /// hold still in play or a jump whole periods don't explain.
    #[cold]
    #[inline(never)]
    fn wrap(&mut self, v: &SourceView<'_>, hold: f32) {
        let (ga, gb) = xgains(self.g + self.dg, self.pow);
        // The output now, near enough: the last sample out.
        let o = self.prev;
        let d = self.carry_w * (self.carry - self.hold * o);
        let mut jump = 0.0;
        let (mut moved, mut whole) = (false, true);
        for x in 0..2 {
            let (u, hi, span) = (self.u(x), self.hi[x], self.span[x]);
            if u.whole() < hi {
                continue;
            }
            let mut w = u - Pos(span);
            if w.whole() >= hi {
                w = at(hi) - Pos(span);
                whole = false;
            }
            let share = if x == 1 { gb } else { ga };
            let was = self.heads[x];
            self.set_u(x, w);
            jump += share * (v.read(was) - v.read(self.heads[x]));
            moved = true;
        }
        if moved {
            // Lifted only as a lock's wrap is (`carry_at_wrap`).
            let level = libm::fabsf(o).max(self.peak);
            let lifts = whole
                && hold == 0.0
                && self.hold * self.carry_w < HOLD_LEFT
                && libm::fabsf(jump) <= HOLD_OVER * level;
            let hold = if lifts { 0.0 } else { 1.0 };
            self.carry = if hold != 0.0 { o } else { d + jump };
            (self.carry_w, self.hold, self.vel) = (1.0, hold, 0.0);
        }
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
        self.holds[0] = self.holds[1];
        self.rev[0] = self.rev[1];
        // A flip lands here: the new grain plays the new way, and locks
        // only to a grain going its way.
        self.rev[1] = k.rev;
        let locked = k.locked && self.rev[0] == k.rev;
        let u = if locked {
            self.lock_at(self.u(0), t, k)
        } else {
            t
        };
        self.locked = locked;
        self.set_u(1, u);
        self.hi[1] = k.hi;
        self.span[1] = k.span;
        self.tiny[1] = k.tiny;
        self.holds[1] = k.hold;
        self.clock.len = k.half;
        self.clock.pos = 0;
        if self.carry_w < 1e-9 {
            self.carry_w = 0.0;
        }
        self.g = 0.0;
        self.dg = k.inv_half;
        self.runup = false;
        #[cfg(any(test, feature = "test-support"))]
        {
            self.events += 1;
        }
    }

    /// The splice just made's power: a sum to 1 on a pitched source, else
    /// `power_of` the two grains over the fade (`span` frames, about), or
    /// a sum to 1 where too few frames are held.
    fn splice_power(&self, v: &SourceView<'_>, span: u32) -> f32 {
        // A pitched source's grains read one tone, correlated, locked or
        // not: summing to 1 keeps the holds and lifts' bounds (ADR 0103).
        // A flip's grains run opposite ways: their phase drifts through
        // the fade, so no one power holds it.
        let flip = self.rev[0] != self.rev[1];
        if self.locked || flip || self.pf > 0.0 {
            return 0.0;
        }
        // A grain that wraps inside the fade reads elsewhere, and its hold
        // or lift is bounded for gains summing to 1: those splice so.
        let clear = (0..2).all(|x| self.u(x).whole().saturating_add(span) < self.hi[x]);
        if !clear {
            return 0.0;
        }
        let from = |x: usize| {
            let h = self.heads[x].whole();
            if self.rev[x] {
                h.saturating_sub(span)
            } else {
                h
            }
        };
        power_of(v, from(0), v, from(1), span).map_or(0.0, |c| c.min(0.5 * POWER))
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
            span: 0,
            lift_k: 1.0,
            locked: true,
            hold: 0.0,
            half: 240,
            inv_half: 1.0 / 240.0,
            off: 0,
            tstep: 0,
            rev: false,
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
        ramp_of(frames, 0, 16)
    }

    /// `ramp`, sample `i` at `base + i · step`.
    fn ramp_of(frames: usize, base: usize, step: usize) -> ([u8; 16 + 336], usize) {
        use super::super::super::read::{DESC_LEN, Descriptor, Shape};
        let d = Descriptor::whole(Shape::Plain, 1, 48_000).unwrap();
        let mut x = [0i16; 224];
        for (i, y) in x.iter_mut().enumerate() {
            *y = ((base + i * step) % 2_048) as i16;
        }
        let mut held = [0u8; 16 + 336];
        held[..DESC_LEN].copy_from_slice(&d.to_bytes());
        crate::storage::twelve::pack12(&x[..frames], &mut held[DESC_LEN..]);
        (held, DESC_LEN + 3 * frames / 2)
    }

    /// A wrap carries the grain's value at the share it plays next: the
    /// new grain's `g + dg`, not the last sample's `g`. In the loop, what
    /// it read the sample before against the same place wrapped; before a
    /// block (the run-up), what it reads against where it lands. A waveform
    /// loop's grain wraps bare. A slow ramp, so the jump is one whole
    /// periods could leave (within an eighth of the level): it lifts.
    #[test]
    fn wrap_carries_the_next_share() {
        let (held, n) = ramp_of(128, 1_024, 1);
        let v = SourceView::new(&held[..n]).unwrap();
        let mut s = Stretch::IDLE;
        (s.heads, s.hi, s.span) = ([at(1), at(40)], [40; 2], [32 << 32; 2]);
        (s.g, s.dg, s.peak) = (0.25, 0.25, 0.5);
        s.wrap(&v, 0.0);
        assert_eq!(s.hold, 0.0, "lifts");
        assert_eq!(s.heads[1], at(8));
        let want = smooth(0.5) * (v.read(at(40)) - v.read(at(8)));
        assert!((s.carry - want).abs() < 1e-6, "{} against {want}", s.carry);

        let k = Block {
            ws: 1,
            hi: 100,
            wq: 99 << 32,
            span: 30 << 32,
            tiny: false,
            lift_k: 1.0,
            locked: false,
            hold: 0.0,
            half: 1_000,
            inv_half: 0.001,
            off: 0,
            tstep: 0,
            rev: false,
        };
        let mut out = [0.0; BLOCK_SIZE];
        for tiny in [false, true] {
            let mut s = Stretch::IDLE;
            let h1 = Pos::from_f32(39.9);
            (s.heads, s.hi, s.span) = ([at(1), h1], [1_000, 40], [30 << 32; 2]);
            (s.g, s.dg, s.tiny) = (0.25, 0.25, [tiny; 2]);
            s.clock = GrainClock {
                t: at(1),
                len: 1_000,
                pos: 0,
            };
            // A quarter frame a sample: one wrap, at the first sample.
            s.play(&mut out, &v, &k, (1 << 30, 0), 0);
            let want = if tiny {
                0.0
            } else {
                smooth(0.75) * (v.read(h1) - v.read(Pos::from_f32(9.9)))
            };
            let got = s.carry * s.carry_w;
            assert!((got - want).abs() < 1e-5, "{got} against {want}");
        }
    }

    /// A locked wrap whose jump whole periods can't leave (a steep ramp: a
    /// quarter of full scale against a level of a half), or one with a hold
    /// still in play, holds the output rather than lifting the jump.
    #[test]
    fn a_jump_periods_cant_explain_holds() {
        let (held, n) = ramp(128);
        let v = SourceView::new(&held[..n]).unwrap();
        let mut s = Stretch::IDLE;
        (s.heads, s.hi, s.span) = ([at(1), at(40)], [40; 2], [32 << 32; 2]);
        (s.g, s.dg, s.peak, s.prev) = (0.25, 0.25, 0.5, 0.3);
        s.wrap(&v, 0.0);
        assert_eq!((s.hold, s.carry), (1.0, 0.3));

        let (held, n) = ramp_of(128, 1_024, 1);
        let v = SourceView::new(&held[..n]).unwrap();
        let mut s = Stretch::IDLE;
        (s.heads, s.hi, s.span) = ([at(1), at(40)], [40; 2], [32 << 32; 2]);
        (s.g, s.dg, s.peak, s.prev) = (0.25, 0.25, 0.5, 0.3);
        (s.hold, s.carry, s.carry_w) = (1.0, 0.6, 0.5);
        s.wrap(&v, 0.0);
        assert_eq!((s.hold, s.carry), (1.0, 0.3), "a hold in play");
    }

    /// A hold leaves at the output's slope and comes to rest where its
    /// moves add up to, within an eighth past the level; with no room for a
    /// move it steps to rest at once.
    #[test]
    fn hold_leaves_at_the_slope_and_rests_in_bounds() {
        for (o, prev, x) in [
            (0.1f32, 0.09f32, 0.2f32),
            (-0.3, -0.28, 0.1),
            (0.5, 0.4, 0.0),
        ] {
            let (mut c, mut vel, k) = hold_from(o, prev, x.abs());
            let level = o.abs().max(x.abs());
            if k > 0.0 {
                assert!((c - (2.0 * o - prev)).abs() < 1e-6, "{o}: leaves at {c}");
            }
            for _ in 0..1_000 {
                assert!(c.abs() <= 1.125 * level + 1e-6, "{o}: {c} past {level}");
                c += vel;
                vel *= k;
            }
            let want = clampf(o + 12.0 * (o - prev), -1.125 * level, 1.125 * level);
            assert!(
                (c - want).abs() < 2e-3 || k == 0.0,
                "{o}: rests at {c}, want {want}"
            );
        }
    }
    /// A loud wave crossing 0 at a wrap (`|o|` small, the grains' block at
    /// 0.8) moves on at its slope: with `|o|` alone for its level the hold
    /// rested at once, a stall the QA flagged at 8.4× on TX BASS.
    #[test]
    fn a_loud_wave_crossing_0_moves_on() {
        let (c, vel, k) = hold_from(-0.099, -0.085, 0.8);
        assert!(k > 0.5 && vel < 0.0, "rests at once: {c}, {vel}, {k}");
        assert!((c - (-0.113)).abs() < 1e-6, "{c}");
    }

    /// MONO's wrap is whole periods inside the window, even where the
    /// window is a whole number of them and `w / P` rounds up in `f32`
    /// (P = 2.13, w = 213: a source of 216 frames at `inc` 16).
    #[test]
    fn mono_span_fits_the_window() {
        let (held, n) = ramp(216);
        let v = SourceView::new(&held[..n]).unwrap();
        let p = 2.13f32;
        let mut s = Stretch::IDLE;
        (s.period, s.pf, s.inv_p) = (Pos::from_f32(p).0, p, 1.0 / p);
        let k = s.block(&v, (16.0, 16.0), (0.0, 0.0, 0.0), 1.0, 48_000);
        assert_eq!(k.hi - k.ws, 213);
        assert!(k.locked);
        assert!(k.span <= k.wq, "{} > {}", k.span, k.wq);
        assert!(k.span + s.period > k.wq);
    }
}
