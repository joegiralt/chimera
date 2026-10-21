//! GRAIN, the delay's second TYPE (ADR 0199): repeats made of grains read
//! from the delay's own line by the grain core (`dsp::grain::core`, after
//! Mutable Instruments Beads, MIT, its notice there). This module is the
//! delay's side: the line as a `GrainSource`, the knobs' laws, where a
//! grain starts behind the write head (`DelayGrain::Start`'s trims), its
//! gain, the feedback path and the TYPE crossfade against TAPE.
//!
//! Ported from Beads here: a grain's start (the sub-sample offset, the
//! duration trimmed to the line, the delay held off the write head) and
//! the feedback's soft clip (`stmlib::SoftClip`, `fast_tanh`) and DC
//! blocker. Not ported, and why, in ADR 0199.
//!
//! With `bank2` this module links into bank 2 whole (`bank2.x`,
//! `*5delay5grain*`, and the core's `*3dsp5grain*`); bank 1 reaches `run`
//! only through `GrainDoor`. With the door closed GRAIN plays as TAPE.
//
// The start's trims are Beads' (Copyright 2017 Emilie Gillet), under the
// MIT License; the notice is `dsp::grain::core`'s.

use super::{
    DelayParams, DelayType, Eased, FLUTTER_SAMPLES, LINE_SCALE, MAX_DELAY_SAMPLES, TIME_FADE, Tap,
    TapeDelay, WOW_SAMPLES, read, to_line,
};
use crate::dsp::algo::math::exp2;
use crate::dsp::clampf;
use crate::dsp::ease::{at, ease_coeff, step_of};
pub use crate::dsp::grain::core::{
    Clock, Engine, Grain, GrainSource, MAX_STARTS, ONE, Q32, Sched, Start, bell, hermite,
    log_one_minus_x, q32, uniform,
};
use chimera_hal::BLOCK_SIZE;

const N: usize = MAX_DELAY_SAMPLES;

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

/// `stmlib::SoftClip` (Beads' feedback limit): already ported, ADR 0097.
pub use crate::dsp::fast_tanh as soft_clip;

/// The delay's line as a grain source, at its `i16` scale (`Ctl::start`'s
/// gain takes `LINE_SCALE` off).
pub struct Line<'a>(pub &'a [i16; N]);

impl GrainSource for Line<'_> {
    #[inline(always)]
    fn frames(&self) -> usize {
        N
    }

    #[inline(always)]
    fn frame(&self, i: usize) -> f32 {
        f32::from(self.0[i])
    }

    #[inline(always)]
    fn taps(&self, i: usize) -> [f32; 4] {
        // SAFETY: the core asks only with `1 ≤ i` and `i + 2 < N`
        // (`GrainSource::taps`), so `i − 1 ..= i + 2` lie inside the line.
        unsafe {
            [
                *self.0.get_unchecked(i - 1),
                *self.0.get_unchecked(i),
                *self.0.get_unchecked(i + 1),
                *self.0.get_unchecked(i + 2),
            ]
        }
        .map(f32::from)
    }
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

    /// The clock's settings.
    pub fn sched(&self) -> Sched {
        Sched {
            rate: self.rate,
            texture: self.texture,
            instant: self.instant,
            preempt: self.preempt,
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

    /// The grain a trigger at sample `at` of the block starts, `frac` of a
    /// sample late, the write head at `wp` on the block's first sample, `u`
    /// TEXTURE's spray (`DelayGrain::Start`): TIME behind the head, the
    /// length trimmed so the line holds it, the delay held so the head
    /// never crosses the write head.
    pub fn start(&self, wp: usize, at: usize, frac: f32, u: f32) -> Start {
        let delay = self.delay * (1.0 + self.texture * SPRAY * (2.0 * u - 1.0));
        let c = self.speed - 1.0;
        let delay = delay - frac * c;
        let dur = if c != 0.0 {
            self.dur.min((MAX_D - MIN_D) / c.abs())
        } else {
            self.dur
        }
        .max(2.0);
        let lo = MIN_D + c.max(0.0) * dur;
        let hi = MAX_D + c.min(0.0) * dur;
        let back = q32(clampf(delay, lo, hi));
        let head = (((wp + at) % N) as Q32) * ONE;
        let pos = if head >= back {
            head - back
        } else {
            head + (N as Q32) * ONE - back
        };
        Start {
            pos,
            speed: self.speed,
            dur,
            amp: self.amp() * (1.0 / LINE_SCALE),
        }
    }
}

/// GRAIN's state beside TAPE's in `TapeDelay`: the grains, the TYPE
/// crossfade, its four knobs' eases and the feedback's filters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub(super) engine: Engine<GRAINS>,
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
    let wp = d.write_pos;
    let line = Line(&d.buffer);
    d.grain
        .engine
        .block(&line, &ctl.sched(), &mut g, |at, frac, u| {
            ctl.start(wp, at, frac, u)
        });
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
