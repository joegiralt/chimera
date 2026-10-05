//! TAPE, the sample engine (spec 2026-10-04-tape-core-design, ADR 0093):
//! 12-bit sources held in the pool's arena, played by RAM models.
//!
//! LOOP (Digitakt II manual § Playback modes): the head starts at START,
//! plays forward to the loop's end, then loops under a crossfade that folds
//! inside the loop when it must (`heads`). MOTION walks the loop through the
//! window (Morphagene manual, Organize).
//!
//! REVERSE (Digitakt II manual § Playback modes, REVERSE LOOP) is LOOP
//! mirrored: the head starts at END and runs back. PING-PONG (Polyend
//! Tracker v1.5 § Instrument) turns at each edge with no crossfade, slowing
//! to a stop there over 1 ms so the wave has no corner; new bounds latch at
//! a turn, and a head far outside them moves under a 5 ms fade. SCRUB
//! (Morphagene manual, Slide and Vari-Speed) loops behind a finger that
//! follows POSITION through DRAG; MOTION is its vari-speed. TRANSWAVE and
//! STRETCH play as LOOP until their kernels land.

pub mod heads;
pub mod params;
pub mod read;

use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::dsp::algo::math::{clampf, exp2};
use crate::dsp::ease::Ramp;
use crate::dsp::glide::Glide;
use heads::{
    Bounds, Fade, Ghost, Head, Plan, Turn, at, plan, shortest, window_end_min, window_start,
};
use read::{Pos, SourceView};

pub use heads::loop_len;
pub use params::{StretchType, TAPE_ENGINE_SPECS, TapeEngineParams, TapeModel, drag_s};

/// `inc`'s clamp: four octaves over a 48 kHz source's own speed.
pub const INC_MAX: f32 = 16.0;
/// A note's first samples ramp from 0 (1 ms), so a START inside a waveform
/// never steps.
pub const NOTE_ON_RAMP: u32 = 48;
/// PING-PONG's clamp fade: a head outside new bounds moves inside over 5 ms.
pub const CLAMP_FADE_S: f32 = 0.005;
/// PING-PONG's turn: the head slows to a stop at an edge over 1 ms and comes
/// back over 1 ms, as a reel does, so the wave has no corner there.
pub const TURN_S: f32 = 0.001;
/// The shortest loop and window: 2 ms of the source, and at least
/// `MIN_LOOP_FRAMES` (or the whole source, if shorter).
pub const MIN_LOOP_S: f32 = 0.002;
/// Four samples' travel at `INC_MAX`: a loop's events come at most one in 4
/// samples, so a pass's arithmetic never runs every sample.
pub const MIN_LOOP_FRAMES: u32 = 4 * INC_MAX as u32;
/// A loop is at least this many output samples of travel at the block's
/// fastest `inc` (`MIN_PASS_SAMPLES · inc` frames): a pass, or a turn,
/// comes at most once in 16 samples, so a pass's events and calls never
/// dominate a block (spec § 2). At `inc` 16 a loop is at least 256 frames.
pub const MIN_PASS_SAMPLES: u32 = 16;
/// SCRUB's tape stops under this speed, frames a sample (loop and finger
/// both), and starts again over twice it: its output is silent while
/// stopped (spec § 2 SCRUB).
pub const STILL: f32 = 1e-3;
/// A mid-note stop or start eases SCRUB's output over 30 ms, smoothstep, so
/// the held value leaves through the DC blocker without a tick.
pub const HUSH_S: f32 = 0.030;
/// The macros' share of the way each block: MODAL's (ADR 0056).
const EASE: f32 = 0.3;

const _: () = assert!(NOTE_ON_RAMP as usize <= BLOCK_SIZE);

/// Events one sample handles: a put-off fade's start, then the fade's start
/// and its wrap, then the new pass's fade, which is one pass of an ordinary
/// loop. Only a loop shorter than a sample's step needs more; it takes them
/// the next sample, its head held in reach by the splice's modulo.
const MAX_EVENTS: usize = 4;

/// Frames a head advances each output sample:
/// `rate / sample_rate · 2^((note·100 + pitch_cents − root) / 1200)`,
/// clamped to `[ε, INC_MAX]` so a head never stands still.
pub fn inc(rate: u16, note: u8, pitch_cents: f32, root: f32, sample_rate: u32) -> f32 {
    let oct = clampf(
        (note as f32 * 100.0 + pitch_cents - root) / 1_200.0,
        -40.0,
        40.0,
    );
    clampf(
        rate as f32 / sample_rate as f32 * exp2(oct),
        f32::EPSILON,
        INC_MAX,
    )
}

/// POSITION, MOTION, TAPE, AGE and SIZE as the heads play them: eased
/// toward the block's modulated values by `EASE` a block, taken whole at a
/// note's first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Macros {
    pub position: f32,
    pub motion: f32,
    pub tape: f32,
    pub age: f32,
    pub size: f32,
}

impl Macros {
    fn of(p: &TapeEngineParams) -> Self {
        Self {
            position: clampf(p.position, 0.0, 1.0),
            motion: clampf(p.motion, -1.0, 1.0),
            tape: clampf(p.tape, 0.0, 1.0),
            age: clampf(p.age, 0.0, 1.0),
            size: clampf(p.size, 0.0, 1.0),
        }
    }

    fn ease(&mut self, to: &Self) {
        for (x, t) in [
            (&mut self.position, to.position),
            (&mut self.motion, to.motion),
            (&mut self.tape, to.tape),
            (&mut self.age, to.age),
            (&mut self.size, to.size),
        ] {
            *x += EASE * (t - *x);
        }
    }
}

/// What the strip and the tests read of a playing note.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    /// The first head's frame.
    pub head: u32,
    /// The second head's, while one plays.
    pub other: Option<u32>,
    /// The second head's share of the fade.
    pub gain: f32,
    /// TRANSWAVE's `log2 N`.
    pub level: u8,
}

/// What a wrap or a turn reads from the block's params: END (START, for
/// REVERSE's mirrored window) and XFADE latch there.
#[derive(Clone, Copy)]
struct AtWrap {
    end: f32,
    xfade: f32,
    frames: u32,
    rate: u16,
}

#[derive(Clone, Copy)]
pub struct TapeEngine {
    model: TapeModel,
    /// The head, and the head a fade reads into, in the frame `u`.
    heads: [Head; 2],
    /// The frame is mirrored: the source is read at `off − u`, else at `u`
    /// (`off + u` in SCRUB).
    mirror: bool,
    /// REVERSE: the source's last frame. PING-PONG down: `ls + le`. SCRUB:
    /// the finger, plus the source's length when mirrored.
    off: Pos,
    /// SCRUB: the finger's move each sample (wrapping), and its change a
    /// sample: it ramps across the block, as `inc` does.
    df: u64,
    ddf: u64,
    /// SCRUB: the tape is stopped; the output's share, linear before its
    /// smoothstep, and its move a block (`HUSH_S`).
    stopped: bool,
    hush: f32,
    hush_step: f32,
    /// SCRUB: the loop's speed in frames a sample at the last block's end,
    /// signed; DRAG's one-pole share a block.
    speed: f32,
    pull: f32,
    /// The source's frames, latched at note-on.
    n: u32,
    /// PING-PONG's clamp in progress, and its turn.
    ghost: Option<Ghost>,
    turning: Option<Turn>,
    /// `inc` at the block's end and its reciprocal, a turn's longest
    /// half (samples), a clamp's length (samples) and the distance (frames)
    /// a head outside covers in one: PING-PONG's, set once a block.
    v: f32,
    inv_v: f32,
    turn_len: u16,
    clamp_len: u32,
    clamp_dg: f32,
    reach: f32,
    /// Events handled: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    events: u32,
    /// Frames a sample, ramped across the block.
    inc: Ramp,
    mac: Macros,
    /// The note's first block is to come: the macros are taken whole.
    first: bool,
    /// MOTION's walk, in windows: [0, 1).
    walk: f32,
    /// The window's start, latched at note-on (REVERSE: mirrored).
    ws: u32,
    /// The pass in play.
    bounds: Bounds,
    /// The shortest loop this block, frames: `heads::shortest` at its
    /// fastest head.
    min_len: u32,
    /// The next pass's bounds, worked out once a block from its params;
    /// latched where its fade was put off.
    next: Bounds,
    pending: Option<Bounds>,
    fade: Option<Fade>,
    /// The first head's frame at which the next fade, wrap or turn is
    /// handled.
    event: u32,
    /// The second head's share, last sample.
    gain: f32,
    /// The note-on ramp is to come.
    ramp: bool,
    /// The last sample out: a note-on ramps from it, so a retrigger never
    /// steps.
    last: f32,
    note: u8,
    /// PIT's PITCH and FINE with their routes, cents.
    pitch: f32,
    /// A glide steal's ratio, gliding to 1.
    slide: Glide,
    /// ROOT, latched at note-on.
    root: f32,
    active: bool,
}

impl TapeEngine {
    fn idle() -> TapeEngine {
        TapeEngine {
            model: TapeModel::Loop,
            heads: [Head { pos: Pos(0) }; 2],
            mirror: false,
            off: Pos(0),
            df: 0,
            ddf: 0,
            stopped: false,
            hush: 1.0,
            hush_step: 1.0,
            speed: 0.0,
            pull: 1.0,
            n: 2,
            ghost: None,
            turning: None,
            v: 1.0,
            inv_v: 1.0,
            turn_len: 48,
            clamp_len: 240,
            clamp_dg: 1.0 / 240.0,
            reach: 241.0,
            #[cfg(any(test, feature = "test-support"))]
            events: 0,
            inc: Ramp::default(),
            mac: Macros {
                position: 0.0,
                motion: 0.0,
                tape: 0.0,
                age: 0.0,
                size: 1.0,
            },
            first: false,
            walk: 0.0,
            ws: 0,
            bounds: Bounds {
                ls: 0,
                le: 1,
                lx: 0,
            },
            min_len: MIN_LOOP_FRAMES,
            next: Bounds {
                ls: 0,
                le: 1,
                lx: 0,
            },
            pending: None,
            fade: None,
            event: 0,
            gain: 0.0,
            ramp: false,
            last: 0.0,
            note: 60,
            pitch: 0.0,
            slide: Glide::new(1.0, 1.0),
            root: 6_000.0,
            active: false,
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        slot.write(Self::idle())
    }

    /// The voice's PITCH and FINE with their routes, in cents, for the next
    /// `note_on` or `render`.
    pub fn set_pitch(&mut self, cents: f32) {
        self.pitch = cents;
    }

    /// MODEL, START, ROOT and DRAG latch here; the head starts at START
    /// (REVERSE: at END).
    pub fn note_on(
        &mut self,
        note: u8,
        p: &TapeEngineParams,
        v: &SourceView<'_>,
        sample_rate: u32,
    ) {
        let (frames, rate) = (v.frames(), v.desc().rate);
        self.model = p.model;
        self.note = note;
        self.root = p.root;
        self.slide = Glide::new(1.0, 1.0);
        self.inc.hold();
        self.mac = Macros::of(p);
        self.first = true;
        self.walk = 0.0;
        self.n = frames;
        self.ghost = None;
        self.turning = None;
        self.turn_len = clampf(libm::roundf(TURN_S * sample_rate as f32), 2.0, 1_000.0) as u16;
        self.clamp_len = libm::roundf(CLAMP_FADE_S * sample_rate as f32).max(1.0) as u32;
        self.clamp_dg = 1.0 / self.clamp_len as f32;
        self.df = 0;
        self.ddf = 0;
        self.hush = 1.0;
        self.stopped = false;
        self.hush_step = BLOCK_SIZE as f32 / (HUSH_S * sample_rate as f32);
        self.speed = 0.0;
        self.pull = 1.0 - libm::expf(-(BLOCK_SIZE as f32) / (drag_s(p.drag) * sample_rate as f32));
        let (start, end) = self.window(p);
        self.ws = window_start(start, frames);
        // The first pass's floor, at the note's own `inc` (SCRUB: its loop's
        // vari-speed).
        let mut fast = inc(rate, note, self.pitch, self.root, sample_rate);
        if self.model == TapeModel::Scrub {
            fast = (2.0 * libm::fabsf(clampf(p.motion, -1.0, 1.0)) * fast).min(INC_MAX);
        }
        self.min_len = shortest(rate, fast);
        let we = window_end_min(self.ws, end, frames, self.min_len);
        let xfade = if self.model == TapeModel::PingPong {
            0.0
        } else {
            p.xfade
        };
        self.mirror = false;
        self.off = Pos(0);
        let mut start_at = self.ws;
        match self.model {
            TapeModel::Scrub => {
                self.bounds = Bounds::of_min(
                    0,
                    we - self.ws,
                    (self.mac.size, 0.0, xfade),
                    rate,
                    self.min_len,
                );
                self.off = self.finger_target(p.position, we);
                start_at = 0;
                if p.motion < 0.0 {
                    self.mirror = true;
                    self.bounds = self.bounds.mirror(frames);
                    self.off = self.off + at(frames);
                    start_at = self.bounds.ls;
                }
            }
            model => {
                self.bounds = Bounds::of_min(
                    self.ws,
                    we,
                    (self.mac.size, self.mac.position, xfade),
                    rate,
                    self.min_len,
                );
                if model == TapeModel::Reverse {
                    self.mirror = true;
                    self.off = at(frames - 1);
                }
            }
        }
        self.heads = [Head { pos: at(start_at) }; 2];
        self.next = self.bounds;
        self.pending = None;
        self.fade = None;
        self.gain = 0.0;
        self.event = self.bounds.fade_at();
        self.ramp = true;
        self.active = true;
    }

    /// START and END as the model plays them: REVERSE's window is mirrored,
    /// so END is its start.
    fn window(&self, p: &TapeEngineParams) -> (f32, f32) {
        if self.model == TapeModel::Reverse {
            (1.0 - p.end, 1.0 - p.start)
        } else {
            (p.start, p.end)
        }
    }

    /// SCRUB's finger for POSITION `position`, in a window ending at `we`:
    /// where the loop behind it fits in the window.
    fn finger_target(&self, position: f32, we: u32) -> Pos {
        let room = (we - self.ws).saturating_sub(self.bounds.frames());
        Pos::from_f32(self.ws as f32 + clampf(position, 0.0, 1.0) * room as f32)
    }

    /// `note_on` on a sounding engine (STEAL GLIDE, ADR 0065): the heads
    /// play on, and `inc` glides from the pitch it sounds to `note`'s. On an
    /// idle engine, a plain `note_on`.
    pub fn glide_on(
        &mut self,
        note: u8,
        p: &TapeEngineParams,
        v: &SourceView<'_>,
        (sample_rate, tau): (u32, f32),
    ) {
        if self.active {
            self.glide_to(note, (sample_rate, tau));
        } else {
            self.note_on(note, p, v, sample_rate);
        }
    }

    /// `inc` glides to `note`'s from the pitch it sounds, a one-pole of
    /// `tau` seconds in log pitch. Idle, or on `note`, nothing moves.
    pub fn glide_to(&mut self, note: u8, (_sample_rate, tau): (u32, f32)) {
        if !self.active || note == self.note {
            return;
        }
        let from = exp2((self.note as f32 - note as f32) / 12.0) * self.slide.value();
        self.slide = Glide::new(from, tau);
        self.slide.toward(1.0);
        self.note = note;
    }

    /// Nothing: a loop has no end, so the note ends when its VCA lets go.
    pub fn note_off(&mut self) {}

    /// A fresh note is to come on an idle voice: its ramp starts from 0, not
    /// from the last note's sample (as `Ramp::hold`).
    pub fn hold(&mut self) {
        self.last = 0.0;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The model latched at the last note-on.
    pub fn model(&self) -> TapeModel {
        self.model
    }

    /// The source frame a head at `u` reads; one before frame 0 reads 0.
    fn frame(&self, u: Pos) -> u32 {
        let r = if self.mirror {
            self.off - u
        } else if self.model == TapeModel::Scrub {
            self.off + u
        } else {
            u
        };
        let w = r.whole();
        if w > i32::MAX as u32 { 0 } else { w }
    }

    pub fn probe(&self) -> Probe {
        let first = self.frame(self.heads[0].pos);
        match self.ghost {
            // The head left outside fades out; the one inside fades in.
            Some(g) => Probe {
                head: g.pos.whole().min(self.n),
                other: Some(first),
                gain: self.gain,
                level: 0,
            },
            None => Probe {
                head: first,
                other: self.fade.map(|_| self.frame(self.heads[1].pos)),
                gain: self.gain,
                level: 0,
            },
        }
    }

    /// Events handled since `init_in_place`: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn events(&self) -> u32 {
        self.events
    }

    /// The eased macros: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn macros(&self) -> Macros {
        self.mac
    }

    pub fn render(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &TapeEngineParams,
        v: &SourceView<'_>,
        sample_rate: u32,
    ) {
        if !self.active {
            out.fill(0.0);
            self.last = 0.0;
            return;
        }
        let to = Macros::of(p);
        let first = core::mem::take(&mut self.first);
        if first {
            self.mac = to;
        } else {
            self.mac.ease(&to);
        }
        let w = self.walk + self.mac.motion * BLOCK_SIZE as f32 / sample_rate as f32;
        self.walk = w - libm::floorf(w);

        let rate = v.desc().rate;
        self.slide.tick();
        let target = clampf(
            inc(rate, self.note, self.pitch, self.root, sample_rate) * self.slide.value(),
            f32::EPSILON,
            INC_MAX,
        );
        let (mut from, mut to) = self.inc.step(target);
        if self.model == TapeModel::PingPong {
            self.v = to;
            self.inv_v = 1.0 / to;
            self.reach = to * self.clamp_len as f32 + 1.0;
            if self.turning.is_none() {
                self.event = self.pp_event();
            }
        }
        let (_, end) = self.window(p);
        let k = AtWrap {
            end,
            xfade: p.xfade,
            frames: v.frames(),
            rate,
        };
        let scrub = self.model == TapeModel::Scrub;
        if scrub {
            (from, to) = self.scrub_block(p, to, first, &k);
        }
        // Every event this block latches these: worked out once.
        // The floor follows the block's fastest head, so it moves only with
        // `inc`, and the bounds it sets latch at a wrap or a turn, under the
        // crossfades every other change goes through.
        self.min_len = shortest(rate, from.max(to));
        self.next = self.next_bounds(&k);
        if scrub {
            // The finger's speed at the block's ends, frames a sample.
            let end =
                (self.df as i64).wrapping_add((self.ddf as i64).wrapping_mul(BLOCK_SIZE as i64));
            let finger = (self.df as i64).unsigned_abs().max(end.unsigned_abs()) as f32
                * (1.0 / 4_294_967_296.0);
            let speed = from.max(to).max(finger);
            // Hysteresis: stop under STILL, start over twice it.
            self.stopped = if self.stopped && !first {
                speed <= 2.0 * STILL
            } else {
                speed < STILL
            };
        }
        let (from, to) = (Pos::from_f32(from).0, Pos::from_f32(to).0);
        let step = (to.wrapping_sub(from) as i64 / BLOCK_SIZE as i64) as u64;

        let (mut i, mut cur) = (0, from);
        while i < BLOCK_SIZE {
            let c = (i, cur, step);
            (i, cur) = match (self.ghost.is_some(), self.turning.is_some()) {
                (true, _) => self.run_slow::<true>(out, v, c, &k),
                (false, true) => self.run_slow::<false>(out, v, c, &k),
                (false, false) => match (self.mirror, scrub) {
                    (false, false) => self.run::<false, false>(out, v, c, &k),
                    (true, false) => self.run::<true, false>(out, v, c, &k),
                    (false, true) => self.run::<false, true>(out, v, c, &k),
                    (true, true) => self.run::<true, true>(out, v, c, &k),
                },
            };
        }
        if scrub {
            self.hush_block(out, first);
        }

        if core::mem::take(&mut self.ramp) {
            let last = self.last;
            for (i, y) in out[..NOTE_ON_RAMP as usize].iter_mut().enumerate() {
                *y = last + (*y - last) * (i as f32 / NOTE_ON_RAMP as f32);
            }
        }
        self.last = out[BLOCK_SIZE - 1];
    }

    /// SCRUB's block (spec § 2 SCRUB): the loop's speed is MOTION's
    /// vari-speed, −2 to +2 times the note's; turning round, it ramps to 0
    /// first and the frame mirrors the block after. The finger follows
    /// POSITION through DRAG's one-pole, its speed bounded so the head's
    /// never passes `INC_MAX`. Returns the loop's `(from, to)` speed.
    fn scrub_block(
        &mut self,
        p: &TapeEngineParams,
        inc: f32,
        first: bool,
        k: &AtWrap,
    ) -> (f32, f32) {
        let want = 2.0 * self.mac.motion * inc;
        if self.speed == 0.0 && want != 0.0 && (want < 0.0) != self.mirror {
            self.turn_scrub();
        }
        let ahead = if self.mirror { -want } else { want };
        let to = if ahead < 0.0 { 0.0 } else { ahead.min(INC_MAX) };
        let from = if first { to } else { libm::fabsf(self.speed) };
        self.speed = if self.mirror { -to } else { to };

        let we = window_end_min(self.ws.min(k.frames - 1), p.end, k.frames, self.min_len);
        let at_n = if self.mirror { at(self.n) } else { Pos(0) };
        let finger = self.off - at_n;
        let target = self.finger_target(p.position, we);
        // The finger's speed ramps from the last block's to this one's, so
        // the head's speed is linear across the block, and bounded at
        // both ends, so everywhere in it.
        let df_to = if first {
            self.off = target + at_n;
            self.df = 0;
            0
        } else {
            let gap = (target.0 as i64).wrapping_sub(finger.0 as i64) as f32;
            let most = (INC_MAX - from.max(to)) * 4_294_967_296.0;
            clampf(self.pull * gap / BLOCK_SIZE as f32, -most, most) as i64
        };
        self.ddf = (df_to.wrapping_sub(self.df as i64) / BLOCK_SIZE as i64) as u64;
        (from, to)
    }

    /// SCRUB's stopped tape is silent. A note-on into a stopped tape is
    /// silent from its first sample; a mid-note stop or start eases the
    /// output over `HUSH_S`, a smoothstep, ramped linearly across each block.
    /// The heads play on underneath, so nothing jumps when it comes back.
    fn hush_block(&mut self, out: &mut [f32; BLOCK_SIZE], first: bool) {
        let to = if self.stopped { 0.0 } else { 1.0 };
        if first {
            self.hush = to;
        }
        let x0 = self.hush;
        let x1 = if self.stopped {
            (x0 - self.hush_step).max(0.0)
        } else {
            (x0 + self.hush_step).min(1.0)
        };
        self.hush = x1;
        let (g0, g1) = (x0 * x0 * (3.0 - 2.0 * x0), x1 * x1 * (3.0 - 2.0 * x1));
        if g0 == 1.0 && g1 == 1.0 {
            return;
        }
        if g0 == 0.0 && g1 == 0.0 {
            out.fill(0.0);
            return;
        }
        let dg = (g1 - g0) * (1.0 / BLOCK_SIZE as f32);
        let mut g = g0;
        for y in out.iter_mut() {
            g += dg;
            *y *= g;
        }
    }

    /// SCRUB's loop turns round: the frame mirrors about the source's
    /// length. A fade in progress runs back the way it came, so the head it
    /// faded from takes over where the fade began.
    #[cold]
    fn turn_scrub(&mut self) {
        let n = at(self.n);
        let [h0, h1] = self.heads.map(|h| h.pos);
        self.off = if self.mirror {
            self.off - n
        } else {
            self.off + n
        };
        self.mirror = !self.mirror;
        self.pending = None;
        match self.fade.take() {
            None => {
                self.heads[0].pos = n - h0;
                self.bounds = self.bounds.mirror(self.n);
                self.event = self.bounds.fade_at();
            }
            Some(f) => {
                // The fade runs back to where it began: the head it faded
                // from (`h0`) takes over there. Its end is rounded up to a
                // whole frame, as a wrap's is, and its slope set so the
                // gain carries on from this sample's.
                let left = f.end - h0;
                let left = left.whole() as f32 + left.frac();
                let g = clampf(1.0 - left * f.inv, 0.0, 1.0);
                // A fade with no slope (one reversed at a share of 0) has no
                // way back to run: it ends at the next whole frame.
                let back = if f.inv > 0.0 {
                    Pos::from_f32((1.0 / f.inv - left).max(0.0))
                } else {
                    Pos(0)
                };
                let h = n - h1;
                let whole = (1u64 << 32) - 1;
                let mut end = Pos((h + back).0.wrapping_add(whole) & !whole);
                if end.whole() <= h.whole() {
                    end = at(h.whole() + 1);
                }
                let span = end - h;
                self.heads = [Head { pos: h }, Head { pos: n - h0 }];
                self.fade = Some(Fade {
                    end,
                    inv: g / (span.whole() as f32 + span.frac()),
                    next: self.bounds.mirror(self.n),
                });
                self.bounds = f.next.mirror(self.n);
                self.gain = 1.0 - g;
                self.event = end.whole();
            }
        }
    }

    /// Samples of one head, or of a fade's two, each run up to the next
    /// event, in a frame fixed at compile time: `MIRROR` reads at
    /// `off − u`, `MOVING` (SCRUB) moves `off` by `df` a sample, `df`
    /// ramped by `ddf`. REVERSE and PING-PONG down track the read
    /// position `r = off − u` itself, with the event and the fade's end
    /// moved into it. The heads live in locals; the state is written back
    /// only around an event and at the end. Returns where it stopped: the
    /// block's end, or an event that changed the frame or started a
    /// slow run.
    #[inline(never)]
    fn run<const MIRROR: bool, const MOVING: bool>(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        (mut i, mut cur, step): (usize, u64, u64),
        k: &AtWrap,
    ) -> (usize, u64) {
        let [mut h0, mut h1] = self.heads.map(|h| h.pos);
        let (mut ev, mut g, mut off, mut df) = (self.event, self.gain, self.off, self.df);
        let ddf = self.ddf;
        while i < BLOCK_SIZE {
            let mut hit = false;
            if MIRROR && !MOVING {
                // In `r`: `h0 ≥ ev` is `r0 ≤ off − ev`, signed.
                let mut r0 = off - h0;
                let rev = (off.0 as i64).wrapping_sub((ev as i64) << 32);
                match self.fade {
                    None => {
                        while i < BLOCK_SIZE {
                            out[i] = v.read(r0);
                            cur = cur.wrapping_add(step);
                            r0 = r0 - Pos(cur);
                            i += 1;
                            if r0.0 as i64 <= rev {
                                hit = true;
                                break;
                            }
                        }
                    }
                    Some(f) => {
                        // The second head reads `e` behind the first.
                        let rend = off - f.end;
                        let e = h1 - h0;
                        while i < BLOCK_SIZE {
                            let a = v.read(r0);
                            let rem = r0 - rend;
                            g = 1.0 - (rem.whole() as f32 + rem.frac()) * f.inv;
                            out[i] = a + g * (v.read(r0 - e) - a);
                            cur = cur.wrapping_add(step);
                            r0 = r0 - Pos(cur);
                            i += 1;
                            if r0.0 as i64 <= rev {
                                hit = true;
                                break;
                            }
                        }
                        h1 = off - r0 + e;
                    }
                }
                h0 = off - r0;
            } else if MOVING {
                // SCRUB tracks its read positions too. Each moves by the
                // loop's step and the finger's together, `sr`, which ramps
                // as both do. `h0` stays, for the event and the fade's gain.
                let mut r0 = if MIRROR { off - h0 } else { off + h0 };
                let (mut sr, srs) = if MIRROR {
                    (df.wrapping_sub(cur), ddf.wrapping_sub(step))
                } else {
                    (df.wrapping_add(cur), ddf.wrapping_add(step))
                };
                match self.fade {
                    None => {
                        while i < BLOCK_SIZE {
                            out[i] = v.read(r0);
                            cur = cur.wrapping_add(step);
                            sr = sr.wrapping_add(srs);
                            r0 = r0 + Pos(sr);
                            h0 = h0 + Pos(cur);
                            i += 1;
                            if h0.whole() >= ev {
                                hit = true;
                                break;
                            }
                        }
                    }
                    Some(f) => {
                        let mut r1 = if MIRROR { off - h1 } else { off + h1 };
                        while i < BLOCK_SIZE {
                            let a = v.read(r0);
                            g = f.gain(h0);
                            // The loop crawls near MOTION 0, where a linear
                            // fade's corners stand out: smoothstep.
                            g = g * g * (3.0 - 2.0 * g);
                            out[i] = a + g * (v.read(r1) - a);
                            cur = cur.wrapping_add(step);
                            sr = sr.wrapping_add(srs);
                            r0 = r0 + Pos(sr);
                            r1 = r1 + Pos(sr);
                            h0 = h0 + Pos(cur);
                            i += 1;
                            if h0.whole() >= ev {
                                hit = true;
                                break;
                            }
                        }
                        h1 = if MIRROR { r0 + h0 - r1 } else { r1 - r0 + h0 };
                    }
                }
                df = if MIRROR {
                    sr.wrapping_add(cur)
                } else {
                    sr.wrapping_sub(cur)
                };
                off = if MIRROR { r0 + h0 } else { r0 - h0 };
            } else {
                // LOOP, and PING-PONG going up: the frame is the source's.
                match self.fade {
                    None => {
                        while i < BLOCK_SIZE {
                            out[i] = v.read(h0);
                            cur = cur.wrapping_add(step);
                            h0 = h0 + Pos(cur);
                            i += 1;
                            if h0.whole() >= ev {
                                hit = true;
                                break;
                            }
                        }
                    }
                    Some(f) => {
                        while i < BLOCK_SIZE {
                            let a = v.read(h0);
                            g = f.gain(h0);
                            out[i] = a + g * (v.read(h1) - a);
                            cur = cur.wrapping_add(step);
                            h0 = h0 + Pos(cur);
                            h1 = h1 + Pos(cur);
                            i += 1;
                            if h0.whole() >= ev {
                                hit = true;
                                break;
                            }
                        }
                    }
                }
            }
            if hit {
                self.heads = [Head { pos: h0 }, Head { pos: h1 }];
                (self.gain, self.off) = (g, off);
                if MOVING {
                    self.df = df;
                }
                self.on_event(k);
                if self.mirror != MIRROR || self.ghost.is_some() || self.turning.is_some() {
                    return (i, cur);
                }
                [h0, h1] = self.heads.map(|h| h.pos);
                (ev, g, off) = (self.event, self.gain, self.off);
            }
        }
        self.heads = [Head { pos: h0 }, Head { pos: h1 }];
        (self.gain, self.off) = (g, off);
        if MOVING {
            self.df = df;
        }
        (i, cur)
    }

    /// PING-PONG's slow samples: turns (`turning`), where the head slows to
    /// a stop at the edge and comes back, its step scaled by a Q16
    /// `|2·th − 2k − 1| · unit`; and a clamp (`GHOST`), where the head left
    /// outside plays on its way while the output fades, by the sample, to
    /// the head inside. Turns keep their bounds while a clamp fades.
    ///
    /// A short loop turns every few samples, so turns, flips and the next
    /// turn's start run here, one after another, without returning. It
    /// returns at the block's end, when a clamp starts or ends (`GHOST`
    /// changes), or when a turn ends with the next edge beyond the block
    /// (the plain run takes the pass).
    #[cold]
    #[inline(never)]
    fn run_slow<const GHOST: bool>(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        (mut i, mut cur, step): (usize, u64, u64),
        k: &AtWrap,
    ) -> (usize, u64) {
        let dg = self.clamp_dg;
        let mut gh = self.ghost.unwrap_or(Ghost {
            pos: Pos(0),
            back: false,
            done: 0,
            len: 1,
        });
        let mut g = self.gain;
        while i < BLOCK_SIZE {
            let (mirror, off) = (self.mirror, self.off);
            // The head's read position, moving down in a mirrored frame.
            let mut r = self.real(self.heads[0].pos);
            let turning = self.turning;
            let mut t = turning.unwrap_or(Turn {
                k: 0,
                th: 1,
                unit: 0,
            });
            // A step's scale in Q16: the distance from the stop, falling to
            // it (where the half ends) and rising after it, `2 · unit` a
            // sample either way.
            let num = 2 * t.th as i64 - 2 * t.k as i64 - 1;
            let mut fq = num.unsigned_abs() * t.unit as u64;
            let dfq = if num > 0 {
                (2 * t.unit as u64).wrapping_neg()
            } else {
                2 * t.unit as u64
            };
            // Samples until the turn's half ends or the clamp does.
            let mut left = BLOCK_SIZE - i;
            if turning.is_some() {
                let to = if t.k < t.th { t.th } else { 2 * t.th };
                left = left.min((to - t.k) as usize);
            }
            if GHOST {
                left = left.min((gh.len - gh.done) as usize);
            }
            let (start, stop) = (i, i + left);
            // `h ≥ event`, in `r`.
            let ev = (self.event as i64) << 32;
            let rev = (off.0 as i64).wrapping_sub(ev);
            let mut hit = false;
            while i < stop {
                let mut y = v.read(r);
                if GHOST {
                    g += dg;
                    let a = v.read(gh.pos);
                    y = a + g * (y - a);
                }
                out[i] = y;
                cur = cur.wrapping_add(step);
                let mut s = Pos(cur);
                if turning.is_some() {
                    s = Pos((cur >> 16) * fq);
                    fq = fq.wrapping_add(dfq);
                }
                r = if mirror { r - s } else { r + s };
                if GHOST {
                    gh.pos = if gh.back { gh.pos - s } else { gh.pos + s };
                }
                i += 1;
                if turning.is_none()
                    && (if mirror {
                        r.0 as i64 <= rev
                    } else {
                        r.0 as i64 >= ev
                    })
                {
                    hit = true;
                    break;
                }
            }
            let n = (i - start) as u32;
            let h = if mirror { off - r } else { r };
            self.heads[0].pos = h;
            if GHOST {
                gh.done += n;
                if gh.done >= gh.len {
                    // The clamp is done: the plain run takes over.
                    self.ghost = None;
                    self.gain = 0.0;
                    if turning.is_some() {
                        t.k += n as u16;
                        self.turning = Some(t);
                        self.end_half(t);
                    }
                    if hit || (turning.is_some() && self.turning.is_none()) {
                        self.on_event(k);
                    }
                    return (i, cur);
                }
            }
            if turning.is_some() {
                t.k += n as u16;
                self.turning = Some(t);
                if self.end_half(t) && !GHOST && self.ghost.is_some() {
                    // A clamp started at the flip.
                    return (i, cur);
                }
                if self.turning.is_none() && !GHOST {
                    // The turn is over: on to the next edge, here if it is
                    // within the block, else in the plain run.
                    let to = self.event.saturating_sub(self.heads[0].pos.whole());
                    if to as f32 > self.v * (BLOCK_SIZE - i) as f32 {
                        return (i, cur);
                    }
                }
            }
            if hit || (turning.is_some() && self.turning.is_none()) {
                self.on_event(k);
                if !GHOST && self.ghost.is_some() {
                    return (i, cur);
                }
            }
        }
        if GHOST {
            self.ghost = Some(gh);
            self.gain = g;
        }
        (i, cur)
    }

    /// A turn's half may have ended at `t`: at rest at the edge the head
    /// comes back (`flip_at`; true); at the end of the turn, the next edge's
    /// event is set.
    fn end_half(&mut self, t: Turn) -> bool {
        if t.k == t.th {
            let r = self.real(self.heads[0].pos);
            self.flip_at(r);
            true
        } else {
            if t.k == 2 * t.th {
                self.turning = None;
                self.event = self.pp_event();
            }
            false
        }
    }

    /// The next pass's bounds from the eased macros, the walk, and END and
    /// XFADE now. SCRUB's loop is `[0, Ll)` behind the finger.
    fn next_bounds(&self, k: &AtWrap) -> Bounds {
        let ws = self.ws.min(k.frames - 1);
        let we = window_end_min(ws, k.end, k.frames, self.min_len);
        match self.model {
            TapeModel::Scrub => {
                let b = Bounds::of_min(
                    0,
                    we - ws,
                    (self.mac.size, 0.0, k.xfade),
                    k.rate,
                    self.min_len,
                );
                if self.mirror { b.mirror(self.n) } else { b }
            }
            model => {
                let mut place = self.mac.position + self.walk;
                if place > 1.0 {
                    place -= 1.0;
                }
                let xfade = if model == TapeModel::PingPong {
                    0.0
                } else {
                    k.xfade
                };
                Bounds::of_min(ws, we, (self.mac.size, place, xfade), k.rate, self.min_len)
            }
        }
    }

    /// The head reached `event`: a fade starts (or is put off), a fade
    /// ends at the wrap, a pass with no fade splices, or PING-PONG turns.
    /// A loop shorter than a sample's step may need more than one.
    #[cold]
    #[inline(never)]
    fn on_event(&mut self, k: &AtWrap) {
        let ws = self.ws.min(k.frames - 1);
        for _ in 0..MAX_EVENTS {
            let h = self.heads[0].pos;
            if h.whole() < self.event {
                return;
            }
            #[cfg(any(test, feature = "test-support"))]
            {
                self.events += 1;
            }
            if self.model == TapeModel::PingPong {
                self.turn();
                continue;
            }
            let le = self.bounds.le;
            if let Some(f) = self.fade.take() {
                // The wrap: the second head plays on as the first.
                self.heads[0] = self.heads[1];
                self.bounds = f.next;
                self.gain = 0.0;
                self.event = self.bounds.fade_at();
            } else if h.whole() >= le {
                let next = self.pending.take().unwrap_or(self.next);
                let len = (next.frames() as u64) << 32;
                let over = (h - at(le)).0;
                let over = if over >= len { over % len } else { over };
                self.heads[0].pos = at(next.ls) + Pos(over);
                self.bounds = next;
                self.event = next.fade_at();
            } else {
                let next = self.pending.unwrap_or(self.next);
                // SCRUB's loop has nothing before it: its fades fold.
                let ws = if self.model == TapeModel::Scrub {
                    next.ls
                } else {
                    ws
                };
                let rem = at(le) - h;
                match plan(rem, &next, ws, le) {
                    Plan::Fade { restart } => {
                        self.pending = None;
                        self.heads[1].pos = restart - rem;
                        self.fade = Some(Fade::new(at(le), rem, next));
                        self.event = le;
                    }
                    Plan::Later(frame) => {
                        self.pending = Some(next);
                        self.event = frame;
                    }
                    Plan::Splice => {
                        self.pending = Some(next);
                        self.event = le;
                    }
                }
            }
        }
    }

    /// PING-PONG reached `event`: the run-up to an edge starts, so the
    /// head slows to a stop there and comes back (`TURN_S` each way, less in
    /// a short loop). With no room for one, the head reflects at the edge,
    /// `h = 2·edge − h`, and its direction flips (`flip_at`).
    fn turn(&mut self) {
        let b = self.bounds;
        let h = self.heads[0].pos;
        if h.whole() < b.le {
            let rem = at(b.le) - h;
            let rem = rem.whole() as f32 + rem.frac();
            let th = (2.0 * rem * self.inv_v).min(self.turn_len as f32) as u16;
            if th >= 2 {
                self.turning = Some(Turn {
                    k: 0,
                    th,
                    unit: (1u32 << 16) / (2 * th as u32),
                });
                self.event = u32::MAX;
            } else {
                self.event = b.le;
            }
            return;
        }
        let len = (b.frames() as u64) << 32;
        let over = (h - at(b.le)).0;
        let over = Pos(if over >= len { over % len } else { over });
        // The reflected head, in source frames.
        let r = if self.mirror {
            at(b.ls) + over
        } else {
            at(b.le) - over
        };
        self.flip_at(r);
    }

    /// PING-PONG's frame for a head at `r` (source frames) going down
    /// (`back`) or up in loop `b`: its `u`, signed.
    fn enter(&mut self, back: bool, r: Pos, b: &Bounds) -> i64 {
        self.mirror = back;
        if back {
            self.off = at(b.ls + b.le);
            self.off.0 as i64 - r.0 as i64
        } else {
            self.off = Pos(0);
            r.0 as i64
        }
    }

    /// The source frame a head at `u` reads.
    fn real(&self, u: Pos) -> Pos {
        if self.mirror { self.off - u } else { u }
    }

    /// Where PING-PONG's run-up to the pass's far edge starts: half the
    /// distance a turn at this speed takes, at most a quarter of the loop.
    fn pp_event(&self) -> u32 {
        let b = self.bounds;
        let half = self.v * self.turn_len as f32 * 0.5;
        b.le - (half as u32).min(b.frames() / 4)
    }

    /// PING-PONG turns at `r` (source frames): the frame mirrors, so the
    /// head runs forward in `u` again. New bounds latch here (not while a
    /// clamp fades). A head outside them that would not be in within
    /// `CLAMP_FADE_S` moves to the clamped place inside under a fade that
    /// long.
    fn flip_at(&mut self, r: Pos) {
        let nb = if self.ghost.is_some() {
            self.bounds
        } else {
            self.next
        };
        let back = !self.mirror;
        let (lo, hi) = (at(nb.ls).0 as i64, at(nb.le).0 as i64 - 1);
        let mut u = self.enter(back, r, &nb);
        if u > hi {
            // Past the far edge: the head comes in from the near one,
            // the other way.
            u = self.enter(!back, r, &nb);
        }
        // A head outside that would be in within a clamp's time plays on
        // from where it is (bounds moved by a frame, a turn's overshoot).
        // Further out, it moves to the clamped place under a fade; while
        // one fades the bounds hold, so that is only a nudge.
        let mut inside = u.clamp(lo, hi);
        // Below 0 (a loop shrunk under a head going down at POSITION 0) it
        // would wrap: clamped.
        if u >= 0 && (((inside - u) >> 32) as f32) < self.reach {
            inside = u;
        } else if self.ghost.is_none() {
            self.ghost = Some(Ghost {
                pos: r,
                back,
                done: 0,
                len: self.clamp_len,
            });
            self.gain = 0.0;
        }
        self.heads[0].pos = Pos(inside as u64);
        self.bounds = nb;
        self.event = self.pp_event();
    }
}
