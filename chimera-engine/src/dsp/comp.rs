//! The master compressor (FX diet spec § Master comp): the last stage, one
//! detector on the sum of all three DAC pairs, one gain on every pair.
//! Hard knee, smoothed in the log domain (octaves of amplitude), the gain
//! computed every `STEP` samples and ramped between. MIX 0, or RATIO 1:1 with MAKEUP 0, is an exact bypass.

use crate::dsp::clampf;
use chimera_hal::BLOCK_SIZE;
use core::mem::MaybeUninit;

use crate::block::{Block, Change, Motion, ParamId, ParamSpec, Smoothing, ValFmt, apply_code};
use crate::comp_key::{CompKey, KEY_NAMES, KeyState};
use crate::dsp::algo::math::exp2;
use crate::part_eq::smart::{Amount, SWITCH_NAMES, SmartEq, Switch};

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
const LOG2_E: f32 = core::f32::consts::LOG2_E;
/// The detector's ceiling, +48 dBFS: one inf or runaway sample asks for a
/// bounded reduction and releases in a bounded time.
const PEAK_MAX: f32 = 256.0;

/// Each ratio with its frozen disk ident (ADR 0045), side by side so the
/// stored index can't be reordered away from what it selects. Append-only.
const RATIO_TABLE: [(f32, &str); 9] = [
    (1.0, "1:1"),
    (1.5, "1.5:1"),
    (2.0, "2:1"),
    (3.0, "3:1"),
    (4.0, "4:1"),
    (6.0, "6:1"),
    (10.0, "10:1"),
    (20.0, "20:1"),
    // A limiter's slope (ADR 0303): everything over THRESH comes off.
    (f32::INFINITY, "MAX"),
];

/// RATIO's choices in all.
pub const RATIO_COUNT: usize = RATIO_TABLE.len();

/// RATIO's names on the box, by index: the table's idents.
static RATIO_NAMES: [&str; RATIO_COUNT] = {
    let mut n = [""; RATIO_COUNT];
    let mut i = 0;
    while i < RATIO_COUNT {
        n[i] = RATIO_TABLE[i].1;
        i += 1;
    }
    n
};

/// The ratios alone, by index: what the audio reads.
const RATIOS: [f32; RATIO_COUNT] = {
    let mut r = [0.0; RATIO_COUNT];
    let mut i = 0;
    while i < RATIO_COUNT {
        r[i] = RATIO_TABLE[i].0;
        i += 1;
    }
    r
};

crate::project::bounded_index!(
    /// RATIO: an index into `RATIO_TABLE`, the stored code itself. 0 is
    /// 1:1, off.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    Ratio < RATIO_COUNT
);

impl Ratio {
    /// 1:1: the compressor off.
    pub const UNITY: Ratio = Ratio::ALL[0];
    /// The last: everything over THRESH comes off.
    pub const MAX: Ratio = Ratio::ALL[RATIO_COUNT - 1];

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub const fn code(self) -> u8 {
        self.0
    }

    /// A knob's or a stored value, held to the table; NaN and below are
    /// 1:1.
    pub fn from_f32(v: f32) -> Ratio {
        match Ratio::new(v as u8) {
            Some(r) => r,
            None => Ratio::MAX,
        }
    }

    /// The ratio itself: 1 ..= 20, then infinity.
    pub const fn ratio(self) -> f32 {
        RATIOS[self.index()]
    }

    /// The share of what is over THRESH that comes off: 0 at 1:1, 1 at
    /// MAX.
    #[inline(always)]
    pub fn slope(self) -> f32 {
        1.0 - 1.0 / self.ratio()
    }

    /// Its frozen disk ident, which is its name on the box.
    pub fn ident(self) -> &'static str {
        RATIO_NAMES[self.index()]
    }
}

const _: () = assert!(Ratio::UNITY.ratio() == 1.0 && Ratio::MAX.ratio() == f32::INFINITY);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompParams {
    /// −40 dB at 0 to 0 dB at 1.
    pub thresh: f32,
    pub ratio: Ratio,
    /// 0.1 ms at 0 to 100 ms at 1, logarithmic.
    pub attack: f32,
    /// 10 ms at 0 to 1 s at 1, logarithmic.
    pub release: f32,
    /// 0 to +24 dB.
    pub makeup: f32,
    /// Parallel blend: 0 dry (bypass), 1 all compressed.
    pub mix: f32,
    /// Master VOL (MST > LVL), after the limiter: gain = vol², 1 unity.
    pub vol: f32,
    /// Master PAN, -1..1: a balance, the far side cos-tapered.
    pub pan: f32,
    /// SMART EQ and AMOUNT (ADR 0296), one byte in the struct's padding:
    /// the master's switch for every Part's SMART EQ cuts, riding MST's
    /// block.
    pub smart: SmartEq,
    /// KEY (ADR 0303): what the detector listens to, in the padding's
    /// other two bytes.
    pub key: CompKey,
}

impl CompParams {
    pub const DEFAULT: Self = Self {
        thresh: 0.7,
        ratio: Ratio::UNITY,
        attack: 0.5,
        release: 0.5,
        makeup: 0.0,
        mix: 1.0,
        vol: 1.0,
        pan: 0.0,
        smart: SmartEq::OFF,
        key: CompKey::Full,
    };
}

impl Default for CompParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl CompParams {
    pub const THRESH: ParamId = ParamId(0);
    pub const RATIO: ParamId = ParamId(1);
    pub const ATTACK: ParamId = ParamId(2);
    pub const RELEASE: ParamId = ParamId(3);
    pub const MAKEUP: ParamId = ParamId(4);
    pub const MIX: ParamId = ParamId(5);
    pub const VOL: ParamId = ParamId(6);
    pub const PAN: ParamId = ParamId(7);
    pub const SMART_EQ: ParamId = ParamId(8);
    pub const AMOUNT: ParamId = ParamId(9);
    pub const KEY: ParamId = ParamId(10);

    /// Off at MIX 0, or at 1:1 with no makeup: every pair passes untouched.
    pub fn is_on(&self) -> bool {
        self.mix >= 0.001 && (self.ratio > Ratio::UNITY || self.makeup > 0.0)
    }

    pub fn thresh_db(&self) -> f32 {
        -40.0 + 40.0 * self.thresh
    }

    pub fn ratio(&self) -> f32 {
        self.ratio.ratio()
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
        let f = |v: f32, def: f32| if v.is_nan() { def } else { clampf(v, 0.0, 1.0) };
        Self {
            thresh: f(self.thresh, d.thresh),
            ratio: self.ratio,
            attack: f(self.attack, d.attack),
            release: f(self.release, d.release),
            makeup: f(self.makeup, d.makeup),
            mix: f(self.mix, d.mix),
            vol: f(self.vol, d.vol),
            pan: if self.pan.is_nan() {
                0.0
            } else {
                clampf(self.pan, -1.0, 1.0)
            },
            smart: self.smart,
            key: self.key,
        }
    }
}

const _: () = assert!(RATIO_COUNT == COMP_SPECS[1].max as usize + 1);

/// The compressor runs outside `Voice`: nothing is modulatable.
pub static COMP_SPECS: [ParamSpec; 11] = [
    ParamSpec::continuous(
        0,
        "THRESH",
        ValFmt::Uni,
        0.0,
        1.0,
        0.7,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("THRESH"),
    ParamSpec::choice(
        1,
        "RATIO",
        ValFmt::Names(&RATIO_NAMES),
        (RATIO_COUNT - 1) as f32,
        0.0,
        Change::Snap,
    )
    .ident("RATIO"),
    ParamSpec::continuous(
        2,
        "ATK",
        ValFmt::Uni,
        0.0,
        1.0,
        0.5,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("ATK"),
    ParamSpec::continuous(
        3,
        "REL",
        ValFmt::Uni,
        0.0,
        1.0,
        0.5,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("REL"),
    ParamSpec::continuous(
        4,
        "MAKEUP",
        ValFmt::Uni,
        0.0,
        1.0,
        0.0,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("MAKEUP")
    .glyph(crate::glyph::FocusGlyph::LevelBar),
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
    .glyph(crate::glyph::FocusGlyph::LevelBar),
    ParamSpec::continuous(
        6,
        "VOL",
        ValFmt::Uni,
        0.0,
        1.0,
        1.0,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("VOL")
    .glyph(crate::glyph::FocusGlyph::LevelBar),
    ParamSpec::continuous(
        7,
        "PAN",
        ValFmt::Pan,
        -1.0,
        1.0,
        0.0,
        2.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("PAN"),
    // SMART EQ (ADR 0296): the switch and how deep it cuts. AMOUNT moves a
    // target the cuts follow over seconds: the DSP smooths it.
    ParamSpec::choice(
        8,
        "SMART EQ",
        ValFmt::Names(&SWITCH_NAMES),
        1.0,
        0.0,
        Change::Snap,
    )
    .ident("SMART_EQ")
    .glyph(crate::glyph::FocusGlyph::Switch),
    ParamSpec::stepped(
        9,
        "AMOUNT",
        ValFmt::Int(Amount::MAX),
        0.0,
        Amount::MAX as f32,
        Amount::DEFAULT.code() as f32,
        false,
        Motion::Continuous(Smoothing::Own),
    )
    .ident("AMOUNT")
    .glyph(crate::glyph::FocusGlyph::LevelBar),
    // KEY (ADR 0303): the detector's source. The reduction follows a new
    // one at ATK and REL, so the choice itself snaps.
    ParamSpec::choice(
        10,
        "KEY",
        ValFmt::Names(&KEY_NAMES),
        (KEY_NAMES.len() - 1) as f32,
        0.0,
        Change::Snap,
    )
    .ident("KEY")
    .glyph(crate::glyph::FocusGlyph::None),
];

impl Block for CompParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &COMP_SPECS
    }

    #[inline(never)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::THRESH => self.thresh,
            Self::RATIO => f32::from(self.ratio.code()),
            Self::ATTACK => self.attack,
            Self::RELEASE => self.release,
            Self::MAKEUP => self.makeup,
            Self::MIX => self.mix,
            Self::VOL => self.vol,
            Self::PAN => self.pan,
            Self::SMART_EQ => f32::from(self.smart.switch().code()),
            Self::AMOUNT => f32::from(self.smart.amount().code()),
            Self::KEY => f32::from(self.key.code()),
            _ => 0.0,
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::THRESH => self.thresh = v,
            Self::RATIO => self.ratio = Ratio::from_f32(v),
            Self::ATTACK => self.attack = v,
            Self::RELEASE => self.release = v,
            Self::MAKEUP => self.makeup = v,
            Self::MIX => self.mix = v,
            Self::VOL => self.vol = v,
            Self::PAN => self.pan = v,
            Self::SMART_EQ => self.smart = self.smart.with_switch(Switch::from_f32(v)),
            Self::AMOUNT => self.smart = self.smart.with_amount(Amount::from_f32(v)),
            Self::KEY => self.key = CompKey::from_f32(v),
            _ => {}
        }
    }

    /// RATIO's code is its index (`Ratio`), stored as is.
    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::RATIO => Some(self.ratio.code()),
            Self::SMART_EQ => Some(self.smart.switch().code()),
            Self::KEY => Some(self.key.code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::RATIO => Some(self.ratio.ident()),
            Self::SMART_EQ => Some(self.smart.switch().ident()),
            Self::KEY => Some(self.key.ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::RATIO => apply_code(Ratio::new(code), |r| self.ratio = r),
            Self::SMART_EQ => apply_code(Switch::from_code(code), |s| {
                self.smart = self.smart.with_switch(s)
            }),
            Self::KEY => apply_code(CompKey::from_code(code), |k| self.key = k),
            _ => false,
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
    /// KEY's detector filters and pass (ADR 0303): bank 2's to run, at
    /// rest under FULL.
    key_state: KeyState,
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
    /// ATTACK and RELEASE, eased as THRESH (a step makes a lagging
    /// reduction catch up at once).
    attack: f32,
    release: f32,
    /// The coefficients below were made for these eased ATTACK and RELEASE
    /// bits at this rate (0: not yet).
    key: [u32; 3],
    att: f32,
    rel: f32,
    /// THRESH, MAKEUP and MIX smoothing, per block.
    k: f32,
}

crate::in_place::field_list!(MasterComp => MasterComp { key_state, gr, e, thresh, makeup, mix, engage, attack, release, key, att, rel, k });

impl Default for MasterComp {
    fn default() -> Self {
        Self::new()
    }
}

impl MasterComp {
    pub const fn new() -> Self {
        Self {
            key_state: KeyState::REST,
            gr: 0.0,
            e: 0.0,
            thresh: 0.0,
            makeup: 0.0,
            mix: 0.0,
            engage: 0.0,
            attack: 0.0,
            release: 0.0,
            key: [0; 3],
            att: 0.0,
            rel: 0.0,
            k: 0.0,
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: `f32`s and `u32`s, valid as zero bytes, and `KeyState`,
        // whose zero bytes are its `REST` (its own test); zero is `new()`'s
        // state.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }

    /// KEY's state: bank 1 asks only whether it rests.
    #[inline(always)]
    pub(crate) fn key_state(&self) -> &KeyState {
        &self.key_state
    }

    /// KEY's state after bank 2's block.
    #[inline(always)]
    pub(crate) fn set_key_state(&mut self, st: KeyState) {
        self.key_state = st;
    }

    /// KEY at rest: the door closed, FULL plays.
    #[inline(always)]
    pub(crate) fn rest_key(&mut self) {
        self.key_state = KeyState::REST;
    }

    /// Engaged or fading: switched off with this false, it runs nothing.
    pub fn is_running(&self) -> bool {
        self.engage != 0.0
    }

    /// Gain reduction now, dB (0 while bypassed); the GR meter reads it.
    pub fn gr_db(&self) -> f32 {
        if self.is_running() {
            self.gr * crate::dsp::DB_PER_OCTAVE
        } else {
            0.0
        }
    }

    /// Compress every pair (interleaved L, R) by one linked gain, in place.
    /// Whether it ran: bypassed, nothing is touched.
    pub fn process<const PAIRS: usize>(
        &mut self,
        out: &mut [[f32; 2 * BLOCK_SIZE]; PAIRS],
        p: &CompParams,
        sample_rate: u32,
    ) -> bool {
        self.process_keyed(out, p, sample_rate, &mut WholeMix)
    }

    /// `process`, the detector's level and what follows each chunk's gain
    /// taken from `listen` (ADR 0303). `WholeMix` is `process` itself; bank
    /// 2's `comp_key::kern` brings the others, and this inlines into it.
    #[inline(always)]
    pub(crate) fn process_keyed<const PAIRS: usize, K: Listen<PAIRS>>(
        &mut self,
        out: &mut [[f32; 2 * BLOCK_SIZE]; PAIRS],
        p: &CompParams,
        sample_rate: u32,
        listen: &mut K,
    ) -> bool {
        const { assert!(PAIRS > 0) };
        let p = p.sanitised();
        let on = p.is_on();
        if !on && !self.is_running() {
            return false;
        }
        let oct = 1.0 / crate::dsp::DB_PER_OCTAVE;
        if !self.is_running() {
            // From bypass: unity, nothing reduced, the controls at their
            // targets.
            (self.gr, self.e) = (0.0, 1.0);
            (self.thresh, self.makeup, self.mix) =
                (p.thresh_db() * oct, p.makeup_db() * oct, p.mix);
            (self.attack, self.release) = (p.attack, p.release);
        }
        let per_fs = 1.0 / sample_rate.max(1) as f32;
        if sample_rate != self.key[2] {
            self.k = exp2(-LOG2_E * BLOCK_SIZE as f32 / SMOOTH_S * per_fs);
        }
        let k = self.k;
        let smooth = |s: f32, t: f32| t + k * (s - t);
        (self.attack, self.release) = (
            smooth(self.attack, p.attack),
            smooth(self.release, p.release),
        );
        let key = [self.attack.to_bits(), self.release.to_bits(), sample_rate];
        if key[0] != self.key[0] || key[1] != self.key[1] || key[2] != self.key[2] {
            // Divide-free but for 1/fs: α = exp(−STEP/(τ·fs)), with 1/τ
            // folded into the inner `exp2` (τ = 0.1 ms·1000^a, 10 ms·100^r).
            let c = -LOG2_E * STEP as f32 * per_fs;
            self.att = exp2(c * 1.0e4 * exp2(-9.965_784 * self.attack));
            self.rel = exp2(c * 1.0e2 * exp2(-6.643_856 * self.release));
            self.key = key;
        }
        let slope = p.ratio.slope();
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
        // Fully in and staying in: the fade's arithmetic drops out, where
        // the key keeps a loop for it.
        if K::SETTLED_LOOP && on && self.engage == 1.0 {
            self.run::<PAIRS, false, K>(out, ctl, on, listen);
        } else {
            self.run::<PAIRS, true, K>(out, ctl, on, listen);
        }
        (self.thresh, self.makeup, self.mix) = (t1, m1, x1);
        true
    }

    #[inline(always)]
    fn run<const PAIRS: usize, const FADE: bool, K: Listen<PAIRS>>(
        &mut self,
        out: &mut [[f32; 2 * BLOCK_SIZE]; PAIRS],
        r: Ramp,
        on: bool,
        listen: &mut K,
    ) {
        let de = STEP as f32 / ENGAGE as f32;
        let (mut t, mut m, mut x) = (r.t, r.m, r.x);
        let (mut gr, mut e0, mut engage) = (self.gr, self.e, self.engage);
        for c in 0..BLOCK_SIZE / STEP {
            let i0 = c * STEP;
            let peak = listen.peak(out, i0);
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
            let gains = ramp_gains(e0, e1);
            scale(out, i0, &gains);
            listen.pass(out, i0, &gains);
            e0 = e1;
        }
        (self.gr, self.e, self.engage) = (gr, e0, engage);
    }
}

/// A chunk's gains, ramped linearly from `e0` (the last chunk's end) to
/// `e1`, reached on its last sample.
#[inline(always)]
pub(crate) fn ramp_gains(e0: f32, e1: f32) -> [f32; STEP] {
    let step = (e1 - e0) * (1.0 / STEP as f32);
    core::array::from_fn(|j| e0 + step * (j + 1) as f32)
}

/// What the loop asks of the compressor's KEY, a chunk at a time (ADR
/// 0303).
pub(crate) trait Listen<const PAIRS: usize> {
    /// Whether the loop is built twice, once without the engage fade's
    /// arithmetic for the blocks fully in. Without it the one loop plays
    /// them all, to the same bits (a fade of 1 is a multiply by 1.0).
    const SETTLED_LOOP: bool;
    /// The detector's peak over chunk `i0..i0 + STEP` of `out`, not yet
    /// scaled.
    fn peak(&mut self, out: &[[f32; 2 * BLOCK_SIZE]; PAIRS], i0: usize) -> f32;
    /// The chunk scaled by `gains`: what the key puts back.
    fn pass(&mut self, out: &mut [[f32; 2 * BLOCK_SIZE]; PAIRS], i0: usize, gains: &[f32; STEP]);
}

/// FULL: the sum of every pair, and nothing put back.
pub(crate) struct WholeMix;

impl<const PAIRS: usize> Listen<PAIRS> for WholeMix {
    const SETTLED_LOOP: bool = true;

    #[inline(always)]
    fn peak(&mut self, out: &[[f32; 2 * BLOCK_SIZE]; PAIRS], i0: usize) -> f32 {
        let mut peak = 0.0f32;
        for i in i0..i0 + STEP {
            let (l, r) = summed(out, i);
            peak = peak.max(l.abs()).max(r.abs());
        }
        peak
    }

    #[inline(always)]
    fn pass(&mut self, _: &mut [[f32; 2 * BLOCK_SIZE]; PAIRS], _: usize, _: &[f32; STEP]) {}
}

/// Sample `i` of every pair summed: the mix as the detector hears it.
#[inline(always)]
pub(crate) fn summed<const PAIRS: usize>(
    out: &[[f32; 2 * BLOCK_SIZE]; PAIRS],
    i: usize,
) -> (f32, f32) {
    let (mut l, mut r) = (out[0][2 * i], out[0][2 * i + 1]);
    for pair in &out[1..] {
        l += pair[2 * i];
        r += pair[2 * i + 1];
    }
    (l, r)
}

/// Scale chunk `i0..i0 + STEP` of every pair by `gains`.
#[inline(always)]
fn scale<const PAIRS: usize>(
    out: &mut [[f32; 2 * BLOCK_SIZE]; PAIRS],
    i0: usize,
    gains: &[f32; STEP],
) {
    for (j, &ej) in gains.iter().enumerate() {
        for pair in out.iter_mut() {
            pair[2 * (i0 + j)] *= ej;
            pair[2 * (i0 + j) + 1] *= ej;
        }
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
