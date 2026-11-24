// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

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
//! a table of single-cycle frames (`wave`). STRETCH plays the source over
//! LENGTH seconds at the key's pitch, in grains (`stretch`, ADR 0204).
//!
//! The engine itself is the `tape` feature's (ADR 0132): a build without
//! it links `off`'s silent stand-in. With `bank2` (the firmware's) the
//! engine, `TapeDeck`, is linked into bank 2 and bank 1 plays it through
//! `proxy` (ADR 0137). The params, sources and UI helpers stay either way.
#![cfg_attr(not(feature = "tape"), allow(dead_code, unused_imports))]

pub mod age;
mod desc;
pub mod drift;
pub mod heads;
pub mod machine;
pub mod oneshot;
pub mod params;
pub mod read;
pub mod run_out;
pub mod scrub;
#[cfg(feature = "tape")]
pub mod span;
pub mod stretch;
pub mod tron;
pub mod warble;
pub mod wave;
pub mod window;

#[cfg(not(feature = "tape"))]
mod off;
#[cfg(not(feature = "tape"))]
pub use off::TapeEngine;
#[cfg(feature = "bank2")]
mod proxy;
#[cfg(feature = "bank2")]
pub use proxy::{TapeDoor, TapeEngine};
/// Without `bank2`, the deck itself (with it, `proxy`'s, ADR 0137).
#[cfg(all(feature = "tape", not(feature = "bank2")))]
pub type TapeEngine = TapeDeck;

use core::mem::MaybeUninit;
use core::ops::Range;

use chimera_hal::{BLOCK_SIZE, MidiNote, SAMPLE_RATE};

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
use crate::stream::admit::ReadRatio;
#[cfg(feature = "tape")]
use crate::stream::cursor::{Cursor, DROP_FADE, Duck, HeadIdx, Streamed, Wrap};
use crate::stream::port::StreamPort;
#[cfg(feature = "tape")]
use crate::stream::stage::xfade_max;
#[cfg(feature = "tape")]
use crate::stream::units::Frames;
use crate::stream::units::{Cents, Inc, SourceFrame, SourceRate};
use age::Age;
use drift::{DriftRng, Place as DriftPlace};
use heads::{
    Bounds, Dir, Fade, Ghost, Head, POWER_FADE_S, Plan, Sizing, Turn, Window, at, min_frames, plan,
    power_of, shortest, window_end_min, window_start, xmix,
};
use oneshot::slice_bounds;
use read::{Descriptor, Pos, SourceView};
#[cfg(feature = "tape")]
use span::{Reads, Stop, StreamRow};
use warble::Warble;
use wave::Wave;

pub use heads::loop_len;
pub use oneshot::SliceCode;
pub use params::{
    Choice, DIRECTION_NAMES, Direction, Drift, FADE_HOLD, KeyMode, MACHINE_NAMES, MODEL_NAMES,
    MachineIndex, NeedsRam, RetiredModel, Retrig, STRETCH_DEFAULTS, Seeks, StretchKnobs,
    StretchType, TAPE_ENGINE_SPECS, TapeEngineParams, TapeMachine, TapeModel, Tron, Way, Wow,
    at_root, backward, cuts, drag_s, fade_default, jogs, needs_ram, reads,
};

// Each model's bill: its measured worst a sample, the M7's static count
// (instructions × 1.46 × 1.1, + 12 a head for `inc` 16's misses), rounded
// up to 10 (spec § 5). The bench's cold rows hold them (TAPE Task 14).
/// LOOP either way, its loop's floor (ADR 0211): a pass every 256 output
/// samples or more, its fade summing to 1. A shorter pass, or an
/// equal-power fade, bills `heads::loop_extra` over it at note-on: the
/// unit reads (2026-10-22, TAPE COLD bench, rev V at 480 MHz, less the
/// chain's 36, TAPE's 10 and AGE's 30) a pass every 4 samples 816, every
/// 16 samples 588, and a 40 ms loop's equal-power fade every 62 samples
/// 699; a pass's cost `512 + 1,216 / spw`, an equal-power fade 10 more a
/// sample and 10,500 a pass.
pub const COST_LOOP: Cost = Cost(520);
/// ONESHOT: a short slice's end ramp, retriggered, 135.4 → 229.5. The unit
/// reads 1SHOT 418 (2026-10-22, TAPE COLD bench, rev V at 480 MHz, less the chain's 36, TAPE's 10 and AGE's 30): 345.
pub const COST_ONESHOT: Cost = Cost(345);
/// TRON goes round its window as LOOP does (ADR 0203): LOOP's floor and
/// its bands; struck and let go every block on a 64-frame source the unit
/// reads 1,035 (`heads::loop_extra`'s tiny band).
pub const COST_TRON: Cost = COST_LOOP;
/// PING-PONG clamping in its shortest legal loop, a turn every 16 samples:
/// the unit reads 480 (PING 256). Under 16 samples a turn (a source
/// shorter than the loop's floor), 567: `heads::loop_extra`.
pub const COST_PING: Cost = Cost(490);
/// SCRUB fading in its shortest loop, an ease in, 283.4 → 479.
pub const COST_SCRUB: Cost = Cost(480);
/// TRANSWAVE steady: two heads, 232.9 → 398. The unit reads WAVE 530
/// (2026-10-22, TAPE COLD bench, rev V at 480 MHz, less the chain's 36, TAPE's 10 and AGE's 30): 455.
pub const COST_WAVE: Cost = Cost(455);
/// TRANSWAVE's level change over `COST_WAVE`: four heads, 432.9 → 743,
/// billed 770 in all. TAPE's `glide_cost` term.
pub const COST_WAVE_LEVEL: Cost = Cost(370);
/// STRETCH CLASSIC either way, a short source's lifted wraps, 442.5. The
/// unit reads MONO 613, POLY 607 and with no PERIOD (`power_of` at every
/// splice, #771) 612 (2026-10-22, TAPE COLD bench, rev V at 480 MHz, less the chain's 36, TAPE's 10 and AGE's 30): 540.
pub const COST_STRETCH: Cost = Cost(540);
/// STRETCH GRAIN (ADR 0204): four grains of the core's run loop, 84
/// instructions a sample each (`tools/loop_count.py`, 135 cycles), and the
/// block's windows, level and power reads, 25: 565. The unit reads 1,020
/// with the source's PERIOD and 1,019 with its own (2026-10-22, TAPE COLD bench, rev V at 480 MHz, less the chain's 36, TAPE's 10 and AGE's 30): 945.
pub const COST_STRETCH_GRAIN: Cost = Cost(945);
/// TAPE when it runs (`tape_runs`): the transport, 1.2 → 1.9.
pub const COST_TAPE_RUNS: Cost = Cost(10);
/// AGE when it runs (`age::age_runs`): the wear's loop and block, 15.9 →
/// 25.5.
pub const COST_AGE: Cost = Cost(30);
/// A streamed source on ONE's, LOOP's and TRON's bill: span switching,
/// slot checks, the ports and the block's re-bill (`refresh_bills`, #596).
/// Measured on the unit (TAPE STREAM bench, 2026-10-18, ADR 0013): LOOP
/// and HIT both +100 a voice over the same eight on RAM sources; the gain's
/// `vdiv.f32` a sample (14 cycles, not pipelined, the ELF's `render_reads`)
/// is gone since, and at rest so is its loop (#698): 100 − 14, rounded up.
/// The unit's bench re-measures it once the card's stress run frees it.
pub const COST_STREAM: Cost = Cost(90);
/// TRANSWAVE's pitch can cross a level (`COST_WAVE_LEVEL`) in a note past
/// this many cents: the hysteresis' 0.35 to 0.715 step, ±6.2 st.
pub const LEVEL_CENTS: f32 = 620.0;

/// `inc`'s clamp: four octaves over a 48 kHz source's own speed, the
/// cursor's bound too.
pub const INC_MAX: f32 = Inc::MAX.get();
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
/// SCRUB's aim: POSITION through two one-poles of 20 ms each ahead of
/// DRAG's (ADR 0200, two since ADR 0205). A step of POSITION (a detent, a
/// UI pass's jog, a CC) moves the aim, never the finger's speed at once;
/// two poles of 20 ms delay it as one of 40 ms did and hold a 33 ms UI
/// pass's staircase to under half its swing.
pub const AIM_S: f32 = 0.020;
pub use scrub::{KEY_PLACES, LoopTime, Place, SCRUB_LEN_MAX_S, key_place};
/// The macros' share of the way each block: MODAL's (ADR 0056).
const EASE: f32 = 0.3;
/// TAPE or AGE easing to 0 is 0 under this, so the transport or the wear
/// stops and its bill with it (0.03 cents of wow; hiss at −102 dBFS, the
/// one-pole a wire).
const MACRO_SETTLE: f32 = 1e-3;
/// The block rate the transport runs at: the hardware's, so its rates are
/// constants (`Age` takes 48 kHz too).
const BLOCK_RATE: f32 = (SAMPLE_RATE as usize / BLOCK_SIZE) as f32;

/// Each model's measured worst, either way round (spec § 5): bank 1's
/// table, which the proxy reads without the door.
pub const fn model_cost(m: Way) -> Cost {
    match m {
        Way::One => COST_ONESHOT,
        Way::Loop => COST_LOOP,
        Way::Ping => COST_PING,
        Way::Tron => COST_TRON,
        Way::Scrub => COST_SCRUB,
        Way::Wave => COST_WAVE,
        Way::Stretch => COST_STRETCH,
    }
}

/// STRETCH's worst by its TYPE (ADR 0204): CLASSIC's heads, or GRAIN's
/// four grains.
pub const fn stretch_cost(k: StretchType) -> Cost {
    match k {
        StretchType::Mono | StretchType::Poly => COST_STRETCH,
        StretchType::Grain => COST_STRETCH_GRAIN,
    }
}

/// `model_cost` on a streamed source: WALK forward streams.
pub const fn streamed_cost(m: Way) -> Cost {
    Cost(model_cost(m).0 + COST_STREAM.0)
}

/// The fastest read ratio a streamed note on `p` reaches (slice 1 § 5.5,
/// #585): `cents` over its key (PITCH, TUNING, the routes' live reach and
/// wow when TAPE runs) and the bend; SLICE NOTE plays at ROOT. MOTION and DRIFT move
/// WALK's loop, never its speed, so they add nothing. A glide's reach is
/// the caller's: it passes the note the glide can start from.
pub fn ratio_max(p: &TapeEngineParams, note: MidiNote, cents: Cents, bend: Cents) -> ReadRatio {
    let root = if params::at_root(p) {
        f32::from(note.get()) * 100.0
    } else {
        p.root
    };
    crate::stream::admit::ratio_max(note, Cents::new(root), cents, bend)
}

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

/// A retargeted clamp's `hold` (R1) on its samples `out`: each moves to
/// `held` by the ghost's share, `y + (1 − gain)·(held − y)`, where the last
/// sample's gain is `g` and it rises `dg` a sample. A convex mix, so the
/// output never passes `held` or the reads, however often the fade is
/// retargeted (a constant lift on a moving read random-walked to +6.6 dB).
/// It runs after the clamp's loop, not in it, and only while a hold is set:
/// the plain clamp pays nothing for it.
#[inline(always)]
fn hold(out: &mut [f32], held: f32, g: f32, dg: f32) {
    let mut w = 1.0 - g;
    for y in out.iter_mut().rev() {
        *y += w * (held - *y);
        w += dg;
    }
}

#[cfg(feature = "tape")]
impl TapeDeck {
    /// A note's first `NOTE_ON_RAMP` samples ramp from `last` by a
    /// smoothstep, so a START inside a waveform never steps and the ramp has
    /// no corner where it lands: a fast attack is near full scale there (a
    /// linear ramp bent TX BASS 7× its own, #385). Once a note, out of line;
    /// `TapeDeck`'s, so bank 2's.
    #[cold]
    #[inline(never)]
    fn note_on_ramp(out: &mut [f32; BLOCK_SIZE], last: f32) {
        for (i, y) in out[..NOTE_ON_RAMP as usize].iter_mut().enumerate() {
            let x = i as f32 / NOTE_ON_RAMP as f32;
            *y = last + (*y - last) * (x * x * (3.0 - 2.0 * x));
        }
    }
}

/// Frames a head advances each output sample:
/// `rate / sample_rate · 2^((note·100 + pitch − root) / 1200)`, clamped
/// (`Inc`).
pub fn inc(rate: SourceRate, note: MidiNote, pitch: Cents, root: Cents, sample_rate: u32) -> Inc {
    let oct = clampf(
        (f32::from(note.get()) * 100.0 + pitch.get() - root.get()) / 1_200.0,
        -40.0,
        40.0,
    );
    Inc::new(f32::from(rate) / sample_rate as f32 * exp2(oct))
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
/// The word is private: built by `new`, read by `head` and `second`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heads(u64);

impl Heads {
    /// No note: nothing drawn.
    pub const NONE: Heads = Heads(u64::MAX);

    /// The word, for `PartHeads` to store and load.
    pub(crate) const fn from_raw(w: u64) -> Heads {
        Heads(w)
    }

    pub(crate) const fn raw(self) -> u64 {
        self.0
    }

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
    rate: SourceRate,
    /// The source: a retargeted fade reads what it plays (`fade_from`).
    v: &'a SourceView<'a>,
    /// What a fade's second head reads (`Reads::second`).
    v1: &'a SourceView<'a>,
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
pub struct Strip {
    wow: Transport,
    rng: u32,
    age: Age,
    warble: Warble,
}

/// A source's row as a block reads it: held whole, or streamed (its
/// descriptor, read once).
#[cfg(feature = "tape")]
enum Row<'v> {
    Whole(SourceView<'v>),
    Streamed(&'v [u8], Descriptor),
}

/// A deck's note: none, one waiting on its moving source, or sounding,
/// its row read whole or streamed through the voice's ring. One flat tag
/// (a byte): bank 1 reads `streams` and `is_active` off it every block.
#[cfg(feature = "tape")]
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
enum Note {
    Idle,
    /// A note-on whose source a closed gate moves: silent, it starts
    /// (`note_on`) on the first block its row reads (spec § 4), catching
    /// up the blocks it waited on (#595).
    Gated {
        waited: u32,
    },
    /// Sounding from its row, held whole.
    Ram,
    /// Sounding through the cursor.
    Streamed(Stream),
    /// A streamed note ended this block: the block's end stops its cursor
    /// (`end_stream`), with a port or without.
    Ending(Stream),
}

#[cfg(feature = "tape")]
impl Note {
    /// A note sounds or waits on its source; one that ended this block
    /// (`Ending`) does not. The one meaning the deck and bank 1's proxy
    /// both read, inlined into each.
    #[inline(always)]
    fn is_active(&self) -> bool {
        matches!(self, Note::Gated { .. } | Note::Ram | Note::Streamed(_))
    }
}

/// A streamed note's own: its rate, and the fastest step a sample its
/// cursor plans for (`inc_max`).
#[cfg(feature = "tape")]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stream {
    rate: SourceRate,
    inc_cap: Inc,
    /// The fade in play started before its second head's reads landed: it
    /// reads silence to its end (`Reads::mute_second`).
    mute: bool,
}

/// TRON's tape: none with TRON off, so no run-out can be counted then;
/// rolling with its output samples left; or run out, the note falling. One
/// word: its states are built only by these, read only by `state`.
#[cfg(feature = "tape")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tape(u32);

#[cfg(feature = "tape")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TapeState {
    Off,
    Rolling(u32),
    Out,
}

#[cfg(feature = "tape")]
impl Tape {
    const OFF: Tape = Tape(u32::MAX);
    const OUT: Tape = Tape(0);

    /// `left` output samples, at least one and short of `OFF`'s word.
    fn rolling(left: u32) -> Tape {
        Tape(left.clamp(1, u32::MAX - 1))
    }

    fn state(self) -> TapeState {
        match self.0 {
            u32::MAX => TapeState::Off,
            0 => TapeState::Out,
            n => TapeState::Rolling(n),
        }
    }
}

/// How a TRON note plays its window: as its DIRECTION does (around a
/// loop), or once through round again, forward or flipped on REVERSE.
#[cfg(feature = "tape")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Round {
    No,
    Window,
    Reversed,
}

/// The machine a note plays, and what only it keeps. STRETCH's grains are
/// the largest; the deck holds one state in place, no heap, its size held
/// under 1 KB (`proxy`).
#[cfg(feature = "tape")]
#[derive(Clone, Copy)]
#[allow(clippy::large_enum_variant)]
enum MachineState {
    One(Once),
    Loop,
    Ping(PingPass),
    Scrub(Scrub),
    Stretch(stretch::Note),
    Wave(Wave),
}

/// ONE's note: the window's end in the head's frame.
#[cfg(feature = "tape")]
#[derive(Clone, Copy)]
struct Once {
    end: u32,
}

/// PING-PONG's: its clamp in progress and its turn; `inc`'s reciprocal at
/// the block's end, a turn's longest half (samples), the clamp's step a
/// sample, and the distance (frames) a head outside covers in one clamp.
#[cfg(feature = "tape")]
#[derive(Clone, Copy)]
struct PingPass {
    ghost: Option<Ghost>,
    turning: Option<Turn>,
    inv_v: f32,
    turn_len: u16,
    clamp_dg: f32,
    reach: f32,
}

/// SCRUB's finger's move a sample, or its change a sample: signed 32.32
/// frames. The run's loop adds it to the heads' `u64` steps wrapping
/// (`wrapping`, two's complement), so it is stored signed and read signed.
#[cfg(feature = "tape")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct FingerStep(i64);

#[cfg(feature = "tape")]
impl FingerStep {
    /// The run loop's bits.
    #[inline(always)]
    fn wrapping(self) -> u64 {
        self.0 as u64
    }

    /// From the run loop's bits.
    #[inline(always)]
    fn of_wrapping(x: u64) -> FingerStep {
        FingerStep(x as i64)
    }
}

/// SCRUB's: the finger's move each sample (wrapping) and its change a
/// sample, which ramps across the block as `inc` does; the tape stopped,
/// the output's share (linear before its smoothstep) and its move a block
/// (`HUSH_S`); the loop's speed in frames a sample at the last block's
/// end, signed, and DRAG's one-pole share a block.
#[cfg(feature = "tape")]
#[derive(Clone, Copy)]
struct Scrub {
    df: FingerStep,
    ddf: FingerStep,
    stopped: bool,
    hush: f32,
    hush_step: f32,
    speed: f32,
    pull: f32,
    /// Where the finger heads: POSITION through two one-poles of `AIM_S`
    /// (`lead`, then `aim`), `aim_pull` a block each, so the finger's
    /// speed never steps when POSITION does (a detent, a UI pass, a CC).
    lead: Pos,
    aim: Pos,
    aim_pull: f32,
    /// KEY POSITION's place, which FINGER is added to, so the finger
    /// starts at the key's place (FINGER 0) and moves from it; `None` at
    /// KEY PITCH, where FINGER alone places it.
    key: Option<Place>,
}

#[cfg(feature = "tape")]
pub struct TapeDeck {
    /// The source a note plays, latched at note-on (ADR 0087): never
    /// re-resolved, its row read each block.
    src: Option<SourceId>,
    state: Note,
    /// The machine and its own state, and whether FADE holds, latched at
    /// note-on.
    machine: MachineState,
    hold: bool,
    /// The head, and the head a fade reads into, each in its frame `u`:
    /// mirrored, the source is read at `off − u`, else at `u` (`off + u`
    /// in SCRUB). They differ only while a flip fades (LOOP's END moved
    /// across START), the second head's `off` then its own frame's.
    heads: [Head; 2],
    /// LOOP reversed: the source's last frame. ONESHOT and TRON reversed:
    /// `lo + hi − 1` of the window. PING-PONG down: `ls + le`. SCRUB: the
    /// finger, plus the source's length when mirrored.
    off: Pos,
    /// The source's frames, latched at note-on.
    n: u32,
    /// `inc` at the block's end, and a clamp's length (samples): a LOOP
    /// flip's fade and PING-PONG's clamp take it.
    v: f32,
    clamp_len: u32,
    /// Events handled: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    events: u32,
    /// Frames a sample, ramped across the block.
    inc: Ramp,
    mac: Macros,
    /// The note's first block is to come: the macros are taken whole.
    first: bool,
    /// DRIFT's place: it steps at each note-on and wanders from note to
    /// note (`drift::step`), at rest with DRIFT off.
    drift: DriftPlace,
    /// DRIFT's draws, this voice's own: seeded, so a render repeats.
    drift_rng: DriftRng,
    /// TRON's tape, latched at note-on, and whether its window comes round.
    tape: Tape,
    round: Round,
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
    /// The release's gain and its fall a sample (1 until key up), and
    /// FADE's fall: TRON's, and WALK's but at HOLD.
    rel: f32,
    rel_k: f32,
    fade_k: f32,
    /// `params::at_root` at note-on: ONE under SLICE NOTE, the note picks
    /// the slice and plays at ROOT. The deck's one SLICE NOTE test.
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
    note: MidiNote,
    /// PIT's PITCH and FINE with their routes.
    pitch: Cents,
    /// A glide steal's ratio, gliding to 1.
    slide: Glide,
    /// ROOT, latched at note-on.
    root: Cents,
    /// TAPE: this voice's transport, run once a block, at its own rates
    /// at depth 1; its speed deviation this block, cents.
    wow: Transport,
    tap: Tap,
    /// The voice's own draws: each note's wow rate.
    rng: u32,
    cents: Cents,
    /// AGE: this voice's wear.
    age: Age,
    /// WOW, latched at note-on, and WARBLE's draws, this voice's own.
    wow_kind: Wow,
    warble: Warble,
    /// The transport is skipped: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    no_transport: bool,
    /// A streamed note's cursor (slice 1 § 7.3): its `Note::Streamed`
    /// plays through it.
    cursor: Cursor,
    /// A LOOP wrap moved the head this block: the cursor starts there.
    jumped: bool,
    /// The voice fades out before a wrap no head holds. It outlives its
    /// note, as it always has: the next streamed note's first block may
    /// duck back in.
    ducked: bool,
    /// Streamed views check every tap: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    strict: bool,
}

#[cfg(feature = "tape")]
impl TapeDeck {
    fn idle() -> TapeDeck {
        TapeDeck {
            src: None,
            state: Note::Idle,
            machine: MachineState::Loop,
            hold: false,
            heads: [Head::default(); 2],
            off: Pos(0),
            n: 2,
            v: 1.0,
            clamp_len: 240,
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
            drift: DriftPlace::ZERO,
            drift_rng: DriftRng::IDLE,
            tape: Tape::OFF,
            round: Round::No,
            ws: 0,
            bounds: Bounds::plain(0, 1),
            min_len: MIN_LOOP_FRAMES,
            we: 1,
            runup: false,
            next: Bounds::plain(0, 1),
            next_rev: false,
            next_ws: 0,
            pending: None,
            fade_ws: 0,
            start: 0.0,
            rel: 1.0,
            rel_k: 1.0,
            fade_k: 1.0,
            at_root: false,
            fade: None,
            event: 0,
            gain: 0.0,
            ramp: false,
            last: 0.0,
            note: MidiNote::clamped(60),
            pitch: Cents::ZERO,
            slide: Glide::new(1.0, 1.0),
            root: Cents::new(6_000.0),
            wow: Transport::new(),
            tap: Tap::speed_cents(1.0, BLOCK_RATE),
            rng: 0x510E_527F,
            cents: Cents::ZERO,
            age: Age::new(),
            wow_kind: Wow::Capstan,
            warble: Warble::new(),
            #[cfg(any(test, feature = "test-support"))]
            no_transport: false,
            cursor: Cursor::new(),
            jumped: false,
            ducked: false,
            #[cfg(any(test, feature = "test-support"))]
            strict: false,
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
        self.drift_rng = DriftRng::seed(s);
        self.age.seed(s);
        self.warble.seed(s);
    }

    /// The strip a rebuilt slot keeps (`restore`).
    pub fn strip(&self) -> Strip {
        Strip {
            wow: self.wow,
            rng: self.rng,
            age: self.age,
            warble: self.warble,
        }
    }

    /// A fresh engine on the voice's own strip of tape, as it last ran.
    pub fn restore(&mut self, s: Strip) {
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

    /// A streamed note owns its voice's ring, from its start to its end.
    pub fn streams(&self) -> bool {
        matches!(self.state, Note::Streamed(_))
    }

    /// The streamed note's own, to the block's end once it ended.
    fn stream(&self) -> Option<&Stream> {
        match &self.state {
            Note::Streamed(s) | Note::Ending(s) => Some(s),
            _ => None,
        }
    }

    fn stream_mut(&mut self) -> Option<&mut Stream> {
        match &mut self.state {
            Note::Streamed(s) | Note::Ending(s) => Some(s),
            _ => None,
        }
    }

    /// The note reads through the cursor this block.
    fn fed(&self) -> bool {
        self.stream().is_some()
    }

    /// The fastest step a sample: `Inc::MAX`, or a streamed note's cap.
    fn inc_cap(&self) -> Inc {
        self.stream().map_or(Inc::MAX, |s| s.inc_cap)
    }

    fn set_cap(&mut self, c: Inc) {
        let s = self.stream_mut();
        debug_assert!(s.is_some(), "a cap off a stream");
        if let Some(s) = s {
            s.inc_cap = c;
        }
    }

    fn set_mute(&mut self, m: bool) {
        let s = self.stream_mut();
        debug_assert!(s.is_some(), "a mute off a stream");
        if let Some(s) = s {
            s.mute = m;
        }
    }

    /// Path `m` before its note-on sets it up.
    fn idle_machine(m: Way) -> MachineState {
        let once = Once { end: 1 };
        match m {
            Way::One => MachineState::One(once),
            Way::Loop => MachineState::Loop,
            Way::Ping => MachineState::Ping(PingPass {
                ghost: None,
                turning: None,
                inv_v: 1.0,
                turn_len: 48,
                clamp_dg: 1.0 / 240.0,
                reach: 241.0,
            }),
            // TRON goes round its window until the tape runs out.
            Way::Tron => MachineState::Loop,
            Way::Scrub => MachineState::Scrub(Scrub {
                df: FingerStep(0),
                ddf: FingerStep(0),
                stopped: false,
                hush: 1.0,
                hush_step: 1.0,
                speed: 0.0,
                pull: 1.0,
                lead: Pos(0),
                aim: Pos(0),
                aim_pull: 1.0,
                key: None,
            }),
            Way::Stretch => MachineState::Stretch(stretch::IDLE),
            Way::Wave => MachineState::Wave(Wave::IDLE),
        }
    }

    /// The path latched at the last note-on.
    fn tape_machine(&self) -> Way {
        match self.machine {
            MachineState::One(_) => Way::One,
            MachineState::Loop if self.round != Round::No => Way::Tron,
            MachineState::Loop => Way::Loop,
            MachineState::Ping(_) => Way::Ping,
            MachineState::Scrub(_) => Way::Scrub,
            MachineState::Stretch(_) => Way::Stretch,
            MachineState::Wave(_) => Way::Wave,
        }
    }

    fn once_state(&self) -> Option<&Once> {
        match &self.machine {
            MachineState::One(o) => Some(o),
            _ => None,
        }
    }

    fn pp(&self) -> Option<&PingPass> {
        match &self.machine {
            MachineState::Ping(pp) => Some(pp),
            _ => None,
        }
    }

    fn pp_mut(&mut self) -> Option<&mut PingPass> {
        match &mut self.machine {
            MachineState::Ping(pp) => Some(pp),
            _ => None,
        }
    }

    /// PING-PONG's clamp in progress.
    fn ghost(&self) -> Option<Ghost> {
        self.pp().and_then(|pp| pp.ghost)
    }

    fn set_ghost(&mut self, g: Option<Ghost>) {
        let pp = self.pp_mut();
        debug_assert!(pp.is_some(), "a clamp off PING-PONG");
        if let Some(pp) = pp {
            pp.ghost = g;
        }
    }

    /// PING-PONG's turn in progress.
    fn turning(&self) -> Option<Turn> {
        self.pp().and_then(|pp| pp.turning)
    }

    fn set_turning(&mut self, t: Option<Turn>) {
        let pp = self.pp_mut();
        debug_assert!(pp.is_some(), "a turn off PING-PONG");
        if let Some(pp) = pp {
            pp.turning = t;
        }
    }

    fn scrub(&self) -> Option<&Scrub> {
        match &self.machine {
            MachineState::Scrub(sc) => Some(sc),
            _ => None,
        }
    }

    fn scrub_mut(&mut self) -> Option<&mut Scrub> {
        match &mut self.machine {
            MachineState::Scrub(sc) => Some(sc),
            _ => None,
        }
    }

    /// SCRUB's finger's move a sample and its change; 0 off SCRUB.
    fn finger(&self) -> (u64, u64) {
        self.scrub()
            .map_or((0, 0), |sc| (sc.df.wrapping(), sc.ddf.wrapping()))
    }

    fn set_df(&mut self, df: u64) {
        let sc = self.scrub_mut();
        debug_assert!(sc.is_some(), "a finger off SCRUB");
        if let Some(sc) = sc {
            sc.df = FingerStep::of_wrapping(df);
        }
    }

    /// The note is over: a streamed one's cursor stops at the block's end.
    fn end(&mut self) {
        self.state = match self.state {
            Note::Streamed(s) | Note::Ending(s) => Note::Ending(s),
            _ => Note::Idle,
        };
    }

    /// The latched source's row this block, its descriptor read once:
    /// `None` with none latched, its row empty or moving, or a descriptor
    /// that doesn't read.
    fn row_of<'v>(&self, src: &'v GateView<'_>) -> Option<Row<'v>> {
        let held = src.bytes(self.src?)?;
        let d = Descriptor::from_bytes(held)?;
        if d.is_streamed() {
            Some(Row::Streamed(held, d))
        } else {
            SourceView::of(held, d).map(Row::Whole)
        }
    }

    /// `note_on` on the latched source through the block's view. With no
    /// source to read, or one `p`'s model can't play (`wave::playable`), the
    /// engine stays idle. One a closed gate moves waits, active and silent,
    /// and starts on the first block after the gate opens (`play`). A
    /// streamed source plays through `streams` (`start_streamed`).
    pub fn start(
        &mut self,
        note: u8,
        p: &TapeEngineParams,
        src: &GateView<'_>,
        sample_rate: u32,
        streams: Option<&mut StreamPort<'_, '_>>,
    ) {
        self.start_from(MidiNote::clamped(note), p, src, sample_rate, streams, 0);
    }

    /// `start`, a gated note `waited` blocks after its note-on.
    fn start_from(
        &mut self,
        note: MidiNote,
        p: &TapeEngineParams,
        src: &GateView<'_>,
        sample_rate: u32,
        mut streams: Option<&mut StreamPort<'_, '_>>,
        waited: u32,
    ) {
        if self.fed()
            && let Some(port) = streams.as_deref_mut()
        {
            self.cursor.stop(port);
        }
        let v = match self.row_of(src) {
            Some(Row::Streamed(held, d)) => {
                self.start_streamed(note, p, (held, d), (sample_rate, waited), streams);
                return;
            }
            Some(Row::Whole(v)) => Some(v),
            None => None,
        };
        match v {
            Some(v) if wave::playable(p.way(), &v) => self.note_on(note.get(), p, &v, sample_rate),
            None if self.src.is_some_and(|id| src.moving(id)) => {
                self.note = note;
                if self.tape_machine() != p.way() {
                    self.machine = Self::idle_machine(p.way());
                }
                self.hold = params::hold(p);
                self.state = Note::Gated { waited: 0 };
            }
            _ => self.state = Note::Idle,
        }
    }

    /// A note on a streamed row (slice 1 § 7): WALK ONE or a forward LOOP
    /// only, through the voice's rings. Its window is worked out over the
    /// file's frames; the cursor starts on START's head, cold if that
    /// doesn't hold where the head starts.
    #[inline(never)]
    fn start_streamed(
        &mut self,
        note: MidiNote,
        p: &TapeEngineParams,
        held: (&[u8], Descriptor),
        (sample_rate, waited): (u32, u32),
        streams: Option<&mut StreamPort<'_, '_>>,
    ) {
        self.state = Note::Idle;
        let (Some(port), None) = (streams, params::needs_ram(p)) else {
            return;
        };
        let Some(stream) = self.stream_of(port) else {
            return;
        };
        let Some(row) = self.row(held, stream.frames) else {
            return;
        };
        let Some(head) = row.head_span(HeadIdx::Start) else {
            return;
        };
        // Reads nothing: the window from the file's frames and the rate.
        self.note_on(note.get(), p, &head.view, sample_rate);
        let rate = row.desc().rate();
        let r = port.ratio_max.unwrap_or_else(|| self.own_ratio(note));
        self.state = Note::Streamed(Stream {
            rate,
            inc_cap: Self::cap_at(rate, r, sample_rate),
            mute: false,
        });
        if waited > 0 {
            self.catch_up(waited, sample_rate);
            if !self.is_active() {
                // Ended before its cursor started: nothing to stop.
                self.state = Note::Idle;
                return;
            }
        }
        let at = self.heads[0].pos.whole();
        self.cursor.start(
            SourceFrame::new(at),
            self.inc_cap(),
            row.head(HeadIdx::Start),
            stream,
            port,
        );
    }

    /// A streamed note's cap at `rate` and read ratio `r`, frames a sample.
    fn cap_at(rate: SourceRate, r: ReadRatio, sample_rate: u32) -> Inc {
        Inc::new(f32::from(rate) / sample_rate as f32 * r.get())
    }

    /// An unbilled port's ratio (the bench's, the deck tests'): `note` at
    /// its pitch, and wow. SLICE NOTE plays at ROOT.
    fn own_ratio(&self, note: MidiNote) -> ReadRatio {
        let root = if self.at_root {
            Cents::new(f32::from(note.get()) * 100.0)
        } else {
            self.root
        };
        let wow = crate::stream::admit::WOW_CENTS.get();
        crate::stream::admit::ratio_max(note, root, Cents::new(self.pitch.get() + wow), Cents::ZERO)
    }

    /// A note that waited `blocks` on a moving source plays from where
    /// tape time has reached (#595), its cursor starting there, cold if no
    /// head holds it: ONE to its end, LOOP up to its first event (a gate
    /// moves for a few blocks).
    fn catch_up(&mut self, blocks: u32, sample_rate: u32) {
        let Some(&Stream { rate, .. }) = self.stream() else {
            return;
        };
        let i = inc(
            rate,
            self.note,
            Cents::new(self.pitch.get() + self.tune().get()),
            self.root,
            sample_rate,
        )
        .get();
        // `as` saturates: never past `u32`.
        let frames = (blocks as f32 * BLOCK_SIZE as f32 * i) as u32;
        let to = self.heads[0].pos.whole().saturating_add(frames);
        let to = if let Some(&o) = self.once_state() {
            if to >= o.end {
                self.end();
                return;
            }
            to
        } else {
            to.min(self.event.saturating_sub(1))
                .max(self.heads[0].pos.whole())
        };
        for h in &mut self.heads {
            h.pos = at(to);
        }
    }

    /// What the next wrap reads, `from..=to`: the fade's second head from
    /// where it starts (`heads::plan`: the fade's length before the next
    /// loop, or from its start, folded), through the wrap's landing and a
    /// block's travel and the taps after it.
    fn next_reads(&self) -> (u32, u32) {
        let travel = libm::ceilf(self.inc_cap().get() * BLOCK_SIZE as f32) as u32 + 3;
        let le = self.bounds.le();
        if self.fade.is_some() {
            let h1 = self.heads[1].pos.whole();
            let rem = le.saturating_sub(self.heads[0].pos.whole());
            return (h1, h1.saturating_add(rem + travel));
        }
        // Put off to a later event, or this block's.
        let (b, at) = match self.pending {
            Some(n) => (n.b, self.event.min(le)),
            None => (self.next, self.bounds.fade_at()),
        };
        let rem = le.saturating_sub(at);
        let from = if rem <= b.ls().saturating_sub(self.ws) {
            b.ls() - rem
        } else {
            b.ls()
        };
        (from, from.saturating_add(rem + travel))
    }

    /// The next wrap lands on a head: the fade's second head reads it from
    /// `XFADE` before the loop, and the head after the wrap from there on.
    fn lands(&self, row: &StreamRow<'_>) -> bool {
        let (from, to) = self.next_reads();
        let holds = |s: Option<span::Span<'_>>| s.is_some_and(|s| s.holds(from) && s.holds(to));
        holds(row.head_span(HeadIdx::Loop))
            || holds(self.cursor.head().and_then(|h| row.head_span(h.idx)))
    }

    /// The latched source's stream: its file's frames from the card's
    /// lookup by its own id, never the Part's (#594).
    fn stream_of(&self, port: &StreamPort<'_, '_>) -> Option<Streamed> {
        let id = self.src?;
        Some(Streamed {
            id,
            frames: (port.frames)(id)?,
        })
    }

    fn row<'v>(&self, (held, d): (&'v [u8], Descriptor), frames: Frames) -> Option<StreamRow<'v>> {
        let row = StreamRow::new(held, d, frames)?;
        #[cfg(any(test, feature = "test-support"))]
        if self.strict {
            return Some(row.strict());
        }
        Some(row)
    }

    /// Streamed notes read strict views, every tap checked: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn strict_spans(&mut self) {
        self.strict = true;
    }

    /// SLICE NOTE plays at ROOT: the cents that take the note there.
    fn tune(&self) -> Cents {
        if self.at_root {
            Cents::new(self.root.get() - f32::from(self.note.get()) * 100.0)
        } else {
            Cents::ZERO
        }
    }

    /// A note-on on `p` and `id` (a Part's) plays nothing: no source, an
    /// absent one (its row empty, the gate open), one `p`'s model can't
    /// play, or a streamed one `p` needs in RAM (`needs_ram`). One a closed
    /// gate moves isn't refused: it waits (`start`). Refused, it is never
    /// allocated (spec § 2).
    pub fn refuses(p: &TapeEngineParams, id: Option<SourceId>, src: &GateView<'_>) -> bool {
        let Some(id) = id else {
            return true;
        };
        if src.moving(id) {
            return false;
        }
        let Some(held) = src.bytes(id) else {
            return true;
        };
        match Descriptor::from_bytes(held) {
            Some(d) if d.is_streamed() => params::needs_ram(p).is_some(),
            Some(d) => !SourceView::of(held, d).is_some_and(|v| wave::playable(p.way(), &v)),
            None => true,
        }
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
        mut streams: Option<&mut StreamPort<'_, '_>>,
    ) {
        let v = match self.row_of(src) {
            Some(Row::Streamed(held, d)) => {
                self.play_streamed(out, p, (held, d, src), sample_rate, streams);
                return;
            }
            Some(Row::Whole(v)) => Some(v),
            None => None,
        };
        if let Some(v) = v {
            if let Note::Gated { waited } = self.state {
                self.start_from(
                    self.note,
                    p,
                    src,
                    sample_rate,
                    streams.as_deref_mut(),
                    waited,
                );
            } else if v.frames() < self.n || self.fed() {
                self.end();
            } else if let Some(q) = self.moved_start(p) {
                self.once_on(self.note, &q, &v, sample_rate);
            }
            self.render(out, p, &v, sample_rate);
        } else {
            out.fill(0.0);
            if !self.src.is_some_and(|id| src.moving(id)) {
                self.end();
            } else if let Note::Gated { waited } = &mut self.state {
                *waited = waited.saturating_add(1);
            }
        }
        self.end_stream(streams);
    }

    /// ONE's START as the note's first block plays it, its
    /// routes on (VEL, NOTE, an LFO: #698) differs from the stored one its
    /// note-on latched: the head starts there instead, before a sample has
    /// played. START latches there, kept on its own side of END: a route
    /// never flips a hit to reverse, nor asks a streamed one to read back
    /// (`needs_ram` passed on the stored START). `p` with that START.
    fn moved_start(&mut self, p: &TapeEngineParams) -> Option<TapeEngineParams> {
        if !(self.first && self.is_active() && self.one()) {
            return None;
        }
        let start = if self.start <= p.end {
            p.start.min(p.end)
        } else {
            p.start.max(p.end)
        };
        if start == self.start {
            return None;
        }
        self.start = start;
        Some(TapeEngineParams { start, ..*p })
    }

    /// A streamed note that ended drops its slots. With no port to drop
    /// them through, the note still ends: the voice's ring is the
    /// instrument's to drop (`streams` false).
    fn end_stream(&mut self, streams: Option<&mut StreamPort<'_, '_>>) {
        if let Note::Ending(_) = self.state {
            self.state = Note::Idle;
            if let Some(port) = streams {
                self.cursor.stop(port);
            }
        }
    }

    /// `play` on a streamed row: with no rings to read through, nothing
    /// plays, and a streamed note ends.
    #[inline(never)]
    fn play_streamed(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &TapeEngineParams,
        (held, d, src): (&[u8], Descriptor, &GateView<'_>),
        sample_rate: u32,
        streams: Option<&mut StreamPort<'_, '_>>,
    ) {
        let Some(port) = streams else {
            out.fill(0.0);
            self.state = Note::Idle;
            return;
        };
        if let Note::Gated { waited } = self.state {
            self.start_from(self.note, p, src, sample_rate, Some(&mut *port), waited);
        }
        let row = self
            .stream_of(port)
            .and_then(|st| self.row((held, d), st.frames));
        match row {
            Some(row) if self.streams() && row.frames() >= self.n => {
                if let Some(q) = self.moved_start(p)
                    && let Some(stream) = self.stream_of(port)
                {
                    // Warm if START's head holds the new start, else cold.
                    self.once_on(self.note, &q, &row.silent(), sample_rate);
                    let at = self.heads[0].pos.whole();
                    let head = row.head(HeadIdx::Start);
                    self.cursor
                        .start(SourceFrame::new(at), self.inc_cap(), head, stream, port);
                }
                self.stream_block(out, p, row, sample_rate, port);
            }
            _ => {
                self.end();
                self.last = 0.0;
                out.fill(0.0);
            }
        }
        self.end_stream(Some(port));
    }

    /// A streamed note's block (slice 1 § 7.3): heads re-staged under the
    /// cursor start it again there; the cursor plans the block's spans, the
    /// model reads them, its output takes the cursor's gain; a LOOP wrap
    /// the cursor's head doesn't hold starts it on the loop-start head.
    fn stream_block(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &TapeEngineParams,
        row: StreamRow<'_>,
        sample_rate: u32,
        port: &mut StreamPort<'_, '_>,
    ) {
        // Admission paid for faster (a knob, a glide, a re-snap: #595):
        // the cursor plans for it from here.
        debug_assert!(self.fed(), "a stream's block off a stream");
        let rate = self.stream().map_or(SourceRate::MIN, |s| s.rate);
        if let Some(r) = port.ratio_max
            && Self::cap_at(rate, r, sample_rate) > self.inc_cap()
        {
            self.set_cap(Self::cap_at(rate, r, sample_rate));
            self.cursor.raise(self.inc_cap());
        }
        if let Some(h) = self.cursor.head()
            && row.head(h.idx) != Some(h)
        {
            let at = self.heads[0].pos.whole();
            self.cursor
                .jump(SourceFrame::new(at), self.inc_cap(), row.head(h.idx), port);
        }
        let h = self.heads[0].pos;
        // The next wrap's reads: off the RAM heads (POSITION, LENGTH, DRIFT
        // or a route moved the loop), the cursor prefetches them a pass
        // ahead into the slots the pass's end frees (#698).
        let next = self.lp().then(|| self.next_reads());
        let off_heads = next.is_some() && !self.lands(&row);
        self.cursor
            .expect(next.filter(|_| off_heads).map(|(from, to)| Wrap {
                last: SourceFrame::new(self.bounds.le().saturating_sub(1)),
                fade: SourceFrame::new(self.bounds.fade_at()),
                from: SourceFrame::new(from),
                to: SourceFrame::new(to),
            }));
        let plan = self
            .cursor
            .plan(f64::from(h.whole()) + f64::from(h.frac()), port);
        self.set_cap(plan.inc_max);
        let mut reads = Reads::streamed(&plan, &row, &self.cursor, &*port.turn, port.voice, next);
        let fading = self.fade.is_some();
        let mute = fading && self.stream().is_some_and(|s| s.mute);
        if !fading {
            self.set_mute(false);
        } else if mute {
            reads.mute_second();
        }
        // The second head runs out of what's landed within the block and a
        // fade: the voice fades out first, as before a cold wrap.
        let out_soon = |last: u32| {
            let h1 = self.heads[1].pos.whole() as f32;
            h1 + self.inc_cap().get() * (BLOCK_SIZE + DROP_FADE.get() as usize) as f32
                >= last as f32
        };
        if self.lp() && fading && reads.short.is_some_and(out_soon) {
            self.cursor.duck(Duck::Out);
            self.ducked = true;
        } else if self.lp() && (!reads.landed || mute) {
            // The prefetch hasn't landed (a pass shorter than the card's
            // read, a burst): the fade's second head reads silence, and the
            // voice fades out before the wrap, which starts cold (spec §
            // 5.7: drop-outs fade).
            let left = self.bounds.le().saturating_sub(h.whole()) as f32;
            if left <= self.inc_cap().get() * (BLOCK_SIZE + DROP_FADE.get() as usize) as f32 {
                self.cursor.duck(Duck::Out);
                self.ducked = true;
            }
        } else if core::mem::take(&mut self.ducked) {
            self.cursor.duck(Duck::In);
        }
        self.render_reads(out, p, &reads, sample_rate);
        // A fade that started into silence stays silent to its end.
        if !fading && self.fade.is_some() && !reads.landed {
            self.set_mute(true);
        }
        if core::mem::take(&mut self.jumped) {
            self.ducked = false;
            let at = self.heads[0].pos.whole();
            let held = self
                .cursor
                .head()
                .and_then(|h| row.head_span(h.idx))
                .is_some_and(|s| s.holds(at));
            if !held {
                self.cursor.jump(
                    SourceFrame::new(at),
                    self.inc_cap(),
                    row.head(HeadIdx::Loop),
                    port,
                );
            }
        }
    }

    /// A TAPE voice's bill for `p`: its model's, and TAPE's and AGE's each
    /// when it runs, stored above 0 or routed (spec § 5). TRANSWAVE's level
    /// change is `EngineSlot`'s, which sees the pitch.
    pub fn cost(p: &TapeEngineParams, mods: &ModState) -> Cost {
        let routed = |q| mods.moves_addr(ParamAddr::new(BlockRef::TapeEngine, q));
        let mut c = Self::model_cost(p);
        if tape_runs(p.tape, routed(TapeEngineParams::TAPE)) {
            c = c + COST_TAPE_RUNS;
        }
        if age::age_runs(p.age, routed(TapeEngineParams::AGE)) {
            c = c + COST_AGE;
        }
        c
    }

    /// A STRETCH cell its TYPE `k` doesn't play, and the band's words why:
    /// GRAIN's under CLASSIC, CLASSIC's under GRAIN (ADR 0204). Bank 2's:
    /// with the door closed nothing dims.
    pub fn stretch_only(k: StretchType, id: crate::block::ParamId) -> Option<&'static str> {
        type P = TapeEngineParams;
        let grain = matches!(id, P::LENGTH | P::GRAIN | P::SPRAY | P::FINISH);
        let classic = matches!(id, P::SIZE | P::POSITION);
        match k {
            StretchType::Grain if classic => Some("CLASSIC TYPE ONLY"),
            StretchType::Mono | StretchType::Poly if grain => Some("GRAIN TYPE ONLY"),
            _ => None,
        }
    }

    /// `p`'s model's measured worst, either way round (spec § 5), and
    /// STRETCH's by its TYPE.
    pub const fn model_cost(p: &TapeEngineParams) -> Cost {
        match p.way() {
            Way::Stretch => stretch_cost(p.kind),
            w => model_cost(w),
        }
    }

    /// What the sounding note costs: its note-on's model, and the
    /// stream's term on a streamed source.
    pub fn playing_cost(&self) -> Option<Cost> {
        self.is_active().then(|| {
            if self.fed() {
                streamed_cost(self.tape_machine())
            } else if let MachineState::Stretch(n) = &self.machine
                && n.grains()
            {
                COST_STRETCH_GRAIN
            } else {
                model_cost(self.tape_machine())
            }
        })
    }

    /// A streamed note's speed now, frames a sample, as it would be with
    /// no cap, and its cap: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn speed(&self) -> Option<(f32, f32)> {
        let cents = f32::from(self.note.get()) * 100.0
            + self.pitch.get()
            + self.cents.get()
            + self.tune().get()
            - self.root.get();
        let Note::Streamed(s) = self.state else {
            return None;
        };
        let wants =
            f32::from(s.rate) / SAMPLE_RATE as f32 * exp2(cents / 1_200.0) * self.slide.value();
        Some((wants, s.inc_cap.get()))
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
        self.pitch = Cents::new(cents);
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
        let note = MidiNote::clamped(note);
        let (frames, rate) = (v.frames(), v.desc().rate());
        self.state = Note::Ram;
        self.at_root = params::plays_at_root(p);
        self.hold = params::hold(p);
        self.note = note;
        self.root = Cents::new(p.root);
        self.slide = Glide::new(1.0, 1.0);
        self.inc.hold();
        // This note's wow rate, ±`WOW_SPREAD`: one draw, the phase runs on.
        let d = xorshift32(&mut self.rng) as i32 as f32 * (WOW_SPREAD / 2_147_483_648.0);
        self.tap.set_wow_rate(WOW_RATE * (1.0 + d));
        self.mac = Macros::of(p);
        self.first = true;
        self.drift = drift::step(self.drift, p.drift, &mut self.drift_rng);
        // TRON: around a loop the tape runs out at 8 s; once through at
        // LENGTH's, 0.25 to 8 s whatever the strip, its window round again
        // until then (ADR 0203).
        self.round = match (p.way(), p.direction) {
            (Way::Tron, Direction::Reverse) => Round::Reversed,
            (Way::Tron, _) => Round::Window,
            _ => Round::No,
        };
        let tape = if self.round == Round::No {
            tron::TapeSeconds::MAX
        } else {
            tron::TapeSeconds::of_length(p.size)
        };
        self.tape = if p.tron_on() {
            Tape::rolling(tape.samples(sample_rate))
        } else {
            Tape::OFF
        };
        self.n = frames;
        self.clamp_len = libm::roundf(CLAMP_FADE_S * sample_rate as f32).max(1.0) as u32;
        self.machine = match Self::idle_machine(p.way()) {
            MachineState::Ping(pp) => MachineState::Ping(PingPass {
                turn_len: clampf(libm::roundf(TURN_S * sample_rate as f32), 2.0, 1_000.0) as u16,
                clamp_dg: 1.0 / self.clamp_len as f32,
                ..pp
            }),
            MachineState::Scrub(sc) => MachineState::Scrub(Scrub {
                hush_step: BLOCK_SIZE as f32 / (HUSH_S * sample_rate as f32),
                pull: 1.0
                    - libm::expf(-(BLOCK_SIZE as f32) / (drag_s(p.drag) * sample_rate as f32)),
                aim_pull: 1.0 - libm::expf(-(BLOCK_SIZE as f32) / (AIM_S * sample_rate as f32)),
                key: Self::keys_place(p).then(|| key_place(note)),
                ..sc
            }),
            m => m,
        };
        self.ramp = true;
        self.wow_kind = p.wow;
        self.age.set_band(rate.get());
        self.start = self.played(p).0;
        (self.rel, self.rel_k) = (1.0, 1.0);
        self.fade_k = tron::release_k(p.drag, sample_rate);
        if self.one() {
            self.once_on(note, p, v, sample_rate);
            return;
        }
        if self.model() == TapeModel::Transwave {
            self.machine = MachineState::Wave(Wave::on(
                v,
                wave::cycles(note.get(), self.pitch.get(), sample_rate),
            ));
            return;
        }
        if self.model() == TapeModel::Stretch {
            let i = inc(rate, note, self.pitch, self.root, sample_rate).get();
            // GRAIN's voice draw; CLASSIC takes none, so its notes are as
            // they always were.
            let seed = if p.kind == StretchType::Grain {
                xorshift32(&mut self.rng)
            } else {
                0
            };
            self.machine =
                MachineState::Stretch(stretch::Note::on(p, v, i, (&self.mac, seed), sample_rate));
            return;
        }
        let rev = self.reversed(self.played(p).1);
        let (start, end) = self.window(p, rev);
        self.ws = window_start(start, v.file_frames());
        // The first pass's floor, at the note's own `inc` (SCRUB: its loop's
        // vari-speed).
        let mut fast = inc(rate, note, self.pitch, self.root, sample_rate).get();
        if self.model() == TapeModel::Scrub {
            // KEY POSITION plays at ROOT.
            let i = inc(
                rate,
                note,
                Cents::new(self.pitch.get() + self.tune().get()),
                self.root,
                sample_rate,
            );
            fast = (2.0 * libm::fabsf(clampf(p.motion, -1.0, 1.0)) * i.get()).min(INC_MAX);
        }
        self.min_len = shortest(rate, fast);
        let we = window_end_min(self.ws, end, frames, self.min_len);
        self.we = we;
        let xfade = if self.ping() { 0.0 } else { p.xfade };
        self.set_mirror(false);
        self.off = Pos(0);
        let mut start_at = self.ws;
        match self.model() {
            TapeModel::Scrub => {
                self.bounds = Bounds::of_min(
                    0,
                    we - self.ws,
                    Sizing {
                        size: LoopTime::of_length(self.mac.size).share_of(we - self.ws, rate),
                        place: 0.0,
                        xfade,
                    },
                    rate,
                    self.min_len,
                );
                self.off = self.finger_target(self.finger_place(p.position), we);
                start_at = 0;
                if p.motion < 0.0 {
                    self.set_mirror(true);
                    self.bounds = self.bounds.mirror(frames);
                    self.off = self.off + at(frames);
                    start_at = self.bounds.ls();
                }
            }
            _ => {
                self.bounds = Bounds::of_min(
                    self.ws,
                    we,
                    Sizing {
                        size: self.loop_size(),
                        place: drift::place(self.mac.position, self.drift),
                        xfade,
                    },
                    rate,
                    self.min_len,
                );
                if self.round != Round::No {
                    // TRON's strip starts at POSITION and DRIFT's place.
                    let w = Window {
                        lo: self.ws,
                        hi: we,
                        dir: Dir::Fwd,
                    };
                    start_at = drift::from(w, drift::place(p.position, self.drift), self.min_len);
                }
                if self.lp() && rev {
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

    /// WALK in MODE ONE, LOOP or PING: the paths the old machines took.
    fn one(&self) -> bool {
        matches!(self.machine, MachineState::One(_))
    }

    fn lp(&self) -> bool {
        matches!(self.machine, MachineState::Loop)
    }

    fn ping(&self) -> bool {
        matches!(self.machine, MachineState::Ping(_))
    }

    /// TRON on at the last note-on.
    fn tron(&self) -> bool {
        self.tape.state() != TapeState::Off
    }

    /// LOOP plays its next pass reversed: END, as played, before the START
    /// latched at note-on (#369).
    fn reversed(&self, end: f32) -> bool {
        // Streamed, forward only (slice 1 § 2.3): END turned before START
        // mid-note never flips it.
        self.lp() && !self.fed() && Dir::of(self.start, end) == Dir::Rev
    }

    /// START and END as played: TRON on REVERSE swaps them, so it plays as
    /// FORWARD with END before START (ADR 0203).
    fn played(&self, p: &TapeEngineParams) -> (f32, f32) {
        if self.round == Round::Reversed {
            (p.end, p.start)
        } else {
            (p.start, p.end)
        }
    }

    /// The loop's SIZE: LENGTH's, but TRON's whole window.
    fn loop_size(&self) -> f32 {
        if self.round == Round::No {
            self.mac.size
        } else {
            1.0
        }
    }

    /// START and END as the model plays them. Reversed (`rev`), LOOP's
    /// window is mirrored, so START is its start in the mirrored frame
    /// (`1 − START`), as REVERSE's was. PING-PONG and SCRUB take the sorted
    /// window.
    fn window(&self, p: &TapeEngineParams, rev: bool) -> (f32, f32) {
        if !self.tape_machine().ordered() {
            (p.start.min(p.end), p.start.max(p.end))
        } else {
            let (start, end) = self.played(p);
            if rev {
                (1.0 - start, 1.0 - end)
            } else {
                (start, end)
            }
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

    /// ONE's note: one head from START toward END in the window's slice,
    /// at least 2 ms (`min_frames`) on END's side but at the source's edge,
    /// mirrored about it when END is before START.
    fn once_on(
        &mut self,
        note: MidiNote,
        p: &TapeEngineParams,
        v: &SourceView<'_>,
        sample_rate: u32,
    ) {
        let (frames, rate) = (v.frames(), v.desc().rate());
        let floor = min_frames(rate);
        let mut w = Window::of(p.start, p.end, frames, floor);
        if p.direction == Direction::Reverse {
            w.dir = w.dir.flip();
        }
        w = slice_bounds(w, p.slice_in_play(), note.get());
        // POSITION and DRIFT move where it starts, counted the way it plays.
        let from = drift::from(w, drift::place(p.position, self.drift), floor);
        // LENGTH is how much of the rest plays (LOOP's law).
        let end = (from + loop_len(p.size, w.hi - from, rate)).min(w.hi);
        let rev = w.dir == Dir::Rev;
        self.off = if rev { at(w.lo + w.hi - 1) } else { Pos(0) };
        self.heads = [Head {
            pos: at(from),
            mirror: rev,
        }; 2];
        self.machine = MachineState::One(Once { end });
        (self.rel, self.rel_k) = (1.0, 1.0);
        self.fade_k = tron::release_k(p.drag, sample_rate);
        self.fade = None;
        self.pending = None;
        self.gain = 0.0;
    }

    /// SCRUB's KEY places the finger: SCRUB at KEY POSITION.
    fn keys_place(p: &TapeEngineParams) -> bool {
        p.machine == TapeMachine::Scrub && p.key == KeyMode::Position
    }

    /// The finger's place for FINGER `finger`: the key's place plus it
    /// at KEY POSITION, FINGER's alone at KEY PITCH.
    fn finger_place(&self, finger: f32) -> Place {
        match self.scrub().and_then(|sc| sc.key) {
            Some(k) => k.plus(finger),
            None => Place::new(finger),
        }
    }

    /// SCRUB's finger at `place`, in a window ending at `we`: where the
    /// loop behind it fits in the window.
    fn finger_target(&self, place: Place, we: u32) -> Pos {
        let room = (we - self.ws).saturating_sub(self.bounds.frames());
        Pos::from_f32(self.ws as f32 + place.get() * room as f32)
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
        if self.is_active() {
            self.glide_to(note, (sample_rate, tau));
        } else {
            self.note_on(note, p, v, sample_rate);
        }
    }

    /// `inc` glides to `note`'s from the pitch it sounds, a one-pole of
    /// `tau` seconds in log pitch. Idle, or on `note`, nothing moves.
    /// Gated, the note it starts on moves (its `note_on` drops the glide).
    pub fn glide_to(&mut self, note: u8, (_sample_rate, tau): (u32, f32)) {
        let note = MidiNote::clamped(note);
        if !self.is_active() || note == self.note {
            return;
        }
        // SLICE NOTE plays every key at ROOT: no pitch to travel.
        let from = if self.at_root {
            self.slide.value()
        } else {
            exp2((f32::from(self.note.get()) - f32::from(note.get())) / 12.0) * self.slide.value()
        };
        self.slide = Glide::new(from, tau);
        self.slide.toward(1.0);
        // KEY POSITION: the finger heads for the new key's place, through
        // the aim's glide.
        if let Some(sc) = self.scrub_mut()
            && sc.key.is_some()
        {
            sc.key = Some(key_place(note));
        }
        self.note = note;
    }

    /// Note-off with no VCA route: `release_by(false)`.
    pub fn note_off(&mut self) {
        self.release_by(false);
    }

    /// TRON's tape fades through FADE's one-pole, the pressure pad lifting.
    /// WALK (short of HOLD in ONE) does only with no VCA route (`vca`
    /// false): with one, its ENV is the note's one release and FADE steps
    /// aside (#698). ONE at HOLD ignores note-off (`sustains`); the other
    /// machines do nothing, their VCA lets go.
    pub fn release_by(&mut self, vca: bool) {
        if self.tron() || (self.model() == TapeModel::Walk && !self.hold && !vca) {
            // A tape run out already falls faster: key up never slows it.
            self.rel_k = self.rel_k.min(self.fade_k);
        }
    }

    /// A HOLD head plays (WALK ONE): the voice keeps its gate, so note-off
    /// changes nothing and the note ends at END.
    pub fn sustains(&self) -> bool {
        self.is_active() && self.hold
    }

    /// A fresh note is to come on an idle voice: its ramp starts from 0, not
    /// from the last note's sample (as `Ramp::hold`).
    pub fn hold(&mut self) {
        self.last = 0.0;
        self.age.hold();
    }

    /// A note sounds or waits on its source; one that ended this block
    /// (`Ending`) does not.
    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }

    /// The model latched at the last note-on.
    pub fn model(&self) -> TapeModel {
        self.tape_machine().machine().model()
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
        } else if self.model() == TapeModel::Scrub {
            self.off + u
        } else {
            u
        };
        let w = r.whole();
        if w > i32::MAX as u32 { 0 } else { w }
    }

    /// The strip's heads (`Heads`): the probe's. None while idle.
    pub fn heads(&self) -> Heads {
        if !self.is_active() {
            return Heads::NONE;
        }
        let p = self.probe();
        Heads::new(p.head, p.other)
    }

    pub fn probe(&self) -> Probe {
        if let MachineState::Wave(w) = &self.machine {
            let (head, other, gain) = w.frames();
            return Probe {
                head,
                other: Some(other),
                gain,
                level: w.level(),
                from: w.from(),
            };
        }
        if let MachineState::Stretch(st) = &self.machine {
            return st.probe();
        }
        let first = self.frame(self.heads[0].pos);
        match self.ghost() {
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

    /// Each head's way: the grains' (STRETCH), the head's and a fade's
    /// second head's (the rest).
    pub fn dirs(&self) -> [Dir; 2] {
        if let MachineState::Stretch(st) = &self.machine {
            return st.dirs();
        }
        self.heads
            .map(|h| if h.mirror { Dir::Rev } else { Dir::Fwd })
    }

    /// Events handled since `init_in_place`: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn events(&self) -> u32 {
        match &self.machine {
            MachineState::Stretch(st) => self.events + st.events(),
            _ => self.events,
        }
    }

    /// STRETCH's wraps in the loop since the note-on, lifted and bare: for
    /// the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn wraps(&self) -> [u32; 2] {
        match &self.machine {
            MachineState::Stretch(st) => st.wraps(),
            _ => [0; 2],
        }
    }

    /// STRETCH's grain clock: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn grain(&self) -> stretch::GrainClock {
        match &self.machine {
            MachineState::Stretch(st) => st.clock(),
            _ => stretch::classic::Stretch::IDLE.clock(),
        }
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
        self.render_reads(out, p, &Reads::whole(*v), sample_rate);
    }

    /// `render` through a block's reads: a row's, or a streamed note's
    /// spans (`span::Reads`), whose output takes the cursor's gain.
    fn render_reads(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &TapeEngineParams,
        reads: &Reads<'_>,
        sample_rate: u32,
    ) {
        let (v, _) = reads.run(0);
        let v = &v;
        if !self.is_active() {
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
                Cents::ZERO
            } else {
                self.transport()
            }
        } else {
            Cents::ZERO
        };
        if self.mac.age > 0.0 && to.age == 0.0 && self.mac.age < MACRO_SETTLE {
            self.mac.age = 0.0;
        }
        self.slide.tick();
        if let MachineState::Wave(w) = &mut self.machine {
            let target = clampf(
                wave::cycles(
                    self.note.get(),
                    self.pitch.get() + self.cents.get(),
                    sample_rate,
                ) * self.slide.value(),
                f32::EPSILON,
                wave::CYCLES_MAX,
            );
            let m = self.mac;
            w.render(
                out,
                v,
                self.inc.step(target),
                (m.position, m.motion, m.size),
                (first, sample_rate),
            );
        } else if self.one() {
            self.render_once(out, reads, sample_rate);
        } else if self.model() == TapeModel::Stretch {
            let target = self.target_inc(v.desc().rate(), Cents::ZERO, sample_rate);
            let ramp = self.inc.step(target);
            let m = self.mac;
            if let MachineState::Stretch(st) = &mut self.machine
                && !st.render(out, v, ramp, &m, p, sample_rate)
            {
                self.end();
            }
        } else {
            self.render_heads(out, reads, (p, first), sample_rate);
            // The gain's fall a sample before the tape ran out this block.
            let mut was = None;
            if let TapeState::Rolling(left) = self.tape.state() {
                // TRON: the tape runs out, a 2 ms fall.
                let left = left.saturating_sub(BLOCK_SIZE as u32);
                self.tape = if left == 0 {
                    let k = libm::expf(-1.0 / (tron::RUN_OUT_S * sample_rate as f32));
                    was = Some(self.rel_k);
                    self.rel_k = self.rel_k.min(k);
                    Tape::OUT
                } else {
                    Tape::rolling(left)
                };
            }
            if self.rel_k != 1.0 {
                // WALK's FADE after note-off: TRON's one-pole release.
                let (mut g, k) = (self.rel, self.rel_k);
                if let Some(was) = was {
                    g = run_out::onset(out, g, (was, k));
                } else {
                    for y in out.iter_mut() {
                        *y *= g;
                        g *= k;
                    }
                }
                self.rel = g;
                if g < tron::TRON_FLOOR {
                    self.end();
                }
            }
        }

        if self.fed() {
            self.cursor.apply(out);
        }
        if core::mem::take(&mut self.ramp) {
            Self::note_on_ramp(out, self.last);
        }
        self.age.process(out, self.mac.age);
        self.last = out[BLOCK_SIZE - 1];
    }

    /// The heads' speed this block, frames a sample: the note's `inc` with
    /// PITCH, FINE, the wow and `extra` cents, a glide steal's ratio on it,
    /// clamped. Out of line: one copy for every model that plays heads.
    #[inline(never)]
    fn target_inc(&self, rate: SourceRate, extra: Cents, sample_rate: u32) -> f32 {
        clampf(
            inc(
                rate,
                self.note,
                Cents::new(self.pitch.get() + self.cents.get() + extra.get()),
                self.root,
                sample_rate,
            )
            .get()
                * self.slide.value(),
            f32::EPSILON,
            self.inc_cap().get(),
        )
    }

    /// TAPE's transport, once a block: the head's speed deviation in cents
    /// (`Tap::speed_cents`), which `inc` ramps to across the block. TAPE 0
    /// adds exact zeros. The voice's rates were set at its seed (`tap`, depth 1), so a block
    /// only scales the offsets by TAPE.
    fn transport(&mut self) -> Cents {
        #[cfg(any(test, feature = "test-support"))]
        if self.no_transport {
            return Cents::ZERO;
        }
        Cents::new(match self.wow_kind {
            Wow::Capstan => {
                let (wow, flutter) = self.wow.offsets(&self.tap);
                (wow + flutter) * self.mac.tape
            }
            // In place of the capstan, never beside it.
            Wow::Warble => self.warble.cents(self.mac.tape, BLOCK_RATE),
        })
    }

    /// ONE's block: one head toward the window's end, its gain the
    /// release's. Within reach of the end each sample ramps to 0 over
    /// `END_RAMP` and the note ends there; a span shorter than a block's
    /// travel ends inside it.
    #[inline(never)]
    fn render_once(&mut self, out: &mut [f32; BLOCK_SIZE], reads: &Reads<'_>, sample_rate: u32) {
        // SLICE NOTE: the note picks the slice and plays at ROOT.
        let tune = self.tune();
        let target = self.target_inc(reads.run(0).0.desc().rate(), tune, sample_rate);
        let (from, to) = self.inc.step(target);
        let fast = from.max(to);
        let Some(&Once { end: once_end }) = self.once_state() else {
            return;
        };
        let rem = Pos(at(once_end).0.saturating_sub(self.heads[0].pos.0));
        let rem = rem.whole() as f32 + rem.frac();
        let reach = (BLOCK_SIZE as u32 + END_RAMP) as f32;
        let near = rem <= reach * fast + 2.0;
        // Near the end the gain ramps to 0 over `END_RAMP` samples' travel
        // at the block's fastest head: a line through its values at the
        // block's ends (the head's travel is linear, as `inc` is ramped),
        // clamped to 0..1 a sample.
        let travel = 0.5 * BLOCK_SIZE as f32 * (from + to) + 0.5 * (to - from);
        let by_end = 1.0 / (END_RAMP as f32 * fast);
        let gain = (rem * by_end, travel * by_end * (1.0 / BLOCK_SIZE as f32));
        let (from, to) = (Pos::from_f32(from).0, Pos::from_f32(to).0);
        let step = (to.wrapping_sub(from) as i64 / BLOCK_SIZE as i64) as u64;
        let mut c = (from, step, gain);
        // Run by run: each to the sample whose read leaves it.
        let (mut i, mut k) = (0, 0);
        while i < BLOCK_SIZE && self.is_active() {
            let (v, stop) = reads.run(k);
            let j = self.reach(i, (c.0, step), stop);
            c = match (self.mirror(), near) {
                (_, false) => self.once(out, &v, c, i..j),
                (false, true) => self.once_near::<false>(out, &v, c, i..j),
                (true, true) => self.once_near::<true>(out, &v, c, i..j),
            };
            (i, k) = (j, k + 1);
        }
        if self.rel < tron::TRON_FLOOR {
            self.end();
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
        (mut cur, step, gain): (u64, u64, (f32, f32)),
        run: Range<usize>,
    ) -> (u64, u64, (f32, f32)) {
        let (m, a) = if self.mirror() {
            (u64::MAX, self.off.0.wrapping_add(1))
        } else {
            (0, 0)
        };
        let mut h = self.heads[0].pos;
        let (mut g, k) = (self.rel, self.rel_k);
        for y in out[run].iter_mut() {
            // Reversed at `lo` 0, the head's last fraction reads just
            // under frame 0: the position wraps, and `SourceView::read`
            // takes its whole part as `i32` −1, clamped to frame 0.
            *y = v.read(Pos((h.0 ^ m).wrapping_add(a))) * g;
            g *= k;
            cur = cur.wrapping_add(step);
            h = h + Pos(cur);
        }
        self.heads[0].pos = h;
        self.rel = g;
        (cur, step, gain)
    }

    /// The sample from `i` whose read reaches `stop`: the head stepped as
    /// `once` steps it, nothing read. A row's run never stops.
    fn reach(&self, mut i: usize, (mut cur, step): (u64, u64), stop: Stop) -> usize {
        let Stop::At(stop) = stop else {
            return BLOCK_SIZE;
        };
        let mut h = self.heads[0].pos;
        while i < BLOCK_SIZE && h.whole() < stop {
            cur = cur.wrapping_add(step);
            h = h + Pos(cur);
            i += 1;
        }
        i
    }

    /// `render_once`'s samples near the end, read forward or
    /// (`REV`) at `off − u`: the end's line falls `dr` a sample from `r`,
    /// its gain that clamped to 0..1 and smoothstepped (so the ramp has no
    /// corner where it starts), to land on 0 there, and the note ends where
    /// the head reaches END.
    #[inline(never)]
    fn once_near<const REV: bool>(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        (mut cur, step, (mut r, dr)): (u64, u64, (f32, f32)),
        run: Range<usize>,
    ) -> (u64, u64, (f32, f32)) {
        let off = self.off;
        let mut h = self.heads[0].pos;
        let (mut g, k) = (self.rel, self.rel_k);
        let Some(&Once { end }) = self.once_state() else {
            return (cur, step, (r, dr));
        };
        let end = at(end);
        let (mut i, stop) = (run.start, run.end.min(BLOCK_SIZE));
        while i < stop && h < end {
            r -= dr;
            let x = clampf(r, 0.0, 1.0);
            // `off − h` under frame 0 at `lo` 0: `read`'s clamp, as in `once`.
            out[i] = v.read(if REV { off - h } else { h }) * g * (x * x * (3.0 - 2.0 * x));
            g *= k;
            cur = cur.wrapping_add(step);
            h = h + Pos(cur);
            i += 1;
        }
        if h >= end {
            // The head reached END: on the block's last sample, too.
            out[i..].fill(0.0);
            self.end();
        }
        self.heads[0].pos = h;
        self.rel = g;
        (cur, step, (r, dr))
    }

    /// The block of a model that plays heads through a window: all but
    /// TRANSWAVE.
    fn render_heads(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        reads: &Reads<'_>,
        (p, first): (&TapeEngineParams, bool),
        sample_rate: u32,
    ) {
        let (v, _) = reads.run(0);
        let v = &v;
        let rate = v.desc().rate();
        // KEY POSITION plays SCRUB at ROOT.
        let target = self.target_inc(rate, self.tune(), sample_rate);
        let (mut from, mut to) = self.inc.step(target);
        // The next pass's frame: LOOP's END moved across START flips it,
        // at the next wrap. Its window's start is that frame's.
        let rev = if self.lp() {
            self.reversed(self.played(p).1)
        } else {
            self.mirror()
        };
        self.next_rev = rev;
        self.next_ws = if rev == self.mirror() {
            self.ws
        } else {
            let start = if rev { 1.0 - self.start } else { self.start };
            window_start(start, v.file_frames())
        };
        let clamp_len = self.clamp_len;
        if let Some(pp) = self.pp_mut() {
            pp.inv_v = 1.0 / to;
            pp.reach = to * clamp_len as f32 + 1.0;
            self.v = to;
            if self.turning().is_none() {
                self.event = self.pp_event();
            }
        }
        let (_, end) = self.window(p, rev);
        // A flip at XFADE 0 fades over the clamp's time at this speed.
        self.v = to;
        let v1 = reads.second_at(0).0;
        let k = AtWrap {
            end,
            xfade: p.xfade,
            frames: v.frames(),
            rate,
            v,
            v1: &v1,
        };
        let scrub = self.model() == TapeModel::Scrub;
        if scrub {
            (from, to) = self.scrub_block(p, to, first, &k);
        } else {
            self.floor(from.max(to), &k);
        }
        // Every event this block latches these: worked out once.
        self.next = self.next_bounds(&k);
        if scrub {
            // A turn mirrored the frame: the next pass is in the new one.
            // A flip is LOOP's; latched before the turn, the turn's block
            // wrapped as one and dropped the finger (#383).
            self.next_rev = self.mirror();
        }
        if self.runup && self.next_rev == self.mirror() {
            self.follow_next();
        }
        if let Some(sc) = self.scrub_mut() {
            // The finger's speed at the block's ends, frames a sample.
            let end = sc
                .df
                .0
                .wrapping_add(sc.ddf.0.wrapping_mul(BLOCK_SIZE as i64));
            let finger =
                sc.df.0.unsigned_abs().max(end.unsigned_abs()) as f32 * (1.0 / 4_294_967_296.0);
            let speed = from.max(to).max(finger);
            // Hysteresis: stop under STILL, start over twice it.
            sc.stopped = if sc.stopped && !first {
                speed <= 2.0 * STILL
            } else {
                speed < STILL
            };
        }
        let (from, to) = (Pos::from_f32(from).0, Pos::from_f32(to).0);
        let step = (to.wrapping_sub(from) as i64 / BLOCK_SIZE as i64) as u64;

        // Run by run: the first head reads each to its stop, and after a
        // wrap the loop-start head, which the second always reads.
        let (mut i, mut cur, mut run) = (0, from, 0);
        self.jumped = false;
        while i < BLOCK_SIZE {
            // The second head's view where it reads now (a prefetched
            // wrap's two slots hand over), and after a wrap the first's.
            let (second, stop1) = reads.second_at(self.heads[1].pos.whole());
            let (r, stop) = if self.jumped {
                reads.second_at(self.heads[0].pos.whole())
            } else {
                reads.run(run)
            };
            let (r, c) = (&r, (i, cur, step));
            let rs = (r, &second, stop, stop1, reads.whole);
            (i, cur) = match (self.ghost().is_some(), self.turning().is_some()) {
                (true, _) => self.run_slow::<true>(out, r, c, &k),
                (false, true) => self.run_slow::<false>(out, r, c, &k),
                (false, false) if self.flipping() => match self.mirror() {
                    false => self.run_flip::<false>(out, r, c, &k),
                    true => self.run_flip::<true>(out, r, c, &k),
                },
                (false, false) => match (self.mirror(), scrub) {
                    (false, false) => self.run::<false, false>(out, rs, c, &k),
                    (true, false) => self.run::<true, false>(out, rs, c, &k),
                    (false, true) => self.run::<false, true>(out, rs, c, &k),
                    (true, true) => self.run::<true, true>(out, rs, c, &k),
                },
            };
            if !self.jumped
                && let Stop::At(s) = stop
                && self.heads[0].pos.whole() >= s
            {
                run += 1;
            }
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
        let Some(&Scrub {
            speed,
            pull,
            df,
            lead,
            aim,
            aim_pull,
            ..
        }) = self.scrub()
        else {
            return (0.0, 0.0);
        };
        let want = 2.0 * self.mac.motion * inc;
        if speed == 0.0 && want != 0.0 && (want < 0.0) != self.mirror() {
            self.turn_scrub();
        }
        let ahead = if self.mirror() { -want } else { want };
        let to = if ahead < 0.0 { 0.0 } else { ahead.min(INC_MAX) };
        let from = if first { to } else { libm::fabsf(speed) };
        let speed = if self.mirror() { -to } else { to };
        self.floor(from.max(to), k);

        let we = self.we;
        let at_n = if self.mirror() { at(self.n) } else { Pos(0) };
        let finger = self.off - at_n;
        let target = self.finger_target(self.finger_place(p.position), we);
        // The finger's speed ramps from the last block's to this one's, so
        // the head's speed is linear across the block, and bounded at
        // both ends, so everywhere in it.
        // The finger follows the aim, which follows POSITION: three poles,
        // so a step in POSITION starts the reel moving smoothly, never at
        // once.
        let toward = |from: Pos, to: Pos| {
            let left = (to.0 as i64).wrapping_sub(from.0 as i64) as f32;
            Pos((from.0 as i64).wrapping_add((aim_pull * left) as i64) as u64)
        };
        let (lead, aim, (df, df_to)) = if first {
            self.off = target + at_n;
            (target, target, (FingerStep(0), 0))
        } else {
            let lead = toward(lead, target);
            let aim = toward(aim, lead);
            let gap = (aim.0 as i64).wrapping_sub(finger.0 as i64) as f32;
            let most = (INC_MAX - from.max(to)) * 4_294_967_296.0;
            (
                lead,
                aim,
                (
                    df,
                    clampf(pull * gap / BLOCK_SIZE as f32, -most, most) as i64,
                ),
            )
        };
        if let Some(sc) = self.scrub_mut() {
            sc.lead = lead;
            sc.aim = aim;
            sc.speed = speed;
            sc.df = df;
            sc.ddf = FingerStep(df_to.wrapping_sub(df.0) / BLOCK_SIZE as i64);
        }
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
        self.event = if self.ping() {
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
        let Some(sc) = self.scrub_mut() else {
            return;
        };
        let to = if sc.stopped { 0.0 } else { 1.0 };
        if first {
            sc.hush = to;
        }
        let x0 = sc.hush;
        let x1 = if sc.stopped {
            (x0 - sc.hush_step).max(0.0)
        } else {
            (x0 + sc.hush_step).min(1.0)
        };
        sc.hush = x1;
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
        // The second head's share now, and what's left of the fade. At a
        // share of 0 the output is all `h0`'s: nothing to run back, so it
        // turns as with no fade.
        let fade = self.fade.take().and_then(|f| {
            let left = f.end - h0;
            let left = left.whole() as f32 + left.frac();
            let g = clampf(1.0 - left * f.inv, 0.0, 1.0);
            (g > 0.0).then_some((f, left, g))
        });
        match fade {
            None => {
                self.heads[0].pos = n - h0;
                self.bounds = self.bounds.mirror(self.n);
                self.event = self.bounds.fade_at();
            }
            Some((f, left, g)) => {
                // The fade runs back to where it began: the head it faded
                // from (`h0`) takes over there. Its end is rounded up to a
                // whole frame, as a wrap's is, and its slope set so the
                // gain carries on from this sample's. A fade with no slope
                // has no way back to run: it ends at the next whole frame.
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
                    power: f.power,
                    ..Fade::back(end, span, g, self.bounds.mirror(self.n))
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
        (v, v1, stop, stop1, whole): (&SourceView<'_>, &SourceView<'_>, Stop, Stop, bool),
        (mut i, mut cur, step): (usize, u64, u64),
        k: &AtWrap<'_>,
    ) -> (usize, u64) {
        let [mut h0, mut h1] = self.heads.map(|h| h.pos);
        // The second head hands to its next view before this read.
        let s1 = match stop1 {
            Stop::At(s) => s,
            Stop::Never => u32::MAX,
        };
        // A run's stop is one more event: it hands the head to the next.
        let at = |ev: u32| match stop {
            Stop::At(s) => ev.min(s),
            Stop::Never => ev,
        };
        let (mut df, ddf) = self.finger();
        let (mut ev, mut g, mut off) = (at(self.event), self.gain, self.off);
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
                            out[i] = xmix(a, v.read(r0 - e), g, f.power);
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
                            out[i] = xmix(a, v.read(r1), g, f.power);
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
                            // A smoothstep, as SCRUB's: a tiny loop's linear
                            // fade bent in one sample at its ends (#385).
                            out[i] = xmix(a, v1.read(h1), g, f.power);
                            cur = cur.wrapping_add(step);
                            h0 = h0 + Pos(cur);
                            h1 = h1 + Pos(cur);
                            i += 1;
                            if h0.whole() >= ev || h1.whole() >= s1 {
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
                    self.set_df(df);
                }
                if self.heads[0].pos.whole() < self.event {
                    // The run's stop, not an event.
                    return (i, cur);
                }
                let fading = self.fade.is_some();
                self.on_event(k);
                // A fade that starts here picks its second head's view
                // where it starts (a prefetched wrap's slot): back out.
                let starts = !fading && self.fade.is_some();
                // A wrap or a fade's start on a whole row stays: back out,
                // `render_heads` would hand this run the same views and
                // stops, and call it again in the same frame (`leaves`
                // false) with the same state, which is all it reloads
                // here. A tiny loop wraps every few samples (#793). SCRUB
                // still backs out: its finger is reloaded as it was set,
                // so staying would be the same bits too (the hash test's
                // tiny SCRUB loops agree), but it is left as it ran.
                if self.leaves(MIRROR) || ((self.jumped || starts) && (MOVING || !whole)) {
                    return (i, cur);
                }
                [h0, h1] = self.heads.map(|h| h.pos);
                (ev, g, off) = (at(self.event), self.gain, self.off);
            }
        }
        (self.heads[0].pos, self.heads[1].pos) = (h0, h1);
        (self.gain, self.off) = (g, off);
        if MOVING {
            self.set_df(df);
        }
        (i, cur)
    }

    /// After an event, `run`'s frame no longer fits: the frame flipped, a
    /// flip fades, or a slow run starts. Out of line, so `run`'s loops
    /// compile as before the flip.
    #[inline(never)]
    fn leaves(&self, mirror: bool) -> bool {
        self.mirror() != mirror
            || self.flipping()
            || self.ghost().is_some()
            || self.turning().is_some()
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
        let dg = self.pp().map_or(0.0, |pp| pp.clamp_dg);
        let mut gh = self.ghost().unwrap_or(Ghost {
            pos: Pos(0),
            hold: None,
            back: false,
            done: 0,
            len: 1,
        });
        let mut g = self.gain;
        while i < BLOCK_SIZE {
            let (mirror, off) = (self.mirror(), self.off);
            // The head's read position, moving down in a mirrored frame.
            let mut r = self.real(self.heads[0].pos);
            let turning = self.turning();
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
            if let (true, Some(held)) = (GHOST, gh.hold) {
                hold(&mut out[start..i], held, g, dg);
            }
            let n = (i - start) as u32;
            let h = if mirror { off - r } else { r };
            self.heads[0].pos = h;
            if GHOST {
                gh.done += n;
                if gh.done >= gh.len {
                    // The clamp is done: the plain run takes over.
                    self.set_ghost(None);
                    self.gain = 0.0;
                    if turning.is_some() {
                        t.k += n as u16;
                        self.set_turning(Some(t));
                        self.end_half(t, k);
                    }
                    if hit || (turning.is_some() && self.turning().is_none()) {
                        self.on_event(k);
                    }
                    return (i, cur);
                }
                // An event may retarget the fade (`fade_from`): it reads
                // the fade's state from `self`, and it comes back here.
                self.set_ghost(Some(gh));
                self.gain = g;
            }
            if turning.is_some() {
                t.k += n as u16;
                self.set_turning(Some(t));
                if self.end_half(t, k) && !GHOST && self.ghost().is_some() {
                    // A clamp started at the flip.
                    return (i, cur);
                }
                if self.turning().is_none() && !GHOST {
                    // The turn is over: on to the next edge, here if it is
                    // within the block, else in the plain run.
                    let to = self.event.saturating_sub(self.heads[0].pos.whole());
                    if to as f32 > self.v * (BLOCK_SIZE - i) as f32 {
                        return (i, cur);
                    }
                }
            }
            if hit || (turning.is_some() && self.turning().is_none()) {
                self.on_event(k);
                if !GHOST && self.ghost().is_some() {
                    return (i, cur);
                }
            }
            if let (true, Some(h)) = (GHOST, self.ghost()) {
                (gh, g) = (h, self.gain);
            }
        }
        if GHOST {
            self.set_ghost(Some(gh));
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
                self.set_turning(None);
                self.event = self.pp_event();
            }
            false
        }
    }

    /// The next pass's bounds from the eased macros, the walk, and END and
    /// XFADE now. SCRUB's loop is `[0, Ll)` behind the finger.
    fn next_bounds(&self, k: &AtWrap<'_>) -> Bounds {
        let (ws, we) = (self.next_ws.min(k.frames - 1), self.we);
        match self.model() {
            TapeModel::Scrub => {
                let b = Bounds::of_min(
                    0,
                    we - ws,
                    Sizing {
                        size: LoopTime::of_length(self.mac.size).share_of(we - ws, k.rate),
                        place: 0.0,
                        xfade: k.xfade,
                    },
                    k.rate,
                    self.min_len,
                );
                if self.mirror() { b.mirror(self.n) } else { b }
            }
            _ => {
                let place = drift::place(self.mac.position, self.drift);
                let xfade = if self.ping() { 0.0 } else { k.xfade };
                let size = self.loop_size();
                let b = Bounds::of_min(ws, we, Sizing { size, place, xfade }, k.rate, self.min_len);
                if self.fed() {
                    // The loop head holds XFADE before the loop, at most
                    // `XFADE_MAX_MS` of source (§ 7.3 step 6, #584).
                    b.lx_at_most(xfade_max(k.rate))
                } else {
                    b
                }
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
            if self.ping() {
                self.turn(k);
                continue;
            }
            let le = self.bounds.le();
            if let Some(f) = self.fade.take() {
                // The wrap: the second head plays on as the first, in its
                // own frame if it was a flip's.
                let flip = self.heads[1].mirror != self.mirror();
                self.heads[0] = self.heads[1];
                self.jumped = true;
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
                let pos = at(next.ls()) + Pos(over);
                if nx.rev != self.mirror() {
                    // A hard splice that flips: the head going out plays on
                    // the old way under the clamp's fade.
                    self.flip_fade(h, pos, nx);
                    continue;
                }
                self.heads[0].pos = pos;
                self.jumped = true;
                self.bounds = next;
                self.event = next.fade_at();
            } else {
                let nx = self.pending.unwrap_or(self.upcoming());
                let next = nx.b;
                // SCRUB's loop has nothing before it: its fades fold.
                let ws = if self.model() == TapeModel::Scrub {
                    next.ls()
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
                        let mut f = Fade::new(at(le), rem, next);
                        let secs = (rem.whole() as f32 + rem.frac()) / f32::from(k.rate);
                        if secs >= POWER_FADE_S {
                            f.power = self.fade_power(h, restart - rem, rem.whole(), k);
                        }
                        self.fade = Some(f);
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

    /// A long fade's power (`heads::power_of`) from its heads at `h0` and
    /// `h1`, `span` frames: read where they will, forward in the source's
    /// frame; elsewhere (mirrored, SCRUB, or too few frames held) a sum to
    /// 1, which never peaks.
    #[cold]
    fn fade_power(&self, h0: Pos, h1: Pos, span: u32, k: &AtWrap<'_>) -> f32 {
        let plain = !self.mirror() && self.model() != TapeModel::Scrub;
        let measured = plain
            .then(|| power_of(k.v, h0.whole(), k.v1, h1.whole(), span))
            .flatten();
        measured.unwrap_or(0.0)
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
        if h.whole() < b.le() {
            let rem = at(b.le()) - h;
            let rem = rem.whole() as f32 + rem.frac();
            let Some(&PingPass {
                inv_v, turn_len, ..
            }) = self.pp()
            else {
                return;
            };
            let th = (2.0 * rem * inv_v).min(turn_len as f32) as u16;
            if th >= 2 {
                self.set_turning(Some(Turn {
                    k: 0,
                    th,
                    unit: (1u32 << 16) / (2 * th as u32),
                }));
                self.event = u32::MAX;
            } else {
                self.event = b.le();
            }
            return;
        }
        let len = (b.frames() as u64) << 32;
        let past = (h - at(b.le())).0;
        let over = Pos(if past >= len { past % len } else { past });
        // The reflected head, in source frames.
        let r = if self.mirror() {
            at(b.ls()) + over
        } else {
            at(b.le()) - over
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
    /// is retargeted (R1): its ghost restarts at `pos`, and the output it
    /// had reached, `a + g·(head − a)` (moved to its own hold, if any), is
    /// the new fade's `hold`, so the output carries on and fades from there.
    fn fade_from(&mut self, pos: Pos, back: bool, k: &AtWrap<'_>) {
        let hold = self.ghost().map(|gh| {
            let (g, a) = (self.gain, k.v.read(gh.pos));
            let y = a + g * (k.v.read(pos) - a);
            gh.hold.map_or(y, |h| y + (1.0 - g) * (h - y))
        });
        self.set_ghost(Some(Ghost {
            pos,
            hold,
            back,
            done: 0,
            len: self.clamp_len,
        }));
        self.gain = 0.0;
    }

    /// PING-PONG's frame for a head at `r` (source frames) going down
    /// (`back`) or up in loop `b`: its `u`, signed.
    fn enter(&mut self, back: bool, r: Pos, b: &Bounds) -> i64 {
        self.set_mirror(back);
        if back {
            self.off = at(b.ls() + b.le());
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
        let turn_len = self.pp().map_or(0, |pp| pp.turn_len);
        let half = self.v * turn_len as f32 * 0.5;
        b.le() - (half as u32).min(b.frames() / 4)
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
            b.le().saturating_sub(len).max(ws)
        } else {
            b.ls().min(self.we - len)
        };
        Bounds::plain(ls, len)
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
        let nb = if self.ghost().is_some() {
            self.held(back)
        } else {
            self.next
        };
        let (lo, hi) = (at(nb.ls()).0 as i64, at(nb.le()).0 as i64 - 1);
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
        let reach = self.pp().map_or(0.0, |pp| pp.reach);
        if u >= 0 && (((inside - u) >> 32) as f32) < reach {
            inside = u;
        } else if self.ghost().is_none_or(|g| g.done > 0 || g.pos != pre) {
            self.fade_from(pre, back, k);
        }
        self.heads[0].pos = Pos(inside as u64);
        self.bounds = nb;
        self.event = self.pp_event();
    }
}

#[cfg(all(test, feature = "tape"))]
mod tests {
    use super::*;

    /// `idle_machine` and `tape_machine` name the same path, all seven.
    #[test]
    fn each_machine_state_names_its_machine() {
        let mut d = TapeDeck::idle();
        for m in Way::ALL {
            d.machine = TapeDeck::idle_machine(m);
            d.round = if m == Way::Tron {
                Round::Window
            } else {
                Round::No
            };
            assert_eq!(d.tape_machine(), m);
        }
    }
}
