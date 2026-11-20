//! The delay, TYPE TAPE or GRAIN (ADR 0202). TAPE: Space Echo-style, with
//! MECHANICS (wow and flutter), saturation and high-frequency rolloff.
//! GRAIN: Beads-style repeats, grains read from the same line (`grain`,
//! bank 2's).

pub mod grain;
pub mod sync;
mod tape;

use crate::block::{Block, Change, Motion, ParamId, ParamSpec, Smoothing, ValFmt};
use crate::dsp::algo::math::inv_sqrt;
use crate::dsp::ease::{Ease, ease_coeff};
use crate::dsp::rev::sync::{Division, FoldState, GATE_LENGTH, Tempo, index_of};
use crate::dsp::{SendLevel, clampf};
use crate::dsp::{sin_turns, xorshift_noise};
use chimera_hal::BLOCK_SIZE;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

/// 500 ms at 48 kHz plus headroom for the transport's swing
/// (ADR 0014: the delay's range is 10..500 ms so the FX bus fits AXI).
pub const MAX_DELAY_SAMPLES: usize = 24_064;

/// MECHANICS (ADR 0053), each depth linear in the knob. The slow wow is
/// today's: 0.5 Hz, ±14 samples at full.
pub const WOW_HZ: f32 = 0.5;
pub const WOW_SAMPLES: f32 = 14.0;
/// The flutter: half a capstan sine, half band-limited noise, ±8 samples
/// at full.
pub const FLUTTER_SAMPLES: f32 = 8.0;
pub const CAPSTAN_HZ: f32 = 9.0;
/// The noise: white through two one-poles at this corner, scaled to this
/// RMS of its ±1 range, then limited to it.
pub const JITTER_HZ: f32 = 10.0;
pub const JITTER_RMS: f32 = 0.4;

/// The line's full scale, ±2.0 in `i16` (ADR 0090): 1 LSB is 2⁻¹⁴, the
/// reverb's scale (ADR 0028).
pub const LINE_SCALE: f32 = 16384.0;

/// A sample into the line: the cast truncates toward zero, so a tail
/// decays to exact zero with no limit cycle, and saturates at ±2.0, so a
/// hot write clips, never wraps (ADR 0090).
#[inline(always)]
fn to_line(x: f32) -> i16 {
    (x * LINE_SCALE) as i16
}

/// The read never leaves the line: 500 ms plus both excursions, with the
/// interpolator's second tap.
const _: () = assert!(24_000 + (WOW_SAMPLES + FLUTTER_SAMPLES) as usize + 2 <= MAX_DELAY_SAMPLES);

/// What makes the repeats; stored as its index, which is forever.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum DelayType {
    #[default]
    Tape = 0,
    Grain = 1,
}

impl DelayType {
    pub const NAMES: [&'static str; 2] = ["TAPE", "GRAIN"];

    /// The TYPE at `v`, its index; NaN and out of range are held in it.
    pub const fn at(v: f32) -> Self {
        if v >= 0.5 { Self::Grain } else { Self::Tape }
    }

    /// Its index, as `DelayParams::ty` holds it.
    pub const fn value(self) -> f32 {
        self as u8 as f32
    }
}

/// How TIME is read (ADR 0302); stored as its index, which is forever.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum DelaySync {
    /// The knob's ms.
    #[default]
    Free = 0,
    /// A note of the tempo (`DelayNote`).
    Sync = 1,
}

impl DelaySync {
    pub const NAMES: [&'static str; 2] = ["FREE", "SYNC"];

    /// The state at `v`, its index; NaN and out of range are held in it.
    pub const fn at(v: f32) -> Self {
        if v >= 0.5 { Self::Sync } else { Self::Free }
    }

    /// Its index, as `DelayParams::more` holds it.
    pub const fn value(self) -> f32 {
        self as u8 as f32
    }
}

/// TIME's note under SYNC: a place in the reverb's `GATE_LENGTH`, 1/32T
/// to 1/4 by length (ADR 0302). Always in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayNote(u8);

impl DelayNote {
    /// The notes a turn walks.
    pub const COUNT: usize = GATE_LENGTH.len();
    /// 1/8D: 375 ms at 120 BPM, TIME's own default.
    pub const DEFAULT: Self = Self(9);

    /// Place `i`, held in the list.
    pub const fn new(i: usize) -> Self {
        Self(if i < Self::COUNT { i } else { Self::COUNT - 1 } as u8)
    }

    /// The note stored as `p` (`rev::sync::index_of`: `i / (COUNT − 1)`).
    pub fn at(p: f32) -> Self {
        Self::new(index_of(p, Self::COUNT))
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// As `DelayParams::more` holds it.
    pub const fn value(self) -> f32 {
        self.0 as f32 / (Self::COUNT - 1) as f32
    }

    /// The note value.
    pub const fn division(self) -> Division {
        GATE_LENGTH[self.0 as usize]
    }
}

// The default is the dotted eighth.
const _: () = {
    let d = DelayNote::DEFAULT.division();
    assert!(d.num == 1 && d.den == 8 && matches!(d.kind, crate::dsp::rev::sync::DivKind::Dotted));
};

/// Delay parameters: TIME, FDBK, TONE, MIX, REV, SYNC and its NOTE are
/// both TYPEs'.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelayParams {
    /// Delay time in ms (10..500)
    pub time_ms: f32,
    /// Feedback amount (0..1)
    pub feedback: f32,
    /// MECHANICS (0..1): tape speed instability, a slow wow and an
    /// irregular flutter (ADR 0053); the field and disk ident predate it.
    pub wow_flutter: f32,
    /// Tape saturation amount (0..1) — soft clipping in feedback path; 0 is
    /// the gentlest, never none (ADR 0038)
    pub saturation: f32,
    /// Tone: high-frequency rolloff in feedback (0..1, 0=dark, 1=bright)
    pub tone: f32,
    /// Dry/wet mix (0..1)
    pub mix: f32,
    /// The delay's return into the reverb's send: FX diet spec § REV SEND,
    /// fed as its tapered gain (ADR 0278).
    pub rev_send: SendLevel,
    /// Ids 7–13 in order. GRAIN's TYPE and knobs: TYPE's index (0 TAPE,
    /// 1 GRAIN, `kind`), SIZE, DENSITY, PITCH (semitones, −24..24) and
    /// TEXTURE (ADR 0202). Then SYNC's index (0 FREE, 1 SYNC, `sync`) and
    /// its NOTE (`note`, ADR 0302), kept beside `time_ms`, never in it.
    /// One array, so `get` and `write` reach them in one arm each and
    /// bank 1's codec code stays as it was (ADR 0198).
    pub more: [f32; 7],
}

impl DelayParams {
    pub const DEFAULT: Self = Self {
        time_ms: 375.0, // ~1/8 note at 120bpm
        feedback: 0.4,
        wow_flutter: 0.15,
        saturation: 0.2,
        tone: 0.6,
        mix: 1.0, // unity return (sends spec § 4)
        rev_send: SendLevel::OFF,
        more: [0.0, 0.6, 0.6, 0.0, 0.2, 0.0, DelayNote::DEFAULT.value()],
    };
}

impl Default for DelayParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl DelayParams {
    /// Off when the mix is below audibility: the return fades out, the line
    /// runs on (ADR 0061).
    pub fn is_on(&self) -> bool {
        self.mix >= 0.001
    }

    pub const TIME_MS: ParamId = ParamId(0);
    pub const FEEDBACK: ParamId = ParamId(1);
    pub const WOW_FLUTTER: ParamId = ParamId(2);
    pub const SATURATION: ParamId = ParamId(3);
    pub const TONE: ParamId = ParamId(4);
    pub const MIX: ParamId = ParamId(5);
    pub const REV_SEND: ParamId = ParamId(6);
    pub const TYPE: ParamId = ParamId(7);

    /// The TYPE `more` holds.
    pub const fn kind(&self) -> DelayType {
        DelayType::at(self.more[0])
    }

    pub const fn size(&self) -> f32 {
        self.more[1]
    }

    pub const fn density(&self) -> f32 {
        self.more[2]
    }

    pub const fn pitch(&self) -> f32 {
        self.more[3]
    }

    pub const fn texture(&self) -> f32 {
        self.more[4]
    }

    /// SYNC's state.
    pub const fn sync(&self) -> DelaySync {
        DelaySync::at(self.more[5])
    }

    /// SYNC's note, whatever `sync` is: FREE keeps it.
    pub fn note(&self) -> DelayNote {
        DelayNote::at(self.more[6])
    }

    /// These params with `id` (7–13) at `v`; any other id leaves them as
    /// they are.
    pub const fn with(mut self, id: ParamId, v: f32) -> Self {
        if let Some(i) = more_slot(id) {
            self.more[i] = v;
        }
        self
    }
    pub const SIZE: ParamId = ParamId(8);
    pub const DENSITY: ParamId = ParamId(9);
    pub const PITCH: ParamId = ParamId(10);
    pub const TEXTURE: ParamId = ParamId(11);
    pub const SYNC: ParamId = ParamId(12);
    pub const NOTE: ParamId = ParamId(13);
}

/// Delay runs outside `Voice`, on the FX bus: nothing is modulatable.
/// Every delay param shows the rings (ADR 0067).
const RINGS: crate::glyph::FocusGlyph =
    crate::glyph::FocusGlyph::Composite(crate::glyph::CompositeId::DelayRings);

pub static DELAY_SPECS: [ParamSpec; 14] = [
    ParamSpec::continuous(
        0,
        "TIME",
        ValFmt::Uni,
        10.0,
        500.0,
        375.0,
        8.0,
        false,
        Smoothing::Own,
    )
    .ident("TIME")
    .glyph(RINGS),
    ParamSpec::continuous(
        1,
        "FDBK",
        ValFmt::Uni,
        0.0,
        1.0,
        0.4,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("FDBK")
    .glyph(RINGS),
    ParamSpec::continuous(
        2,
        "MECHANICS",
        ValFmt::Uni,
        0.0,
        1.0,
        0.15,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("WOW")
    .glyph(RINGS)
    .short("MECH"),
    ParamSpec::continuous(
        3,
        "SAT",
        ValFmt::Uni,
        0.0,
        1.0,
        0.2,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("SAT")
    .glyph(RINGS),
    ParamSpec::continuous(
        4,
        "TONE",
        ValFmt::Uni,
        0.0,
        1.0,
        0.6,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("TONE")
    .glyph(RINGS),
    ParamSpec::continuous(
        5,
        "MIX",
        ValFmt::Uni,
        0.0,
        1.0,
        1.0,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("MIX")
    .glyph(RINGS),
    ParamSpec::continuous(
        6,
        "REV",
        ValFmt::Uni,
        0.0,
        1.0,
        0.0,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("REV")
    .glyph(RINGS),
    // GRAIN (ADR 0202): its pots hold their values under TAPE.
    // TYPE stored as its index, a real, as HINTS is: no enum codes, so no
    // codec code in bank 1 (ADR 0198).
    ParamSpec::stepped(
        7,
        "TYPE",
        ValFmt::Names(&DelayType::NAMES),
        0.0,
        1.0,
        0.0,
        false,
        Motion::Stepped(Change::Crossfade),
    )
    .ident("TYPE")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::continuous(
        8,
        "SIZE",
        ValFmt::Uni,
        0.0,
        1.0,
        0.6,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("SIZE"),
    ParamSpec::continuous(
        9,
        "DENSITY",
        ValFmt::Uni,
        0.0,
        1.0,
        0.6,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("DENSITY")
    .short("DENS"),
    ParamSpec::stepped(
        10,
        "PITCH",
        ValFmt::Signed(24),
        -24.0,
        24.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::Own),
    )
    .ident("PITCH"),
    ParamSpec::continuous(
        11,
        "TEXTURE",
        ValFmt::Uni,
        0.0,
        1.0,
        0.2,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("TEXTURE")
    .short("TEXT"),
    // Tempo sync (ADR 0302). SYNC stored as its index, as TYPE is; NOTE
    // as the reverb stores a division, a tick a note. A project without
    // them loads FREE and 1/8D.
    ParamSpec::stepped(
        12,
        "SYNC",
        ValFmt::Names(&DelaySync::NAMES),
        0.0,
        1.0,
        0.0,
        false,
        Motion::Stepped(Change::Crossfade),
    )
    .ident("SYNC")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::continuous(
        13,
        "NOTE",
        ValFmt::Law(crate::dsp::modulator::law::Law::DelayNote),
        0.0,
        1.0,
        NOTE_DEFAULT,
        1.0 / (DelayNote::COUNT - 1) as f32,
        false,
        Smoothing::Own,
    )
    .ident("NOTE"),
];

const NOTE_DEFAULT: f32 = DelayNote::DEFAULT.value();

impl Block for DelayParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &DELAY_SPECS
    }

    #[inline(never)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::TIME_MS => self.time_ms,
            Self::FEEDBACK => self.feedback,
            Self::WOW_FLUTTER => self.wow_flutter,
            Self::SATURATION => self.saturation,
            Self::TONE => self.tone,
            Self::MIX => self.mix,
            Self::REV_SEND => self.rev_send.get(),
            _ => more_slot(id).map_or(0.0, |i| self.more[i]),
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::TIME_MS => self.time_ms = v,
            Self::FEEDBACK => self.feedback = v,
            Self::WOW_FLUTTER => self.wow_flutter = v,
            Self::SATURATION => self.saturation = v,
            Self::TONE => self.tone = v,
            Self::MIX => self.mix = v,
            Self::REV_SEND => self.rev_send = SendLevel::new(v),
            _ => {
                if let Some(i) = more_slot(id) {
                    self.more[i] = v;
                }
            }
        }
    }
}

/// GRAIN's steady block over the bus's bill, which holds TAPE's
/// (`tools/loop_count.py` on the ORBIT ELF, ADR 0202): its sample loop 61
/// instructions and a divide, 110 cycles; the block's own 10.1 and 0.23,
/// 20, and two starts' gains (a `powf` each), 5; six grains at 46
/// instructions a sample each (the loop runs two samples a round), 74, and
/// their runs' setup, 1: 580. The feedback's trim, a `powf` a block and a
/// multiply a sample, about 4 more (estimated): billed 590. TAPE's head,
/// idle under GRAIN, isn't taken off. A TYPE crossfade runs both for 20
/// ms, in the headroom (ADR 0061). The unit reads less (#746; bench FX,
/// 2026-10-22, rev V at 480 MHz): BUS GRAIN 1,386 against BUS 1,110, 276,
/// with every grain playing at PITCH +12; a TYPE crossfade always running
/// 42 over GRAIN alone: billed 320.
pub const GRAIN_COST: crate::hw::Cost = crate::hw::Cost(320);

/// What the delay bills over the bus: GRAIN's cost while TYPE is GRAIN.
pub const fn extra_cost(p: &DelayParams) -> crate::hw::Cost {
    match p.kind() {
        DelayType::Grain => GRAIN_COST,
        DelayType::Tape => crate::hw::Cost::ZERO,
    }
}

/// `id`'s place in `DelayParams::more`, if it is there.
#[inline(always)]
const fn more_slot(id: ParamId) -> Option<usize> {
    let i = id.0.wrapping_sub(DelayParams::TYPE.0) as usize;
    if i < DelayParams::DEFAULT.more.len() {
        Some(i)
    } else {
        None
    }
}

/// The delay's per-sample work, TAPE's and GRAIN's, bank 2's (ADR 0202,
/// 0213): bank 1 calls `run` once a block.
#[repr(C)]
pub struct DelayDoor {
    pub run: fn(&mut TapeDelay, &mut [f32; BLOCK_SIZE], &DelayParams, Tempo, u32, bool) -> bool,
}

impl DelayDoor {
    pub const DOOR: DelayDoor = DelayDoor { run: tape::run };
}

/// One block's constants for the transport: the base delay, the depths
/// and the per-sample rates.
#[derive(Clone, Copy, Debug)]
pub struct Tap {
    base: f32,
    /// The wow's peak excursion, `wow_flutter × WOW_SAMPLES`.
    wow: f32,
    wow_rate: f32,
    /// The flutter's peak excursion, `wow_flutter × FLUTTER_SAMPLES`.
    flutter: f32,
    capstan_rate: f32,
    jitter_coeff: f32,
    jitter_gain: f32,
}

impl Tap {
    pub fn new(params: &DelayParams, sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        let jitter_coeff = core::f32::consts::TAU * JITTER_HZ / sr;
        Self {
            base: clampf(
                params.time_ms * sr / 1000.0,
                1.0,
                (MAX_DELAY_SAMPLES - 2) as f32,
            ),
            wow: params.wow_flutter * WOW_SAMPLES,
            wow_rate: WOW_HZ / sr,
            flutter: params.wow_flutter * FLUTTER_SAMPLES,
            capstan_rate: CAPSTAN_HZ / sr,
            jitter_coeff,
            // Two one-poles pass a/4 of uniform noise's 1/3 variance.
            jitter_gain: JITTER_RMS / libm::sqrtf(jitter_coeff / 12.0),
        }
    }

    /// The read delay in samples before the transport moves it.
    pub fn base(&self) -> f32 {
        self.base
    }

    /// TAPE's transport (spec 2026-10-04 § 2): run once a block, its
    /// offsets' sum is the head's speed deviation in cents, not a delay to
    /// differentiate, so the wow and the flutter move pitch directly, as a
    /// capstan's do. Wow ±`TAPE_WOW_CENTS`·`tape`, flutter
    /// ±`TAPE_FLUTTER_CENTS`·`tape`, every rate at `block_rate`; base 0,
    /// built directly (`new` clamps a base to ≥ 1).
    pub fn speed_cents(tape: f32, block_rate: f32) -> Tap {
        let per = 1.0 / block_rate;
        let jitter_coeff = core::f32::consts::TAU * JITTER_HZ * per;
        Self {
            base: 0.0,
            wow: tape * TAPE_WOW_CENTS,
            wow_rate: WOW_HZ * per,
            flutter: tape * TAPE_FLUTTER_CENTS,
            capstan_rate: CAPSTAN_HZ * per,
            jitter_coeff,
            jitter_gain: JITTER_RMS * inv_sqrt(jitter_coeff * (1.0 / 12.0)),
        }
    }
}

impl Tap {
    /// The wow's rate, turns a tick.
    pub fn wow_rate(&self) -> f32 {
        self.wow_rate
    }

    /// A voice's wow at its own rate, turns a tick: its phase runs on, so
    /// the pitch stays continuous.
    pub fn set_wow_rate(&mut self, rate: f32) {
        self.wow_rate = rate;
    }
}

/// TAPE's depths at 1, cents (spec 2026-10-04 § 2).
pub const TAPE_WOW_CENTS: f32 = 25.0;
pub const TAPE_FLUTTER_CENTS: f32 = 8.0;

/// The tape transport under MECHANICS: a slow wow plus an irregular
/// flutter. Pure state; `TapeDelay` reads the line where it points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transport {
    wow_phase: f32,
    capstan_phase: f32,
    /// The jitter's two one-poles
    jitter: [f32; 2],
    rng: u32,
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport {
    pub const fn new() -> Self {
        Self {
            wow_phase: 0.0,
            capstan_phase: 0.0,
            jitter: [0.0; 2],
            rng: 0x2545_f491,
        }
    }

    /// Its own strip of tape: phases and noise from `s`, spread by the
    /// golden ratio, so voices seeded 0, 1, 2, … sit far apart on the wow.
    pub fn seeded(s: u32) -> Self {
        const PHI: f32 = 0.618_034;
        let turn = |k: f32| {
            let x = s as f32 * k;
            x - libm::floorf(x)
        };
        Self {
            wow_phase: turn(PHI),
            capstan_phase: turn(PHI * PHI),
            jitter: [0.0; 2],
            rng: (s.wrapping_mul(0x9E37_79B9) ^ 0x2545_f491) | 1,
        }
    }

    /// One sample: the read delay in samples, before the line's clamp.
    #[inline(always)]
    pub fn next(&mut self, tap: &Tap) -> f32 {
        let (wow, flutter) = self.offsets(tap);
        tap.base + wow + flutter
    }

    /// One sample: the wow's and the flutter's offsets from the base, in
    /// samples, summed onto it in that order (`next`).
    #[inline(always)]
    pub fn offsets(&mut self, tap: &Tap) -> (f32, f32) {
        let b = self.advance(tap);
        let wow = sin_turns(self.wow_phase);
        let capstan = sin_turns(self.capstan_phase);
        // `max`/`min`, not `clamp`: `b` is never NaN, and they skip the
        // FPU's flag round trip.
        #[allow(clippy::manual_clamp)]
        let jitter = (b * tap.jitter_gain).max(-1.0).min(1.0);
        let flutter = 0.5 * capstan + 0.5 * jitter;
        // MECH 0 adds two exact zeros: today's delay at WOW 0, bit for bit.
        (wow * tap.wow, flutter * tap.flutter)
    }

    /// One sample's state: both phases on and the jitter's two one-poles,
    /// whose second it returns. All a sample moves, without the sines.
    #[inline(always)]
    pub fn advance(&mut self, tap: &Tap) -> f32 {
        turn(&mut self.wow_phase, tap.wow_rate);
        turn(&mut self.capstan_phase, tap.capstan_rate);
        let [a, b] = &mut self.jitter;
        *a += tap.jitter_coeff * (xorshift_noise(&mut self.rng) - *a);
        *b += tap.jitter_coeff * (*a - *b);
        *b
    }

    /// The jitter's states are finite: every offset it gives is.
    pub fn is_finite(&self) -> bool {
        self.jitter.iter().all(|j| j.is_finite())
            && self.wow_phase.is_finite()
            && self.capstan_phase.is_finite()
    }
}

/// The phase advanced by `rate` turns, wrapped once.
#[inline(always)]
fn turn(phase: &mut f32, rate: f32) {
    *phase += rate;
    if *phase >= 1.0 {
        *phase -= 1.0;
    }
}

/// A TIME change's crossfade from the old read head to the new, samples:
/// 20 ms, so no pitch sweeps and no step (never snap).
pub const TIME_FADE: u16 = 960;
// A fade starts on a block and so ends on one: every sample of a fading
// block fades.
const _: () = assert!((TIME_FADE as usize).is_multiple_of(BLOCK_SIZE));

/// The line runs whatever MIX is, so a return brought back up plays what
/// the send is doing now, never a frozen tail (#61). MIX, FDBK, SAT, TONE
/// and MECHANICS ease (FDBK, SAT and TONE change what is written, heard a
/// TIME later; MECHANICS moves the read head); a TIME change crossfades
/// two read heads; under SYNC a small tempo change glides the one
/// (`sync::aim`, ADR 0302).
pub struct TapeDelay {
    /// At `LINE_SCALE` (ADR 0090).
    buffer: [i16; MAX_DELAY_SAMPLES],
    write_pos: usize,
    /// LP filter state for tone control in feedback
    lp_state: f32,
    transport: Transport,
    /// The read head's base delay, samples; `next` the one it fades to.
    time: f32,
    next: f32,
    /// Samples of the TIME crossfade left; 0 when none.
    fade: u16,
    /// A block has set `time`.
    primed: bool,
    /// SYNC and its note as the last block read them, and how its note
    /// was folded into the line (`sync::tap`).
    aim: sync::Aim,
    fold: FoldState,
    /// The gliding head's speed off the tape's, samples of delay a
    /// sample; 0 at rest.
    speed: f32,
    mix: Ease,
    feedback: Ease,
    saturation: Ease,
    tone: Ease,
    mechanics: Ease,
    /// GRAIN's grains and its share of the return (bank 2's, ADR 0202).
    grain: grain::State,
    /// Samples since the line was last written anything but 0, counted a
    /// block at a time (saturating): at `MAX_DELAY_SAMPLES` the line is all
    /// zero (`tape::run`'s idle skip).
    zeros: u32,
}

crate::in_place::field_list!(TapeDelay => TapeDelay { buffer, write_pos, lp_state, transport, time, next, fade, primed, aim, fold, speed, mix, feedback, saturation, tone, mechanics, grain, zeros });

/// One block's eased settings, each `(from, to)`.
#[derive(Clone, Copy)]
struct Eased {
    mix: (f32, f32),
    feedback: (f32, f32),
    saturation: (f32, f32),
    tone: (f32, f32),
    mechanics: (f32, f32),
}

impl Eased {
    fn moving(&self) -> bool {
        [
            self.mix,
            self.feedback,
            self.saturation,
            self.tone,
            self.mechanics,
        ]
        .iter()
        .any(|e| e.0 != e.1)
    }
}

impl Default for TapeDelay {
    fn default() -> Self {
        Self::new()
    }
}

impl TapeDelay {
    pub fn new() -> Self {
        Self {
            buffer: [0; MAX_DELAY_SAMPLES],
            write_pos: 0,
            lp_state: 0.0,
            transport: Transport::new(),
            time: 0.0,
            next: 0.0,
            fade: 0,
            primed: false,
            aim: sync::Aim::NEW,
            fold: FoldState(0),
            speed: 0.0,
            mix: Ease::default(),
            feedback: Ease::default(),
            saturation: Ease::default(),
            tone: Ease::default(),
            mechanics: Ease::default(),
            grain: grain::State::NEW,
            zeros: 0,
        }
    }

    /// TAPE's head: its delay in samples, and the samples its crossfade
    /// has left.
    #[cfg(any(test, feature = "test-support"))]
    pub fn head(&self) -> (f32, u16) {
        (self.time, self.fade)
    }

    /// The line holds exact zeros: its tail has ended (ADR 0090).
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_silent(&self) -> bool {
        self.buffer.iter().all(|&s| s == 0)
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid for writes of one `Self`. Every field is
        // valid as zero bytes (floats, integers, `false`, `Ease`, FREE's
        // `Aim` at note 0, no fold, and
        // GRAIN's state, which its kernel renews before its first block)
        // and all but the transport's seed are zero in `new()`; the
        // transport is written whole before the slot is assumed
        // initialised.
        unsafe {
            p.write_bytes(0, 1);
            addr_of_mut!((*p).transport).write(Transport::new());
            slot.assume_init_mut()
        }
    }

    /// Insert use: dry/wet mix in place, at 120 BPM.
    pub fn process(&mut self, buf: &mut [f32; BLOCK_SIZE], params: &DelayParams, sample_rate: u32) {
        self.run(buf, params, Tempo::DEFAULT, sample_rate, true);
    }

    /// `play` at 120 BPM: FREE never reads the tempo.
    pub fn process_wet(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        params: &DelayParams,
        sample_rate: u32,
    ) -> bool {
        self.play(buf, params, Tempo::DEFAULT, sample_rate)
    }

    /// Send/return use (the FX bus): writes only the wet signal × MIX, the
    /// return level, in place of the send; SYNC's note is read at `tempo`
    /// (ADR 0302). Whether the return is known all +0 (#793).
    pub fn play(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        params: &DelayParams,
        tempo: Tempo,
        sample_rate: u32,
    ) -> bool {
        self.run(buf, params, tempo, sample_rate, false)
    }

    /// How the last block folded SYNC's note into the line; none under
    /// FREE. What the TIME cell's mark reads.
    pub fn fold(&self) -> FoldState {
        self.fold
    }

    /// Through the door with `bank2`; closed, the return is silent and an
    /// insert passes its dry (ADR 0213). Whether `buf` is known all +0.
    fn run(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        params: &DelayParams,
        tempo: Tempo,
        sample_rate: u32,
        insert: bool,
    ) -> bool {
        #[cfg(feature = "bank2")]
        {
            crate::bank2::DOOR_CELL
                .with(|d| (d.delay.run)(self, buf, params, tempo, sample_rate, insert))
                .unwrap_or_else(|| {
                    if !insert {
                        buf.fill(0.0);
                    }
                    !insert
                })
        }
        #[cfg(not(feature = "bank2"))]
        tape::run(self, buf, params, tempo, sample_rate, insert)
    }

    /// The block's eased settings: MIX (0 while off), FDBK, SAT, TONE and
    /// MECHANICS, each a step on.
    #[inline(always)]
    fn eased(&mut self, p: &DelayParams, fs: u32) -> Eased {
        let mix = if p.is_on() { p.mix } else { 0.0 };
        let k = ease_coeff(fs);
        Eased {
            mix: self.mix.step(mix, k),
            feedback: self.feedback.step(p.feedback, k),
            saturation: self.saturation.step(p.saturation, k),
            tone: self.tone.step(p.tone, k),
            mechanics: self.mechanics.step(p.wow_flutter, k),
        }
    }
}

/// The line read `delay` samples behind `write_pos`, linearly interpolated,
/// back to ±2.0.
#[inline(always)]
fn read(buffer: &[i16; MAX_DELAY_SAMPLES], write_pos: usize, delay: f32) -> f32 {
    let d_int = delay as usize;
    let d_frac = delay - d_int as f32;
    let pos_a = if write_pos >= d_int {
        write_pos - d_int
    } else {
        write_pos + MAX_DELAY_SAMPLES - d_int
    };
    let pos_b = if pos_a == 0 {
        MAX_DELAY_SAMPLES - 1
    } else {
        pos_a - 1
    };
    let (a, b) = (f32::from(buffer[pos_a]), f32::from(buffer[pos_b]));
    // Two multiplies, as the f32 line's.
    (a + (b - a) * d_frac) * (1.0 / LINE_SCALE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_truncates_toward_zero_and_saturates() {
        assert_eq!(to_line(0.9 / 16384.0), 0);
        assert_eq!(to_line(-0.9 / 16384.0), 0);
        assert_eq!(to_line(3.0), i16::MAX);
        assert_eq!(to_line(-3.0), i16::MIN);
        assert_eq!(to_line(1.0), 16384);
    }
}
