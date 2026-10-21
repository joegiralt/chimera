//! GRAIN, the delay's second TYPE (ADR 0202): repeats made of grains read
//! from the delay's own line by the grain core (`dsp::grain::core`, after
//! Mutable Instruments Beads, MIT, its notice there). This module is the
//! delay's side: the line as a `GrainSource`, the knobs' laws, where a
//! grain starts behind the write head (`DelayGrain::Start`'s trims), its
//! gain, the feedback path and the TYPE crossfade against TAPE.
//!
//! Ported from Beads here: a grain's start (the sub-sample offset, the
//! duration trimmed to the line, the delay held off the write head) and
//! the feedback's soft clip (`stmlib::SoftClip`, `fast_tanh`) and DC
//! blocker. Not ported, and why, in ADR 0202.
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
use crate::dsp::ease::{at, ease_coeff, step_of};
pub use crate::dsp::grain::core::{
    At, Clock, Engine, Frames, Gain, Grain, GrainLen, GrainSource, MAX_STARTS, Pos, Rate, Rng,
    Samples, Sched, Speed, Start, TapAt, Trigger, bell, hermite, log_one_minus_x,
};
use crate::dsp::{Unit, clampf};
use chimera_hal::BLOCK_SIZE;

const N: usize = MAX_DELAY_SAMPLES;

/// Grains at once (Beads' delay player: 32): six keep an ALGO INIT's
/// voices on rev Y but one (ADR 0202).
pub const GRAINS: usize = 6;
/// The nearest a grain reads to the write head, samples: the block is read
/// before it is written, so a grain at sample 63 must still find its
/// Hermite's newest tap written (63 + 3), and two to spare.
pub const MIN_D: Frames = Frames::new((BLOCK_SIZE + 4) as f32);
/// The farthest: the line less the Hermite's oldest tap, and two to spare.
pub const MAX_D: Frames = Frames::new((N - 4) as f32);

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
    const FRAMES: usize = N;

    #[inline(always)]
    fn frame(&self, i: At<Self>) -> f32 {
        // SAFETY: an `At<Line>` is below `FRAMES`, the line's length.
        f32::from(unsafe { *self.0.get_unchecked(i.get()) })
    }

    #[inline(always)]
    fn taps(&self, i: TapAt<Self>) -> [f32; 4] {
        let i = i.get();
        // SAFETY: a `TapAt<Line>` has `1 ≤ i` and `i + 2 < FRAMES`, the
        // line's length, so `i − 1 ..= i + 2` lie inside it.
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
    /// TIME, behind the head.
    pub delay: Frames,
    /// SIZE.
    pub dur: Samples,
    /// DENSITY.
    pub rate: Rate,
    /// PITCH as a read speed.
    pub speed: Speed,
    /// How far PITCH is from 0, up to a semitone: shifted grains add in
    /// power (`amp`).
    pub shift: Unit,
    pub texture: Unit,
    /// The crossfades.
    pub instant: Samples,
    pub preempt: Samples,
}

impl Ctl {
    /// SIZE, DENSITY, PITCH and TEXTURE as `p` holds them (each held in
    /// range; NaN as its floor), TIME `delay` behind the head, at `fs`.
    pub fn new(p: &DelayParams, delay: Frames, fs: u32) -> Self {
        let pitch = if p.pitch().is_nan() {
            0.0
        } else {
            clampf(p.pitch(), -24.0, 24.0)
        };
        Self {
            delay,
            dur: Samples::secs(
                SIZE_MIN * libm::powf(SIZE_SPAN, Unit::new(p.size()).get()),
                fs,
            ),
            rate: Rate::hz(
                RATE_MIN * exp2(RATE_OCTAVES * Unit::new(p.density()).get()),
                fs,
            ),
            speed: Speed::of(exp2(pitch * (1.0 / 12.0))),
            shift: Unit::new(pitch.abs()),
            texture: Unit::new(p.texture()),
            instant: Samples::secs(INSTANT_S, fs),
            preempt: Samples::secs(PREEMPT_S, fs),
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

    /// The bells' mean overlap, `rate · dur / 2` (a bell's mean is ½), at
    /// most half the grains. TEXTURE's random intervals start grains
    /// faster than DENSITY's train: Beads' draw, blended in by TEXTURE,
    /// averages `1 + t(0.74 − 0.61t)` of its rate (within 3 % above).
    fn depth(&self) -> f32 {
        let t = self.texture.get();
        let busy = 1.0 + t * (0.74 - 0.61 * t);
        clampf(
            self.rate.get() * busy * self.dur.get() * 0.5,
            1.0,
            GRAINS as f32 * 0.5,
        )
    }

    /// How the grains add: in phase read from one place (TEXTURE 0, PITCH
    /// 0), in power sprayed or shifted (each grain then reads the line a
    /// different distance back); a semitone of PITCH blends the one into
    /// the other.
    fn law(&self) -> f32 {
        let phase = 1.0 - 0.5 * self.texture.get();
        phase + (0.5 - phase) * self.shift.get()
    }

    /// A grain's gain: the depth to the power of `law` (Beads:
    /// 1/√(grains + 1), always in power).
    pub fn amp(&self) -> Gain {
        Gain::new(libm::powf(self.depth(), -self.law()))
    }

    /// The feedback's share of the return, `depth^(law − 1)`: the loop
    /// counts the grains in phase whatever `law` assumes, as a low tone
    /// under a small spray, or a shifted regular train's own lines, add;
    /// so FDBK under 1 always dies away.
    pub fn fb_trim(&self) -> Unit {
        Unit::new(libm::powf(self.depth(), self.law() - 1.0))
    }

    /// The grain trigger `t` starts (its sample in the block, its fraction
    /// late), the write head at `wp` on the block's first sample, `u`
    /// TEXTURE's spray (`DelayGrain::Start`): TIME behind the head, the
    /// length trimmed so the line holds it, the delay held so the head
    /// never crosses the write head.
    pub fn start(&self, wp: usize, t: Trigger, u: Unit) -> Start {
        let (tex, speed) = (self.texture.get(), self.speed.ratio());
        let delay = self.delay.get() * (1.0 + tex * SPRAY * (2.0 * u.get() - 1.0));
        let c = speed - 1.0;
        let delay = delay - t.frac().get() * c;
        let (min_d, max_d) = (MIN_D.get(), MAX_D.get());
        let dur = if c != 0.0 {
            self.dur.get().min((max_d - min_d) / c.abs())
        } else {
            self.dur.get()
        };
        let len = GrainLen::new(Samples::new(dur));
        let lo = min_d + c.max(0.0) * len.get();
        let hi = max_d + c.min(0.0) * len.get();
        let back = Frames::new(clampf(delay, lo, hi));
        Start {
            pos: Pos::behind(wp + t.at(), back, N),
            speed: self.speed,
            len,
            amp: Gain::new(self.amp().get() * (1.0 / LINE_SCALE)),
        }
    }
}

/// GRAIN's state beside TAPE's in `TapeDelay`: the grains, the TYPE
/// crossfade, its four knobs' eases and the feedback's filters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub(super) engine: Engine<GRAINS>,
    /// GRAIN's share of the return, 0 (TAPE) to 1, TYPE's crossfade.
    pub(super) share: Unit,
    /// The feedback's tone and DC states.
    lp: f32,
    dc: [f32; 2],
}

impl State {
    pub const NEW: Self = Self {
        engine: Engine::NEW,
        share: Unit::ZERO,
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
    if d.grain.share == Unit::ZERO {
        d.grain = State::NEW;
    }
    let to = if p.kind() == DelayType::Grain {
        1.0
    } else {
        0.0
    };
    let step = BLOCK_SIZE as f32 / f32::from(TIME_FADE);
    let s = d.grain.share.get();
    let share = (
        s,
        if to > s {
            (s + step).min(to)
        } else {
            (s - step).max(to)
        },
    );
    d.grain.share = Unit::new(share.1);
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
    let ctl = Ctl::new(p, Frames::new(tap.base()), fs);
    let mut g = [0.0; BLOCK_SIZE];
    let wp = d.write_pos;
    let line = Line(&d.buffer);
    d.grain
        .engine
        .block(&line, &ctl.sched(), &mut g, |t, u| ctl.start(wp, t, u));
    span(
        d,
        buf,
        &g,
        ctl.fb_trim().get(),
        &tap,
        e,
        share,
        tape_heard,
        insert,
        fs,
    );
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
    trim: f32,
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
        // GRAIN's feedback: its trim, TONE's one-pole, the DC blocker, the
        // soft clip.
        let y = g[i];
        glp += c * (y * trim - glp);
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
