//! STRETCH's one door onto the grain core (`dsp::grain::core`, #749): the
//! source as a `GrainSource`, and the cloud, the core's `Engine` with what
//! STRETCH reads of it that the core keeps to itself. Nothing else in
//! STRETCH names a core type, so a re-copied core changes this file alone.

use chimera_hal::BLOCK_SIZE;

use super::units::{Samples, Speed, WinPos};
use crate::dsp::algo::math::clampf;
use crate::dsp::grain::core::{
    Engine, Grain, GrainSource, MAX_STARTS, ONE, Sched, Start, bell, hermite, q32,
};
use crate::dsp::tape_engine::read::SourceView;

/// Grain slots: four overlap at most; two more spare, so a start never
/// steals one still sounding.
const GRAINS: usize = 6;
/// `Src`'s length: 2³⁰ frames, past any source, inside `Q32`'s whole part.
const SRC_LEN: usize = 1 << 30;
/// The window's last frames ramp to silence over this many, so a grain
/// read over its end never steps.
const EDGE: f32 = 256.0;
/// A source sample's scale, 12 bits to ±1.
const SCALE: f32 = 1.0 / 2048.0;

/// The latched window read in its own frame, always forward: frame `i` is
/// window frame `i − 1` (so a grain's taps never wrap at its start), and
/// `REV` the window mirrored. Its length is far past any window, so the
/// core never wraps a head; past the window's end it reads silence.
pub struct Src<'a, const REV: bool> {
    v: SourceView<'a>,
    /// The window's frames.
    win: u32,
    /// Source frame of window frame 0; `REV` reads down from it.
    base: u32,
}

impl<'a, const REV: bool> Src<'a, REV> {
    pub fn new(v: SourceView<'a>, base: u32, win: u32) -> Self {
        Src { v, win, base }
    }

    /// The window's last `EDGE` frames ramp to silence, and past it is
    /// silence: no branch, a clamp a read.
    #[inline(always)]
    fn edge(&self, i: usize) -> f32 {
        clampf((self.win as f32 - i as f32) * (1.0 / EDGE), 0.0, 1.0)
    }

    /// Window frame `i − 1`'s source frame (past the source's ends
    /// `row_taps` and `sample` clamp).
    #[inline(always)]
    fn at(&self, i: usize) -> u32 {
        let u = (i as u32).wrapping_sub(1);
        if REV {
            self.base.saturating_sub(u)
        } else {
            self.base.wrapping_add(u)
        }
    }

    /// The source at `at`, scaled to ±1.
    pub fn read(&self, at: WinPos) -> f32 {
        let pos = q32(at.get()) + ONE;
        let t = ((pos as u32) >> 8) as f32 * (1.0 / 16_777_216.0);
        hermite(self.taps((pos >> 32) as usize), t) * SCALE
    }
}

impl<const REV: bool> GrainSource for Src<'_, REV> {
    fn frames(&self) -> usize {
        SRC_LEN
    }

    fn frame(&self, i: usize) -> f32 {
        self.v.sample(self.at(i)) as f32 * self.edge(i)
    }

    #[inline(always)]
    fn taps(&self, i: usize) -> [f32; 4] {
        let [a, b, c, d] = self.v.row_taps(self.at(i));
        let x = if REV { [d, c, b, a] } else { [a, b, c, d] }.map(|x| x as f32);
        // The edge ramp only where it is: a compare, not four multiplies.
        if i + EDGE as usize >= self.win as usize {
            let g = self.edge(i);
            x.map(|x| x * g)
        } else {
            x
        }
    }
}

/// A grain to start: where, at what speed, for how long.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrainAt {
    pub at: WinPos,
    pub speed: Speed,
    pub dur: Samples,
}

impl GrainAt {
    fn start(self) -> Start {
        Start {
            pos: q32(self.at.get()) + ONE,
            speed: self.speed.get(),
            dur: self.dur.get().max(2.0),
            amp: SCALE,
        }
    }
}

/// A block's starts: how many, and the last one's sample and grain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Starts {
    pub count: u32,
    pub last: Option<(usize, GrainAt)>,
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
/// the samples it has left), and the note's strike, a grain of its own
/// started half a bell in.
#[derive(Clone, Copy, Debug)]
pub struct Cloud {
    engine: Engine<GRAINS>,
    /// Each slot's length and the share of a sample it started late.
    durs: [(f32, f32); GRAINS],
    strike: Grain,
    /// The strike's samples to play, of a bell this long.
    strike_left: u32,
    strike_dur: f32,
}

impl Cloud {
    pub const IDLE: Cloud = Cloud {
        engine: Engine::NEW,
        durs: [(2.0, 0.0); GRAINS],
        strike: Grain::IDLE,
        strike_left: 0,
        strike_dur: 2.0,
    };

    /// A cloud whose first grain, `g`, starts at full: its bell's second
    /// half, so an attack plays whole.
    pub fn struck(g: GrainAt) -> Cloud {
        let s = g.start();
        let mut c = IDLE_CLOUD;
        c.strike.start(s, 0.5 * s.dur);
        c.strike_dur = s.dur;
        c.strike_left = (0.5 * s.dur) as u32;
        c
    }

    /// The windows' sum and the sum of their squares, now.
    pub fn windows(&self) -> (f32, f32) {
        let mut w = if self.strike_left > 0 {
            bell(1.0 - self.strike_left as f32 / self.strike_dur)
        } else {
            0.0
        };
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
        self.engine.active() > 0 || self.strike_left > 0
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
        // No steal: four grains in six slots always leave one done, so the
        // fades are set past any grain's length.
        let never = 1.0e9;
        let sched = Sched {
            rate: every.map_or(0.0, |e| 1.0 / e.get().max(1.0)),
            texture: 0.0,
            instant: never,
            preempt: never,
        };
        let before = self.engine.grains().map(|g| g.remaining());
        let mut starts = [(
            0usize,
            0.0f32,
            GrainAt {
                at: WinPos::START,
                speed: Speed::ONE,
                dur: Samples::ZERO,
            },
        ); MAX_STARTS];
        let mut n = 0;
        let quiet = every.is_none();
        self.engine.block(src, &sched, out, |k, frac, u| {
            if quiet {
                // The clock's floor rate, a start in 20 s: silent.
                return Start {
                    pos: ONE,
                    speed: 1.0,
                    dur: 2.0,
                    amp: 0.0,
                };
            }
            let g = place(k, u);
            if n < MAX_STARTS {
                starts[n] = (k, frac, g);
                n += 1;
            }
            g.start()
        });
        // Each slot that started: its start, by the samples it has left.
        for (i, g) in self.engine.grains().iter().enumerate() {
            let r = g.remaining();
            if r > 0 && r > before[i] - BLOCK_SIZE as i32 {
                let left = |s: &(usize, f32, GrainAt)| {
                    s.2.dur.get().max(2.0) as i32 - (BLOCK_SIZE - s.0) as i32
                };
                if let Some(s) = starts[..n].iter().min_by_key(|s| (left(s) - r).abs()) {
                    self.durs[i] = (s.2.dur.get().max(2.0), s.1);
                }
            }
        }
        if self.strike_left > 0 {
            let m = (self.strike_left as usize).min(BLOCK_SIZE);
            self.strike.render(src, out, 0, m);
            self.strike_left -= m as u32;
        }
        Starts {
            count: n as u32,
            last: starts[..n].last().map(|&(k, _, g)| (k, g)),
        }
    }
}

/// A core grain's bell now, from the samples it has left and its start's
/// length and lateness (`Grain::start`: phase `(frac + played) / dur`).
fn phase_bell(left: i32, (dur, frac): (f32, f32)) -> f32 {
    bell((frac + (dur as i32 - left) as f32) / dur)
}
