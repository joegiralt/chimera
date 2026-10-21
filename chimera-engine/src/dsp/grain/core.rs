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

use crate::dsp::clampf;
use chimera_hal::BLOCK_SIZE;

/// A position or a step in frames, Q32.32: whole frames in the high word,
/// so a head reaches 2³² frames (a TAPE source's length, not only the
/// delay's line) and a step holds a pitch within 10⁻⁹.
pub type Q32 = u64;
pub const ONE: Q32 = 1 << 32;

/// `frames` as a `Q32` (exact to `f32`'s own precision).
#[inline]
pub fn q32(frames: f32) -> Q32 {
    (frames.max(0.0) * 4_294_967_296.0) as Q32
}

/// Audio the grains read. Frames are at the source's own scale: a
/// `Start`'s `amp` carries them to the output's.
pub trait GrainSource {
    /// Frames it holds; a head wraps at the end.
    fn frames(&self) -> usize;
    /// Frame `i`, `i < frames()`.
    fn frame(&self, i: usize) -> f32;
    /// Frames `i − 1 ..= i + 2`. The core asks only with `1 ≤ i` and
    /// `i + 2 < frames()`: a source may skip its own bounds checks there.
    #[inline(always)]
    fn taps(&self, i: usize) -> [f32; 4] {
        [
            self.frame(i - 1),
            self.frame(i),
            self.frame(i + 1),
            self.frame(i + 2),
        ]
    }
}

/// Starts a block holds; at 128 Hz and Beads' widest random interval
/// (9.5×) a block sees two.
pub const MAX_STARTS: usize = 4;

/// A uniform draw in [0, 1) (`stmlib::Random::GetFloat`'s use), from the
/// top 24 bits.
#[inline]
pub fn uniform(rng: &mut u32) -> f32 {
    let x = crate::dsp::xorshift32(rng);
    (x >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Beads' `LogOneMinusX`: ln(1 − x) by four terms of its series.
#[inline]
pub fn log_one_minus_x(x: f32) -> f32 {
    -x * (1.0 + x * (0.5 + x * (0.3333 + x * 0.25)))
}

/// Beads' bell (`GrainEnvelope` at SHAPE ⅔: attack ½, release ½,
/// smoothness 1): smoothstep up the first half, down the second. `phase`
/// in [0, 1]; 0 at both ends and past them.
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

    /// One block at `f` cycles a sample: each start's sample and the share
    /// of a sample past the crossing (Beads' `fractional_sample`). Returns
    /// how many of `out` it filled.
    pub fn block(
        &mut self,
        f: f32,
        texture: f32,
        rng: &mut u32,
        out: &mut [(usize, f32); MAX_STARTS],
    ) -> usize {
        let mut n = 0;
        let mut i = 0usize;
        loop {
            // Beads' CONSTRAIN(f, 0.000001, 0.25).
            let g = clampf(f * self.ratio, 1e-6, 0.25);
            let k = libm::ceilf((1.0 - self.phase) / g).max(1.0);
            if k > (BLOCK_SIZE - i) as f32 {
                self.phase += g * (BLOCK_SIZE - i) as f32;
                return n;
            }
            let k = k as usize;
            i += k;
            self.phase = clampf(self.phase + g * k as f32 - 1.0, 0.0, 0.999_999);
            if n < MAX_STARTS {
                out[n] = (i - 1, self.phase / g);
                n += 1;
            }
            let x = 0.1 + 0.9 * uniform(rng);
            let random = -1.0 / log_one_minus_x(x);
            self.ratio = 1.0 + texture * (random - 1.0);
        }
    }
}

/// Where and how a grain plays, its user's choice at a trigger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start {
    /// The head's first frame (the trigger's fraction already in it).
    pub pos: Q32,
    /// Frames a sample: the pitch.
    pub speed: f32,
    /// Samples it lasts, at least 2.
    pub dur: f32,
    /// Its gain, the source's scale to the output's.
    pub amp: f32,
}

/// One grain: a read head and its envelopes (`DelayGrain`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grain {
    pos: Q32,
    speed: Q32,
    /// The bell's phase, 0..1, and its step.
    phase: f32,
    inc: f32,
    /// The stealing fade (`VoiceStealingEnvelope`): value and step down.
    steal: f32,
    dec: f32,
    amp: f32,
    /// Samples left; 0 is done.
    remaining: i32,
}

impl Grain {
    pub const IDLE: Self = Self {
        pos: 0,
        speed: 0,
        phase: 0.0,
        inc: 0.0,
        steal: 0.0,
        dec: 0.0,
        amp: 0.0,
        remaining: 0,
    };

    pub fn done(&self) -> bool {
        self.remaining <= 0
    }

    pub fn remaining(&self) -> i32 {
        self.remaining
    }

    /// Beads' `FadeOut`: a grain with more than `samples` left ends in
    /// `samples`, linearly.
    pub fn fade_out(&mut self, samples: f32) {
        let samples = samples.max(1.0);
        if self.remaining as f32 > samples {
            self.dec = 1.0 / samples;
            self.remaining = (self.steal * samples) as i32;
        }
    }

    /// Starts at `s`, `frac` of a sample late: the bell starts that far in
    /// (`GrainEnvelope::Start`).
    pub fn start(&mut self, s: Start, frac: f32) {
        let dur = s.dur.max(2.0);
        self.pos = s.pos;
        self.speed = q32(s.speed);
        self.inc = 1.0 / dur;
        self.phase = (self.inc * frac).min(1.0);
        self.steal = 1.0;
        self.dec = 0.0;
        self.amp = s.amp;
        self.remaining = dur as i32;
    }

    /// Samples `from..to` of `out` from `src`: in runs whose taps stay
    /// clear of the source's ends (no wrap test a sample), and the
    /// stealing fade only while one runs.
    #[inline(always)]
    pub fn render<S: GrainSource>(&mut self, src: &S, out: &mut [f32], from: usize, to: usize) {
        let len = src.frames();
        let n = (to - from).min(self.remaining.max(0) as usize);
        let mut at = from;
        while at < from + n {
            let i = (self.pos >> 32) as usize;
            let want = from + n - at;
            let run = if i >= 1 && i + 2 < len {
                // Samples until the newest tap passes the end.
                let room = ((((len - 2) as Q32) << 32) - 1 - self.pos) / self.speed.max(1) + 1;
                want.min(room as usize)
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
        self.remaining -= n as i32;
    }

    /// `out`'s samples, every tap inside the source.
    #[inline(always)]
    fn run<S: GrainSource, const STEAL: bool>(&mut self, src: &S, out: &mut [f32]) {
        let (mut pos, speed) = (self.pos, self.speed);
        let (mut phase, inc) = (self.phase, self.inc);
        let (mut steal, dec) = (self.steal, self.dec);
        let amp = self.amp;
        for o in out {
            let i = (pos >> 32) as usize;
            let t = ((pos as u32) >> 8) as f32 * (1.0 / 16_777_216.0);
            let mut g = amp * bell(phase);
            if STEAL {
                steal -= dec;
                g *= steal.max(0.0);
            }
            *o += hermite(src.taps(i), t) * g;
            phase += inc;
            pos += speed;
        }
        (self.pos, self.phase, self.steal) = (pos, phase, steal);
    }

    /// One sample whose taps wrap the source.
    fn wrapped<S: GrainSource>(&mut self, src: &S, o: &mut f32) {
        let len = src.frames();
        let i = (self.pos >> 32) as usize;
        let t = ((self.pos as u32) >> 8) as f32 * (1.0 / 16_777_216.0);
        let x: [f32; 4] = ::core::array::from_fn(|k| src.frame((i + len + k - 1) % len));
        self.steal -= self.dec;
        let g = self.amp * bell(self.phase) * self.steal.max(0.0);
        *o += hermite(x, t) * g;
        self.phase += self.inc;
        self.pos += self.speed;
        let end = (len as Q32) << 32;
        if self.pos >= end {
            self.pos -= end;
        }
    }
}

/// The clock's settings for a block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sched {
    /// Grains a sample.
    pub rate: f32,
    /// 0 a regular train, 1 Beads' random intervals.
    pub texture: f32,
    /// The steal's crossfade and the pre-emptive one, samples
    /// (`kInstantCrossfadeTime`, `kPreemptiveCrossfadeTime`).
    pub instant: f32,
    pub preempt: f32,
}

/// `G` grains, their clock and the samples since the last start
/// (`GranularPlayer`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Engine<const G: usize> {
    grains: [Grain; G],
    clock: Clock,
    rng: u32,
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
        rng: 0x9E37_79B9,
        since: 0,
    };

    pub fn grains(&self) -> &[Grain; G] {
        &self.grains
    }

    /// Every grain at `speed` frames a sample from now: a user whose speed
    /// moves under its grains (TAPE's flutter), not only at their starts.
    pub fn set_speed(&mut self, speed: f32) {
        let q = q32(speed);
        for g in &mut self.grains {
            g.speed = q;
        }
    }

    /// Grains playing.
    pub fn active(&self) -> usize {
        self.grains.iter().filter(|g| !g.done()).count()
    }

    /// The block's grains from `src` into `out`, zeroed first. At each
    /// trigger the grains render up to it, `start(sample, fraction, u)`
    /// (`u` a uniform draw for the user's spray) places the new grain, it
    /// takes the grain nearest its end, and the next two fade (Beads'
    /// sorted scheduler).
    pub fn block<S: GrainSource>(
        &mut self,
        src: &S,
        sched: &Sched,
        out: &mut [f32; BLOCK_SIZE],
        mut start: impl FnMut(usize, f32, f32) -> Start,
    ) {
        *out = [0.0; BLOCK_SIZE];
        let mut starts = [(0usize, 0.0f32); MAX_STARTS];
        let n = self
            .clock
            .block(sched.rate, sched.texture, &mut self.rng, &mut starts);
        let mut from = 0;
        for &(at, frac) in &starts[..n] {
            self.render(src, out, from, at);
            from = at;
            let u = uniform(&mut self.rng);
            let s = start(at, frac, u);
            let k = self.nearest_end();
            self.grains[k].start(s, frac);
            let interval = self.since.wrapping_add(at as u32);
            self.steal_ahead(sched, interval as f32);
            // Counted from this start: the block's end adds the rest.
            self.since = 0u32.wrapping_sub(at as u32);
        }
        self.render(src, out, from, BLOCK_SIZE);
        self.since = self.since.wrapping_add(BLOCK_SIZE as u32).min(1 << 24);
    }

    fn render<S: GrainSource>(&mut self, src: &S, out: &mut [f32], from: usize, to: usize) {
        if from >= to {
            return;
        }
        for g in self.grains.iter_mut().filter(|g| !g.done()) {
            g.render(src, out, from, to);
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
    fn steal_ahead(&mut self, sched: &Sched, interval: f32) {
        let first = self.nearest_end();
        let mut second = usize::from(first == 0);
        for (i, g) in self.grains.iter().enumerate() {
            if i != first && g.remaining < self.grains[second].remaining {
                second = i;
            }
        }
        self.grains[first].fade_out(sched.instant);
        self.grains[second].fade_out(interval.max(sched.instant).min(sched.preempt));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ramp;

    impl GrainSource for Ramp {
        fn frames(&self) -> usize {
            4096
        }

        fn frame(&self, i: usize) -> f32 {
            i as f32
        }
    }

    fn played() -> Engine<4> {
        let sched = Sched {
            rate: 0.01,
            texture: 0.0,
            instant: 240.0,
            preempt: 4800.0,
        };
        let mut e = Engine::<4>::NEW;
        let mut out = [0.0; BLOCK_SIZE];
        for _ in 0..8 {
            e.block(&Ramp, &sched, &mut out, |_, _, _| Start {
                pos: 64 * ONE,
                speed: 1.0,
                dur: 2000.0,
                amp: 1.0,
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
        after.set_speed(1.5);
        for (a, b) in after.grains.iter().zip(&before.grains) {
            assert_eq!(a.speed, q32(1.5));
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
}
