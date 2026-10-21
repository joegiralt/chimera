//! GRAIN, the delay's second TYPE (ADR 0199): repeats made of grains read
//! from the delay's own line, after Mutable Instruments Beads' grain engine
//! (`beads/dsp/`: `granular_player.h`, `delay_grain.h`, `grain_envelope.h`,
//! `voice_stealing_envelope.h`, `trigger_generator.cc`,
//! `granular_processor.h`).
//!
//! Ported from Beads:
//! - the pulse-train trigger and its random intervals (`RenderPulseTrain`,
//!   `-1/ln(1 − x)`, `LogOneMinusX`'s series), TEXTURE blending them in;
//! - a grain's start: the sub-sample offset, the duration trimmed to the
//!   line and the delay held off the write head (`DelayGrain::Start`);
//! - the bell envelope (`GrainEnvelope` at SHAPE ⅔: smoothstep up, down);
//! - the stealing: after each start the grains closest to their end are
//!   faded (`SortGrainList`, `VoiceStealingEnvelope`);
//! - the Hermite read (`DelayLine::Read<INTERPOLATION_HERMITE>`);
//! - the feedback's soft clip (`stmlib::SoftClip`, `fast_tanh`) and DC
//!   blocker.
//!
//! Not ported, and why, in ADR 0199: the seconds-long buffer and its
//! time-stretch clouds (500 ms here, ADR 0014), stereo pan, SHAPE, reverse
//! grains, the 32-grain pool (6 here), the delay mode's taps, the media
//! models and the reverb.
//!
//! With `bank2` this module links into bank 2 whole (`bank2.x`,
//! `*5delay5grain*`); bank 1 reaches `run` only through
//! `GrainDoor`. With the door closed GRAIN plays as TAPE.
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

use super::{
    DelayParams, DelayType, Eased, FLUTTER_SAMPLES, MAX_DELAY_SAMPLES, TIME_FADE, Tap, TapeDelay,
    WOW_SAMPLES, read, to_line,
};
use crate::dsp::algo::math::exp2;
use crate::dsp::clampf;
use crate::dsp::ease::{at, ease_coeff, step_of};
use chimera_hal::BLOCK_SIZE;

const N: usize = MAX_DELAY_SAMPLES;
/// The line's length in Q16.16.
const N_Q: u32 = (N as u32) << 16;
const Q: f32 = 65_536.0;

/// Grains at once (Beads' delay player: 32): six keep an ALGO INIT's
/// voices on rev Y but one (ADR 0199).
pub const GRAINS: usize = 6;
/// The nearest a grain reads to the write head, samples: the block is read
/// before it is written, so a grain at sample 63 must still find its
/// Hermite's newest tap written (63 + 3), and two to spare.
pub const MIN_D: f32 = (BLOCK_SIZE + 4) as f32;
/// The farthest: the line less the Hermite's oldest tap, and two to spare.
pub const MAX_D: f32 = (N - 4) as f32;

/// SIZE: a grain's length, `SIZE_MIN · SIZE_SPAN^size` seconds, 10 ms to
/// 500 ms (Beads: 8 ms to 4 s over its buffer).
pub const SIZE_MIN: f32 = 0.010;
pub const SIZE_SPAN: f32 = 50.0;
/// DENSITY: grains a second, `RATE_MIN · 2^(RATE_OCTAVES·density)`, 1 to
/// 128 Hz (Beads' pulse train tops out at 130.81 Hz).
pub const RATE_MIN: f32 = 1.0;
pub const RATE_OCTAVES: f32 = 7.0;
/// TEXTURE's spray of a grain's start, a share of TIME either side.
pub const SPRAY: f32 = 0.5;
/// Beads' crossfades: a steal (`kInstantCrossfadeTime`) and the one ahead
/// of it (`kPreemptiveCrossfadeTime`), seconds.
pub const INSTANT_S: f32 = 0.005;
pub const PREEMPT_S: f32 = 0.1;
/// The feedback's DC blocker, Hz (Beads: an SVF high-pass at 40 Hz).
pub const DC_HZ: f32 = 20.0;
/// Starts a block holds; at 128 Hz and Beads' widest random interval
/// (9.5×) a block sees two.
const MAX_STARTS: usize = 4;

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

/// `stmlib::SoftClip` (Beads' feedback limit): already ported, ADR 0097.
pub use crate::dsp::fast_tanh as soft_clip;

/// Beads' bell (`GrainEnvelope` at SHAPE ⅔: attack ½, release ½,
/// smoothness 1): smoothstep up the first half, down the second. `phase`
/// in [0, 1]; 0 at both ends.
#[inline(always)]
pub fn bell(phase: f32) -> f32 {
    let tri = 1.0 - (2.0 * phase - 1.0).abs();
    let tri = tri.max(0.0);
    tri * tri * (3.0 - 2.0 * tri)
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

/// One grain: a read head on the line and its envelopes (`DelayGrain`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grain {
    /// The read head, Q16.16 into the line (Beads: 22.10 behind the head).
    pos: u32,
    /// Its step a sample, Q16.16: the pitch.
    speed: u32,
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

    /// Starts at the write head `head` (a ring position, the sample it
    /// starts on), `delay` samples behind it, at `speed`, for `dur`
    /// samples, `frac` of a sample late (`DelayGrain::Start`): the length
    /// trimmed so the line holds it, the delay so the head never crosses.
    pub fn start(&mut self, head: usize, delay: f32, speed: f32, dur: f32, amp: f32, frac: f32) {
        let c = speed - 1.0;
        let delay = delay - frac * c;
        let dur = if c != 0.0 {
            dur.min((MAX_D - MIN_D) / c.abs())
        } else {
            dur
        }
        .max(2.0);
        let lo = MIN_D + c.max(0.0) * dur;
        let hi = MAX_D + c.min(0.0) * dur;
        let delay = clampf(delay, lo, hi);
        let back = (delay * Q) as u32;
        let head = (head as u32) << 16;
        self.pos = if head >= back {
            head - back
        } else {
            head + N_Q - back
        };
        self.speed = (speed * Q) as u32;
        self.inc = 1.0 / dur;
        self.phase = (self.inc * frac).min(1.0);
        self.steal = 1.0;
        self.dec = 0.0;
        self.amp = amp;
        self.remaining = dur as i32;
    }

    /// Samples `from..to` of the block into `out`, read from `line`: in
    /// runs whose taps stay clear of the ring's ends (no wrap test a
    /// sample), and the stealing fade only while one runs.
    #[inline(always)]
    fn render(&mut self, line: &[i16; N], out: &mut [f32; BLOCK_SIZE], from: usize, to: usize) {
        let n = (to - from).min(self.remaining.max(0) as usize);
        let mut at = from;
        while at < from + n {
            let i = (self.pos >> 16) as usize;
            let want = from + n - at;
            let run = if i >= 1 && i + 2 < N {
                // Samples until the newest tap passes the ring's end.
                let room = ((((N - 2) as u32) << 16) - 1 - self.pos) / self.speed.max(1) + 1;
                want.min(room as usize)
            } else {
                0
            };
            if run == 0 {
                self.wrapped(line, &mut out[at]);
                at += 1;
            } else if self.dec == 0.0 {
                self.run::<false>(line, &mut out[at..at + run]);
                at += run;
            } else {
                self.run::<true>(line, &mut out[at..at + run]);
                at += run;
            }
        }
        self.remaining -= n as i32;
    }

    /// `out`'s samples, every tap inside the ring.
    #[inline(always)]
    fn run<const STEAL: bool>(&mut self, line: &[i16; N], out: &mut [f32]) {
        let (mut pos, speed) = (self.pos, self.speed);
        let (mut phase, inc) = (self.phase, self.inc);
        let (mut steal, dec) = (self.steal, self.dec);
        let amp = self.amp * (1.0 / super::LINE_SCALE);
        for o in out {
            let i = (pos >> 16) as usize;
            let t = (pos & 0xFFFF) as f32 * (1.0 / Q);
            // SAFETY: `render` sized the run so that every sample's taps,
            // `i − 1 ..= i + 2`, lie inside the line (`i ≥ 1`, `i + 2 < N`):
            // the head only advances, and the run ends before the newest
            // tap passes `N − 1`.
            let x = unsafe {
                [
                    *line.get_unchecked(i - 1),
                    *line.get_unchecked(i),
                    *line.get_unchecked(i + 1),
                    *line.get_unchecked(i + 2),
                ]
            }
            .map(f32::from);
            let mut g = amp * bell(phase);
            if STEAL {
                steal -= dec;
                g *= steal.max(0.0);
            }
            *o += hermite(x, t) * g;
            phase += inc;
            pos += speed;
        }
        (self.pos, self.phase, self.steal) = (pos, phase, steal);
    }

    /// One sample whose taps wrap the ring.
    fn wrapped(&mut self, line: &[i16; N], o: &mut f32) {
        let i = (self.pos >> 16) as usize;
        let t = (self.pos & 0xFFFF) as f32 * (1.0 / Q);
        let x: [f32; 4] = core::array::from_fn(|k| f32::from(line[(i + N + k - 1) % N]));
        self.steal -= self.dec;
        let g = self.amp * (1.0 / super::LINE_SCALE) * bell(self.phase) * self.steal.max(0.0);
        *o += hermite(x, t) * g;
        self.phase += self.inc;
        self.pos += self.speed;
        if self.pos >= N_Q {
            self.pos -= N_Q;
        }
    }
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

/// One block's grain settings, from the knobs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ctl {
    /// TIME, samples behind the head.
    pub delay: f32,
    /// SIZE, samples.
    pub dur: f32,
    /// DENSITY, cycles a sample.
    pub rate: f32,
    /// PITCH as a read speed.
    pub speed: f32,
    /// How far PITCH is from 0, up to a semitone: shifted grains add in
    /// power (`amp`).
    pub shift: f32,
    pub texture: f32,
    /// The crossfades, samples.
    pub instant: f32,
    pub preempt: f32,
}

impl Ctl {
    /// SIZE, DENSITY, PITCH and TEXTURE as `p` holds them (each held in
    /// range; NaN as its floor), TIME `delay` samples, at `fs`.
    pub fn new(p: &DelayParams, delay: f32, fs: u32) -> Self {
        let sr = fs as f32;
        let unit = |v: f32| if v.is_nan() { 0.0 } else { clampf(v, 0.0, 1.0) };
        let pitch = if p.pitch().is_nan() {
            0.0
        } else {
            clampf(p.pitch(), -24.0, 24.0)
        };
        Self {
            delay,
            dur: SIZE_MIN * libm::powf(SIZE_SPAN, unit(p.size())) * sr,
            rate: RATE_MIN * exp2(RATE_OCTAVES * unit(p.density())) / sr,
            speed: exp2(pitch * (1.0 / 12.0)),
            shift: pitch.abs().min(1.0),
            texture: unit(p.texture()),
            instant: INSTANT_S * sr,
            preempt: PREEMPT_S * sr,
        }
    }

    /// A grain's gain: its bells overlap `rate · dur / 2` deep on average
    /// (a bell's mean is ½), at most half the grains. Read from one place
    /// at PITCH 0 they add in phase, so TEXTURE 0 divides by the depth;
    /// sprayed, or shifted (each grain then reads the line a different
    /// distance back), they add in power, so by its root (Beads:
    /// 1/√(grains + 1), always). A semitone of PITCH blends the one into
    /// the other.
    pub fn amp(&self) -> f32 {
        let depth = clampf(self.rate * self.dur * 0.5, 1.0, GRAINS as f32 * 0.5);
        let phase = 1.0 - 0.5 * self.texture;
        let law = phase + (0.5 - phase) * self.shift;
        libm::powf(depth, -law)
    }
}

/// The grains, their clock and the samples since the last start
/// (`GranularPlayer`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Engine {
    grains: [Grain; GRAINS],
    clock: Clock,
    rng: u32,
    since: u32,
}

impl Default for Engine {
    fn default() -> Self {
        Self::NEW
    }
}

impl Engine {
    pub const NEW: Self = Self {
        grains: [Grain::IDLE; GRAINS],
        clock: Clock::NEW,
        rng: 0x9E37_79B9,
        since: 0,
    };

    pub fn grains(&self) -> &[Grain; GRAINS] {
        &self.grains
    }

    /// Grains playing.
    pub fn active(&self) -> usize {
        self.grains.iter().filter(|g| !g.done()).count()
    }

    /// The block's grains into `out` (zeroed first): the line is read, never
    /// written, so it must hold every sample up to `wp`, the write head at
    /// the block's first sample. Each start renders the grains up to it,
    /// takes the grain nearest its end, and fades the next two (Beads'
    /// sorted scheduler: the 2 nearest their end, of 6).
    pub fn block(&mut self, line: &[i16; N], wp: usize, ctl: &Ctl, out: &mut [f32; BLOCK_SIZE]) {
        *out = [0.0; BLOCK_SIZE];
        let mut starts = [(0usize, 0.0f32); MAX_STARTS];
        let n = self
            .clock
            .block(ctl.rate, ctl.texture, &mut self.rng, &mut starts);
        let amp = ctl.amp();
        let mut from = 0;
        for &(at, frac) in &starts[..n] {
            self.render(line, out, from, at);
            from = at;
            let u = uniform(&mut self.rng);
            let delay = ctl.delay * (1.0 + ctl.texture * SPRAY * (2.0 * u - 1.0));
            let k = self.nearest_end();
            self.grains[k].start((wp + at) % N, delay, ctl.speed, ctl.dur, amp, frac);
            let interval = self.since.wrapping_add(at as u32);
            self.steal_ahead(ctl, interval as f32);
            // Counted from this start: the block's end adds the rest.
            self.since = 0u32.wrapping_sub(at as u32);
        }
        self.render(line, out, from, BLOCK_SIZE);
        self.since = self.since.wrapping_add(BLOCK_SIZE as u32).min(1 << 24);
    }

    fn render(&mut self, line: &[i16; N], out: &mut [f32; BLOCK_SIZE], from: usize, to: usize) {
        if from >= to {
            return;
        }
        for g in self.grains.iter_mut().filter(|g| !g.done()) {
            g.render(line, out, from, to);
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
    /// finds a grain done, or nearly.
    fn steal_ahead(&mut self, ctl: &Ctl, interval: f32) {
        let first = self.nearest_end();
        let mut second = usize::from(first == 0);
        for (i, g) in self.grains.iter().enumerate() {
            if i != first && g.remaining < self.grains[second].remaining {
                second = i;
            }
        }
        self.grains[first].fade_out(ctl.instant);
        self.grains[second].fade_out(interval.max(ctl.instant).min(ctl.preempt));
    }
}

/// GRAIN's state beside TAPE's in `TapeDelay`: the grains, the TYPE
/// crossfade, its four knobs' eases and the feedback's filters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub(super) engine: Engine,
    /// GRAIN's share of the return, 0 (TAPE) to 1, TYPE's crossfade.
    pub(super) share: f32,
    /// The feedback's tone and DC states.
    lp: f32,
    dc: [f32; 2],
}

impl State {
    pub const NEW: Self = Self {
        engine: Engine::NEW,
        share: 0.0,
        lp: 0.0,
        dc: [0.0; 2],
    };
}

/// The door's entry (`GrainDoor::run`): one block of the delay with GRAIN
/// heard, or fading in or out against TAPE. Bank 1 calls it while TYPE is
/// GRAIN or its share is above 0.
#[inline(never)]
pub fn run(d: &mut TapeDelay, buf: &mut [f32; BLOCK_SIZE], p: &DelayParams, fs: u32, insert: bool) {
    let tap = Tap::new(p, fs);
    if !d.primed {
        (d.time, d.primed) = (tap.base(), true);
    }
    // From silence the grains start fresh: none left reading where the
    // line has since moved on.
    if d.grain.share == 0.0 {
        d.grain = State::NEW;
    }
    let to = if p.kind() == DelayType::Grain {
        1.0
    } else {
        0.0
    };
    let step = BLOCK_SIZE as f32 / f32::from(TIME_FADE);
    let s = d.grain.share;
    let share = (
        s,
        if to > s {
            (s + step).min(to)
        } else {
            (s - step).max(to)
        },
    );
    d.grain.share = share.1;
    let tape_heard = share.0 < 1.0 || share.1 < 1.0;
    if !tape_heard {
        // TAPE silent: its head waits at TIME.
        (d.time, d.fade) = (tap.base(), 0);
    } else if d.fade == 0 && tap.base() != d.time {
        (d.next, d.fade) = (tap.base(), TIME_FADE);
    }
    let mix = if p.is_on() { p.mix } else { 0.0 };
    let k = ease_coeff(fs);
    let e = Eased {
        mix: d.mix.step(mix, k),
        feedback: d.feedback.step(p.feedback, k),
        saturation: d.saturation.step(p.saturation, k),
        tone: d.tone.step(p.tone, k),
        mechanics: d.mechanics.step(p.wow_flutter, k),
    };
    let ctl = Ctl::new(p, tap.base(), fs);
    let mut g = [0.0; BLOCK_SIZE];
    d.grain.engine.block(&d.buffer, d.write_pos, &ctl, &mut g);
    span(d, buf, &g, &tap, e, share, tape_heard, insert, fs);
}

/// The block's samples: TAPE's head too while `tape` (its crossfade), each
/// return weighted by GRAIN's `share`; the line written with the dry and
/// both feedbacks so weighted.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn span(
    d: &mut TapeDelay,
    buf: &mut [f32; BLOCK_SIZE],
    g: &[f32; BLOCK_SIZE],
    tap: &Tap,
    e: Eased,
    share: (f32, f32),
    tape: bool,
    insert: bool,
    fs: u32,
) {
    let ramp = |v: (f32, f32)| (v.0, step_of(v, BLOCK_SIZE));
    let lp = |t: f32| 0.2 + t * 0.75;
    let lp_coeff = ramp((lp(e.tone.0), lp(e.tone.1)));
    let gain = |s: f32| 1.0 + s * 3.0;
    let (g0, g1) = (gain(e.saturation.0), gain(e.saturation.1));
    let sat_gain = ramp((g0, g1));
    let sat_inv = ramp((1.0 / g0, 1.0 / g1));
    let (m, f, mech, w) = (
        ramp(e.mix),
        ramp(e.feedback),
        ramp(e.mechanics),
        ramp(share),
    );
    let unit = Tap {
        wow: WOW_SAMPLES,
        flutter: FLUTTER_SAMPLES,
        ..*tap
    };
    let top = (MAX_DELAY_SAMPLES - 2) as f32;
    // `DcBlocker`'s one-pole, its state in locals.
    let r = libm::expf(-core::f32::consts::TAU * DC_HZ / fs as f32);
    let gdc = (1.0 + r) * 0.5;
    let [mut x1, mut y1] = d.grain.dc;
    let (mut transport, mut write_pos, mut tape_lp, mut glp) =
        (d.transport, d.write_pos, d.lp_state, d.grain.lp);
    let (time, next, fading) = (d.time, d.next, d.fade > 0);
    let dt = 1.0 / f32::from(TIME_FADE);
    let mut t = 1.0 - f32::from(d.fade) * dt;
    for (i, s) in buf.iter_mut().enumerate() {
        let dry = *s;
        let share = at(w.0, w.1, i);
        let c = at(lp_coeff.0, lp_coeff.1, i);
        // GRAIN's feedback: TONE's one-pole, the DC blocker, the soft clip.
        let y = g[i];
        glp += c * (y - glp);
        y1 = gdc * (glp - x1) + r * y1;
        x1 = glp;
        let gfb = soft_clip(y1);
        let (mut wet, mut fb) = (share * y, share * gfb);
        if tape {
            let ((wo, fl), depth) = (transport.offsets(&unit), at(mech.0, mech.1, i));
            let head = |base: f32| clampf(base + (wo + fl) * depth, 1.0, top);
            let mut delayed = read(&d.buffer, write_pos, head(time));
            if fading {
                delayed += (read(&d.buffer, write_pos, head(next)) - delayed) * t;
                t += dt;
            }
            tape_lp += c * (delayed - tape_lp);
            // `fast_tanh` for TAPE's tanh over the 20 ms TYPE crossfade
            // alone (within 0.024): no call in the loop.
            let sat =
                soft_clip(tape_lp * at(sat_gain.0, sat_gain.1, i)) * at(sat_inv.0, sat_inv.1, i);
            wet += (1.0 - share) * delayed;
            fb += (1.0 - share) * sat;
        }
        d.buffer[write_pos] = to_line(dry + fb * at(f.0, f.1, i));
        write_pos += 1;
        if write_pos == MAX_DELAY_SAMPLES {
            write_pos = 0;
        }
        let mix = at(m.0, m.1, i);
        // The send alone: a NaN in it never reaches the return.
        *s = if insert {
            dry * (1.0 - mix) + wet * mix
        } else {
            wet * mix
        };
    }
    (d.transport, d.write_pos, d.lp_state, d.grain.lp, d.grain.dc) =
        (transport, write_pos, tape_lp, glp, [x1, y1]);
    if fading {
        d.fade -= BLOCK_SIZE as u16;
        if d.fade == 0 {
            d.time = next;
        }
    }
}
