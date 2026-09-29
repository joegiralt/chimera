//! The per-voice modulators' pure parts (filter-routing spec § 1): the
//! slider laws, Envelope A and Envelope B. `Envelope` and `Lfo` wrap them.

pub mod env_a;
pub mod func;
pub mod law;

use crate::block::{DiskCode, apply_code};

/// The choice at `v` among `all` (a `Block` write), clamped.
pub fn pick<T: Copy>(all: &[T], v: f32) -> T {
    all[(v.max(0.0) as usize).min(all.len() - 1)]
}

/// An ENV slot of the pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvSlot {
    Env1,
    Env2,
    Env3,
}

impl EnvSlot {
    pub const ALL: [EnvSlot; 3] = [EnvSlot::Env1, EnvSlot::Env2, EnvSlot::Env3];

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// An ENV slot's TYPE: the Cascadia's Envelope A or Envelope B.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EnvType {
    A = 0,
    B = 1,
}

impl EnvType {
    pub const ALL: [EnvType; 2] = [EnvType::A, EnvType::B];
}

impl DiskCode for EnvType {
    fn disk_code(self) -> u8 {
        match self {
            EnvType::A => 0,
            EnvType::B => 1,
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(EnvType::A),
            1 => Some(EnvType::B),
            _ => None,
        }
    }
}

/// Envelope A's SPEED: the manual's time ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EnvSpeed {
    Fast = 0,
    Med = 1,
    Slow = 2,
}

impl EnvSpeed {
    pub const ALL: [EnvSpeed; 3] = [EnvSpeed::Fast, EnvSpeed::Med, EnvSpeed::Slow];
}

impl DiskCode for EnvSpeed {
    fn disk_code(self) -> u8 {
        match self {
            EnvSpeed::Fast => 0,
            EnvSpeed::Med => 1,
            EnvSpeed::Slow => 2,
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(EnvSpeed::Fast),
            1 => Some(EnvSpeed::Med),
            2 => Some(EnvSpeed::Slow),
            _ => None,
        }
    }
}

/// Envelope A's HOLD POSITION.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HoldPos {
    Off = 0,
    Ahdsr = 1,
    GateExt = 2,
}

impl HoldPos {
    pub const ALL: [HoldPos; 3] = [HoldPos::Off, HoldPos::Ahdsr, HoldPos::GateExt];
}

impl DiskCode for HoldPos {
    fn disk_code(self) -> u8 {
        match self {
            HoldPos::Off => 0,
            HoldPos::Ahdsr => 1,
            HoldPos::GateExt => 2,
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(HoldPos::Off),
            1 => Some(HoldPos::Ahdsr),
            2 => Some(HoldPos::GateExt),
            _ => None,
        }
    }
}

/// Envelope B's MODE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FuncMode {
    Env = 0,
    Lfo = 1,
    Burst = 2,
}

impl FuncMode {
    pub const ALL: [FuncMode; 3] = [FuncMode::Env, FuncMode::Lfo, FuncMode::Burst];
}

impl DiskCode for FuncMode {
    fn disk_code(self) -> u8 {
        match self {
            FuncMode::Env => 0,
            FuncMode::Lfo => 1,
            FuncMode::Burst => 2,
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(FuncMode::Env),
            1 => Some(FuncMode::Lfo),
            2 => Some(FuncMode::Burst),
            _ => None,
        }
    }
}

/// ENV's and BURST's FORMs (the Cascadia's TYPE SELECT, renamed so it
/// doesn't clash with the slot's TYPE), as the knob steps them, default first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EnvForm {
    Ad = 0,
    Ahr = 1,
    Cycle = 2,
}

impl EnvForm {
    pub const ALL: [EnvForm; 3] = [EnvForm::Ad, EnvForm::Ahr, EnvForm::Cycle];
}

impl DiskCode for EnvForm {
    fn disk_code(self) -> u8 {
        match self {
            EnvForm::Ad => 0,
            EnvForm::Ahr => 1,
            EnvForm::Cycle => 2,
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(EnvForm::Ad),
            1 => Some(EnvForm::Ahr),
            2 => Some(EnvForm::Cycle),
            _ => None,
        }
    }
}

/// LFO's FORMs, default first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LfoForm {
    Free = 0,
    Sync = 1,
    Lfv = 2,
}

impl LfoForm {
    pub const ALL: [LfoForm; 3] = [LfoForm::Free, LfoForm::Sync, LfoForm::Lfv];
}

impl DiskCode for LfoForm {
    fn disk_code(self) -> u8 {
        match self {
            LfoForm::Free => 0,
            LfoForm::Sync => 1,
            LfoForm::Lfv => 2,
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(LfoForm::Free),
            1 => Some(LfoForm::Sync),
            2 => Some(LfoForm::Lfv),
            _ => None,
        }
    }
}

/// What Envelope B runs: a MODE and one of that MODE's FORMs, so a
/// mismatch can't be represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Func {
    Env(EnvForm),
    Lfo(LfoForm),
    Burst(EnvForm),
}

impl Func {
    pub const fn mode(self) -> FuncMode {
        match self {
            Func::Env(_) => FuncMode::Env,
            Func::Lfo(_) => FuncMode::Lfo,
            Func::Burst(_) => FuncMode::Burst,
        }
    }
}

/// Envelope B's switches and sliders; also a FUNC LFO's, whose MODE is
/// always LFO. Each MODE keeps its own FORM, so a MODE switched away and
/// back finds its FORM again. Sliders are positions, 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FuncParams {
    pub mode: FuncMode,
    pub env_form: EnvForm,
    pub lfo_form: LfoForm,
    pub burst_form: EnvForm,
    pub rise: f32,
    pub fall: f32,
    pub shape: f32,
}

impl FuncParams {
    /// ENV 3's default: ENV, AD, 10 ms rise, 300 ms fall, linear.
    pub const ENV: Self = Self {
        mode: FuncMode::Env,
        env_form: EnvForm::Ad,
        lfo_form: LfoForm::Free,
        burst_form: EnvForm::Ad,
        rise: 0.206,
        fall: 0.640,
        shape: 0.5,
    };
    /// A FUNC LFO's default: FREE at 1 Hz, PHASE 0, TILT centre (triangle).
    pub const LFO: Self = Self {
        mode: FuncMode::Lfo,
        rise: 0.309,
        fall: 0.0,
        ..Self::ENV
    };

    pub fn func(&self) -> Func {
        match self.mode {
            FuncMode::Env => Func::Env(self.env_form),
            FuncMode::Lfo => Func::Lfo(self.lfo_form),
            FuncMode::Burst => Func::Burst(self.burst_form),
        }
    }

    /// Run `f`: its MODE, with that MODE's FORM set.
    pub fn set_func(&mut self, f: Func) {
        match f {
            Func::Env(x) => (self.mode, self.env_form) = (FuncMode::Env, x),
            Func::Lfo(x) => (self.mode, self.lfo_form) = (FuncMode::Lfo, x),
            Func::Burst(x) => (self.mode, self.burst_form) = (FuncMode::Burst, x),
        }
    }

    /// FORM as a `Block` value: the index in its MODE's list.
    pub fn form_index(&self) -> f32 {
        match self.func() {
            Func::Env(x) | Func::Burst(x) => x as u8 as f32,
            Func::Lfo(x) => x as u8 as f32,
        }
    }

    /// FORM's disk code: the current MODE's FORM.
    pub fn form_code(&self) -> u8 {
        match self.func() {
            Func::Env(x) | Func::Burst(x) => x.disk_code(),
            Func::Lfo(x) => x.disk_code(),
        }
    }

    /// Set the current MODE's FORM from its code (so MODE goes first);
    /// `false`, nothing written, if that MODE has no such FORM.
    pub fn set_form_code(&mut self, c: u8) -> bool {
        match self.mode {
            FuncMode::Env => apply_code(EnvForm::from_disk_code(c), |x| self.env_form = x),
            FuncMode::Lfo => apply_code(LfoForm::from_disk_code(c), |x| self.lfo_form = x),
            FuncMode::Burst => apply_code(EnvForm::from_disk_code(c), |x| self.burst_form = x),
        }
    }

    pub fn set_form_index(&mut self, v: f32) {
        match self.mode {
            FuncMode::Env => self.env_form = pick(&EnvForm::ALL, v),
            FuncMode::Lfo => self.lfo_form = pick(&LfoForm::ALL, v),
            FuncMode::Burst => self.burst_form = pick(&EnvForm::ALL, v),
        }
    }
}

/// A TYPE, MODE or FORM change's leftover `d`, gliding linearly to 0 over
/// 256 samples (spec § 1). Shared by the ENV and LFO slots.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Glide {
    d: f32,
    left: u16,
}

impl Glide {
    pub const SAMPLES: u16 = 256;
    pub const NONE: Self = Self { d: 0.0, left: 0 };

    /// Glide `d` out; below 1e-6 there is nothing to glide.
    pub fn start(&mut self, d: f32) {
        *self = if d.abs() > 1e-6 {
            Self {
                d,
                left: Self::SAMPLES,
            }
        } else {
            Self::default()
        };
    }

    /// The leftover now.
    pub fn value(&self) -> f32 {
        self.at(0)
    }

    /// The leftover `n` samples on.
    pub fn at(&self, n: usize) -> f32 {
        let left = (self.left as usize).saturating_sub(n);
        if left == 0 {
            0.0
        } else {
            self.d * (left as f32 / Self::SAMPLES as f32)
        }
    }

    pub fn advance(&mut self, n: u16) {
        self.left = self.left.saturating_sub(n);
    }

    pub fn active(&self) -> bool {
        self.left > 0
    }
}

/// An LFO slot of the pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoSlot {
    Lfo1,
    Lfo2,
    Lfo3,
}

impl LfoSlot {
    pub const ALL: [LfoSlot; 3] = [LfoSlot::Lfo1, LfoSlot::Lfo2, LfoSlot::Lfo3];

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// An LFO slot's TYPE: today's LFO, or Envelope B locked to LFO mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LfoType {
    Classic = 0,
    Func = 1,
}

impl LfoType {
    pub const ALL: [LfoType; 2] = [LfoType::Classic, LfoType::Func];
}

impl DiskCode for LfoType {
    fn disk_code(self) -> u8 {
        match self {
            LfoType::Classic => 0,
            LfoType::Func => 1,
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(LfoType::Classic),
            1 => Some(LfoType::Func),
            _ => None,
        }
    }
}
