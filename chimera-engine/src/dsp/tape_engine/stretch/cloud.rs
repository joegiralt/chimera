//! STRETCH's one door onto the grain core (`dsp::grain::core`, #749): the
//! source as a `GrainSource`, and the cloud, the core's `Engine` with what
//! STRETCH reads of it that the core keeps to itself. Nothing else in
//! STRETCH names a core type, so a re-copied core changes this file alone.

use core::num::NonZeroU32;

use chimera_hal::BLOCK_SIZE;

use super::units::{Samples, Speed, WinPos};
use crate::dsp::Unit;
use crate::dsp::algo::math::clampf;
use crate::dsp::grain::core::{
    At, Engine, Frames, Gain, GrainLen, GrainSource, MAX_STARTS, Pos, Rate, Rng, Sched, Start,
    TapAt, bell, hermite,
};
use crate::dsp::grain::core::{Samples as CoreSamples, Speed as CoreSpeed};
use crate::dsp::tape_engine::read::SourceView;

/// Grain slots: four overlap at most; two more spare, so a start never
/// steals one still sounding.
const GRAINS: usize = 6;
/// `Src`'s length: 2³⁰ frames, past any source, inside the core's Q32.32.
const SRC_LEN: usize = 1 << 30;
/// The window's lowest frame sits at `MID` of `Src`, so no head wraps
/// either way: forward past the window's top or backward past its foot.
const MID: i64 = 1 << 29;
/// The window's last frames (the way it plays) ramp to silence over this
/// many, so a grain read over its end never steps.
const EDGE: f32 = 256.0;
/// A source sample's scale, 12 bits to ±1.
const SCALE: f32 = 1.0 / 2048.0;

/// A voice's draws from its seed, made odd so never 0.
pub const fn rng(seed: u32) -> Rng {
    Rng::seeded(NonZeroU32::MIN.saturating_add(seed & !1))
}

/// The latched window, read forward: `Src` frame `MID + f` is the
/// window's frame `f` up from its lowest source frame; `REV` plays it
/// from its top down, its grains at a negative speed. Past the window's
/// end, the way it plays, it reads silence.
pub struct Src<'a, const REV: bool> {
    v: SourceView<'a>,
    /// The window's frames.
    win: u32,
    /// Its lowest source frame.
    lo: u32,
}

impl<'a, const REV: bool> Src<'a, REV> {
    /// The window of `win` frames from `base`, its first frame the way it
    /// plays (`REV`'s top).
    pub fn new(v: SourceView<'a>, base: u32, win: u32) -> Self {
        let lo = if REV {
            (base + 1).saturating_sub(win)
        } else {
            base
        };
        Src { v, win, lo }
    }

    /// `at`, the way it plays, as a forward frame.
    #[inline(always)]
    fn forward(&self, at: WinPos) -> f32 {
        if REV {
            (self.win - 1) as f32 - at.get()
        } else {
            at.get()
        }
    }

    /// Forward frame `f`'s edge: the last `EDGE` frames the way it plays
    /// ramp to silence, and past them is silence; a clamp, no branch.
    #[inline(always)]
    fn edge(&self, f: i64) -> f32 {
        let to_end = if REV {
            f as f32
        } else {
            (i64::from(self.win) - 1 - f) as f32
        };
        clampf(to_end * (1.0 / EDGE), 0.0, 1.0)
    }

    /// Forward frame `f` is within `EDGE` of the end it plays to.
    #[inline(always)]
    fn near_end(&self, f: i64) -> bool {
        if REV {
            f <= EDGE as i64
        } else {
            f + EDGE as i64 >= i64::from(self.win) - 1
        }
    }

    /// Forward frame `f`'s source frame (past the source's ends `row_taps`
    /// and `sample` clamp).
    #[inline(always)]
    fn source(&self, f: i64) -> u32 {
        (i64::from(self.lo) + f).clamp(0, i64::from(u32::MAX)) as u32
    }

    /// Forward frames `f − 1 ..= f + 2`, the edge ramp only where it is:
    /// a compare, not four multiplies.
    #[inline(always)]
    fn taps_at(&self, f: i64) -> [f32; 4] {
        let x = self.v.row_taps(self.source(f)).map(|x| x as f32);
        if self.near_end(f) {
            let g = self.edge(f);
            x.map(|x| x * g)
        } else {
            x
        }
    }

    /// Forward frame `f` and `t` of the next, scaled to ±1.
    #[inline(always)]
    fn at(&self, f: i64, t: f32) -> f32 {
        hermite(self.taps_at(f), t) * SCALE
    }

    /// The source at `at`, the way it plays, scaled to ±1.
    pub fn read(&self, at: WinPos) -> f32 {
        let f = self.forward(at);
        let w = libm::floorf(f);
        self.at(w as i64, f - w)
    }

    /// `g` as the core starts it: its head at `g.at` the way the window
    /// plays, read forward, so `REV`'s speed is negative.
    fn start(&self, g: GrainAt) -> Start {
        Start {
            pos: head(self.forward(g.at)),
            speed: self.speed(g.speed),
            len: GrainLen::new(CoreSamples::new(g.dur.get())),
            amp: Gain::new(SCALE),
        }
    }

    /// `s` the way the window plays.
    fn speed(&self, s: Speed) -> CoreSpeed {
        CoreSpeed::of(if REV { -s.get() } else { s.get() })
    }
}

/// `Src`'s place of forward frame `f`: its whole frame exact, its
/// fraction to `f32`'s precision.
fn head(f: f32) -> Pos {
    let w = libm::ceilf(f);
    Pos::behind((MID + w as i64) as usize, Frames::new(w - f), SRC_LEN)
}

impl<const REV: bool> GrainSource for Src<'_, REV> {
    const FRAMES: usize = SRC_LEN;

    fn frame(&self, i: At<Self>) -> f32 {
        let f = i.get() as i64 - MID;
        self.v.sample(self.source(f)) as f32 * self.edge(f)
    }

    #[inline(always)]
    fn taps(&self, i: TapAt<Self>) -> [f32; 4] {
        self.taps_at(i.get() as i64 - MID)
    }
}

/// A grain to start: where, at what speed, for how long.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrainAt {
    pub at: WinPos,
    pub speed: Speed,
    pub dur: Samples,
}

/// A block's starts: how many, and the last one's sample and grain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Starts {
    pub count: u32,
    pub last: Option<(usize, GrainAt)>,
}

/// The note's strike: its first grain from full, its bell's second half,
/// read forward from a whole frame and its fraction (the core starts a
/// bell at its foot only).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Strike {
    whole: i64,
    frac: f32,
    /// Forward frames a sample: negative on `REV`.
    step: f32,
    /// Samples to play, of a bell `dur` long.
    left: u32,
    dur: f32,
}

impl Strike {
    const NONE: Strike = Strike {
        whole: 0,
        frac: 0.0,
        step: 0.0,
        left: 0,
        dur: 2.0,
    };

    /// Its bell now.
    fn window(&self) -> f32 {
        if self.left > 0 {
            bell(1.0 - self.left as f32 / self.dur)
        } else {
            0.0
        }
    }

    /// Its next samples into `out`, as many as it has left.
    fn render<const REV: bool>(&mut self, src: &Src<'_, REV>, out: &mut [f32]) {
        let n = out.len().min(self.left as usize);
        let inc = 1.0 / self.dur;
        let mut phase = 1.0 - self.left as f32 * inc;
        for o in &mut out[..n] {
            *o += src.at(self.whole, self.frac) * bell(phase);
            phase += inc;
            self.frac += self.step;
            let carry = libm::floorf(self.frac);
            self.whole += carry as i64;
            self.frac -= carry;
        }
        self.left -= n as u32;
    }
}

/// `Cloud::IDLE`'s image: in bank 2 with the code that copies it (an
/// anonymous constant would land in bank 1).
#[cfg_attr(
    feature = "bank2",
    unsafe(link_section = ".bank2.rodata.stretch_cloud")
)]
static IDLE_CLOUD: Cloud = Cloud::IDLE;

/// The grains: the core's engine, each slot's start as it was set (the
/// core keeps its bell's phase to itself, so a window is read back from
/// the samples it has left), and the note's strike.
#[derive(Clone, Copy, Debug)]
pub struct Cloud {
    engine: Engine<GRAINS>,
    /// Each slot's length and the share of a sample it started late.
    durs: [(f32, f32); GRAINS],
    strike: Strike,
}

impl Cloud {
    pub const IDLE: Cloud = Cloud {
        engine: Engine::NEW,
        durs: [(2.0, 0.0); GRAINS],
        strike: Strike::NONE,
    };

    /// A cloud whose first grain, `g` on `src`, starts at full: its bell's
    /// second half, so an attack plays whole.
    pub fn struck<const REV: bool>(src: &Src<'_, REV>, g: GrainAt) -> Cloud {
        let dur = g.dur.get().max(2.0);
        let f = src.forward(g.at);
        let w = libm::floorf(f);
        let mut c = IDLE_CLOUD;
        c.strike = Strike {
            whole: w as i64,
            frac: f - w,
            step: src.speed(g.speed).ratio(),
            left: (0.5 * dur) as u32,
            dur,
        };
        c
    }

    /// Every grain, the strike's too, at `s` from now: the key's speed as
    /// the bend and the wow move it, not only at a grain's start.
    pub fn set_speed<const REV: bool>(&mut self, src: &Src<'_, REV>, s: Speed) {
        let s = src.speed(s);
        self.engine.set_speed(s);
        self.strike.step = s.ratio();
    }

    /// The windows' sum and the sum of their squares, now.
    pub fn windows(&self) -> (f32, f32) {
        let mut w = self.strike.window();
        let mut q = w * w;
        for (g, &d) in self.engine.grains().iter().zip(&self.durs) {
            if !g.done() {
                let x = phase_bell(g.remaining(), d);
                w += x;
                q += x * x;
            }
        }
        (w, q)
    }

    /// The newest grain's window.
    pub fn newest(&self) -> f32 {
        self.engine
            .grains()
            .iter()
            .zip(&self.durs)
            .filter(|(g, _)| !g.done())
            .max_by_key(|(g, _)| g.remaining())
            .map_or(0.0, |(g, &d)| phase_bell(g.remaining(), d))
    }

    /// A grain or the strike still sounds.
    pub fn sounding(&self) -> bool {
        self.engine.active() > 0 || self.strike.left > 0
    }

    /// One block into `out` (zeroed first): a start every `every` samples
    /// (`None`, none), each placed by `place(sample, draw)`, `draw` the
    /// core's uniform one.
    pub fn block<const REV: bool>(
        &mut self,
        src: &Src<'_, REV>,
        every: Option<Samples>,
        out: &mut [f32; BLOCK_SIZE],
        mut place: impl FnMut(usize, f32) -> GrainAt,
    ) -> Starts {
        // No steal: four grains in six slots always leave one done.
        let sched = Sched {
            rate: every.map_or(Rate::ZERO, |e| Rate::hz(1.0 / e.get().max(1.0), 1)),
            texture: Unit::ZERO,
            steal: None,
        };
        let mut placed: [Option<GrainAt>; MAX_STARTS] = [None; MAX_STARTS];
        let mut n = 0;
        let started = self.engine.block(src, &sched, out, |t, u| {
            // No rate: the clock's floor still triggers, and starts none.
            every?;
            let g = place(t.at(), u.get());
            if let Some(p) = placed.get_mut(n) {
                *p = Some(g);
            }
            n += 1;
            Some(src.start(g))
        });
        let started = started.as_slice();
        let each = || started.iter().zip(placed.iter().flatten());
        for (&(t, slot), g) in each() {
            self.durs[slot.get()] = (g.dur.get().max(2.0), t.frac().get());
        }
        self.strike.render(src, out);
        Starts {
            count: started.len() as u32,
            last: each().last().map(|(&(t, _), &g)| (t.at(), g)),
        }
    }
}

/// A core grain's bell now, from the samples it has left and its start's
/// length and lateness (`Grain::start`: phase `(frac + played) / len`).
fn phase_bell(left: u32, (dur, frac): (f32, f32)) -> f32 {
    bell((frac + (dur as i64 - i64::from(left)) as f32) / dur)
}
