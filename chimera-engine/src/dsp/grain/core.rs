//! The grain core (ADR 0202), after Mutable Instruments Beads' grain engine
//! (`beads/dsp/`: `granular_player.h`, `delay_grain.h`, `grain_envelope.h`,
//! `voice_stealing_envelope.h`, `trigger_generator.cc`, `delay_line.h`):
//! - the pulse-train trigger and its random intervals (`RenderPulseTrain`,
//!   `-1/ln(1 − x)`, `LogOneMinusX`'s series), TEXTURE blending them in;
//! - the bell (`GrainEnvelope` at SHAPE ⅔: smoothstep up, then down);
//! - the stealing: after each start the grains nearest their end fade
//!   (`SortGrainList`, `VoiceStealingEnvelope`);
//! - the Hermite read (`DelayLine::Read<INTERPOLATION_HERMITE>`).
//!
//! It reads audio only through `GrainSource`, so any buffer can feed it:
//! the delay's line (`delay::grain`) and TAPE's source. Where a grain
//! starts, how fast and for how long is its user's: `Engine::block` asks a
//! `Start` of it at each trigger.
//
// Beads is Copyright 2017 Emilie Gillet (emilie.o.gillet@gmail.com), under
// the MIT License:
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.

use crate::dsp::{Unit, clampf};
use chimera_hal::BLOCK_SIZE;
use core::marker::PhantomData;
use core::num::NonZeroU32;

/// One frame, Q32.32.
const ONE: u64 = 1 << 32;

/// `frames` as Q32.32, to `f32`'s own precision; below 0 and NaN are 0,
/// past 2³² frames saturates.
#[inline]
fn q32(frames: f32) -> u64 {
    (frames.max(0.0) * 4_294_967_296.0) as u64
}

/// The fastest a head reads, frames a sample: four octaves up.
pub const MAX_SPEED: f32 = 16.0;

/// A head's place in a source: whole frames in the high word, the fraction
/// in the low (Q32.32), so a head reaches 2³² frames (a TAPE source's
/// length, not only the delay's line) and moves within 10⁻⁹ of a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos(u64);

impl Pos {
    pub const ZERO: Self = Self(0);

    /// Frame `i`, exactly.
    pub const fn frame(i: usize) -> Self {
        Self((i as u64) << 32)
    }

    /// `frames` in, to `f32`'s precision.
    pub fn of(frames: Frames) -> Self {
        Self(q32(frames.get()))
    }

    /// `back` before frame `head` on a ring of `len` frames.
    pub fn behind(head: usize, back: Frames, len: usize) -> Self {
        let (head, back, end) = (
            Self::frame(head % len).0,
            q32(back.get()),
            Self::frame(len).0,
        );
        let back = back % end;
        Self(if head >= back {
            head - back
        } else {
            head + end - back
        })
    }

    /// The whole frame it is in.
    pub const fn whole(self) -> usize {
        (self.0 >> 32) as usize
    }

    /// Its share of the way to the next frame, from the fraction's top 24
    /// bits.
    pub fn frac(self) -> f32 {
        ((self.0 as u32) >> 8) as f32 * (1.0 / 16_777_216.0)
    }
}

/// A distance in a source's frames, 0 or more (NaN is 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Frames(f32);

impl Frames {
    pub const fn new(v: f32) -> Self {
        Self(clampf(v, 0.0, 4_294_967_040.0))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A time in output samples, 0 or more (NaN is 0), up to 2²⁴ (5.8 min at
/// 48 kHz) so a count of them is exact.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Samples(f32);

impl Samples {
    pub const fn new(v: f32) -> Self {
        Self(clampf(v, 0.0, 16_777_216.0))
    }

    /// `s` seconds at `fs`.
    pub fn secs(s: f32, fs: u32) -> Self {
        Self::new(s * fs as f32)
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A grain's length: 2 samples or more (a bell needs both ends).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct GrainLen(f32);

impl GrainLen {
    pub const MIN: Self = Self(2.0);

    pub const fn new(s: Samples) -> Self {
        Self(s.get().max(2.0))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A read speed, frames a sample: 0 (still) to `MAX_SPEED`, Q32.32 (NaN
/// is 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Speed(u64);

impl Speed {
    pub const UNITY: Self = Self(ONE);

    pub fn of(ratio: f32) -> Self {
        Self(q32(clampf(ratio, 0.0, MAX_SPEED)))
    }

    /// As a ratio, to `f32`'s precision.
    pub fn ratio(self) -> f32 {
        self.0 as f32 * (1.0 / 4_294_967_296.0)
    }
}

/// A gain, 0 or more and finite (NaN is 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Gain(f32);

impl Gain {
    pub const ZERO: Self = Self(0.0);

    pub const fn new(v: f32) -> Self {
        Self(clampf(v, 0.0, f32::MAX))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Grains a sample: 0 to ¼ (Beads' `CONSTRAIN(f, …, 0.25)`; NaN is 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Rate(f32);

impl Rate {
    pub const ZERO: Self = Self(0.0);

    /// `hz` grains a second at `fs`.
    pub fn hz(hz: f32, fs: u32) -> Self {
        Self(clampf(hz / fs as f32, 0.0, 0.25))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// The grains' draws: xorshift32 from a seed that is never 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng(u32);

impl Rng {
    /// The core's own seed.
    pub const NEW: Self = Self(0x9E37_79B9);

    pub const fn seeded(seed: NonZeroU32) -> Self {
        Self(seed.get())
    }

    /// A uniform draw in [0, 1) (`stmlib::Random::GetFloat`'s use), from
    /// the top 24 bits.
    #[inline]
    pub fn uniform(&mut self) -> Unit {
        let x = crate::dsp::xorshift32(&mut self.0);
        Unit::new((x >> 8) as f32 * (1.0 / 16_777_216.0))
    }
}

/// Frame `i` of `S`, `i < S::FRAMES`: made by the core alone, so a source
/// reads it unchecked.
pub struct At<S>(usize, PhantomData<fn(&S)>);

impl<S> At<S> {
    pub const fn get(&self) -> usize {
        self.0
    }
}

/// Frames `i − 1 ..= i + 2` of `S`, all inside it (`1 ≤ i`, `i + 2 <
/// S::FRAMES`): made by the core alone, so a source reads them unchecked.
pub struct TapAt<S>(usize, PhantomData<fn(&S)>);

impl<S> TapAt<S> {
    pub const fn get(&self) -> usize {
        self.0
    }
}

/// Audio the grains read. Frames are at the source's own scale: a
/// `Start`'s `amp` carries them to the output's.
pub trait GrainSource: Sized {
    /// Frames it holds, at least 4 (a Hermite's taps) and more than
    /// `MAX_SPEED`, so a head wraps once at most a sample: held at compile
    /// time where the core is built for it.
    const FRAMES: usize;
    /// Frame `i`.
    fn frame(&self, i: At<Self>) -> f32;
    /// Frames `i − 1 ..= i + 2`.
    #[inline(always)]
    fn taps(&self, i: TapAt<Self>) -> [f32; 4] {
        let i = i.get();
        [i - 1, i, i + 1, i + 2].map(|k| self.frame(At(k, PhantomData)))
    }
}

/// Starts a block holds; at 128 Hz and Beads' widest random interval
/// (9.5×) a block sees two.
pub const MAX_STARTS: usize = 4;

/// Beads' `LogOneMinusX`: ln(1 − x) by four terms of its series.
#[inline]
pub fn log_one_minus_x(x: f32) -> f32 {
    -x * (1.0 + x * (0.5 + x * (0.3333 + x * 0.25)))
}

/// Beads' bell (`GrainEnvelope` at SHAPE ⅔: attack ½, release ½,
/// smoothness 1): smoothstep up the first half, down the second. `phase`
/// in [0, 1]; 0 at both ends and past them, so the per-sample phase needs
/// no clamp.
#[inline(always)]
pub fn bell(phase: f32) -> f32 {
    let tri = 1.0 - (2.0 * phase - 1.0).abs();
    let tri = tri.max(0.0);
    tri * tri * (3.0 - 2.0 * tri)
}

/// Beads' Hermite (`DelayLine::Read<INTERPOLATION_HERMITE>`), its own form:
/// between `x[1]` and `x[2]` at `t`.
#[inline(always)]
pub fn hermite(x: [f32; 4], t: f32) -> f32 {
    let c = (x[2] - x[0]) * 0.5;
    let v = x[1] - x[2];
    let w = c + v;
    let a = w + v + (x[3] - x[1]) * 0.5;
    let b = w + a;
    ((a * t - b) * t + c) * t + x[1]
}

/// A trigger in a block: its sample, and the share of a sample past the
/// crossing (Beads' `fractional_sample`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trigger {
    at: usize,
    frac: Unit,
}

impl Trigger {
    /// Its sample, below `BLOCK_SIZE`.
    pub const fn at(self) -> usize {
        self.at
    }

    pub const fn frac(self) -> Unit {
        self.frac
    }
}

/// A block's triggers, in order, `MAX_STARTS` at most.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Triggers {
    at: [Trigger; MAX_STARTS],
    n: u8,
}

impl Triggers {
    const NONE: Self = Self {
        at: [Trigger {
            at: 0,
            frac: Unit::ZERO,
        }; MAX_STARTS],
        n: 0,
    };

    pub fn as_slice(&self) -> &[Trigger] {
        &self.at[..usize::from(self.n)]
    }

    fn push(&mut self, t: Trigger) {
        if let Some(slot) = self.at.get_mut(usize::from(self.n)) {
            *slot = t;
            self.n += 1;
        }
    }
}

/// The grains' trigger: Beads' pulse train. TEXTURE 0 is its regular
/// train; 1 its random one, each interval's rate × `-1/ln(1 − x)`, x
/// uniform in [0.1, 1); between, the ratio blends linearly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    phase: f32,
    ratio: f32,
}

impl Default for Clock {
    fn default() -> Self {
        Self::NEW
    }
}

impl Clock {
    pub const NEW: Self = Self {
        phase: 0.0,
        ratio: 1.0,
    };

    /// One block's triggers at `rate`; past `MAX_STARTS`, none.
    pub fn block(&mut self, rate: Rate, texture: Unit, rng: &mut Rng) -> Triggers {
        let mut out = Triggers::NONE;
        let mut i = 0usize;
        loop {
            // Beads' CONSTRAIN(f, 0.000001, 0.25).
            let g = clampf(rate.get() * self.ratio, 1e-6, 0.25);
            let k = libm::ceilf((1.0 - self.phase) / g).max(1.0);
            if k > (BLOCK_SIZE - i) as f32 {
                self.phase += g * (BLOCK_SIZE - i) as f32;
                return out;
            }
            let k = k as usize;
            i += k;
            self.phase = clampf(self.phase + g * k as f32 - 1.0, 0.0, 0.999_999);
            out.push(Trigger {
                at: i - 1,
                frac: Unit::new(self.phase / g),
            });
            let x = 0.1 + 0.9 * rng.uniform().get();
            let random = -1.0 / log_one_minus_x(x);
            self.ratio = 1.0 + texture.get() * (random - 1.0);
        }
    }
}

/// Where and how a grain plays, its user's choice at a trigger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start {
    /// The head's first frame (the trigger's fraction already in it).
    pub pos: Pos,
    /// The pitch.
    pub speed: Speed,
    pub len: GrainLen,
    /// The source's scale to the output's.
    pub amp: Gain,
}

/// One grain: a read head and its envelopes (`DelayGrain`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grain {
    pos: Pos,
    speed: Speed,
    /// The bell's phase, 0..1, and its step.
    phase: f32,
    inc: f32,
    /// The stealing fade (`VoiceStealingEnvelope`): value and step down.
    steal: f32,
    dec: f32,
    amp: Gain,
    /// Samples left; 0 is done.
    remaining: u32,
}

impl Grain {
    pub const IDLE: Self = Self {
        pos: Pos::ZERO,
        speed: Speed(0),
        phase: 0.0,
        inc: 0.0,
        steal: 0.0,
        dec: 0.0,
        amp: Gain::ZERO,
        remaining: 0,
    };

    pub const fn done(&self) -> bool {
        self.remaining == 0
    }

    pub const fn remaining(&self) -> u32 {
        self.remaining
    }

    /// Beads' `FadeOut`: a grain with more than `s` left ends in `s` (at
    /// least one), linearly.
    pub fn fade_out(&mut self, s: Samples) {
        let samples = s.get().max(1.0);
        if self.remaining as f32 > samples {
            self.dec = 1.0 / samples;
            self.remaining = (self.steal * samples) as u32;
        }
    }

    /// Starts at `s`, `frac` of a sample late: the bell starts that far in
    /// (`GrainEnvelope::Start`).
    pub fn start(&mut self, s: Start, frac: Unit) {
        let len = s.len.get();
        self.pos = s.pos;
        self.speed = s.speed;
        self.inc = 1.0 / len;
        self.phase = (self.inc * frac.get()).min(1.0);
        self.steal = 1.0;
        self.dec = 0.0;
        self.amp = s.amp;
        self.remaining = len as u32;
    }

    /// Adds its next samples to `out` from `src`, as many as it has left:
    /// in runs whose taps stay clear of the source's ends (no wrap test a
    /// sample), the stealing fade only while one runs.
    #[inline(always)]
    pub fn render<S: GrainSource>(&mut self, src: &S, out: &mut [f32]) {
        const { assert!(S::FRAMES >= 4 && S::FRAMES as f32 > MAX_SPEED && (S::FRAMES as u64) < ONE) };
        let len = S::FRAMES;
        let n = out.len().min(self.remaining as usize);
        let mut at = 0;
        while at < n {
            let i = self.pos.whole();
            let run = if i >= 1 && i + 2 < len {
                // Samples until the newest tap passes the end.
                let room = ((((len - 2) as u64) << 32) - 1 - self.pos.0) / self.speed.0.max(1) + 1;
                (n - at).min(room as usize)
            } else {
                0
            };
            if run == 0 {
                self.wrapped(src, &mut out[at]);
                at += 1;
            } else if self.dec == 0.0 {
                self.run::<S, false>(src, &mut out[at..at + run]);
                at += run;
            } else {
                self.run::<S, true>(src, &mut out[at..at + run]);
                at += run;
            }
        }
        self.remaining -= n as u32;
    }

    /// `out`'s samples, every tap inside the source (`render`'s run).
    #[inline(always)]
    fn run<S: GrainSource, const STEAL: bool>(&mut self, src: &S, out: &mut [f32]) {
        let (mut pos, speed) = (self.pos, self.speed.0);
        let (mut phase, inc) = (self.phase, self.inc);
        let (mut steal, dec) = (self.steal, self.dec);
        let amp = self.amp.get();
        for o in out {
            let mut g = amp * bell(phase);
            if STEAL {
                steal -= dec;
                g *= steal.max(0.0);
            }
            *o += hermite(src.taps(TapAt(pos.whole(), PhantomData)), pos.frac()) * g;
            phase += inc;
            pos.0 += speed;
        }
        (self.pos, self.phase, self.steal) = (pos, phase, steal);
    }

    /// One sample whose taps wrap the source.
    fn wrapped<S: GrainSource>(&mut self, src: &S, o: &mut f32) {
        let len = S::FRAMES;
        let i = self.pos.whole();
        let x: [f32; 4] =
            ::core::array::from_fn(|k| src.frame(At((i + len + k - 1) % len, PhantomData)));
        self.steal -= self.dec;
        let g = self.amp.get() * bell(self.phase) * self.steal.max(0.0);
        *o += hermite(x, self.pos.frac()) * g;
        self.phase += self.inc;
        self.pos.0 += self.speed.0;
        let end = Pos::frame(len).0;
        if self.pos.0 >= end {
            self.pos.0 -= end;
        }
    }
}

/// The clock's settings for a block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sched {
    pub rate: Rate,
    /// 0 a regular train, 1 Beads' random intervals.
    pub texture: Unit,
    /// The steal's crossfade and the pre-emptive one
    /// (`kInstantCrossfadeTime`, `kPreemptiveCrossfadeTime`).
    pub instant: Samples,
    pub preempt: Samples,
}

/// `G` grains, their clock and the samples since the last start
/// (`GranularPlayer`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Engine<const G: usize> {
    grains: [Grain; G],
    clock: Clock,
    rng: Rng,
    since: u32,
}

impl<const G: usize> Default for Engine<G> {
    fn default() -> Self {
        Self::NEW
    }
}

impl<const G: usize> Engine<G> {
    pub const NEW: Self = Self {
        grains: [Grain::IDLE; G],
        clock: Clock::NEW,
        rng: Rng::NEW,
        since: 0,
    };

    pub fn grains(&self) -> &[Grain; G] {
        &self.grains
    }

    /// Every grain at `speed` from now: a user whose speed moves under its
    /// grains (TAPE's flutter), not only at their starts.
    pub fn set_speed(&mut self, speed: Speed) {
        for g in &mut self.grains {
            g.speed = speed;
        }
    }

    /// Grains playing.
    pub fn active(&self) -> usize {
        self.grains.iter().filter(|g| !g.done()).count()
    }

    /// The block's grains from `src` into `out`, zeroed first. At each
    /// trigger the grains render up to it, `start(trigger, u)` (`u` a
    /// uniform draw for the user's spray) places the new grain, it takes
    /// the grain nearest its end, and the next two fade (Beads' sorted
    /// scheduler).
    pub fn block<S: GrainSource>(
        &mut self,
        src: &S,
        sched: &Sched,
        out: &mut [f32; BLOCK_SIZE],
        mut start: impl FnMut(Trigger, Unit) -> Start,
    ) {
        *out = [0.0; BLOCK_SIZE];
        let starts = self.clock.block(sched.rate, sched.texture, &mut self.rng);
        let mut from = 0;
        for &t in starts.as_slice() {
            self.render(src, &mut out[from..t.at]);
            from = t.at;
            let u = self.rng.uniform();
            let s = start(t, u);
            let k = self.nearest_end();
            self.grains[k].start(s, t.frac);
            let interval = self.since.wrapping_add(t.at as u32);
            self.steal_ahead(sched, Samples::new(interval as f32));
            // Counted from this start: the block's end adds the rest.
            self.since = 0u32.wrapping_sub(t.at as u32);
        }
        self.render(src, &mut out[from..]);
        self.since = self.since.wrapping_add(BLOCK_SIZE as u32).min(1 << 24);
    }

    fn render<S: GrainSource>(&mut self, src: &S, out: &mut [f32]) {
        if out.is_empty() {
            return;
        }
        for g in self.grains.iter_mut().filter(|g| !g.done()) {
            g.render(src, out);
        }
    }

    /// The grain with the fewest samples left (a done one has none).
    fn nearest_end(&self) -> usize {
        let mut k = 0;
        for (i, g) in self.grains.iter().enumerate() {
            if g.remaining < self.grains[k].remaining {
                k = i;
            }
        }
        k
    }

    /// Beads' `SortGrainList`, sorted branch: the grain now nearest its end
    /// ends within the steal's crossfade, the next within the interval just
    /// gone or the pre-emptive fade, whichever is shorter; so the next start
    /// finds a grain done, or nearly. Beads fades four of 32; this, two.
    fn steal_ahead(&mut self, sched: &Sched, interval: Samples) {
        let first = self.nearest_end();
        let mut second = usize::from(first == 0);
        for (i, g) in self.grains.iter().enumerate() {
            if i != first && g.remaining < self.grains[second].remaining {
                second = i;
            }
        }
        self.grains[first].fade_out(sched.instant);
        let next = interval
            .get()
            .max(sched.instant.get())
            .min(sched.preempt.get());
        self.grains[second].fade_out(Samples::new(next));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ramp;

    impl GrainSource for Ramp {
        const FRAMES: usize = 4096;

        fn frame(&self, i: At<Self>) -> f32 {
            i.get() as f32
        }
    }

    fn played() -> Engine<4> {
        let sched = Sched {
            rate: Rate::hz(480.0, 48_000),
            texture: Unit::ZERO,
            instant: Samples::new(240.0),
            preempt: Samples::new(4800.0),
        };
        let mut e = Engine::<4>::NEW;
        let mut out = [0.0; BLOCK_SIZE];
        for _ in 0..8 {
            e.block(&Ramp, &sched, &mut out, |_, _| Start {
                pos: Pos::frame(64),
                speed: Speed::UNITY,
                len: GrainLen::new(Samples::new(2000.0)),
                amp: Gain::new(1.0),
            });
        }
        e
    }

    /// `set_speed` moves every grain's step, playing or not, and nothing
    /// else: heads, bells, fades, gains, the clock and its draws stay.
    #[test]
    fn set_speed_moves_the_live_grains_step_alone() {
        let before = played();
        assert!(before.active() > 1);
        let mut after = before;
        after.set_speed(Speed::of(1.5));
        for (a, b) in after.grains.iter().zip(&before.grains) {
            assert_eq!(a.speed, Speed::of(1.5));
            assert_eq!(
                Grain {
                    speed: b.speed,
                    ..*a
                },
                *b
            );
        }
        assert_eq!(
            (after.clock, after.rng, after.since),
            (before.clock, before.rng, before.since)
        );
    }

    /// A head behind another wraps the ring, and reads its own fraction.
    #[test]
    fn a_head_behind_wraps_the_ring() {
        let p = Pos::behind(10, Frames::new(12.5), 100);
        assert_eq!(p.whole(), 97);
        assert_eq!(p.frac(), 0.5);
        assert_eq!(Pos::behind(10, Frames::new(4.0), 100), Pos::frame(6));
    }
}
