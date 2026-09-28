//! The per-voice modulators' pure parts (filter-routing spec § 1): the
//! slider laws, Envelope A and Envelope B. `Envelope` and `Lfo` wrap them.

pub mod env_a;
pub mod law;

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
