//! The master compressor (FX diet spec § Master comp): the last stage, one
//! detector on the sum of all three DAC pairs, one gain on every pair.
//! Hard knee, smoothed in dB, the gain computed every `STEP` samples and
//! ramped between. MIX 0, or RATIO 1:1 with MAKEUP 0, is an exact bypass.

use chimera_hal::BLOCK_SIZE;
use core::mem::MaybeUninit;

use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::dsp::algo::math::{exp2, log2};

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
    /// Smoothed gain reduction, dB.
    gr: f32,
    /// The last applied multiplier, where the next ramp starts.
    e: f32,
    /// Threshold, makeup and mix at the end of the last block.
    thresh_db: f32,
    makeup_db: f32,
    mix: f32,
    /// 0 bypassed, 1 fully in; fades over `ENGAGE` samples.
    engage: f32,
}

crate::in_place::field_list!(MasterComp => MasterComp { gr, e, thresh_db, makeup_db, mix, engage });

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
            thresh_db: 0.0,
            makeup_db: 0.0,
            mix: 0.0,
            engage: 0.0,
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: six `f32`s, valid as zero bytes; zero is `new()`'s state.
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
        if self.is_running() { self.gr } else { 0.0 }
    }

    /// Compress every pair (interleaved L, R) by one linked gain, in place.
    pub fn process<const PAIRS: usize>(
        &mut self,
        out: &mut [[f32; 2 * BLOCK_SIZE]; PAIRS],
        p: &CompParams,
        sample_rate: u32,
    ) {
        const { assert!(PAIRS > 0) };
        let on = p.is_on();
        if !on && !self.is_running() {
            return;
        }
        let p = p.sanitised();
        if !self.is_running() {
            // From bypass: unity, nothing reduced, the controls at their
            // targets.
            (self.gr, self.e) = (0.0, 1.0);
            (self.thresh_db, self.makeup_db, self.mix) = (p.thresh_db(), p.makeup_db(), p.mix);
        }
        let fs = sample_rate as f32;
        let coef = |samples: f32, tau: f32| exp2(-LOG2_E * samples / (tau * fs));
        let (att, rel) = (
            coef(STEP as f32, p.attack_s()),
            coef(STEP as f32, p.release_s()),
        );
        let k = coef(BLOCK_SIZE as f32, SMOOTH_S);
        let smooth = |s: f32, t: f32| t + k * (s - t);
        let slope = 1.0 - 1.0 / p.ratio();
        let (t0, t1) = (self.thresh_db, smooth(self.thresh_db, p.thresh_db()));
        let (m0, m1) = (self.makeup_db, smooth(self.makeup_db, p.makeup_db()));
        let (x0, x1) = (self.mix, smooth(self.mix, p.mix));
        // Per chunk, as fractions of the block.
        let dw = STEP as f32 / BLOCK_SIZE as f32;
        let (dt, dm, dx) = ((t1 - t0) * dw, (m1 - m0) * dw, (x1 - x0) * dw);
        let ctl = Ramp {
            t: t0,
            m: m0,
            x: x0,
            dt,
            dm,
            dx,
            att,
            rel,
            slope,
        };
        // Fully in and staying in: the fade's arithmetic drops out.
        if on && self.engage == 1.0 {
            self.run::<PAIRS, false>(out, ctl, on);
        } else {
            self.run::<PAIRS, true>(out, ctl, on);
        }
        (self.thresh_db, self.makeup_db, self.mix) = (t1, m1, x1);
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
            // The chunk, read once: the detector and the gain both use it.
            let mut v: [[f32; 2 * STEP]; PAIRS] =
                core::array::from_fn(|k| core::array::from_fn(|j| out[k][2 * i0 + j]));
            let mut peak = 0.0f32;
            for j in 0..STEP {
                let (mut l, mut r) = (v[0][2 * j], v[0][2 * j + 1]);
                for pair in &v[1..] {
                    l += pair[2 * j];
                    r += pair[2 * j + 1];
                }
                peak = peak.max(l.abs()).max(r.abs());
            }
            (t, m, x) = (t + r.dt, m + r.dm, x + r.dx);
            let target = (DB_PER_OCT * log2(peak) - t).max(0.0) * r.slope;
            let a = if target > gr { r.att } else { r.rel };
            gr = target + a * (gr - target);
            let g = exp2((m - gr) * (1.0 / DB_PER_OCT));
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
                for pair in v.iter_mut() {
                    pair[2 * j] *= ej;
                    pair[2 * j + 1] *= ej;
                }
            }
            for (o, v) in out.iter_mut().zip(&v) {
                o[2 * i0..2 * (i0 + STEP)].copy_from_slice(v);
            }
            e0 = e1;
        }
        (self.gr, self.e, self.engage) = (gr, e0, engage);
    }
}

/// One block's controls: where each ramp starts and its step per chunk.
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
