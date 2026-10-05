//! TAPE, the sample engine (spec 2026-10-04-tape-core-design, ADR 0093):
//! 12-bit sources held in the pool's arena, played by RAM models.
//!
//! LOOP (Digitakt II manual § Playback modes): the head starts at START,
//! plays forward to the loop's end, then loops under a crossfade that folds
//! inside the loop when it must (`heads`). MOTION walks the loop through the
//! window (Morphagene manual, Organize). REVERSE, PING-PONG, TRANSWAVE,
//! SCRUB and STRETCH play as LOOP until their kernels land.

pub mod heads;
pub mod params;
pub mod read;

use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::dsp::algo::math::{clampf, exp2};
use crate::dsp::ease::Ramp;
use crate::dsp::glide::Glide;
use heads::{Bounds, Fade, Head, Plan, at, plan, window_end, window_start};
use read::{Pos, SourceView};

pub use heads::loop_len;
pub use params::{StretchType, TAPE_ENGINE_SPECS, TapeEngineParams, TapeModel, drag_s};

/// `inc`'s clamp: four octaves over a 48 kHz source's own speed.
pub const INC_MAX: f32 = 16.0;
/// A note's first samples ramp from 0 (1 ms), so a START inside a waveform
/// never steps.
pub const NOTE_ON_RAMP: u32 = 48;
/// The shortest loop and window: 2 ms of the source.
pub const MIN_LOOP_S: f32 = 0.002;
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

/// What a wrap reads from the block's params: END and XFADE latch there.
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
    /// The head, and the head a fade reads into.
    heads: [Head; 2],
    /// Frames a sample, ramped across the block.
    inc: Ramp,
    mac: Macros,
    /// The note's first block is to come: the macros are taken whole.
    first: bool,
    /// MOTION's walk, in windows: [0, 1).
    walk: f32,
    /// The window's start, latched at note-on.
    ws: u32,
    /// The pass in play.
    bounds: Bounds,
    /// The next pass's bounds, latched where its fade was put off.
    pending: Option<Bounds>,
    fade: Option<Fade>,
    /// The first head's frame at which the next fade or wrap is handled.
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

    /// MODEL, START and ROOT latch here; the head starts at START.
    pub fn note_on(
        &mut self,
        note: u8,
        p: &TapeEngineParams,
        v: &SourceView<'_>,
        _sample_rate: u32,
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
        self.ws = window_start(p.start, frames);
        let we = window_end(self.ws, p.end, frames, rate);
        self.bounds = Bounds::of(
            self.ws,
            we,
            (self.mac.size, self.mac.position, p.xfade),
            rate,
        );
        self.heads = [Head { pos: at(self.ws) }; 2];
        self.pending = None;
        self.fade = None;
        self.gain = 0.0;
        self.event = self.bounds.fade_at();
        self.ramp = true;
        self.active = true;
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

    pub fn probe(&self) -> Probe {
        Probe {
            head: self.heads[0].pos.whole(),
            other: self.fade.map(|_| self.heads[1].pos.whole()),
            gain: self.gain,
            level: 0,
        }
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
        if core::mem::take(&mut self.first) {
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
        let (from, to) = self.inc.step(target);
        let (from, to) = (Pos::from_f32(from).0, Pos::from_f32(to).0);
        let step = (to.wrapping_sub(from) as i64 / BLOCK_SIZE as i64) as u64;
        let k = AtWrap {
            end: p.end,
            xfade: p.xfade,
            frames: v.frames(),
            rate,
        };
        self.run_loop(out, v, (from, step), &k);

        if core::mem::take(&mut self.ramp) {
            let last = self.last;
            for (i, y) in out[..NOTE_ON_RAMP as usize].iter_mut().enumerate() {
                *y = last + (*y - last) * (i as f32 / NOTE_ON_RAMP as f32);
            }
        }
        self.last = out[BLOCK_SIZE - 1];
    }

    /// LOOP's samples: runs of one head, or of a fade's two, each up to the
    /// next event. The heads live in locals; the state is written back only
    /// around an event and at the block's end.
    #[inline(always)]
    fn run_loop(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        (mut cur, step): (u64, u64),
        k: &AtWrap,
    ) {
        let [mut h0, mut h1] = self.heads.map(|h| h.pos);
        let (mut ev, mut g) = (self.event, self.gain);
        let mut i = 0;
        while i < BLOCK_SIZE {
            let mut hit = false;
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
            if hit {
                self.heads = [Head { pos: h0 }, Head { pos: h1 }];
                self.gain = g;
                self.on_event(k);
                [h0, h1] = self.heads.map(|h| h.pos);
                (ev, g) = (self.event, self.gain);
            }
        }
        self.heads = [Head { pos: h0 }, Head { pos: h1 }];
        self.gain = g;
    }

    /// The next pass's bounds from the eased macros, the walk, and END and
    /// XFADE now.
    fn next_bounds(&self, k: &AtWrap) -> Bounds {
        let ws = self.ws.min(k.frames - 1);
        let we = window_end(ws, k.end, k.frames, k.rate);
        let mut place = self.mac.position + self.walk;
        if place > 1.0 {
            place -= 1.0;
        }
        Bounds::of(ws, we, (self.mac.size, place, k.xfade), k.rate)
    }

    /// The head reached `event`: a fade starts (or is put off), a fade
    /// ends at the wrap, or a pass with no fade splices. A loop shorter
    /// than a sample's step may need more than one.
    #[cold]
    #[inline(never)]
    fn on_event(&mut self, k: &AtWrap) {
        let ws = self.ws.min(k.frames - 1);
        for _ in 0..MAX_EVENTS {
            let h = self.heads[0].pos;
            if h.whole() < self.event {
                return;
            }
            let le = self.bounds.le;
            if let Some(f) = self.fade.take() {
                // The wrap: the second head plays on as the first.
                self.heads[0] = self.heads[1];
                self.bounds = f.next;
                self.gain = 0.0;
                self.event = self.bounds.fade_at();
            } else if h.whole() >= le {
                let next = self.pending.take().unwrap_or_else(|| self.next_bounds(k));
                let len = (next.frames() as u64) << 32;
                let over = (h - at(le)).0;
                let over = if over >= len { over % len } else { over };
                self.heads[0].pos = at(next.ls) + Pos(over);
                self.bounds = next;
                self.event = next.fade_at();
            } else {
                let next = match self.pending {
                    Some(b) => b,
                    None => self.next_bounds(k),
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
}
