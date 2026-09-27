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

/// Input history kept per side: the wet reads it through the wow, the dry
/// at the wet's whole delay. Each block works on history then block, in
/// one linear buffer.
const LINE_HIST: usize = 24;
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
// The wow reads up to one past its largest swing.
const _: () = assert!(DRY_TAP <= LINE_HIST && ((WOW_BASE + WOW_SWING) as usize) < LINE_HIST);
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
/// Every control: one-pole, once per block; within `SETTLED` of its target
/// it lands on it, so a held control stops ramping.
const SMOOTH_S: f32 = 0.02;
const SETTLED: f32 = 1e-6;

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

/// Input pad: at DRIVE 0 a −6 dBFS peak sits at 0.15 of the knee.
const PAD: f32 = 0.3;
/// DRIVE's span in octaves of gain, bent by d·(2 − d) so DRIVE ½ is ¾ of it.
const DRIVE_OCT: f32 = 1.9;

fn drive_oct(drive: f32) -> f32 {
    DRIVE_OCT * drive * (2.0 - drive)
}

/// Saturator input gain.
pub fn drive_gain(drive: f32) -> f32 {
    PAD * exp2(drive_oct(drive))
}

/// Level compensation: 1/gain, so the small-signal gain is unity at any
/// DRIVE: quiet material keeps its level, loud material is squashed.
pub fn drive_comp(drive: f32) -> f32 {
    (1.0 / PAD) * exp2(-drive_oct(drive))
}

/// The HF roll-off's corner: 12 kHz at TONE ½ and DRIVE 0, ±1.5 octaves
/// across TONE, down 1.5 octaves at full DRIVE.
pub fn rolloff_hz(drive: f32, tone: f32) -> f32 {
    12_000.0 * exp2(3.0 * (tone - 0.5) - 1.5 * drive)
}

/// The divide-free soft clip: the quintic v − ⅔v³ + ⅕v⁵ on v clamped to
/// ±1, slope (1 − v²)², so it meets its ceiling of 8/15 at |v| = 1 with
/// zero slope and curvature.
#[inline(always)]
pub fn soft_clip(v: f32) -> f32 {
    // t = 2·clamp(v, ±1) without a compare (no FPSCR stall on the M7);
    // then the quintic in t/2.
    let t = (v + 1.0).abs() - (v - 1.0).abs();
    let t2 = t * t;
    t * (0.5 + t2 * (t2 * (1.0 / 160.0) - 1.0 / 12.0))
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

type Lr = [f32; 2];

pub struct Tape {
    /// Per side, L and R together: the input's last `LINE_HIST` samples,
    /// oldest first.
    line: [Lr; LINE_HIST],
    /// Pre-emphasis: its last input and output.
    pre: (Lr, Lr),
    /// The oversampler's history: gained, pre-emphasised input, then the
    /// saturated even (filtered) and odd (centre-tap) 2× samples.
    u: [Lr; U_HIST],
    even: [Lr; E_HIST],
    odd: [Lr; O_HIST],
    /// De-emphasis: its last input and output.
    de: (Lr, Lr),
    /// Head bump: x[n−1], x[n−2], y[n−1], y[n−2].
    bump: [Lr; 4],
    lp: Lr,
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

crate::in_place::field_list!(Tape => Tape {
    line, pre, u, even, odd, de, bump, lp, coefs, fs, last, running, engage, wow_phase,
    flutter_phase
});

impl Default for Tape {
    fn default() -> Self {
        Self::new()
    }
}

/// One block's controls, each as its start and per-sample step: the
/// first sample takes one step, the last lands on the end.
#[derive(Clone, Copy)]
struct Ramps {
    /// The wow's read point, samples.
    dly: (f32, f32),
    /// The drive gain: the soft clip's input scale.
    g: (f32, f32),
    c: (f32, f32),
    lp: (f32, f32),
    mix: (f32, f32),
}

impl Tape {
    pub const fn new() -> Self {
        Self {
            line: [[0.0; 2]; LINE_HIST],
            pre: ([0.0; 2], [0.0; 2]),
            u: [[0.0; 2]; U_HIST],
            even: [[0.0; 2]; E_HIST],
            odd: [[0.0; 2]; O_HIST],
            de: ([0.0; 2], [0.0; 2]),
            bump: [[0.0; 2]; 4],
            lp: [0.0; 2],
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
        // SAFETY: every field is `f32`, `u32`, `bool` or arrays and tuples
        // of them, valid as zero bytes (`false` for the bool); zero is
        // exactly `new()`'s state.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }

    /// Engaged, fading or priming: MIX 0 with this false runs nothing.
    pub fn is_running(&self) -> bool {
        self.running
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
            // From bypass: nothing of the tape sounds yet, so it starts from
            // rest, as `new()` (only the transport runs on), with the
            // controls at their targets; the first block primes the line
            // and every filter at weight 0 before the fade in.
            *self = Self {
                coefs: self.coefs,
                fs: self.fs,
                last: t,
                running: true,
                wow_phase: self.wow_phase,
                flutter_phase: self.flutter_phase,
                ..Self::new()
            };
            self.engage = -(BLOCK_SIZE as f32) / ENGAGE as f32;
        }
        let fs = sample_rate as f32;
        // One-pole, once per block, then a linear ramp across it.
        let k = exp2(-LOG2_E * BLOCK_SIZE as f32 / (SMOOTH_S * fs));
        let sm = |from: f32, to: f32| {
            let v = to + k * (from - to);
            if (v - to).abs() < SETTLED { to } else { v }
        };
        let l = self.last;
        let t = TapeParams {
            drive: sm(l.drive, t.drive),
            tone: sm(l.tone, t.tone),
            wow: sm(l.wow, t.wow),
            mix: sm(l.mix, t.mix),
        };
        let a = |d: f32, tone: f32| 1.0 - exp2(-TWO_PI_LOG2E * rolloff_hz(d, tone) / fs);
        let n = BLOCK_SIZE as f32;
        let ramp = |x0: f32, x1: f32| (x0, (x1 - x0) / n);

        // The wow's read point at the block's two ends, ramped across it:
        // at 6 Hz a block is 0.008 turns, so the ramp is within 0.001
        // samples of the sines. Both sides share the transport.
        let turns = |hz: f32| (hz / fs * 4_294_967_296.0) as u32;
        let to_f = |p: u32| (p >> 8) as f32 * (1.0 / 16_777_216.0);
        let swing = |w: u32, f: u32| 0.7 * sin_turns(to_f(w)) + 0.3 * sin_turns(to_f(f));
        let at0 = WOW_BASE + l.wow * WOW_SWING * swing(self.wow_phase, self.flutter_phase);
        self.wow_phase = self.wow_phase.wrapping_add(turns(0.5 * n));
        self.flutter_phase = self.flutter_phase.wrapping_add(turns(6.0 * n));
        let at1 = WOW_BASE + t.wow * WOW_SWING * swing(self.wow_phase, self.flutter_phase);
        let r = Ramps {
            dly: ramp(at0, at1),
            g: ramp(drive_gain(l.drive), drive_gain(t.drive)),
            c: ramp(drive_comp(l.drive), drive_comp(t.drive)),
            lp: ramp(a(l.drive, l.tone), a(t.drive, t.tone)),
            mix: ramp(l.mix, t.mix),
        };
        // Held controls: no ramps. WOW 0: a fixed integer tap.
        let held = l == t;
        let wow = l.wow != 0.0 || t.wow != 0.0;
        let e0 = self.engage;
        let fading = !(on && e0 >= 1.0);

        let mut line = [[0.0f32; 2]; LINE_HIST + BLOCK_SIZE];
        let mut u = [[0.0f32; 2]; U_HIST + BLOCK_SIZE];
        line[..LINE_HIST].copy_from_slice(&self.line);
        u[..U_HIST].copy_from_slice(&self.u);
        match (wow, held) {
            (true, true) => self.pass_in::<true, false>(pair, &mut line, &mut u, &r),
            (true, false) => self.pass_in::<true, true>(pair, &mut line, &mut u, &r),
            (false, true) => self.pass_in::<false, false>(pair, &mut line, &mut u, &r),
            (false, false) => self.pass_in::<false, true>(pair, &mut line, &mut u, &r),
        }
        let sat = self.pass_os(&u);
        match (fading, held) {
            (true, true) => self.pass_out::<true, false>(pair, &line, &sat, &r, e0, on),
            (true, false) => self.pass_out::<true, true>(pair, &line, &sat, &r, e0, on),
            (false, true) => self.pass_out::<false, false>(pair, &line, &sat, &r, e0, on),
            (false, false) => self.pass_out::<false, true>(pair, &line, &sat, &r, e0, on),
        }
        self.line.copy_from_slice(&line[BLOCK_SIZE..]);
        self.u.copy_from_slice(&u[BLOCK_SIZE..]);

        // Priming runs below 0; the fade holds at its ends.
        let e = e0 + if on { n } else { -n } / ENGAGE as f32;
        self.engage = if on { e.min(1.0) } else { e.max(0.0) };
        self.running = on || self.engage > 0.0;
        self.last = t;
    }

    /// Pass 1: the input into the line; the wow's read, pre-emphasis and
    /// the drive gain into `u`. `WOW` off reads a fixed tap; `RAMP` off
    /// holds the gain.
    #[inline(always)]
    fn pass_in<const WOW: bool, const RAMP: bool>(
        &mut self,
        pair: &[f32; 2 * BLOCK_SIZE],
        line: &mut [Lr; LINE_HIST + BLOCK_SIZE],
        u: &mut [Lr; U_HIST + BLOCK_SIZE],
        r: &Ramps,
    ) {
        let [b0, b1, a1] = self.coefs.pre;
        let (mut px, mut py) = self.pre;
        // Held, g's step is 0: it starts where it stays.
        let (mut di, mut gi) = (r.dly.0, r.g.0);
        for i in 0..BLOCK_SIZE {
            let m = LINE_HIST + i;
            line[m] = [pair[2 * i], pair[2 * i + 1]];
            if RAMP {
                gi += r.g.1;
            }
            let x = if WOW {
                // The read point moves with the transport even when the
                // controls are held.
                di += r.dly.1;
                let d = di as usize;
                let fr = di - d as f32;
                let (r0, r1) = (line[m - d], line[m - d - 1]);
                [r0[0] + fr * (r1[0] - r0[0]), r0[1] + fr * (r1[1] - r0[1])]
            } else {
                line[m - WOW_BASE as usize]
            };
            for s in 0..2 {
                py[s] = b0 * x[s] + b1 * px[s] - a1 * py[s];
                u[U_HIST + i][s] = gi * py[s];
            }
            px = x;
        }
        self.pre = (px, py);
    }

    /// Pass 2: up, soft clip, down. The even 2× sample is the odd taps'
    /// sum, the odd one the centre tap's copy; down, the centre tap reads
    /// the odd stream, the odd taps the even stream.
    #[inline(always)]
    fn pass_os(&mut self, u: &[Lr; U_HIST + BLOCK_SIZE]) -> [Lr; BLOCK_SIZE] {
        let mut even = [[0.0f32; 2]; E_HIST + BLOCK_SIZE];
        let mut odd = [[0.0f32; 2]; O_HIST + BLOCK_SIZE];
        even[..E_HIST].copy_from_slice(&self.even);
        odd[..O_HIST].copy_from_slice(&self.odd);
        let mut sat = [[0.0f32; 2]; BLOCK_SIZE];
        for i in 0..BLOCK_SIZE {
            let (m, e) = (U_HIST + i, E_HIST + i);
            for s in 0..2 {
                let mut ve = 0.0;
                for (j, &h) in HB2.iter().enumerate() {
                    ve += h * (u[m - 3 + j][s] + u[m - 4 - j][s]);
                }
                even[e][s] = soft_clip(ve);
                odd[O_HIST + i][s] = soft_clip(u[m - 3][s]);
                let mut y = 0.5 * odd[i][s];
                for (j, &h) in HB.iter().enumerate() {
                    y += h * (even[e - 3 + j][s] + even[e - 4 - j][s]);
                }
                sat[i][s] = y;
            }
        }
        self.even.copy_from_slice(&even[BLOCK_SIZE..]);
        self.odd.copy_from_slice(&odd[BLOCK_SIZE..]);
        sat
    }

    /// Pass 3: level compensation, de-emphasis, head bump, roll-off, the
    /// parallel blend with the dry tap, and, `FADE` on, the engage fade.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn pass_out<const FADE: bool, const RAMP: bool>(
        &mut self,
        pair: &mut [f32; 2 * BLOCK_SIZE],
        line: &[Lr; LINE_HIST + BLOCK_SIZE],
        sat: &[Lr; BLOCK_SIZE],
        r: &Ramps,
        e0: f32,
        on: bool,
    ) {
        let [d0, d1, e1] = self.coefs.de;
        let [k0, k1, k2, m1, m2] = self.coefs.bump;
        let (mut dx, mut dy) = self.de;
        let [mut bx1, mut bx2, mut by1, mut by2] = self.bump;
        let mut lpy = self.lp;
        // Held, every step is 0: each starts where it stays.
        let (mut ci, mut lpi, mut mi) = (r.c.0, r.lp.0, r.mix.0);
        let step = if on { 1.0 } else { -1.0 } / ENGAGE as f32;
        for i in 0..BLOCK_SIZE {
            if RAMP {
                (ci, lpi, mi) = (ci + r.c.1, lpi + r.lp.1, mi + r.mix.1);
            }
            let dry = line[LINE_HIST + i - DRY_TAP];
            let mut y = [0.0f32; 2];
            for s in 0..2 {
                let x = sat[i][s] * ci;
                let v = d0 * x + d1 * dx[s] - e1 * dy[s];
                (dx[s], dy[s]) = (x, v);
                let w = k0 * v + k1 * bx1[s] + k2 * bx2[s] - m1 * by1[s] - m2 * by2[s];
                (bx2[s], bx1[s], by2[s], by1[s]) = (bx1[s], v, by1[s], w);
                lpy[s] += lpi * (w - lpy[s]);
                y[s] = dry[s] + mi * (lpy[s] - dry[s]);
            }
            let out = &mut pair[2 * i..2 * i + 2];
            if FADE {
                // A smoothstep of the linear ramp, so neither end has a
                // corner. At weight 0 (priming, or the release done) the
                // input is untouched, bit for bit; at 1 the tape is.
                let e = e0 + step * (i + 1) as f32;
                if e >= 1.0 {
                    out.copy_from_slice(&y);
                } else if e > 0.0 {
                    let w = e * e * (3.0 - 2.0 * e);
                    for s in 0..2 {
                        out[s] += w * (y[s] - out[s]);
                    }
                }
            } else {
                out.copy_from_slice(&y);
            }
        }
        (self.de, self.bump, self.lp) = ((dx, dy), [bx1, bx2, by1, by2], lpy);
    }
}
