//! TAPE, the sample engine (spec 2026-10-04-tape-core-design, ADR 0093):
//! 12-bit sources held in the pool's arena, played by RAM models.
//!
//! LOOP (Digitakt II manual § Playback modes): the head starts at START,
//! plays forward to the loop's end, then loops under a crossfade that folds
//! inside the loop when it must (`heads`). MOTION walks the loop through the
//! window (Morphagene manual, Organize).
//!
//! END before START (#369) plays LOOP mirrored, as the Digitakt II's
//! REVERSE LOOP (manual § Playback modes): the head starts at START and
//! runs back toward END; a flip lands at the next wrap. ONESHOT plays the
//! window (or a slice) once; TRON (Mellotron M400) plays toward END while
//! the key is held, its tape running out after SIZE's length. PING-PONG (Polyend
//! Tracker v1.5 § Instrument) turns at each edge with no crossfade, slowing
//! to a stop there over 1 ms so the wave has no corner; new bounds latch at
//! a turn, and a head far outside them moves under a 5 ms fade. SCRUB
//! (Morphagene manual, Slide and Vari-Speed) loops behind a finger that
//! follows POSITION through DRAG; MOTION is its vari-speed. TRANSWAVE
//! (Ensoniq VFX/SD-1 transwaves; PPG Wave and Waldorf Microwave mips) scans
//! a table of single-cycle frames (`wave`). STRETCH (Akai S950 Operator's
//! Manual p. 32, TIMESTRETCH) splices two grains at a time pointer
//! (`stretch`).

pub mod age;
pub mod heads;
pub mod machine;
pub mod oneshot;
pub mod params;
pub mod read;
pub mod stretch;
pub mod tron;
pub mod warble;
pub mod wave;

use core::mem::MaybeUninit;

use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::math::{clampf, exp2};
use crate::dsp::delay::{Tap, Transport};
use crate::dsp::ease::Ramp;
use crate::dsp::glide::Glide;
use crate::dsp::xorshift32;
use crate::hw::Cost;
use crate::modulation::ModState;
use crate::project::SourceId;
use crate::source_table::GateView;
use age::Age;
use heads::{
    Bounds, Dir, Fade, Ghost, Head, Plan, Turn, Window, at, min_frames, plan, shortest,
    window_end_min, window_start,
};
use oneshot::{Slice, slice_bounds};
use read::{Pos, SourceView};
use stretch::Stretch;
use warble::Warble;
use wave::Wave;

pub use heads::loop_len;
pub use params::{
    MACHINE_NAMES, MODEL_NAMES, StretchType, TAPE_ENGINE_SPECS, TapeEngineParams, TapeModel, Wow,
    drag_s, reads,
};

// Each model's bill: its measured worst a sample, the M7's static count
// (instructions × 1.46 × 1.1, + 12 a head for `inc` 16's misses), rounded
// up to 10 (spec § 5). The bench's cold rows hold them (TAPE Task 14).
/// LOOP either way: its shortest loop at `inc` 16, XFADE ½, 262.4 → 445.
pub const COST_LOOP: Cost = Cost(450);
/// ONESHOT: a short slice's end ramp, retriggered, 135.4 → 229.5.
pub const COST_ONESHOT: Cost = Cost(230);
/// TRON: its run-out ramp, as ONESHOT's end, 135.4 → 229.5.
pub const COST_TRON: Cost = Cost(230);
/// PING-PONG clamping in its shortest loop, 289.4 → 489.
pub const COST_PING: Cost = Cost(490);
/// SCRUB fading in its shortest loop, an ease in, 283.4 → 479.
pub const COST_SCRUB: Cost = Cost(480);
/// TRANSWAVE steady: two heads, 232.9 → 398.
pub const COST_WAVE: Cost = Cost(400);
/// TRANSWAVE's level change over `COST_WAVE`: four heads, 432.9 → 743,
/// billed 770 in all. TAPE's `glide_cost` term.
pub const COST_WAVE_LEVEL: Cost = Cost(370);
/// STRETCH either way, a short source's lifted wraps, 442.5.
pub const COST_STRETCH: Cost = Cost(450);
/// TAPE when it runs (`tape_runs`): the transport, 1.2 → 1.9.
pub const COST_TAPE_RUNS: Cost = Cost(10);
/// AGE when it runs (`age::age_runs`): the wear's loop and block, 15.9 →
/// 25.5.
pub const COST_AGE: Cost = Cost(30);
/// TRANSWAVE's pitch can cross a level (`COST_WAVE_LEVEL`) in a note past
/// this many cents: the hysteresis' 0.35 to 0.715 step, ±6.2 st.
pub const LEVEL_CENTS: f32 = 620.0;

/// `inc`'s clamp: four octaves over a 48 kHz source's own speed.
pub const INC_MAX: f32 = 16.0;
/// A note's first samples ramp from 0 (1 ms), so a START inside a waveform
/// never steps.
pub const NOTE_ON_RAMP: u32 = 48;
/// PING-PONG's clamp fade: a head outside new bounds moves inside over 5 ms.
/// A LOOP flip at XFADE 0 takes it too, so a flip never clicks.
pub const CLAMP_FADE_S: f32 = 0.005;
/// ONESHOT's end and TRON's run-out ramp to 0 over 48 samples (1 ms), a
/// smoothstep, landing on the last frame; their note-on ramp is one too.
pub const END_RAMP: u32 = NOTE_ON_RAMP;
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
/// TAPE or AGE easing to 0 is 0 under this, so the transport or the wear
/// stops and its bill with it (0.03 cents of wow; hiss at −102 dBFS, the
/// one-pole a wire).
const MACRO_SETTLE: f32 = 1e-3;
/// The block rate the transport runs at: the hardware's, so its rates are
/// constants (`Age` takes 48 kHz too).
const BLOCK_RATE: f32 = (SAMPLE_RATE as usize / BLOCK_SIZE) as f32;

/// TAPE runs, and is billed, whenever it can be heard: stored above 0, or
/// routed (AGE's rule, `age::age_runs`). Stored at 0 and unrouted, the
/// transport is bypassed.
pub fn tape_runs(stored: f32, routed: bool) -> bool {
    stored > 0.0 || routed
}

/// The wow's rate at the block rate, turns a tick; each note draws its own
/// within ±`WOW_SPREAD` of it.
const WOW_RATE: f32 = crate::dsp::delay::WOW_HZ / BLOCK_RATE;
/// Each note's wow runs up to 10 % off the transport's (TAPE Task 9 fix
/// round 2), so voices' wows never lock: their phases random-walk note by
/// note, as reels that never quite run at one speed.
pub const WOW_SPREAD: f32 = 0.1;

const _: () = assert!(NOTE_ON_RAMP as usize <= BLOCK_SIZE);
const _: () = assert!(read::MIN_SOURCE_FRAMES == MIN_LOOP_FRAMES);

/// Events one sample handles: a put-off fade's start, then the fade's start
/// and its wrap, then the new pass's fade, which is one pass of an ordinary
/// loop. Only a loop shorter than a sample's step needs more; it takes them
/// the next sample, its head held in reach by the splice's modulo.
const MAX_EVENTS: usize = 4;

/// A retargeted clamp's `lift` (R1) on its samples `out`, fading with the
/// ghost's share: `lift · (1 − gain)`, where the last sample's gain is `g`
/// and it rises `dg` a sample. It runs after the clamp's loop, not in it,
/// and only while a lift is set: the plain clamp pays nothing for it.
#[inline(always)]
fn lift(out: &mut [f32], lift: f32, g: f32, dg: f32) {
    let mut c = lift * (1.0 - g);
    for y in out.iter_mut().rev() {
        *y += c;
        c += lift * dg;
    }
}

/// A note's first `NOTE_ON_RAMP` samples ramp from `last`, so a START
/// inside a waveform never steps; a one-shot's (`smooth`) by a smoothstep,
/// as it starts mid-wave on a slice, so the ramp has no corner where it
/// lands. Once a note, out of line.
#[cold]
#[inline(never)]
fn note_on_ramp(out: &mut [f32; BLOCK_SIZE], last: f32, smooth: bool) {
    for (i, y) in out[..NOTE_ON_RAMP as usize].iter_mut().enumerate() {
        let x = i as f32 / NOTE_ON_RAMP as f32;
        let w = if smooth { x * x * (3.0 - 2.0 * x) } else { x };
        *y = last + (*y - last) * w;
    }
}

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
    /// TRANSWAVE's level change this block: the level it fades from (four
    /// heads read).
    pub from: Option<u8>,
}

/// What the strip reads of a Part's newest note (spec § 3): the first
/// head's frame in the low word, the second's in the high, `u32::MAX` for
/// none. TRANSWAVE's are frames of its table, and TRON's second is where
/// its tape runs out. The audio stores it once a block (`instrument::HEADS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heads(pub u64);

impl Heads {
    /// No note: nothing drawn.
    pub const NONE: Heads = Heads(u64::MAX);

    pub fn new(head: u32, second: Option<u32>) -> Heads {
        Heads(u64::from(head) | u64::from(second.unwrap_or(u32::MAX)) << 32)
    }

    pub fn head(self) -> Option<u32> {
        Some(self.0 as u32).filter(|&h| h != u32::MAX)
    }

    pub fn second(self) -> Option<u32> {
        Some((self.0 >> 32) as u32).filter(|&h| h != u32::MAX)
    }
}

/// What a wrap or a turn reads from the block's params: END (in the next
/// pass's frame: `1 − END` mirrored) and XFADE latch there.
#[derive(Clone, Copy)]
struct AtWrap<'a> {
    end: f32,
    xfade: f32,
    frames: u32,
    rate: u16,
    /// The source: a retargeted fade reads what it plays (`fade_from`).
    v: &'a SourceView<'a>,
}

/// A pass's bounds put off to a later event, with its frame and its
/// window's start in that frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Next {
    b: Bounds,
    rev: bool,
    ws: u32,
}

/// A voice's strip of tape: its transport, its draws and its wear's, which
/// run on from note to note (spec § 2 TAPE), so its wow's phase
/// random-walks apart from the other voices'.
#[derive(Clone, Copy)]
pub(crate) struct Strip {
    wow: Transport,
    rng: u32,
    age: Age,
    warble: Warble,
}

#[derive(Clone, Copy)]
pub struct TapeEngine {
    /// The source a note plays, latched at note-on (ADR 0087): never
    /// re-resolved, its row read each block.
    src: Option<SourceId>,
    /// A note-on whose source a closed gate moves: active and silent, it
    /// starts (`note_on`) on the first block its row reads (spec § 4).
    gated: bool,
    model: TapeModel,
    /// The head, and the head a fade reads into, each in its frame `u`:
    /// mirrored, the source is read at `off − u`, else at `u` (`off + u`
    /// in SCRUB). They differ only while a flip fades (LOOP's END moved
    /// across START), the second head's `off` then its own frame's.
    heads: [Head; 2],
    /// LOOP reversed: the source's last frame. ONESHOT and TRON reversed:
    /// `lo + hi − 1` of the window. PING-PONG down: `ls + le`. SCRUB: the
    /// finger, plus the source's length when mirrored.
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
    /// TRANSWAVE's note: its frames, level and scan.
    wave: Wave,
    /// STRETCH's note: its grains and clock.
    stretch: Stretch,
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
    /// The window's start, latched at note-on, in the first head's frame
    /// (a flip's latches at its wrap).
    ws: u32,
    /// The pass in play.
    bounds: Bounds,
    /// The shortest loop this block, frames: `heads::shortest` at its
    /// fastest head; the window's end this block, kept that far past its
    /// start.
    min_len: u32,
    we: u32,
    /// The head has yet to reach its first event: nothing is in play, so
    /// the first pass's bounds follow `next` each block.
    runup: bool,
    /// The next pass's bounds, worked out once a block from its params, in
    /// its frame (mirrored: LOOP with END before START) from its window's
    /// start; latched where its fade was put off.
    next: Bounds,
    next_rev: bool,
    next_ws: u32,
    pending: Option<Next>,
    /// A flip's fade in progress: the window's start in the frame it fades
    /// into, latched at the wrap.
    fade_ws: u32,
    /// START, latched at note-on: END before it plays reversed.
    start: f32,
    /// ONESHOT and TRON: the window's end in the head's frame, the output
    /// samples left on TRON's tape (ONESHOT's never run out), the release's
    /// gain and its fall a sample (1 until key up), and FADE's fall.
    once_end: u32,
    left: u32,
    rel: f32,
    rel_k: f32,
    fade_k: f32,
    /// ONESHOT under SLICE NOTE: the note picks the slice and plays at ROOT.
    at_root: bool,
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
    /// TAPE: this voice's transport, run once a block, at its own rates
    /// at depth 1; its speed deviation this block, cents.
    wow: Transport,
    tap: Tap,
    /// The voice's own draws: each note's wow rate.
    rng: u32,
    cents: f32,
    /// AGE: this voice's wear.
    age: Age,
    /// WOW, latched at note-on, and WARBLE's draws, this voice's own.
    wow_kind: Wow,
    warble: Warble,
    /// The transport is skipped: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    no_transport: bool,
}

impl TapeEngine {
    fn idle() -> TapeEngine {
        TapeEngine {
            src: None,
            gated: false,
            model: TapeModel::Loop,
            heads: [Head::default(); 2],
            off: Pos(0),
            df: 0,
            ddf: 0,
            stopped: false,
            hush: 1.0,
            hush_step: 1.0,
            wave: Wave::IDLE,
            stretch: Stretch::IDLE,
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
            we: 1,
            runup: false,
            next: Bounds {
                ls: 0,
                le: 1,
                lx: 0,
            },
            next_rev: false,
            next_ws: 0,
            pending: None,
            fade_ws: 0,
            start: 0.0,
            once_end: 1,
            left: 0,
            rel: 1.0,
            rel_k: 1.0,
            fade_k: 1.0,
            at_root: false,
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
            wow: Transport::new(),
            tap: Tap::speed_cents(1.0, BLOCK_RATE),
            rng: 0x510E_527F,
            cents: 0.0,
            age: Age::new(),
            wow_kind: Wow::Capstan,
            warble: Warble::new(),
            #[cfg(any(test, feature = "test-support"))]
            no_transport: false,
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        slot.write(Self::idle())
    }

    /// Each voice's own strip of tape: its transport's phases and its
    /// hiss and dropouts, from `s` (the voice's index).
    pub fn seed(&mut self, s: u32) {
        self.wow = Transport::seeded(s);
        self.rng = (s.wrapping_mul(0x9E37_79B9) ^ 0x510E_527F) | 1;
        self.age.seed(s);
        self.warble.seed(s);
    }

    /// The strip a rebuilt slot keeps (`restore`).
    pub(crate) fn strip(&self) -> Strip {
        Strip {
            wow: self.wow,
            rng: self.rng,
            age: self.age,
            warble: self.warble,
        }
    }

    /// A fresh engine on the voice's own strip of tape, as it last ran.
    pub(crate) fn restore(&mut self, s: Strip) {
        (self.wow, self.rng, self.age, self.warble) = (s.wow, s.rng, s.age, s.warble);
        // `trigger`'s hold does this too, before any note plays; a rebuilt
        // slot never carries the last note's wear on its own.
        self.age.hold();
    }

    /// The source the next `start` plays.
    pub fn latch(&mut self, id: SourceId) {
        self.src = Some(id);
    }

    /// No source: the next `start` plays nothing.
    pub fn unlatch(&mut self) {
        self.src = None;
    }

    /// The source latched.
    pub fn source(&self) -> Option<SourceId> {
        self.src
    }

    /// The latched source's view this block: `None` with none latched, its
    /// row empty or moving, or a descriptor that doesn't read.
    fn view<'v>(&self, src: &'v GateView<'_>) -> Option<SourceView<'v>> {
        SourceView::new(src.bytes(self.src?)?)
    }

    /// `note_on` on the latched source through the block's view. With no
    /// source to read, or one `p`'s model can't play (`wave::playable`), the
    /// engine stays idle. One a closed gate moves waits, active and silent,
    /// and starts on the first block after the gate opens (`play`).
    pub fn start(&mut self, note: u8, p: &TapeEngineParams, src: &GateView<'_>, sample_rate: u32) {
        match self.view(src) {
            Some(v) if wave::playable(p.model, &v) => self.note_on(note, p, &v, sample_rate),
            None if self.src.is_some_and(|id| src.moving(id)) => {
                (self.note, self.model) = (note, p.model);
                (self.gated, self.active) = (true, true);
            }
            _ => (self.gated, self.active) = (false, false),
        }
    }

    /// A note-on on `model` and `id` (a Part's) plays nothing: no source,
    /// an absent one (its row empty, the gate open), or one `model` can't
    /// play. One a closed gate moves isn't refused: it waits (`start`).
    /// Refused, it is never allocated (spec § 2).
    pub fn refuses(model: TapeModel, id: Option<SourceId>, src: &GateView<'_>) -> bool {
        let Some(id) = id else {
            return true;
        };
        !src.moving(id)
            && !src
                .bytes(id)
                .and_then(SourceView::new)
                .is_some_and(|v| wave::playable(model, &v))
    }

    /// `render` from the latched source's row this block. While a closed
    /// gate moves it, the block is silent and the note waits; a row gone
    /// (retired and compacted) ends it (`GateView::bytes`), as does one
    /// re-stored under the id with fewer frames than the note latched.
    pub fn play(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &TapeEngineParams,
        src: &GateView<'_>,
        sample_rate: u32,
    ) {
        if let Some(v) = self.view(src) {
            if core::mem::take(&mut self.gated) {
                self.start(self.note, p, src, sample_rate);
            } else if v.frames() < self.n {
                self.active = false;
            }
            self.render(out, p, &v, sample_rate);
            return;
        }
        out.fill(0.0);
        if !self.src.is_some_and(|id| src.moving(id)) {
            (self.gated, self.active) = (false, false);
        }
    }

    /// A TAPE voice's bill for `p`: its model's, and TAPE's and AGE's each
    /// when it runs, stored above 0 or routed (spec § 5). TRANSWAVE's level
    /// change is `EngineSlot`'s, which sees the pitch.
    pub fn cost(p: &TapeEngineParams, mods: &ModState) -> Cost {
        let routed = |q| mods.moves_addr(ParamAddr::new(BlockRef::TapeEngine, q));
        let mut c = Self::model_cost(p.model);
        if tape_runs(p.tape, routed(TapeEngineParams::TAPE)) {
            c = c + COST_TAPE_RUNS;
        }
        if age::age_runs(p.age, routed(TapeEngineParams::AGE)) {
            c = c + COST_AGE;
        }
        c
    }

    /// Each model's measured worst, either way round (spec § 5).
    pub const fn model_cost(m: TapeModel) -> Cost {
        match m {
            TapeModel::Loop => COST_LOOP,
            TapeModel::Oneshot => COST_ONESHOT,
            TapeModel::Tron => COST_TRON,
            TapeModel::PingPong => COST_PING,
            TapeModel::Scrub => COST_SCRUB,
            TapeModel::Transwave => COST_WAVE,
            TapeModel::Stretch => COST_STRETCH,
        }
    }

    /// What the sounding note costs: its note-on's model.
    pub fn playing_cost(&self) -> Option<Cost> {
        self.active.then(|| Self::model_cost(self.model))
    }

    /// A glide steal's ratio now, 1 at rest: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn slide(&self) -> f32 {
        self.slide.value()
    }

    /// The transport skipped, as if TAPE added nothing: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn without_transport(&mut self) {
        self.no_transport = true;
    }

    /// The voice's PITCH and FINE with their routes, in cents, for the next
    /// `note_on` or `render`.
    pub fn set_pitch(&mut self, cents: f32) {
        self.pitch = cents;
    }

    /// MODEL, START, ROOT, DRAG, WOW and SLICE latch here; the head starts
    /// at START, heading for END (spec § 2 Machines).
    pub fn note_on(
        &mut self,
        note: u8,
        p: &TapeEngineParams,
        v: &SourceView<'_>,
        sample_rate: u32,
    ) {
        let (frames, rate) = (v.frames(), v.desc().rate);
        self.model = p.model;
        self.gated = false;
        self.note = note;
        self.root = p.root;
        self.slide = Glide::new(1.0, 1.0);
        self.inc.hold();
        // This note's wow rate, ±`WOW_SPREAD`: one draw, the phase runs on.
        let d = xorshift32(&mut self.rng) as i32 as f32 * (WOW_SPREAD / 2_147_483_648.0);
        self.tap.set_wow_rate(WOW_RATE * (1.0 + d));
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
        self.ramp = true;
        self.active = true;
        self.wow_kind = p.wow;
        self.start = p.start;
        if matches!(self.model, TapeModel::Oneshot | TapeModel::Tron) {
            self.once_on(note, p, v, sample_rate);
            return;
        }
        if self.model == TapeModel::Transwave {
            self.wave = Wave::on(v, wave::cycles(note, self.pitch, sample_rate));
            return;
        }
        if self.model == TapeModel::Stretch {
            let i = inc(rate, note, self.pitch, self.root, sample_rate);
            self.stretch = Stretch::on(p, v, i, &self.mac, sample_rate);
            return;
        }
        let rev = self.reversed(p.end);
        let (start, end) = self.window(p, rev);
        self.ws = window_start(start, frames);
        // The first pass's floor, at the note's own `inc` (SCRUB: its loop's
        // vari-speed).
        let mut fast = inc(rate, note, self.pitch, self.root, sample_rate);
        if self.model == TapeModel::Scrub {
            fast = (2.0 * libm::fabsf(clampf(p.motion, -1.0, 1.0)) * fast).min(INC_MAX);
        }
        self.min_len = shortest(rate, fast);
        let we = window_end_min(self.ws, end, frames, self.min_len);
        self.we = we;
        let xfade = if self.model == TapeModel::PingPong {
            0.0
        } else {
            p.xfade
        };
        self.set_mirror(false);
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
                    self.set_mirror(true);
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
                if model == TapeModel::Loop && rev {
                    self.set_mirror(true);
                    self.off = at(frames - 1);
                }
            }
        }
        let m = self.mirror();
        self.heads = [Head {
            pos: at(start_at),
            mirror: m,
        }; 2];
        self.next = self.bounds;
        (self.next_rev, self.next_ws) = (m, self.ws);
        self.pending = None;
        self.fade = None;
        self.gain = 0.0;
        self.event = self.bounds.fade_at();
        self.runup = true;
    }

    /// LOOP plays its next pass reversed: END before the START latched at
    /// note-on (#369).
    fn reversed(&self, end: f32) -> bool {
        self.model == TapeModel::Loop && Dir::of(self.start, end) == Dir::Rev
    }

    /// START and END as the model plays them. Reversed (`rev`), LOOP's
    /// window is mirrored, so START is its start in the mirrored frame
    /// (`1 − START`), as REVERSE's was. PING-PONG and SCRUB take the sorted
    /// window.
    fn window(&self, p: &TapeEngineParams, rev: bool) -> (f32, f32) {
        if !self.model.ordered() {
            (p.start.min(p.end), p.start.max(p.end))
        } else if rev {
            (1.0 - p.start, 1.0 - p.end)
        } else {
            (p.start, p.end)
        }
    }

    /// The first head's frame.
    fn mirror(&self) -> bool {
        self.heads[0].mirror
    }

    /// Both heads in one frame.
    fn set_mirror(&mut self, m: bool) {
        self.heads[0].mirror = m;
        self.heads[1].mirror = m;
    }

    /// A flip fades: the second head plays the other way.
    fn flipping(&self) -> bool {
        self.fade.is_some() && self.heads[1].mirror != self.heads[0].mirror
    }

    /// ONESHOT's and TRON's note: one head from START toward END in the
    /// window (ONESHOT: its slice), at least 2 ms (`min_frames`) on END's
    /// side but at the source's edge, mirrored about it when END is before
    /// START. TRON's tape runs
    /// out after SIZE's length in output samples.
    fn once_on(&mut self, note: u8, p: &TapeEngineParams, v: &SourceView<'_>, sample_rate: u32) {
        let (frames, rate) = (v.frames(), v.desc().rate);
        let mut w = Window::of(p.start, p.end, frames, min_frames(rate));
        self.at_root = false;
        if self.model == TapeModel::Oneshot {
            let s = Slice::of(f32::from(p.slice));
            self.at_root = s.by_note();
            w = slice_bounds(w, s, note);
            self.left = u32::MAX;
        } else {
            self.left = tron::tron_run_samples(tron::tron_len_s(p.size), sample_rate);
        }
        let rev = w.dir == Dir::Rev;
        self.off = if rev { at(w.lo + w.hi - 1) } else { Pos(0) };
        self.heads = [Head {
            pos: at(w.lo),
            mirror: rev,
        }; 2];
        self.once_end = w.hi;
        (self.rel, self.rel_k) = (1.0, 1.0);
        self.fade_k = tron::release_k(p.drag, sample_rate);
        self.fade = None;
        self.pending = None;
        self.gain = 0.0;
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
    /// Gated, the note it starts on moves (its `note_on` drops the glide).
    pub fn glide_to(&mut self, note: u8, (_sample_rate, tau): (u32, f32)) {
        if !self.active || note == self.note {
            return;
        }
        let from = exp2((self.note as f32 - note as f32) / 12.0) * self.slide.value();
        self.slide = Glide::new(from, tau);
        self.slide.toward(1.0);
        self.note = note;
    }

    /// TRON's tape fades through FADE's one-pole, the pressure pad lifting.
    /// Every other machine: nothing (a loop has no end, so the note ends
    /// when its VCA lets go; ONESHOT ignores it, `sustains`).
    pub fn note_off(&mut self) {
        if self.model == TapeModel::Tron {
            self.rel_k = self.fade_k;
        }
    }

    /// An ONESHOT head plays: the voice keeps its gate, so note-off
    /// changes nothing and the note ends at END.
    pub fn sustains(&self) -> bool {
        self.active && self.model == TapeModel::Oneshot
    }

    /// A fresh note is to come on an idle voice: its ramp starts from 0, not
    /// from the last note's sample (as `Ramp::hold`).
    pub fn hold(&mut self) {
        self.last = 0.0;
        self.age.hold();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The model latched at the last note-on.
    pub fn model(&self) -> TapeModel {
        self.model
    }

    /// The source frame head `h` reads; one before frame 0 reads 0.
    fn frame_of(&self, h: Head) -> u32 {
        if h.mirror != self.mirror() {
            // A flip's incoming head, in LOOP's other frame.
            let r = if h.mirror {
                at(self.n - 1) - h.pos
            } else {
                h.pos
            };
            return r.whole().min(self.n);
        }
        self.frame(h.pos)
    }

    /// The source frame a head at `u` in the first head's frame reads; one
    /// before frame 0 reads 0.
    fn frame(&self, u: Pos) -> u32 {
        let r = if self.mirror() {
            self.off - u
        } else if self.model == TapeModel::Scrub {
            self.off + u
        } else {
            u
        };
        let w = r.whole();
        if w > i32::MAX as u32 { 0 } else { w }
    }

    /// The strip's heads (`Heads`): the probe's, but TRON's second is
    /// where its tape runs out at this pitch, `left · inc` frames on, END
    /// at most. None while idle.
    pub fn heads(&self) -> Heads {
        if !self.active {
            return Heads::NONE;
        }
        let p = self.probe();
        if self.model == TapeModel::Tron {
            let run = (self.left as f32 * self.inc.value().unwrap_or(0.0)) as u32;
            let out = self.heads[0]
                .pos
                .whole()
                .saturating_add(run)
                .min(self.once_end);
            return Heads::new(p.head, Some(self.frame(at(out))));
        }
        Heads::new(p.head, p.other)
    }

    pub fn probe(&self) -> Probe {
        if self.model == TapeModel::Transwave {
            let (head, other, gain) = self.wave.frames();
            return Probe {
                head,
                other: Some(other),
                gain,
                level: self.wave.level(),
                from: self.wave.from(),
            };
        }
        if self.model == TapeModel::Stretch {
            let (head, other, gain) = self.stretch.probe();
            return Probe {
                head,
                other: Some(other),
                gain,
                level: 0,
                from: None,
            };
        }
        let first = self.frame(self.heads[0].pos);
        match self.ghost {
            // The head left outside fades out; the one inside fades in.
            Some(g) => Probe {
                head: g.pos.whole().min(self.n),
                other: Some(first),
                gain: self.gain,
                level: 0,
                from: None,
            },
            None => Probe {
                head: first,
                other: self.fade.map(|_| self.frame_of(self.heads[1])),
                gain: self.gain,
                level: 0,
                from: None,
            },
        }
    }

    /// Each head's way: the old grain's and the new one's (STRETCH), the
    /// head's and a fade's second head's (the rest).
    pub fn dirs(&self) -> [Dir; 2] {
        if self.model == TapeModel::Stretch {
            return self.stretch.dirs();
        }
        self.heads
            .map(|h| if h.mirror { Dir::Rev } else { Dir::Fwd })
    }

    /// Events handled since `init_in_place`: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn events(&self) -> u32 {
        self.events + self.stretch.events
    }

    /// STRETCH's wraps in the loop since the note-on, lifted and bare: for
    /// the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn wraps(&self) -> [u32; 2] {
        self.stretch.wraps
    }

    /// STRETCH's grain clock: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn grain(&self) -> stretch::GrainClock {
        self.stretch.clock()
    }

    /// The heads' speed at the last block's end, frames a sample: for the
    /// tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn inc_now(&self) -> Option<f32> {
        self.inc.value()
    }

    /// This note's wow rate, Hz: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn wow_hz(&self) -> f32 {
        self.tap.wow_rate() * BLOCK_RATE
    }

    /// The wear: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn age(&self) -> &Age {
        &self.age
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
        // Eased to 0, TAPE's cents shrink with it; stored at 0 and under
        // `MACRO_SETTLE` it is 0 and the transport rests. AGE likewise. Each
        // settle sits behind its stage's own test, so a stage at rest pays
        // only that test.
        self.cents = if self.mac.tape > 0.0 {
            if to.tape == 0.0 && self.mac.tape < MACRO_SETTLE {
                self.mac.tape = 0.0;
                0.0
            } else {
                self.transport()
            }
        } else {
            0.0
        };
        if self.mac.age > 0.0 && to.age == 0.0 && self.mac.age < MACRO_SETTLE {
            self.mac.age = 0.0;
        }
        self.slide.tick();
        if self.model == TapeModel::Transwave {
            let target = clampf(
                wave::cycles(self.note, self.pitch + self.cents, sample_rate) * self.slide.value(),
                f32::EPSILON,
                wave::CYCLES_MAX,
            );
            let m = self.mac;
            self.wave.render(
                out,
                v,
                self.inc.step(target),
                (m.position, m.motion, m.size),
                (first, sample_rate),
            );
        } else if matches!(self.model, TapeModel::Oneshot | TapeModel::Tron) {
            self.render_once(out, v, sample_rate);
        } else if self.model == TapeModel::Stretch {
            let target = self.target_inc(v.desc().rate, 0.0, sample_rate);
            let m = self.mac;
            self.stretch.render(
                out,
                v,
                self.inc.step(target),
                (m.position, m.motion, m.size),
                p.end,
                sample_rate,
            );
        } else {
            self.render_heads(out, v, (p, first), sample_rate);
        }

        if core::mem::take(&mut self.ramp) {
            let smooth = matches!(self.model, TapeModel::Oneshot | TapeModel::Tron);
            note_on_ramp(out, self.last, smooth);
        }
        self.age.process(out, self.mac.age);
        self.last = out[BLOCK_SIZE - 1];
    }

    /// The heads' speed this block, frames a sample: the note's `inc` with
    /// PITCH, FINE, the wow and `extra` cents, a glide steal's ratio on it,
    /// clamped. Out of line: one copy for every model that plays heads.
    #[inline(never)]
    fn target_inc(&self, rate: u16, extra: f32, sample_rate: u32) -> f32 {
        clampf(
            inc(
                rate,
                self.note,
                self.pitch + self.cents + extra,
                self.root,
                sample_rate,
            ) * self.slide.value(),
            f32::EPSILON,
            INC_MAX,
        )
    }

    /// TAPE's transport, once a block: the head's speed deviation in cents
    /// (`Tap::speed_cents`), which `inc` ramps to across the block. TAPE 0
    /// adds exact zeros. The voice's rates were set at its seed (`tap`, depth 1), so a block
    /// only scales the offsets by TAPE.
    fn transport(&mut self) -> f32 {
        #[cfg(any(test, feature = "test-support"))]
        if self.no_transport {
            return 0.0;
        }
        match self.wow_kind {
            Wow::Capstan => {
                let (wow, flutter) = self.wow.offsets(&self.tap);
                (wow + flutter) * self.mac.tape
            }
            // In place of the capstan, never beside it.
            Wow::Warble => self.warble.cents(self.mac.tape, BLOCK_RATE),
        }
    }

    /// ONESHOT's and TRON's block: one head toward the window's end, its
    /// gain the release's (TRON's key up). Within reach of the end or the
    /// tape's run-out, each sample ramps to 0 over `END_RAMP` and the note
    /// ends there; a span shorter than a block's travel ends inside it.
    #[inline(never)]
    fn render_once(&mut self, out: &mut [f32; BLOCK_SIZE], v: &SourceView<'_>, sample_rate: u32) {
        // SLICE NOTE: the note picks the slice and plays at ROOT.
        let tune = if self.at_root {
            self.root - f32::from(self.note) * 100.0
        } else {
            0.0
        };
        let target = self.target_inc(v.desc().rate, tune, sample_rate);
        let (from, to) = self.inc.step(target);
        let fast = from.max(to);
        let rem = Pos(at(self.once_end).0.saturating_sub(self.heads[0].pos.0));
        let rem = rem.whole() as f32 + rem.frac();
        let reach = (BLOCK_SIZE as u32 + END_RAMP) as f32;
        let near = rem <= reach * fast + 2.0 || (self.left as f32) <= reach;
        // Near the end the gain ramps to 0 over `END_RAMP` samples' travel
        // at the block's fastest head, or over the tape's last `END_RAMP`
        // samples, whichever is lower at the block's end: a line through
        // its values at the block's ends (the head's travel is linear, as
        // `inc` is ramped), clamped to 0..1 a sample.
        let travel = 0.5 * BLOCK_SIZE as f32 * (from + to) + 0.5 * (to - from);
        let (by_end, by_left) = (1.0 / (END_RAMP as f32 * fast), 1.0 / END_RAMP as f32);
        let left = self.left as f32;
        let gain = if (rem - travel) * by_end <= (left - BLOCK_SIZE as f32) * by_left {
            (rem * by_end, travel * by_end * (1.0 / BLOCK_SIZE as f32))
        } else {
            (left * by_left, by_left)
        };
        let (from, to) = (Pos::from_f32(from).0, Pos::from_f32(to).0);
        let step = (to.wrapping_sub(from) as i64 / BLOCK_SIZE as i64) as u64;
        let c = (from, step, gain);
        match (self.mirror(), near) {
            (_, false) => self.once(out, v, c),
            (false, true) => self.once_near::<false>(out, v, c),
            (true, true) => self.once_near::<true>(out, v, c),
        }
        if self.rel < tron::TRON_FLOOR {
            self.active = false;
        }
    }

    /// `render_once`'s samples far from the end: the head read forward,
    /// or reversed at `off − u` (as `(u ^ m) + a`, one loop either way),
    /// under the release's gain.
    #[inline(never)]
    fn once(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        (mut cur, step, _): (u64, u64, (f32, f32)),
    ) {
        let (m, a) = if self.mirror() {
            (u64::MAX, self.off.0.wrapping_add(1))
        } else {
            (0, 0)
        };
        let mut h = self.heads[0].pos;
        let (mut g, k) = (self.rel, self.rel_k);
        for y in out.iter_mut() {
            // Reversed at `lo` 0, the head's last fraction reads just
            // under frame 0: the position wraps, and `SourceView::read`
            // takes its whole part as `i32` −1, clamped to frame 0.
            *y = v.read(Pos((h.0 ^ m).wrapping_add(a))) * g;
            g *= k;
            cur = cur.wrapping_add(step);
            h = h + Pos(cur);
        }
        self.left = self.left.saturating_sub(BLOCK_SIZE as u32);
        self.heads[0].pos = h;
        self.rel = g;
    }

    /// `render_once`'s samples near the end or the run-out, read forward or
    /// (`REV`) at `off − u`: the end's line falls `dr` a sample from `r`,
    /// its gain that clamped to 0..1 and smoothstepped (so the ramp has no
    /// corner where it starts), to land on 0 there, and the note ends where
    /// the head reaches END or the tape runs out.
    #[inline(never)]
    fn once_near<const REV: bool>(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        (mut cur, step, (mut r, dr)): (u64, u64, (f32, f32)),
    ) {
        let off = self.off;
        let mut h = self.heads[0].pos;
        let (mut g, k) = (self.rel, self.rel_k);
        let end = at(self.once_end);
        let mut i = 0;
        while i < BLOCK_SIZE && h < end && self.left > 0 {
            r -= dr;
            let x = clampf(r, 0.0, 1.0);
            // `off − h` under frame 0 at `lo` 0: `read`'s clamp, as in `once`.
            out[i] = v.read(if REV { off - h } else { h }) * g * (x * x * (3.0 - 2.0 * x));
            g *= k;
            self.left -= 1;
            cur = cur.wrapping_add(step);
            h = h + Pos(cur);
            i += 1;
        }
        if h >= end || self.left == 0 {
            // The tape ran out, or the head reached END: on the block's
            // last sample, too.
            out[i..].fill(0.0);
            self.active = false;
        }
        self.heads[0].pos = h;
        self.rel = g;
    }

    /// The block of a model that plays heads through a window: all but
    /// TRANSWAVE.
    fn render_heads(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        (p, first): (&TapeEngineParams, bool),
        sample_rate: u32,
    ) {
        let w = self.walk + self.mac.motion * BLOCK_SIZE as f32 / sample_rate as f32;
        self.walk = w - libm::floorf(w);

        let rate = v.desc().rate;
        let target = self.target_inc(rate, 0.0, sample_rate);
        let (mut from, mut to) = self.inc.step(target);
        // The next pass's frame: LOOP's END moved across START flips it,
        // at the next wrap. Its window's start is that frame's.
        let rev = if self.model == TapeModel::Loop {
            self.reversed(p.end)
        } else {
            self.mirror()
        };
        self.next_rev = rev;
        self.next_ws = if rev == self.mirror() {
            self.ws
        } else {
            let start = if rev { 1.0 - self.start } else { self.start };
            window_start(start, v.frames())
        };
        if self.model == TapeModel::PingPong {
            self.v = to;
            self.inv_v = 1.0 / to;
            self.reach = to * self.clamp_len as f32 + 1.0;
            if self.turning.is_none() {
                self.event = self.pp_event();
            }
        }
        let (_, end) = self.window(p, rev);
        // A flip at XFADE 0 fades over the clamp's time at this speed.
        self.v = to;
        let k = AtWrap {
            end,
            xfade: p.xfade,
            frames: v.frames(),
            rate,
            v,
        };
        let scrub = self.model == TapeModel::Scrub;
        if scrub {
            (from, to) = self.scrub_block(p, to, first, &k);
        } else {
            self.floor(from.max(to), &k);
        }
        // Every event this block latches these: worked out once.
        self.next = self.next_bounds(&k);
        if self.runup && rev == self.mirror() {
            self.follow_next();
        }
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
                (false, false) if self.flipping() => match self.mirror() {
                    false => self.run_flip::<false>(out, v, c, &k),
                    true => self.run_flip::<true>(out, v, c, &k),
                },
                (false, false) => match (self.mirror(), scrub) {
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
        k: &AtWrap<'_>,
    ) -> (f32, f32) {
        let want = 2.0 * self.mac.motion * inc;
        if self.speed == 0.0 && want != 0.0 && (want < 0.0) != self.mirror() {
            self.turn_scrub();
        }
        let ahead = if self.mirror() { -want } else { want };
        let to = if ahead < 0.0 { 0.0 } else { ahead.min(INC_MAX) };
        let from = if first { to } else { libm::fabsf(self.speed) };
        self.speed = if self.mirror() { -to } else { to };
        self.floor(from.max(to), k);

        let we = self.we;
        let at_n = if self.mirror() { at(self.n) } else { Pos(0) };
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

    /// The block's floor at its fastest head, `fast` frames a sample, and
    /// the window's end it keeps. It moves only with `inc`, and the bounds it
    /// sets latch at a wrap or a turn, under the crossfades every other
    /// change goes through (or while the head runs up, with nothing in play).
    fn floor(&mut self, fast: f32, k: &AtWrap<'_>) {
        self.min_len = shortest(k.rate, fast);
        self.we = window_end_min(
            self.next_ws.min(k.frames - 1),
            k.end,
            k.frames,
            self.min_len,
        );
    }

    /// The head has yet to reach its first event, so the first pass takes
    /// this block's bounds and floor (F1): bounds latched at note-on keep
    /// that moment's floor, a fade too short for the speed the head
    /// arrives at. Not if the event would fall behind the head.
    fn follow_next(&mut self) {
        let (b, ev) = (self.bounds, self.event);
        self.bounds = self.next;
        self.event = if self.model == TapeModel::PingPong {
            self.pp_event()
        } else {
            self.next.fade_at()
        };
        if self.heads[0].pos.whole() >= self.event {
            (self.bounds, self.event) = (b, ev);
        }
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
        self.off = if self.mirror() {
            self.off - n
        } else {
            self.off + n
        };
        self.set_mirror(!self.mirror());
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
                let m = self.mirror();
                self.heads = [
                    Head { pos: h, mirror: m },
                    Head {
                        pos: n - h0,
                        mirror: m,
                    },
                ];
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
    /// ramped by `ddf`. LOOP reversed and PING-PONG down track the read
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
        k: &AtWrap<'_>,
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
                (self.heads[0].pos, self.heads[1].pos) = (h0, h1);
                (self.gain, self.off) = (g, off);
                if MOVING {
                    self.df = df;
                }
                self.on_event(k);
                if self.leaves(MIRROR) {
                    return (i, cur);
                }
                [h0, h1] = self.heads.map(|h| h.pos);
                (ev, g, off) = (self.event, self.gain, self.off);
            }
        }
        (self.heads[0].pos, self.heads[1].pos) = (h0, h1);
        (self.gain, self.off) = (g, off);
        if MOVING {
            self.df = df;
        }
        (i, cur)
    }

    /// After an event, `run`'s frame no longer fits: the frame flipped, a
    /// flip fades, or a slow run starts. Out of line, so `run`'s loops
    /// compile as before the flip.
    #[inline(never)]
    fn leaves(&self, mirror: bool) -> bool {
        self.mirror() != mirror || self.flipping() || self.ghost.is_some() || self.turning.is_some()
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
        k: &AtWrap<'_>,
    ) -> (usize, u64) {
        let dg = self.clamp_dg;
        let mut gh = self.ghost.unwrap_or(Ghost {
            pos: Pos(0),
            lift: 0.0,
            back: false,
            done: 0,
            len: 1,
        });
        let mut g = self.gain;
        while i < BLOCK_SIZE {
            let (mirror, off) = (self.mirror(), self.off);
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
            if GHOST && gh.lift != 0.0 {
                lift(&mut out[start..i], gh.lift, g, dg);
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
                        self.end_half(t, k);
                    }
                    if hit || (turning.is_some() && self.turning.is_none()) {
                        self.on_event(k);
                    }
                    return (i, cur);
                }
                // An event may retarget the fade (`fade_from`): it reads
                // the fade's state from `self`, and it comes back here.
                self.ghost = Some(gh);
                self.gain = g;
            }
            if turning.is_some() {
                t.k += n as u16;
                self.turning = Some(t);
                if self.end_half(t, k) && !GHOST && self.ghost.is_some() {
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
            if let (true, Some(h)) = (GHOST, self.ghost) {
                (gh, g) = (h, self.gain);
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
    /// event is set. Out of line: `run_slow`'s loops compile leaner.
    #[inline(never)]
    fn end_half(&mut self, t: Turn, k: &AtWrap<'_>) -> bool {
        if t.k == t.th {
            let r = self.real(self.heads[0].pos);
            self.flip_at(r, k);
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
    fn next_bounds(&self, k: &AtWrap<'_>) -> Bounds {
        let (ws, we) = (self.next_ws.min(k.frames - 1), self.we);
        match self.model {
            TapeModel::Scrub => {
                let b = Bounds::of_min(
                    0,
                    we - ws,
                    (self.mac.size, 0.0, k.xfade),
                    k.rate,
                    self.min_len,
                );
                if self.mirror() { b.mirror(self.n) } else { b }
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
    fn on_event(&mut self, k: &AtWrap<'_>) {
        for _ in 0..MAX_EVENTS {
            let h = self.heads[0].pos;
            if h.whole() < self.event {
                return;
            }
            #[cfg(any(test, feature = "test-support"))]
            {
                self.events += 1;
            }
            self.runup = false;
            if self.model == TapeModel::PingPong {
                self.turn(k);
                continue;
            }
            let le = self.bounds.le;
            if let Some(f) = self.fade.take() {
                // The wrap: the second head plays on as the first, in its
                // own frame if it was a flip's.
                let flip = self.heads[1].mirror != self.mirror();
                self.heads[0] = self.heads[1];
                if flip {
                    self.off = if self.mirror() {
                        at(self.n - 1)
                    } else {
                        Pos(0)
                    };
                    self.ws = self.fade_ws;
                }
                self.bounds = f.next;
                self.gain = 0.0;
                self.event = self.bounds.fade_at();
            } else if h.whole() >= le {
                let nx = self.pending.take().unwrap_or(self.upcoming());
                let next = nx.b;
                let len = (next.frames() as u64) << 32;
                let over = (h - at(le)).0;
                let over = if over >= len { over % len } else { over };
                let pos = at(next.ls) + Pos(over);
                if nx.rev != self.mirror() {
                    // A hard splice that flips: the head going out plays on
                    // the old way under the clamp's fade.
                    self.flip_fade(h, pos, nx);
                    continue;
                }
                self.heads[0].pos = pos;
                self.bounds = next;
                self.event = next.fade_at();
            } else {
                let nx = self.pending.unwrap_or(self.upcoming());
                let next = nx.b;
                // SCRUB's loop has nothing before it: its fades fold.
                let ws = if self.model == TapeModel::Scrub {
                    next.ls
                } else {
                    nx.ws.min(k.frames - 1)
                };
                let rem = at(le) - h;
                match plan(rem, &next, ws, le) {
                    Plan::Fade { restart } => {
                        self.pending = None;
                        self.heads[1] = Head {
                            pos: restart - rem,
                            mirror: nx.rev,
                        };
                        self.fade_ws = nx.ws;
                        self.fade = Some(Fade::new(at(le), rem, next));
                        self.event = le;
                    }
                    Plan::Later(frame) => {
                        self.pending = Some(nx);
                        self.event = frame;
                    }
                    Plan::Splice => {
                        self.pending = Some(nx);
                        self.event = le;
                    }
                }
            }
        }
    }

    /// The next pass as worked out this block, in its frame.
    fn upcoming(&self) -> Next {
        Next {
            b: self.next,
            rev: self.next_rev,
            ws: self.next_ws,
        }
    }

    /// A flip where the pass splices hard (XFADE 0, or no room for its
    /// fade): the head at `h` plays on the old way while the output fades
    /// over `CLAMP_FADE_S` to a head at `pos` in the new frame, so a flip
    /// never clicks.
    #[cold]
    fn flip_fade(&mut self, h: Pos, pos: Pos, nx: Next) {
        let d = Pos::from_f32(self.clamp_len as f32 * self.v);
        let end = at((h + d).whole() + 1);
        self.heads[1] = Head {
            pos,
            mirror: nx.rev,
        };
        self.fade_ws = nx.ws;
        self.fade = Some(Fade::new(end, end - h, nx.b));
        self.gain = 0.0;
        self.event = end.whole();
    }

    /// A flip's fade: the two heads read the source each in its own frame
    /// (LOOP's: mirrored at `n − 1 − u`; the first head's `M0`, the
    /// second's the other), to the fade's end, where the wrap hands over
    /// (`on_event`). A whole block can sit in a long one, so it costs what
    /// a fade does.
    #[inline(never)]
    fn run_flip<const M0: bool>(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        (mut i, mut cur, step): (usize, u64, u64),
        k: &AtWrap<'_>,
    ) -> (usize, u64) {
        let Some(f) = self.fade else { return (i, cur) };
        let last = at(self.n - 1);
        let [mut h0, mut h1] = self.heads.map(|h| h.pos);
        let (ev, mut g) = (self.event, self.gain);
        while i < BLOCK_SIZE {
            let a = v.read(if M0 { last - h0 } else { h0 });
            g = f.gain(h0);
            out[i] = a + g * (v.read(if M0 { h1 } else { last - h1 }) - a);
            cur = cur.wrapping_add(step);
            h0 = h0 + Pos(cur);
            h1 = h1 + Pos(cur);
            i += 1;
            if h0.whole() >= ev {
                break;
            }
        }
        (self.heads[0].pos, self.heads[1].pos) = (h0, h1);
        self.gain = g;
        if h0.whole() >= ev {
            self.on_event(k);
        }
        (i, cur)
    }

    /// PING-PONG reached `event`: the run-up to an edge starts, so the
    /// head slows to a stop there and comes back (`TURN_S` each way, less in
    /// a short loop). With no room for one, the head reflects at the edge,
    /// `h = 2·edge − h`, and its direction flips (`flip_at`); run past it by
    /// more than a step, the output fades to the reflected head (a fade
    /// already running is retargeted).
    fn turn(&mut self, k: &AtWrap<'_>) {
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
        let past = (h - at(b.le)).0;
        let over = Pos(if past >= len { past % len } else { past });
        // The reflected head, in source frames.
        let r = if self.mirror() {
            at(b.ls) + over
        } else {
            at(b.le) - over
        };
        // Past the edge by more than a step (the speed rose mid-turn), the
        // reflected head reads where this one never played: the output
        // fades there, as a clamp's does.
        if (past >> 32) as f32 > self.v + 1.0 {
            self.fade_from(self.real(h), self.mirror(), k);
        }
        self.flip_at(r, k);
    }

    /// The head jumps from `pos` (source frames, going `back` or up): the
    /// output fades from there over `CLAMP_FADE_S`. A fade already running
    /// is retargeted (R1): its ghost restarts at `pos`, and what it still
    /// added to the output, `(1 − gain)·(ghost − head)`, rides on the new
    /// ghost as its `lift`, so the output carries on and fades from there.
    fn fade_from(&mut self, pos: Pos, back: bool, k: &AtWrap<'_>) {
        let lift = match self.ghost {
            Some(g) => (1.0 - self.gain) * (k.v.read(g.pos) + g.lift - k.v.read(pos)),
            None => 0.0,
        };
        self.ghost = Some(Ghost {
            pos,
            lift,
            back,
            done: 0,
            len: self.clamp_len,
        });
        self.gain = 0.0;
    }

    /// PING-PONG's frame for a head at `r` (source frames) going down
    /// (`back`) or up in loop `b`: its `u`, signed.
    fn enter(&mut self, back: bool, r: Pos, b: &Bounds) -> i64 {
        self.set_mirror(back);
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
        if self.mirror() { self.off - u } else { u }
    }

    /// Where PING-PONG's run-up to the pass's far edge starts: half the
    /// distance a turn at this speed takes, at most a quarter of the loop.
    fn pp_event(&self) -> u32 {
        let b = self.bounds;
        let half = self.v * self.turn_len as f32 * 0.5;
        b.le - (half as u32).min(b.frames() / 4)
    }

    /// The bounds a clamp's fade holds, lengthened to this block's floor
    /// within the window (F2), away from the edge the head turns at (going
    /// `back` from `le`, else from `ls`): they only grow round the head, so
    /// it stays inside and nothing moves while the fade runs.
    fn held(&self, back: bool) -> Bounds {
        let b = self.bounds;
        let ws = self.ws.min(self.n - 1);
        let len = self.min_len.min(self.we - ws);
        if b.frames() >= len {
            return b;
        }
        let ls = if back {
            b.le.saturating_sub(len).max(ws)
        } else {
            b.ls.min(self.we - len)
        };
        Bounds {
            ls,
            le: ls + len,
            lx: 0,
        }
    }

    /// PING-PONG turns at `r` (source frames): the frame mirrors, so the
    /// head runs forward in `u` again. New bounds latch here (while a clamp
    /// fades, only the floor: `held`). A head outside them that would not be
    /// in within `CLAMP_FADE_S` moves to the clamped place inside under a
    /// fade that long.
    fn flip_at(&mut self, r: Pos, k: &AtWrap<'_>) {
        let back = !self.mirror();
        // Where the head is before any jump: `r` at a turn's rest, the
        // overshooting head on a late reflection (whose fade is started).
        let pre = self.real(self.heads[0].pos);
        let nb = if self.ghost.is_some() {
            self.held(back)
        } else {
            self.next
        };
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
        // one fades the bounds hold, and the fade is retargeted from here
        // (unless a late reflection just started it here).
        let mut inside = u.clamp(lo, hi);
        // Below 0 (a loop shrunk under a head going down at POSITION 0) it
        // would wrap: clamped.
        if u >= 0 && (((inside - u) >> 32) as f32) < self.reach {
            inside = u;
        } else if self.ghost.is_none_or(|g| g.done > 0 || g.pos != pre) {
            self.fade_from(pre, back, k);
        }
        self.heads[0].pos = Pos(inside as u64);
        self.bounds = nb;
        self.event = self.pp_event();
    }
}
