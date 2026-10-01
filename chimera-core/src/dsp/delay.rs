//! Tape-style delay with MECHANICS (wow and flutter), saturation, and
//! high-frequency rolloff. Inspired by Roland Space Echo / analog tape delay
//! character.

use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::dsp::ease::{Ease, at, ease_coeff, step_of};
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

/// The read never leaves the line: 500 ms plus both excursions, with the
/// interpolator's second tap.
const _: () = assert!(24_000 + (WOW_SAMPLES + FLUTTER_SAMPLES) as usize + 2 <= MAX_DELAY_SAMPLES);

/// Tape delay parameters.
#[derive(Clone, Copy, Debug)]
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
    /// The delay's return into the reverb's send (0..1): FX diet spec
    /// § REV SEND.
    pub rev_send: f32,
}

impl Default for DelayParams {
    fn default() -> Self {
        Self {
            time_ms: 375.0, // ~1/8 note at 120bpm
            feedback: 0.4,
            wow_flutter: 0.15,
            saturation: 0.2,
            tone: 0.6,
            mix: 0.0, // off by default
            rev_send: 0.0,
        }
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
}

/// Delay runs outside `Voice`, on the FX bus: nothing is modulatable.
pub static DELAY_SPECS: [ParamSpec; 7] = [
    ParamSpec::continuous(0, "TIME", ValFmt::Uni, 10.0, 500.0, 375.0, 8.0, false).ident("TIME"),
    ParamSpec::continuous(1, "FDBK", ValFmt::Uni, 0.0, 1.0, 0.4, 1.0 / 128.0, false).ident("FDBK"),
    ParamSpec::continuous(
        2,
        "MECHANICS",
        ValFmt::Uni,
        0.0,
        1.0,
        0.15,
        1.0 / 128.0,
        false,
    )
    .ident("WOW")
    .short("MECH"),
    ParamSpec::continuous(3, "SAT", ValFmt::Uni, 0.0, 1.0, 0.2, 1.0 / 128.0, false).ident("SAT"),
    ParamSpec::continuous(4, "TONE", ValFmt::Uni, 0.0, 1.0, 0.6, 1.0 / 128.0, false).ident("TONE"),
    ParamSpec::continuous(5, "MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false).ident("MIX"),
    ParamSpec::continuous(6, "REV", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false).ident("REV"),
];

impl Block for DelayParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &DELAY_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::TIME_MS => self.time_ms,
            Self::FEEDBACK => self.feedback,
            Self::WOW_FLUTTER => self.wow_flutter,
            Self::SATURATION => self.saturation,
            Self::TONE => self.tone,
            Self::MIX => self.mix,
            Self::REV_SEND => self.rev_send,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::TIME_MS => self.time_ms = v,
            Self::FEEDBACK => self.feedback = v,
            Self::WOW_FLUTTER => self.wow_flutter = v,
            Self::SATURATION => self.saturation = v,
            Self::TONE => self.tone = v,
            Self::MIX => self.mix = v,
            Self::REV_SEND => self.rev_send = v,
            _ => {}
        }
    }
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
            base: (params.time_ms * sr / 1000.0).clamp(1.0, (MAX_DELAY_SAMPLES - 2) as f32),
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
}

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
        let wow = lfo(&mut self.wow_phase, tap.wow_rate);
        let capstan = lfo(&mut self.capstan_phase, tap.capstan_rate);
        let [a, b] = &mut self.jitter;
        *a += tap.jitter_coeff * (xorshift_noise(&mut self.rng) - *a);
        *b += tap.jitter_coeff * (*a - *b);
        // `max`/`min`, not `clamp`: `b` is never NaN, and they skip the
        // FPU's flag round trip.
        #[allow(clippy::manual_clamp)]
        let jitter = (*b * tap.jitter_gain).max(-1.0).min(1.0);
        let flutter = 0.5 * capstan + 0.5 * jitter;
        // MECH 0 adds two exact zeros: today's delay at WOW 0, bit for bit.
        (wow * tap.wow, flutter * tap.flutter)
    }
}

/// sin(2π·phase), then the phase advanced by `rate` turns.
#[inline(always)]
fn lfo(phase: &mut f32, rate: f32) -> f32 {
    *phase += rate;
    if *phase >= 1.0 {
        *phase -= 1.0;
    }
    sin_turns(*phase)
}

/// A TIME change's crossfade from the old read head to the new, samples:
/// 20 ms, so no pitch sweeps and no step (never snap).
pub const TIME_FADE: u16 = 960;
// A fade starts on a block and so ends on one: every sample of a fading
// block fades.
const _: () = assert!((TIME_FADE as usize).is_multiple_of(BLOCK_SIZE));

/// The line runs whatever MIX is, so a return brought back up plays what
/// the send is doing now, never a frozen tail (#61). MIX eases; a TIME
/// change crossfades two read heads.
pub struct TapeDelay {
    buffer: [f32; MAX_DELAY_SAMPLES],
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
    mix: Ease,
}

crate::in_place::field_list!(TapeDelay => TapeDelay { buffer, write_pos, lp_state, transport, time, next, fade, primed, mix });

impl Default for TapeDelay {
    fn default() -> Self {
        Self::new()
    }
}

impl TapeDelay {
    pub fn new() -> Self {
        Self {
            buffer: [0.0; MAX_DELAY_SAMPLES],
            write_pos: 0,
            lp_state: 0.0,
            transport: Transport::new(),
            time: 0.0,
            next: 0.0,
            fade: 0,
            primed: false,
            mix: Ease::default(),
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid for writes of one `Self`. Every field is
        // valid as zero bytes (floats, integers, `false`, `Ease`) and all
        // but the transport's seed are zero in `new()`; the transport is
        // written whole before the slot is assumed initialised.
        unsafe {
            p.write_bytes(0, 1);
            addr_of_mut!((*p).transport).write(Transport::new());
            slot.assume_init_mut()
        }
    }

    /// Insert use: dry/wet mix in place.
    pub fn process(&mut self, buf: &mut [f32; BLOCK_SIZE], params: &DelayParams, sample_rate: u32) {
        self.run(buf, params, sample_rate, true);
    }

    /// Send/return use (the FX bus): writes only the wet signal × MIX, the
    /// return level, in place of the send.
    pub fn process_wet(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        params: &DelayParams,
        sample_rate: u32,
    ) {
        self.run(buf, params, sample_rate, false);
    }

    fn run(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        params: &DelayParams,
        sample_rate: u32,
        insert: bool,
    ) {
        let tap = Tap::new(params, sample_rate);
        if !self.primed {
            (self.time, self.primed) = (tap.base(), true);
        }
        if self.fade == 0 && tap.base() != self.time {
            (self.next, self.fade) = (tap.base(), TIME_FADE);
        }
        let mix = if params.is_on() { params.mix } else { 0.0 };
        let m = self.mix.step(mix, ease_coeff(sample_rate));
        match (self.fade > 0, m.0 != m.1) {
            (false, false) => self.span::<false, false>(buf, params, &tap, m, insert),
            (false, true) => self.span::<false, true>(buf, params, &tap, m, insert),
            (true, false) => self.span::<true, false>(buf, params, &tap, m, insert),
            (true, true) => self.span::<true, true>(buf, params, &tap, m, insert),
        }
    }

    /// The block: `FADE` while a TIME crossfade runs, `RAMP` while MIX
    /// moves from `m.0` to `m.1`.
    #[inline(always)]
    fn span<const FADE: bool, const RAMP: bool>(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        params: &DelayParams,
        tap: &Tap,
        m: (f32, f32),
        insert: bool,
    ) {
        // Tone: LP coefficient (higher = brighter)
        let lp_coeff = 0.2 + params.tone * 0.75;
        let sat_gain = 1.0 + params.saturation * 3.0;
        // Once a block: the loop multiplies, never divides (within 1 ulp).
        let sat_inv = 1.0 / sat_gain;
        let sm = step_of(m, BLOCK_SIZE);
        let top = (MAX_DELAY_SAMPLES - 2) as f32;

        // The loop's state in locals: the stores into the line cannot then
        // make the compiler reload and store it every sample.
        let (mut transport, mut write_pos, mut lp_state) =
            (self.transport, self.write_pos, self.lp_state);
        let (time, next) = (self.time, self.next);
        // From the old head to the new, linearly: the new head's weight.
        let dt = 1.0 / f32::from(TIME_FADE);
        let mut t = 1.0 - f32::from(self.fade) * dt;
        for (i, s) in buf.iter_mut().enumerate() {
            let dry = *s;

            let (wow, flutter) = transport.offsets(tap);
            let head = |base: f32| (base + wow + flutter).clamp(1.0, top);
            let mut delayed = read(&self.buffer, write_pos, head(time));
            if FADE {
                delayed += (read(&self.buffer, write_pos, head(next)) - delayed) * t;
                t += dt;
            }

            // Tone: one-pole LP in feedback path (tape loses highs each pass)
            lp_state += lp_coeff * (delayed - lp_state);
            let filtered = lp_state;

            // Tape saturation in the feedback path, always on (ADR 0038): at
            // SAT 0 the loop is otherwise linear with unity DC gain, so FDBK
            // 1 grows without bound. Bounded by 1 / gain, the write stays
            // within |dry| + FDBK.
            let saturated = libm::tanhf(filtered * sat_gain) * sat_inv;

            // Write: input + feedback
            self.buffer[write_pos] = dry + saturated * params.feedback;
            write_pos += 1;
            if write_pos == MAX_DELAY_SAMPLES {
                write_pos = 0;
            }

            // Mix
            let mix = if RAMP { at(m.0, sm, i) } else { m.1 };
            let dry_gain = if insert { 1.0 - mix } else { 0.0 };
            *s = dry * dry_gain + delayed * mix;
        }
        (self.transport, self.write_pos, self.lp_state) = (transport, write_pos, lp_state);
        if FADE {
            self.fade -= BLOCK_SIZE as u16;
            if self.fade == 0 {
                self.time = next;
            }
        }
    }
}

/// The line read `delay` samples behind `write_pos`, linearly interpolated.
#[inline(always)]
fn read(buffer: &[f32; MAX_DELAY_SAMPLES], write_pos: usize, delay: f32) -> f32 {
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
    buffer[pos_a] * (1.0 - d_frac) + buffer[pos_b] * d_frac
}
