//! Tape on DAC pair 1 (FX diet spec § Tape). Per side: wow, pre-emphasis,
//! 2× oversampled soft saturation, de-emphasis, head bump and an HF
//! roll-off that darkens with DRIVE, blended in parallel with the dry.
//! MIX 0 is an exact bypass.

use chimera_hal::BLOCK_SIZE;
use core::f32::consts::{PI, SQRT_2};
use core::mem::MaybeUninit;

use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::dsp::algo::math::exp2;
use crate::dsp::sin_turns;

/// The input line per side: the wet reads it through the wow, the dry at
/// the wet's whole delay.
const LINE: usize = 32;
const MASK: usize = LINE - 1;
/// The wow's centre tap and its largest swing, in samples.
const WOW_BASE: f32 = 12.0;
const WOW_SWING: f32 = 8.0;
/// The 2× oversampler's 15-tap half-band (spec § Tape): h[±1], h[±3],
/// h[±5], h[±7]; centre 0.5, other even taps 0. Minimax, stopband from
/// 32 kHz at 96 kHz, 49 dB down; passband ±0.03 dB to 16 kHz.
pub const HB: [f32; 4] = [0.309_846, -0.082_748, 0.030_668, -0.009_527];
/// `HB` × 2: the interpolator's zero-stuffing gain.
const HB2: [f32; 4] = [2.0 * HB[0], 2.0 * HB[1], 2.0 * HB[2], 2.0 * HB[3]];
/// The oversampler's delay in samples: 3½ up, 3½ down.
pub const OS_LATENCY: usize = 7;
/// Pre-emphasised input kept per side for the interpolator, and filtered
/// (even) and centre-tap (odd) saturated samples for the decimator.
const U_HIST: usize = 7;
const E_HIST: usize = 7;
const O_HIST: usize = 4;
/// The dry's tap: the wet's delay, so the parallel blend lines up.
pub const DRY_TAP: usize = WOW_BASE as usize + OS_LATENCY;
const _: () = assert!(DRY_TAP < LINE && (WOW_BASE + WOW_SWING) as usize + 1 < LINE);
/// Engaging or releasing crossfades from the undelayed signal over this
/// many samples (10 ms at 48 kHz), after one block of priming.
pub const ENGAGE: u32 = 480;

/// Pre-emphasis: a first-order high shelf, +6 dB, midpoint 3 kHz.
const SHELF_GAIN: f32 = 2.0;
const SHELF_MID_HZ: f32 = 3_000.0;
/// Head bump: +2 dB peak at 80 Hz, Q 0.7.
const BUMP_HZ: f32 = 80.0;
const BUMP_A: f32 = 1.122_018_5; // 10^(2/40)
const BUMP_Q: f32 = 0.7;
/// 2π·log2(e): `exp(−2πf/fs)` is `exp2(−TWO_PI_LOG2E·f/fs)`.
const TWO_PI_LOG2E: f32 = 9.064_72;
const LOG2_E: f32 = core::f32::consts::LOG2_E;
/// Every control: one-pole, once per block.
const SMOOTH_S: f32 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapeParams {
    pub drive: f32,
    /// 0 darker, 1 brighter.
    pub tone: f32,
    pub wow: f32,
    /// Parallel blend: 0 dry (bypass), 1 all tape.
    pub mix: f32,
}

impl Default for TapeParams {
    fn default() -> Self {
        Self {
            drive: 0.0,
            tone: 0.5,
            wow: 0.0,
            mix: 0.0,
        }
    }
}

impl TapeParams {
    pub const DRIVE: ParamId = ParamId(0);
    pub const TONE: ParamId = ParamId(1);
    pub const WOW: ParamId = ParamId(2);
    pub const MIX: ParamId = ParamId(3);

    /// Off when the mix is below audibility: pair 1 passes untouched.
    pub fn is_on(&self) -> bool {
        self.mix >= 0.001
    }

    /// NaN reads as the default; everything clamps to 0..1.
    pub fn sanitised(&self) -> Self {
        let d = Self::default();
        let f = |v: f32, def: f32| if v.is_nan() { def } else { v.clamp(0.0, 1.0) };
        Self {
            drive: f(self.drive, d.drive),
            tone: f(self.tone, d.tone),
            wow: f(self.wow, d.wow),
            mix: f(self.mix, d.mix),
        }
    }
}

/// The tape runs outside `Voice`: nothing is modulatable.
pub static TAPE_SPECS: [ParamSpec; 4] = [
    ParamSpec::continuous(0, "DRIVE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(1, "TONE", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "WOW", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for TapeParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &TAPE_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::DRIVE => self.drive,
            Self::TONE => self.tone,
            Self::WOW => self.wow,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::DRIVE => self.drive = v,
            Self::TONE => self.tone = v,
            Self::WOW => self.wow = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}

/// Saturator input gain: 0 to +24 dB.
pub fn drive_gain(drive: f32) -> f32 {
    exp2(4.0 * drive)
}

/// Level compensation: 1/√gain, so loud material keeps its level.
pub fn drive_comp(drive: f32) -> f32 {
    exp2(-2.0 * drive)
}

/// The HF roll-off's corner: 12 kHz at TONE ½ and DRIVE 0, ±1.5 octaves
/// across TONE, down 1.5 octaves at full DRIVE.
pub fn rolloff_hz(drive: f32, tone: f32) -> f32 {
    12_000.0 * exp2(3.0 * (tone - 0.5) - 1.5 * drive)
}

/// The divide-free soft clip on `x` already scaled by ⅔: the cubic
/// 1.5·x − 0.5·x³ on x clamped to ±1. Slope 1 at 0, as `v` itself would
/// be; it reaches ±1 with zero slope at |v| = 1.5.
#[inline(always)]
fn cubic(x: f32) -> f32 {
    // t = 2·clamp(x, ±1) without a compare (no FPSCR stall on the M7);
    // then 1.5·(t/2) − 0.5·(t/2)³.
    let t = (x + 1.0).abs() - (x - 1.0).abs();
    t * (0.75 - 0.0625 * t * t)
}

/// The soft clip, unscaled: `soft_clip(v)` ≈ `v` for small `v`, ±1 from
/// |v| = 1.5.
pub fn soft_clip(v: f32) -> f32 {
    cubic(v * (2.0 / 3.0))
}

/// The fixed filters' coefficients at one sample rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coefs {
    /// Pre-emphasis `[b0, b1, a1]`.
    pub pre: [f32; 3],
    /// Its exact inverse, `[b0, b1, a1]`.
    pub de: [f32; 3],
    /// Head bump `[b0, b1, b2, a1, a2]`, normalised.
    pub bump: [f32; 5],
}

impl Coefs {
    /// Once per sample-rate change: libm here is not per sample or block.
    pub fn new(sample_rate: u32) -> Self {
        let fs = sample_rate as f32;
        // Bilinear shelf (G·s + ω)/(s + ω), pole prewarped to f_mid·√G.
        let t = libm::tanf(PI * SHELF_MID_HZ * SQRT_2 / fs);
        let g = SHELF_GAIN;
        let (b0, b1, a1) = (
            (g + t) / (1.0 + t),
            (t - g) / (1.0 + t),
            (t - 1.0) / (1.0 + t),
        );
        let w0 = 2.0 * PI * BUMP_HZ / fs;
        let (sn, cs) = (libm::sinf(w0), libm::cosf(w0));
        let alpha = sn / (2.0 * BUMP_Q);
        let a0 = 1.0 + alpha / BUMP_A;
        Self {
            pre: [b0, b1, a1],
            de: [1.0 / b0, a1 / b0, b1 / b0],
            bump: [
                (1.0 + alpha * BUMP_A) / a0,
                -2.0 * cs / a0,
                (1.0 - alpha * BUMP_A) / a0,
                -2.0 * cs / a0,
                (1.0 - alpha / BUMP_A) / a0,
            ],
        }
    }
}

/// One side's state.
#[derive(Clone, Copy)]
struct Side {
    line: [f32; LINE],
    pos: usize,
    pre: (f32, f32),
    /// The oversampler's history: pre-emphasised input, then the
    /// saturated even (filtered) and odd (centre-tap) 2× samples.
    u: [f32; U_HIST],
    even: [f32; E_HIST],
    odd: [f32; O_HIST],
    de: (f32, f32),
    bump: [f32; 4],
    lp: f32,
}

crate::in_place::field_list!(Side => Side { line, pos, pre, u, even, odd, de, bump, lp });

pub struct Tape {
    side: [Side; 2],
    coefs: Coefs,
    /// The rate `coefs` were built for; 0 before the first block.
    fs: u32,
    /// The smoothed controls at the end of the last block: this block ramps
    /// from them.
    last: TapeParams,
    /// Running: engaged, fading, or priming. Off, `process` returns at
    /// once.
    running: bool,
    engage: f32,
    /// The transport's phases, in turns × 2^32: they wrap for free.
    wow_phase: u32,
    flutter_phase: u32,
}

crate::in_place::field_list!(Tape => Tape { side, coefs, fs, last, running, engage, wow_phase, flutter_phase });

impl Default for Tape {
    fn default() -> Self {
        Self::new()
    }
}

impl Tape {
    pub const fn new() -> Self {
        Self {
            side: [Side {
                line: [0.0; LINE],
                pos: 0,
                pre: (0.0, 0.0),
                u: [0.0; U_HIST],
                even: [0.0; E_HIST],
                odd: [0.0; O_HIST],
                de: (0.0, 0.0),
                bump: [0.0; 4],
                lp: 0.0,
            }; 2],
            coefs: Coefs {
                pre: [0.0; 3],
                de: [0.0; 3],
                bump: [0.0; 5],
            },
            fs: 0,
            last: TapeParams {
                drive: 0.0,
                tone: 0.0,
                wow: 0.0,
                mix: 0.0,
            },
            running: false,
            engage: 0.0,
            wow_phase: 0,
            flutter_phase: 0,
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: every field is `f32`, `u32`, `usize`, `bool` or arrays
        // and tuples of them, valid as zero bytes (`false` for the bool);
        // zero is exactly `new()`'s state.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }

    /// Tape over one pair's interleaved block (L, R), in place.
    pub fn process(&mut self, pair: &mut [f32; 2 * BLOCK_SIZE], p: &TapeParams, sample_rate: u32) {
        let on = p.is_on();
        if !on && !self.running {
            return;
        }
        if self.fs != sample_rate {
            self.coefs = Coefs::new(sample_rate);
            self.fs = sample_rate;
        }
        let t = p.sanitised();
        if !self.running {
            // From bypass: nothing of the tape sounds yet, so the controls
            // start at their targets, and the first block primes the line
            // and the filters at weight 0 before the fade in.
            self.last = t;
            self.running = true;
            self.engage = -(BLOCK_SIZE as f32) / ENGAGE as f32;
        }
        let fs = sample_rate as f32;
        // One-pole, once per block, then a linear ramp across it.
        let k = exp2(-LOG2_E * BLOCK_SIZE as f32 / (SMOOTH_S * fs));
        let sm = |from: f32, to: f32| to + k * (from - to);
        let l = self.last;
        let t = TapeParams {
            drive: sm(l.drive, t.drive),
            tone: sm(l.tone, t.tone),
            wow: sm(l.wow, t.wow),
            mix: sm(l.mix, t.mix),
        };
        let a = |d: f32, tone: f32| 1.0 - exp2(-TWO_PI_LOG2E * rolloff_hz(d, tone) / fs);
        let n = BLOCK_SIZE as f32;
        // Each ramp as its start and per-sample step; the first sample
        // takes one step, the last lands on the end.
        let ramp = |x0: f32, x1: f32| (x0, (x1 - x0) / n);
        // ⅔ of the gain is the soft clip's input scale; its ×1.5 output
        // scale sits in `cubic`.
        let g = ramp(
            drive_gain(l.drive) * (2.0 / 3.0),
            drive_gain(t.drive) * (2.0 / 3.0),
        );
        let c = ramp(drive_comp(l.drive), drive_comp(t.drive));
        let lp = ramp(a(l.drive, l.tone), a(t.drive, t.tone));
        let mix = ramp(l.mix, t.mix);

        // Shared by both sides. The wow's read point at the block's two
        // ends, ramped across it: at 6 Hz a block is 0.008 turns, so the
        // ramp is within 0.001 samples of the sines.
        let turns = |hz: f32| (hz / fs * 4_294_967_296.0) as u32;
        let to_f = |p: u32| (p >> 8) as f32 * (1.0 / 16_777_216.0);
        let swing = |w: u32, f: u32| 0.7 * sin_turns(to_f(w)) + 0.3 * sin_turns(to_f(f));
        let at0 = WOW_BASE + l.wow * WOW_SWING * swing(self.wow_phase, self.flutter_phase);
        self.wow_phase = self.wow_phase.wrapping_add(turns(0.5 * n));
        self.flutter_phase = self.flutter_phase.wrapping_add(turns(6.0 * n));
        let at1 = WOW_BASE + t.wow * WOW_SWING * swing(self.wow_phase, self.flutter_phase);
        let dly = ramp(at0, at1);
        // The engage fade, compare-free per sample: clamp(e, 0, 1) as
        // ½(|e| − |e − 1| + 1), then a smoothstep, so neither end has a
        // corner. Fully engaged, it is skipped.
        let step = if on { 1.0 } else { -1.0 } / ENGAGE as f32;
        let e0 = self.engage;
        let fading = !(on && e0 >= 1.0);
        let mut eng = [1.0f32; BLOCK_SIZE];
        if fading {
            for (i, w) in eng.iter_mut().enumerate() {
                let e = e0 + step * (i + 1) as f32;
                let e = 0.5 * (e.abs() - (e - 1.0).abs() + 1.0);
                *w = e * e * (3.0 - 2.0 * e);
            }
        }
        // Priming runs below 0; the fade holds at its ends.
        let e = e0 + step * n;
        self.engage = if on { e.min(1.0) } else { e.max(0.0) };
        self.running = on || self.engage > 0.0;

        let [b0, b1, a1] = self.coefs.pre;
        let [d0, d1, e1] = self.coefs.de;
        let [k0, k1, k2, m1, m2] = self.coefs.bump;
        for s in 0..2 {
            let side = &mut self.side[s];
            // The oversampler's working buffers, history first.
            let mut u = [0.0f32; U_HIST + BLOCK_SIZE];
            let mut even = [0.0f32; E_HIST + BLOCK_SIZE];
            let mut odd = [0.0f32; O_HIST + BLOCK_SIZE];
            u[..U_HIST].copy_from_slice(&side.u);
            even[..E_HIST].copy_from_slice(&side.even);
            odd[..O_HIST].copy_from_slice(&side.odd);
            // Three passes, each light enough to stay in registers.
            // 1. The line; wow (linearly interpolated) and pre-emphasis.
            let mut dry = [0.0f32; BLOCK_SIZE];
            let (mut x1, mut y1) = side.pre;
            let mut di = dly.0;
            for i in 0..BLOCK_SIZE {
                di += dly.1;
                side.pos = (side.pos + 1) & MASK;
                side.line[side.pos] = pair[2 * i + s];
                dry[i] = side.line[(side.pos + LINE - DRY_TAP) & MASK];
                let d = di as usize;
                let fr = di - d as f32;
                let r0 = side.line[(side.pos + LINE - d) & MASK];
                let r1 = side.line[(side.pos + LINE - d - 1) & MASK];
                let x = r0 + fr * (r1 - r0);
                y1 = b0 * x + b1 * x1 - a1 * y1;
                x1 = x;
                u[U_HIST + i] = y1;
            }
            side.pre = (x1, y1);
            // 2. Up, soft clip, down. The even 2× sample is the odd taps'
            // sum, the odd one the centre tap's copy; down, the centre tap
            // reads the odd stream, the odd taps the even stream.
            let mut sat = [0.0f32; BLOCK_SIZE];
            let mut gi = g.0;
            for i in 0..BLOCK_SIZE {
                gi += g.1;
                let m = U_HIST + i;
                let mut ve = 0.0;
                for (j, &h) in HB2.iter().enumerate() {
                    ve += h * (u[m - 3 + j] + u[m - 4 - j]);
                }
                even[E_HIST + i] = cubic(gi * ve);
                odd[O_HIST + i] = cubic(gi * u[m - 3]);
                let e = E_HIST + i;
                let mut y = 0.5 * odd[i];
                for (j, &h) in HB.iter().enumerate() {
                    y += h * (even[e - 3 + j] + even[e - 4 - j]);
                }
                sat[i] = y;
            }
            // 3. Level compensation, de-emphasis, head bump, roll-off, then
            // the parallel blend and the engage fade.
            let (mut dx, mut dy) = side.de;
            let [mut bx1, mut bx2, mut by1, mut by2] = side.bump;
            let mut lpy = side.lp;
            let (mut ci, mut lpi, mut mi) = (c.0, lp.0, mix.0);
            for i in 0..BLOCK_SIZE {
                (ci, lpi, mi) = (ci + c.1, lpi + lp.1, mi + mix.1);
                let x = sat[i] * ci;
                dy = d0 * x + d1 * dx - e1 * dy;
                dx = x;
                let w = k0 * dy + k1 * bx1 + k2 * bx2 - m1 * by1 - m2 * by2;
                (bx2, bx1, by2, by1) = (bx1, dy, by1, w);
                lpy += lpi * (w - lpy);
                let y = dry[i] + mi * (lpy - dry[i]);
                let out = &mut pair[2 * i + s];
                if fading {
                    *out += eng[i] * (y - *out);
                } else {
                    *out = y;
                }
            }
            (side.de, side.bump, side.lp) = ((dx, dy), [bx1, bx2, by1, by2], lpy);
            side.u.copy_from_slice(&u[BLOCK_SIZE..]);
            side.even.copy_from_slice(&even[BLOCK_SIZE..]);
            side.odd.copy_from_slice(&odd[BLOCK_SIZE..]);
        }
        self.last = t;
    }
}
