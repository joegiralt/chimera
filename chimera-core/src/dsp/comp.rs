//! The master compressor (FX diet spec § Master comp): the last stage, one
//! detector on the sum of all three DAC pairs, one gain on every pair.
//! Hard knee, smoothed in the log domain (octaves of amplitude), the gain
//! computed every `STEP` samples and ramped between. MIX 0, or RATIO 1:1 with MAKEUP 0, is an exact bypass.

use chimera_hal::BLOCK_SIZE;
use core::mem::MaybeUninit;

use crate::block::{Block, ParamId, ParamSpec, ValFmt, apply_code, identity_code};
use crate::dsp::algo::math::exp2;

/// Samples per gain computation.
pub const STEP: usize = 4;
const _: () = assert!(BLOCK_SIZE.is_multiple_of(STEP));
/// Engaging or releasing crossfades from unity over this many samples.
pub const ENGAGE: u32 = 480;
/// Switched off, the fade to bypass waits until the gain is this close to
/// unity (−60 dB of change).
const SETTLED: f32 = 0.001;
/// THRESH, MAKEUP and MIX: one-pole, once per block.
const SMOOTH_S: f32 = 0.02;
/// 20·log10(2): dB per octave of amplitude.
const DB_PER_OCT: f32 = 6.020_6;
const LOG2_E: f32 = core::f32::consts::LOG2_E;
/// The detector's ceiling, +48 dBFS: one inf or runaway sample asks for a
/// bounded reduction and releases in a bounded time.
const PEAK_MAX: f32 = 256.0;

pub const RATIOS: [f32; 8] = [1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 10.0, 20.0];
static RATIO_NAMES: [&str; 8] = ["1:1", "1.5:1", "2:1", "3:1", "4:1", "6:1", "10:1", "20:1"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompParams {
    /// −40 dB at 0 to 0 dB at 1.
    pub thresh: f32,
    /// Index into `RATIOS`.
    pub ratio: u8,
    /// 0.1 ms at 0 to 100 ms at 1, logarithmic.
    pub attack: f32,
    /// 10 ms at 0 to 1 s at 1, logarithmic.
    pub release: f32,
    /// 0 to +24 dB.
    pub makeup: f32,
    /// Parallel blend: 0 dry (bypass), 1 all compressed.
    pub mix: f32,
}

impl Default for CompParams {
    fn default() -> Self {
        Self {
            thresh: 0.7,
            ratio: 0,
            attack: 0.5,
            release: 0.5,
            makeup: 0.0,
            mix: 1.0,
        }
    }
}

impl CompParams {
    pub const THRESH: ParamId = ParamId(0);
    pub const RATIO: ParamId = ParamId(1);
    pub const ATTACK: ParamId = ParamId(2);
    pub const RELEASE: ParamId = ParamId(3);
    pub const MAKEUP: ParamId = ParamId(4);
    pub const MIX: ParamId = ParamId(5);

    /// Off at MIX 0, or at 1:1 with no makeup: every pair passes untouched.
    pub fn is_on(&self) -> bool {
        self.mix >= 0.001 && (self.ratio > 0 || self.makeup > 0.0)
    }

    pub fn thresh_db(&self) -> f32 {
        -40.0 + 40.0 * self.thresh
    }

    pub fn ratio(&self) -> f32 {
        RATIOS[(self.ratio as usize).min(RATIOS.len() - 1)]
    }

    pub fn attack_s(&self) -> f32 {
        1.0e-4 * exp2(9.965_784 * self.attack) // 1000^a
    }

    pub fn release_s(&self) -> f32 {
        1.0e-2 * exp2(6.643_856 * self.release) // 100^r
    }

    pub fn makeup_db(&self) -> f32 {
        24.0 * self.makeup
    }

    /// NaN reads as the default; everything clamps to its range.
    pub fn sanitised(&self) -> Self {
        let d = Self::default();
        let f = |v: f32, def: f32| if v.is_nan() { def } else { v.clamp(0.0, 1.0) };
        Self {
            thresh: f(self.thresh, d.thresh),
            ratio: self.ratio.min(RATIOS.len() as u8 - 1),
            attack: f(self.attack, d.attack),
            release: f(self.release, d.release),
            makeup: f(self.makeup, d.makeup),
            mix: f(self.mix, d.mix),
        }
    }
}

/// The compressor runs outside `Voice`: nothing is modulatable.
pub static COMP_SPECS: [ParamSpec; 6] = [
    ParamSpec::continuous(0, "THRESH", ValFmt::Uni, 0.0, 1.0, 0.7, 1.0 / 128.0, false),
    ParamSpec::choice(1, "RATIO", ValFmt::Names(&RATIO_NAMES), 7.0, 0.0),
    ParamSpec::continuous(2, "ATK", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "REL", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "MAKEUP", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "MIX", ValFmt::Uni, 0.0, 1.0, 1.0, 1.0 / 128.0, false),
];

impl Block for CompParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &COMP_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::THRESH => self.thresh,
            Self::RATIO => self.ratio as f32,
            Self::ATTACK => self.attack,
            Self::RELEASE => self.release,
            Self::MAKEUP => self.makeup,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::THRESH => self.thresh = v,
            Self::RATIO => self.ratio = v as u8,
            Self::ATTACK => self.attack = v,
            Self::RELEASE => self.release = v,
            Self::MAKEUP => self.makeup = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }

    /// RATIO's code is its index into `RATIOS`, stored as is.
    fn enum_code(&self, id: ParamId) -> Option<u8> {
        (id == Self::RATIO).then_some(self.ratio)
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        id == Self::RATIO && apply_code(identity_code(&COMP_SPECS, id, code), |c| self.ratio = c)
    }
}

/// Gain reduction the static curve asks for at `level_db`: hard knee.
pub fn static_gr_db(level_db: f32, thresh_db: f32, ratio: f32) -> f32 {
    let over = level_db - thresh_db;
    if over > 0.0 {
        over * (1.0 - 1.0 / ratio)
    } else {
        0.0
    }
}

pub struct MasterComp {
    /// Smoothed gain reduction, octaves.
    gr: f32,
    /// The last applied multiplier, where the next ramp starts.
    e: f32,
    /// Threshold and makeup (octaves) and mix at the end of the last block.
    thresh: f32,
    makeup: f32,
    mix: f32,
    /// 0 bypassed, 1 fully in; fades over `ENGAGE` samples.
    engage: f32,
    /// The coefficients below were made for these ATTACK and RELEASE bits
    /// at this rate (0: not yet).
    key: [u32; 3],
    att: f32,
    rel: f32,
    /// THRESH, MAKEUP and MIX smoothing, per block.
    k: f32,
}

crate::in_place::field_list!(MasterComp => MasterComp { gr, e, thresh, makeup, mix, engage, key, att, rel, k });

impl Default for MasterComp {
    fn default() -> Self {
        Self::new()
    }
}

impl MasterComp {
    pub const fn new() -> Self {
        Self {
            gr: 0.0,
            e: 0.0,
            thresh: 0.0,
            makeup: 0.0,
            mix: 0.0,
            engage: 0.0,
            key: [0; 3],
            att: 0.0,
            rel: 0.0,
            k: 0.0,
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: `f32`s and `u32`s, valid as zero bytes; zero is `new()`'s
        // state.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }

    /// Engaged or fading: switched off with this false, it runs nothing.
    pub fn is_running(&self) -> bool {
        self.engage != 0.0
    }

    /// Gain reduction now, dB (0 while bypassed); the GR meter reads it.
    pub fn gr_db(&self) -> f32 {
        if self.is_running() {
            self.gr * DB_PER_OCT
        } else {
            0.0
        }
    }

    /// Compress every pair (interleaved L, R) by one linked gain, in place.
    pub fn process<const PAIRS: usize>(
        &mut self,
        out: &mut [[f32; 2 * BLOCK_SIZE]; PAIRS],
        p: &CompParams,
        sample_rate: u32,
    ) {
        const { assert!(PAIRS > 0) };
        let p = p.sanitised();
        let on = p.is_on();
        if !on && !self.is_running() {
            return;
        }
        let oct = 1.0 / DB_PER_OCT;
        if !self.is_running() {
            // From bypass: unity, nothing reduced, the controls at their
            // targets.
            (self.gr, self.e) = (0.0, 1.0);
            (self.thresh, self.makeup, self.mix) =
                (p.thresh_db() * oct, p.makeup_db() * oct, p.mix);
        }
        let key = [p.attack.to_bits(), p.release.to_bits(), sample_rate];
        if key[0] != self.key[0] || key[1] != self.key[1] || key[2] != self.key[2] {
            // Divide-free but for 1/fs: α = exp(−STEP/(τ·fs)), with 1/τ
            // folded into the inner `exp2` (τ = 0.1 ms·1000^a, 10 ms·100^r).
            let per_fs = 1.0 / sample_rate.max(1) as f32;
            if sample_rate != self.key[2] {
                self.k = exp2(-LOG2_E * BLOCK_SIZE as f32 / SMOOTH_S * per_fs);
            }
            let c = -LOG2_E * STEP as f32 * per_fs;
            self.att = exp2(c * 1.0e4 * exp2(-9.965_784 * p.attack));
            self.rel = exp2(c * 1.0e2 * exp2(-6.643_856 * p.release));
            self.key = key;
        }
        let k = self.k;
        let smooth = |s: f32, t: f32| t + k * (s - t);
        let slope = 1.0 - 1.0 / p.ratio();
        let (t0, t1) = (self.thresh, smooth(self.thresh, p.thresh_db() * oct));
        let (m0, m1) = (self.makeup, smooth(self.makeup, p.makeup_db() * oct));
        let (x0, x1) = (self.mix, smooth(self.mix, p.mix));
        // Per chunk, as fractions of the block.
        let dw = STEP as f32 / BLOCK_SIZE as f32;
        let ctl = Ramp {
            t: t0,
            m: m0,
            x: x0,
            dt: (t1 - t0) * dw,
            dm: (m1 - m0) * dw,
            dx: (x1 - x0) * dw,
            att: self.att,
            rel: self.rel,
            slope,
        };
        // Fully in and staying in: the fade's arithmetic drops out.
        if on && self.engage == 1.0 {
            self.run::<PAIRS, false>(out, ctl, on);
        } else {
            self.run::<PAIRS, true>(out, ctl, on);
        }
        (self.thresh, self.makeup, self.mix) = (t1, m1, x1);
    }

    #[inline(always)]
    fn run<const PAIRS: usize, const FADE: bool>(
        &mut self,
        out: &mut [[f32; 2 * BLOCK_SIZE]; PAIRS],
        r: Ramp,
        on: bool,
    ) {
        let de = STEP as f32 / ENGAGE as f32;
        let (mut t, mut m, mut x) = (r.t, r.m, r.x);
        let (mut gr, mut e0, mut engage) = (self.gr, self.e, self.engage);
        for c in 0..BLOCK_SIZE / STEP {
            let i0 = c * STEP;
            let mut peak = 0.0f32;
            for i in i0..i0 + STEP {
                let (mut l, mut r) = (out[0][2 * i], out[0][2 * i + 1]);
                for pair in &out[1..] {
                    l += pair[2 * i];
                    r += pair[2 * i + 1];
                }
                peak = peak.max(l.abs()).max(r.abs());
            }
            (t, m, x) = (t + r.dt, m + r.dm, x + r.dx);
            let target = (log2_level(peak.min(PEAK_MAX)) - t).max(0.0) * r.slope;
            let a = if target > gr { r.att } else { r.rel };
            gr = target + a * (gr - target);
            let g = exp2(m - gr);
            let e1 = if FADE {
                // Switched off, it fades to bypass only once the gain it
                // still applies is within 0.001 of unity.
                engage = if !on && x * (g - 1.0).abs() < SETTLED {
                    (engage - de).max(0.0)
                } else {
                    (engage + de).min(1.0)
                };
                // A smoothstep of the fade, so neither end has a corner.
                let w = engage * engage * (3.0 - 2.0 * engage);
                1.0 + w * x * (g - 1.0)
            } else {
                1.0 + x * (g - 1.0)
            };
            let step = (e1 - e0) * (1.0 / STEP as f32);
            for j in 0..STEP {
                let ej = e0 + step * (j + 1) as f32;
                for pair in out.iter_mut() {
                    pair[2 * (i0 + j)] *= ej;
                    pair[2 * (i0 + j) + 1] *= ej;
                }
            }
            e0 = e1;
        }
        (self.gr, self.e, self.engage) = (gr, e0, engage);
    }
}

/// The detector's level, octaves: `log2(x)` for `0 ≤ x < 2^128`, divide-free
/// (a quartic in the mantissa, error < 1.1e-4 octave, 0.0007 dB). Zero and
/// subnormals read as about −127.
#[inline(always)]
fn log2_level(x: f32) -> f32 {
    let bits = x.to_bits();
    let e = (bits >> 23) as i32 - 127;
    let t = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000) - 1.0;
    e as f32 + t * (1.439_014_5 + t * (-0.679_942_87 + t * (0.325_593_64 + t * -0.084_767_58)))
}

/// One block's controls, octaves: where each ramp starts and its step per
/// chunk.
#[derive(Clone, Copy)]
struct Ramp {
    t: f32,
    m: f32,
    x: f32,
    dt: f32,
    dm: f32,
    dx: f32,
    att: f32,
    rel: f32,
    slope: f32,
}
