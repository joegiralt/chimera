//! STRETCH's GRAIN TYPE (ADR 0204): a key plays the source at its pitch, and the
//! source lasts LENGTH seconds, 0.25 to 8, at that pitch, in grains.
//!
//! A pointer `T` walks the window (START to END; END before START walks
//! it reversed) at its length ÷ LENGTH a second, times SPEED (−1× to +2×,
//! 0 freezes), so the source reaches its end at LENGTH on every key.
//! Grains of GRAIN (30–250 ms), four overlapping, read at the key's speed
//! (`inc`) through the shared core (`dsp::grain`, after Mutable
//! Instruments Beads), each centred on where `T` will be at its middle:
//! it starts `A = (inc − tstep) · GRAIN / 2` frames behind `T`, so a grain
//! sounds the source where the pointer says at any key. SPRAY starts each
//! grain up to 25 ms further back at random and draws each interval up to
//! 25 % past a quarter GRAIN (never short of it: four grains overlap at
//! most), so a bell's grains never comb.
//!
//! The note's first grain starts at full and plays its bell's second half:
//! the strike is heard whole. At the end END HOLD parks the grains on the
//! window's last GRAIN of source while the key is held, for 8 s at most;
//! STOP, and HOLD's 8 s, start no more and let the last grains fade out.
//! The note then ends; key-up is the amp envelope's.
//!
//! The level: the grains' sum is divided by their windows' sum (ramped
//! across the block from its ends), which keeps grains that read alike at
//! the source's level through a start, a steal and the attack. Grains
//! that read unlike add in power, not amplitude: the source's power at `T`
//! against the cloud's, both smoothed over 21 ms, lifts the cloud back up,
//! by at most the windows' sum over the root of their squares' (all
//! unlike). At 1× with no SPRAY every grain reads the pointer's sample and
//! the note is the source's, bit for bit but the reads' rounding.

use chimera_hal::BLOCK_SIZE;

use super::super::heads::{Dir, window_end_min, window_start};
use super::super::params::TapeEngineParams;
use super::super::read::{SourceView, WholeRow};
use super::super::{MIN_LOOP_FRAMES, Probe};
use super::classic::time_speed;
use super::cloud::{Cloud, GrainAt, Src, rng};
use super::units::{Period, Samples, Seconds, Speed, StretchEnd, WinPos};
use crate::dsp::algo::math::{clampf, exp2};
use crate::dsp::grain::core::Rng;

/// Grains overlapping: a start every GRAIN / 4 or later.
pub const OVERLAP: f32 = 4.0;

/// GRAIN's cheaper builds under measurement (ADR 0204, the owner's ask):
/// test builds only. `Four` is GRAIN as shipped.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Thrift {
    /// Four grains overlapping.
    Four = 0,
    /// Three, SPRAY hiding the wider spacing.
    Three = 1,
    /// Two at a stretch of 1×, rising to four at 3× and past.
    ByRatio = 2,
    /// One plain head within 3 % of 1× at SPRAY 0, crossfaded in and out.
    Bypass = 3,
}

#[cfg(any(test, feature = "test-support"))]
impl Thrift {
    pub const ALL: [Thrift; 4] = [Thrift::Four, Thrift::Three, Thrift::ByRatio, Thrift::Bypass];

    /// Grains overlapping at a stretch of `ratio` (the key's speed over the
    /// pointer's, 1 or more).
    fn overlap(self, ratio: f32) -> f32 {
        match self {
            Thrift::Four | Thrift::Bypass => OVERLAP,
            Thrift::Three => 3.0,
            Thrift::ByRatio => clampf(1.0 + ratio, 2.0, OVERLAP),
        }
    }
}

/// The build the tests and the listening tool pick. At `Four` (0, the
/// default) the render's arithmetic is the firmware's: the other arms
/// only add branches that `Four` never takes. Only test-support builds
/// (tests, `stretch_listen`) have it; the desktop simulator, like the
/// firmware, does not.
#[cfg(any(test, feature = "test-support"))]
pub static THRIFT: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

#[cfg(any(test, feature = "test-support"))]
#[inline(always)]
fn thrift() -> Thrift {
    let k = THRIFT.load(core::sync::atomic::Ordering::Relaxed);
    Thrift::ALL[usize::from(k).min(3)]
}

/// `Thrift::Bypass`'s plain head: where it reads, and its share of the
/// output against the grains' (0 grains, 1 the head).
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bypass {
    head: WinPos,
    mix: f32,
}

/// The bypass crossfade, samples.
#[cfg(any(test, feature = "test-support"))]
const BYPASS_FADE: f32 = 960.0;
/// END HOLD parks at most this long, seconds.
pub const HOLD_S: f32 = 8.0;
/// The level's power estimates' one-pole share a block: 21 ms at 48 kHz.
const STAT_K: f32 = 1.0 / 16.0;
/// The windows' sum under which the cloud is not divided further (only
/// as its last grains fade out, or none play).
const W_FLOOR: f32 = 0.25;
/// Reads of the source about `T` a block, for its power.
const PROBES: usize = 8;

/// The window a note latches: its first source frame, its frames (at
/// least `MIN_LOOP_FRAMES`) and its way.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Window {
    base: u32,
    len: u32,
    dir: Dir,
}

impl Window {
    /// START to END of `v` (END before START: mirrored).
    fn of(p: &TapeEngineParams, v: &SourceView<'_>) -> Window {
        let frames = v.file_frames();
        let dir = Dir::of(p.start, p.end);
        let (lo, hi) = match dir {
            Dir::Rev => (p.end, p.start),
            Dir::Fwd => (p.start, p.end),
        };
        let ws = window_start(lo, frames).min(frames.get().saturating_sub(MIN_LOOP_FRAMES));
        let we = window_end_min(ws, hi, frames.get(), MIN_LOOP_FRAMES);
        Window {
            base: if dir == Dir::Rev { we - 1 } else { ws },
            len: we - ws,
            dir,
        }
    }

    /// The source's frame at `u`.
    fn frame(self, u: WinPos) -> u32 {
        self.frame_at(u.get() as u32)
    }

    /// The source's frame at whole frame `u`.
    fn frame_at(self, u: u32) -> u32 {
        let u = u.min(self.len - 1);
        match self.dir {
            Dir::Rev => self.base.saturating_sub(u),
            Dir::Fwd => self.base + u,
        }
    }

    /// The last place a grain starts at for `dur` at `speed`, `reach` of it
    /// read inside: HOLD's park reads a whole GRAIN in, STOP's half (a
    /// grain centred on the end, its second half silence, so the source
    /// plays out to its end at LENGTH).
    fn top(self, speed: Speed, dur: Samples, end: StretchEnd) -> WinPos {
        let reach = match end {
            StretchEnd::Hold => dur.get(),
            StretchEnd::Stop => 0.5 * dur.get(),
        };
        WinPos::new(self.len as f32 - 3.0 - speed.get() * reach)
    }
}

/// The newest grain: where it started, its speed and the samples since.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Newest {
    at: WinPos,
    speed: Speed,
    since: Samples,
}

impl Newest {
    /// Its head `k` samples on.
    fn head(self, k: usize) -> WinPos {
        WinPos::new(self.at.get() + self.speed.get() * (self.since.get() + k as f32))
    }
}

/// The note's stage: playing, parked at the end (HOLD, samples so), or
/// its last grains fading (no more start).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Playing,
    Parked(Samples),
    Ending,
}

/// The level's two gains at the last block's end, and the powers that set
/// the lift.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Level {
    /// The divide by the windows' sum.
    r: f32,
    /// The lift for grains that read unlike, 1 or more.
    c: f32,
    /// The source's power about `T` and the divided cloud's, smoothed.
    ph: f32,
    uh: f32,
}

/// `Grains::IDLE`'s image: in bank 2 with the code that copies it (an
/// anonymous constant would land in bank 1, 424 B).
#[cfg_attr(feature = "bank2", unsafe(link_section = ".bank2.rodata.stretch_idle"))]
pub static IDLE_NOTE: Grains = Grains::IDLE;

/// A STRETCH note's state.
#[derive(Clone, Copy, Debug)]
pub struct Grains {
    cloud: Cloud,
    /// The pointer `T`.
    t: WinPos,
    win: Window,
    /// The lock's period on a pitched source.
    period: Option<Period>,
    newest: Newest,
    stage: Stage,
    level: Level,
    /// The next interval's draw, a factor on GRAIN / 4, 1 or more.
    jit: f32,
    rng: Rng,
    #[cfg(any(test, feature = "test-support"))]
    bypass: Bypass,
    /// Grains started: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(in super::super) events: u32,
}

impl Grains {
    pub const IDLE: Grains = Grains {
        cloud: Cloud::IDLE,
        t: WinPos::START,
        win: Window {
            base: 0,
            len: MIN_LOOP_FRAMES,
            dir: Dir::Fwd,
        },
        period: None,
        newest: Newest {
            at: WinPos::START,
            speed: Speed::ONE,
            since: Samples::ZERO,
        },
        stage: Stage::Playing,
        level: Level {
            r: 1.0,
            c: 1.0,
            ph: 0.0,
            uh: 0.0,
        },
        jit: 1.0,
        #[cfg(any(test, feature = "test-support"))]
        bypass: Bypass {
            head: WinPos::START,
            mix: 0.0,
        },
        rng: rng(1),
        #[cfg(any(test, feature = "test-support"))]
        events: 0,
    };

    /// A note with nothing to play: it ends on its first block.
    pub fn ended() -> Grains {
        Grains {
            stage: Stage::Ending,
            ..IDLE_NOTE
        }
    }

    /// A note on `v` at `inc` frames a sample, `seed` the voice's draw:
    /// the window latched from START and END, the first grain at its start,
    /// from full.
    pub fn on(p: &TapeEngineParams, v: &WholeRow<'_>, inc: f32, seed: u32, sr: u32) -> Grains {
        let win = Window::of(p, v);
        let speed = Speed::new(inc);
        let dur = p.grain().seconds().samples(sr);
        let first = GrainAt {
            at: WinPos::START,
            speed,
            dur,
        };
        let cloud = match win.dir {
            Dir::Rev => Cloud::struck(&Src::<true>::new(*v, win.base, win.len), first),
            Dir::Fwd => Cloud::struck(&Src::<false>::new(*v, win.base, win.len), first),
        };
        Grains {
            cloud,
            win,
            period: lock_period(p, v, win),
            newest: Newest {
                at: WinPos::START,
                speed,
                since: Samples::ZERO,
            },
            rng: rng(seed),
            ..IDLE_NOTE
        }
    }

    /// The source's frame `T` reads, and the newest grain's.
    pub fn probe(&self) -> Probe {
        Probe {
            head: self.win.frame(self.t),
            other: Some(self.win.frame(self.newest.head(0))),
            gain: self.cloud.newest(),
            level: 0,
            from: None,
        }
    }

    /// The way it reads.
    pub fn dir(&self) -> Dir {
        self.win.dir
    }

    /// A block at `inc` ramped `(from, to)` (every grain in play, and the
    /// strike, retuned to `to` each block: `Cloud::set_speed`), SPEED's
    /// MOTION `motion` eased, LENGTH, GRAIN, SPRAY and END read from `p`. False once the note has ended: its last
    /// grain faded out.
    pub fn render(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &WholeRow<'_>,
        inc: (f32, f32),
        motion: f32,
        p: &TapeEngineParams,
        sr: u32,
    ) -> bool {
        match self.win.dir {
            Dir::Rev => self.render_way::<true>(out, v, inc, motion, p, sr),
            Dir::Fwd => self.render_way::<false>(out, v, inc, motion, p, sr),
        }
    }

    /// `render` one way, so the grains' reads hold no test of it.
    fn render_way<const REV: bool>(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &WholeRow<'_>,
        (_, to): (f32, f32),
        motion: f32,
        p: &TapeEngineParams,
        sr: u32,
    ) -> bool {
        let src = Src::<REV>::new(*v, self.win.base, self.win.len);
        let dur = p.grain().seconds().samples(sr);
        let spray = p.spray();
        let end = p.end_mode();
        let speed = Speed::new(to);
        let tstep = pointer_speed(self.win, p.length().seconds(), motion, sr);
        let back = spray.reach() * f32::from(v.desc().rate());
        // A grain starts `ahead` behind `T`, so its middle sounds `T`.
        let ahead = (speed.get() - tstep.get()) * dur.get() * 0.5;
        #[cfg(any(test, feature = "test-support"))]
        let (per, (mix0, mix1)) = {
            let thrift = thrift();
            let ratio = speed.get() / libm::fabsf(tstep.get()).max(1e-6);
            (
                thrift.overlap(ratio.max(1.0)),
                self.bypass_mix(thrift, spray, speed, tstep),
            )
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let per = OVERLAP;
        let every = match self.stage {
            Stage::Ending => None,
            #[cfg(any(test, feature = "test-support"))]
            _ if mix0 == 1.0 && mix1 == 1.0 => None,
            // By the longest grain still sounding, not GRAIN's now: a GRAIN
            // turned short keeps four grains in play, none cut.
            _ => Some(Samples::new(
                dur.get().max(self.cloud.longest()) * self.jit / per,
            )),
        };
        let (t, win, period, newest) = (self.t, self.win, self.period, self.newest);
        let top = win.top(speed, dur, end);
        let mut rng = self.rng;
        let mut buf = [0.0; BLOCK_SIZE];
        // The grains in play follow the key's speed as the bend and the
        // wow move it (`set_speed`), not only the grains to come.
        self.cloud.set_speed(&src, speed);
        let starts = self.cloud.block(&src, every, &mut buf, |k, u| {
            // The voice's own draw beside the core's.
            let u = u + rng.uniform().get();
            let u = u - libm::floorf(u);
            let want = clampf(
                t.get() + k as f32 * tstep.get() - ahead - back * u,
                0.0,
                top.get(),
            );
            let at = match period {
                Some(p) => lock_in(newest.head(k), WinPos::new(want), p, top),
                None => WinPos::new(want),
            };
            GrainAt { at, speed, dur }
        });
        self.rng = rng;
        self.newest.since = Samples::new(self.newest.since.get() + BLOCK_SIZE as f32);
        if let Some((k, g)) = starts.last {
            self.newest = Newest {
                at: g.at,
                speed: g.speed,
                since: Samples::new((BLOCK_SIZE - k) as f32),
            };
            // Only ever later than its quarter GRAIN: four grains overlap
            // at most, so four bill the voice.
            self.jit = 1.0 + spray.jitter() * self.rng.uniform().get();
        }
        #[cfg(any(test, feature = "test-support"))]
        {
            self.events += starts.count;
        }
        self.walk(tstep, ahead, top, end, sr);
        self.level(out, &buf, &src, speed.get() * dur.get());
        #[cfg(any(test, feature = "test-support"))]
        self.bypass_out(out, &src, speed, (mix0, mix1));
        let ended = self.stage == Stage::Ending && !self.cloud.sounding();
        #[cfg(any(test, feature = "test-support"))]
        let ended = ended && mix1 == 0.0;
        !ended
    }

    /// `Thrift::Bypass`'s share of the plain head over the block, from and
    /// to; the head latched at `T` as it fades in from none.
    #[cfg(any(test, feature = "test-support"))]
    fn bypass_mix(
        &mut self,
        thrift: Thrift,
        spray: super::units::Spray,
        speed: Speed,
        tstep: Speed,
    ) -> (f32, f32) {
        // Within 3 % of 1× and no SPRAY the plain head plays (Bypass).
        let near = thrift == Thrift::Bypass
            && spray.share() == 0.0
            && libm::fabsf(tstep.get() / speed.get() - 1.0) < 0.03
            && self.stage == Stage::Playing;
        let mix0 = self.bypass.mix;
        if near && mix0 == 0.0 {
            self.bypass.head = self.t;
        }
        let mix1 = clampf(
            mix0 + if near { 1.0 } else { -1.0 } * BLOCK_SIZE as f32 / BYPASS_FADE,
            0.0,
            1.0,
        );
        (mix0, mix1)
    }

    /// The plain head, crossfaded with the grains in `out`.
    #[cfg(any(test, feature = "test-support"))]
    fn bypass_out<const REV: bool>(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        src: &Src<REV>,
        speed: Speed,
        (mix0, mix1): (f32, f32),
    ) {
        if mix0 > 0.0 || mix1 > 0.0 {
            let dm = (mix1 - mix0) * (1.0 / BLOCK_SIZE as f32);
            let (mut m, mut h) = (mix0, self.bypass.head.get());
            for o in out.iter_mut() {
                m += dm;
                *o = *o * (1.0 - m) + src.read(WinPos::new(h)) * m;
                h += speed.get();
            }
            self.bypass.head = WinPos::new(h);
        }
        self.bypass.mix = mix1;
    }

    /// The pointer a block on at `tstep`: held at the start, or at the end
    /// parked (HOLD, 8 s at most) or ending (STOP).
    fn walk(&mut self, tstep: Speed, ahead: f32, top: WinPos, end: StretchEnd, sr: u32) {
        let mut tn = self.t.get() + BLOCK_SIZE as f32 * tstep.get();
        if tn - ahead >= top.get() {
            tn = top.get() + ahead;
            self.stage = match (self.stage, end) {
                (Stage::Ending, _) | (_, StretchEnd::Stop) => Stage::Ending,
                (Stage::Parked(s), StretchEnd::Hold) if s.get() >= HOLD_S * sr as f32 => {
                    Stage::Ending
                }
                (Stage::Parked(s), StretchEnd::Hold) => {
                    Stage::Parked(Samples::new(s.get() + BLOCK_SIZE as f32))
                }
                (Stage::Playing, StretchEnd::Hold) => Stage::Parked(Samples::ZERO),
            };
        }
        self.t = WinPos::new(tn);
    }

    /// `out` from the cloud `buf`: divided by the windows' sum, ramped from
    /// the last block's end to this one's, and lifted by the power the
    /// grains lose reading unlike (both held as the last grains fade out).
    fn level<const REV: bool>(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        buf: &[f32; BLOCK_SIZE],
        src: &Src<'_, REV>,
        span: f32,
    ) {
        let ending = self.stage == Stage::Ending;
        let (w, q) = self.cloud.windows();
        let l = self.level;
        let r1 = if ending { l.r } else { 1.0 / w.max(W_FLOOR) };
        let e: f32 = buf.iter().map(|x| x * x).sum();
        let rm = 0.5 * (l.r + r1);
        let u = e * rm * rm * (1.0 / BLOCK_SIZE as f32);
        // The source's power about `T`: reads across what a grain there
        // covers (`span` frames), at a fresh offset each block, so a frozen
        // pointer samples the wave at every phase, not its one point.
        let step = span * (1.0 / PROBES as f32);
        let o = self.rng.uniform().get();
        let room = self.win.len as f32 - 3.0;
        let mut pw = 0.0;
        for i in 0..PROBES {
            let at = self.t.get() + (i as f32 + o - 0.5 * PROBES as f32) * step;
            let x = src.read(WinPos::new(at.min(room)));
            pw += x * x;
        }
        let ph = l.ph + STAT_K * (pw * (1.0 / PROBES as f32) - l.ph);
        // The cloud's power rises at once (grains back in phase must never
        // be lifted past the source), falls over 21 ms.
        let uh = if u > l.uh {
            u
        } else {
            l.uh + STAT_K * (u - l.uh)
        };
        let most = if q > 0.0 {
            (w / libm::sqrtf(q)).max(1.0)
        } else {
            1.0
        };
        let c1 = if ending {
            l.c
        } else if uh > 1e-12 && ph.is_finite() {
            clampf(libm::sqrtf(ph / uh), 1.0, most)
        } else {
            l.c.min(most)
        };
        let (g0, g1) = (l.r * l.c, r1 * c1);
        let dg = (g1 - g0) * (1.0 / BLOCK_SIZE as f32);
        let mut g = g0;
        for (o, x) in out.iter_mut().zip(buf) {
            g += dg;
            *o = x * g;
        }
        self.level = Level {
            r: r1,
            c: c1,
            ph,
            uh,
        };
    }
}

/// The pointer's speed: the window over LENGTH, times SPEED.
fn pointer_speed(win: Window, length: Seconds, motion: f32, sr: u32) -> Speed {
    Speed::new(win.len as f32 / (length.get() * sr as f32) * time_speed(motion))
}

/// A pitched source's lock (YIN found a PERIOD): its PERIOD, or ROOT's
/// period where the two are whole multiples within 3 %, the note it sounds,
/// so a bell's loudest partial meets itself in phase where YIN read its hum
/// an octave down, a little off. With no PERIOD, the window's own where its
/// first frames repeat (`period::of`), as found: a chord's common period
/// is a multiple of its root's, and ROOT's would put its fifth out of
/// phase. Two periods fit the window, or none.
fn lock_period(p: &TapeEngineParams, v: &WholeRow<'_>, win: Window) -> Option<Period> {
    let Some(yin) = Period::new(p.period) else {
        let rate = u32::from(v.desc().rate());
        return super::period::of(v, rate, win.len, |u| win.frame_at(u));
    };
    let root =
        Period::new(f32::from(v.desc().rate()) / (440.0 * exp2((p.root - 6_900.0) / 1_200.0)));
    let near = |a: f32, b: f32| {
        let m = libm::roundf(a / b);
        m >= 1.0 && libm::fabsf(a / b - m) <= 0.03 * m
    };
    let pick = match root {
        Some(r) if near(yin.get(), r.get()) || near(r.get(), yin.get()) => r,
        _ => yin,
    };
    (2.0 * pick.get() <= win.len as f32).then_some(pick)
}

/// A pitched grain's start: as near `want` as a whole number of periods
/// from the newest grain's head `h` allows, inside `[0, top]` (a period
/// moved back in if the nearest is out), so its partials meet the others'
/// in phase.
fn lock_in(h: WinPos, want: WinPos, period: Period, top: WinPos) -> WinPos {
    let (h, at, p, top) = (h.get(), want.get(), period.get(), top.get());
    let mut u = super::classic::lock(h, at, p);
    if u > top {
        u -= p * libm::ceilf((u - top) / p);
    }
    if u < 0.0 {
        u += p * libm::ceilf(-u / p);
    }
    if (0.0..=top).contains(&u) {
        WinPos::new(u)
    } else {
        want
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lock moves a start by whole periods only, inside its bounds.
    #[test]
    fn lock_is_whole_periods() {
        let p = Period::new(109.3).unwrap();
        let top = WinPos::new(10_000.0);
        for (h, at) in [(5_000.25, 1_234.5), (100.0, 9_000.75), (777.7, 777.0)] {
            let u = lock_in(WinPos::new(h), WinPos::new(at), p, top).get();
            let n = (h - u) / p.get();
            assert!((n - libm::roundf(n)).abs() < 1e-3, "{h}, {at}: {n}");
            assert!((u - at).abs() <= 0.5 * p.get() + 1e-3);
        }
    }
}
