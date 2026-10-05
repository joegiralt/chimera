use crate::block::DiskCode;
use crate::dsp::modulator::pick;
use crate::hw::Cost;
use crate::params::FilterParams;

pub mod clean;
pub mod coef;
#[cfg(any(test, feature = "probe"))]
pub mod probe;

pub use clean::Clean;

/// The SVF's modes. The discriminants are the old `mode` byte (#111), so
/// every Sound keeps its mode (LP24 = 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FilterMode {
    Lp6 = 0,
    Lp12 = 1,
    Lp24 = 2,
    Bp12 = 3,
    Bp24 = 4,
    Hp24 = 5,
    Notch = 6,
    Phaser = 7,
}

impl DiskCode for FilterMode {
    fn disk_code(self) -> u8 {
        match self {
            FilterMode::Lp6 => 0,
            FilterMode::Lp12 => 1,
            FilterMode::Lp24 => 2,
            FilterMode::Bp12 => 3,
            FilterMode::Bp24 => 4,
            FilterMode::Hp24 => 5,
            FilterMode::Notch => 6,
            FilterMode::Phaser => 7,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            FilterMode::Lp6 => "LP6",
            FilterMode::Lp12 => "LP12",
            FilterMode::Lp24 => "LP24",
            FilterMode::Bp12 => "BP12",
            FilterMode::Bp24 => "BP24",
            FilterMode::Hp24 => "HP24",
            FilterMode::Notch => "NOTCH",
            FilterMode::Phaser => "PHASER",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(FilterMode::Lp6),
            1 => Some(FilterMode::Lp12),
            2 => Some(FilterMode::Lp24),
            3 => Some(FilterMode::Bp12),
            4 => Some(FilterMode::Bp24),
            5 => Some(FilterMode::Hp24),
            6 => Some(FilterMode::Notch),
            7 => Some(FilterMode::Phaser),
            _ => None,
        }
    }
}

impl FilterMode {
    /// In discriminant order.
    pub const ALL: [FilterMode; 8] = [
        FilterMode::Lp6,
        FilterMode::Lp12,
        FilterMode::Lp24,
        FilterMode::Bp12,
        FilterMode::Bp24,
        FilterMode::Hp24,
        FilterMode::Notch,
        FilterMode::Phaser,
    ];

    pub const fn label(self) -> &'static str {
        SVF_MODE_NAMES_BY_ID[self as usize]
    }
}

/// The SVF's modes as its MODE knob steps them, the default first (spec § 7).
pub const SVF_MODES: [FilterMode; 8] = [
    FilterMode::Lp24,
    FilterMode::Lp6,
    FilterMode::Lp12,
    FilterMode::Bp12,
    FilterMode::Bp24,
    FilterMode::Hp24,
    FilterMode::Notch,
    FilterMode::Phaser,
];

/// `SVF_MODES`' names, for MODE's spec.
pub static SVF_MODE_NAMES: [&str; 8] = [
    "LP24", "LP6", "LP12", "BP12", "BP24", "HP24", "NOTCH", "PHASER",
];

const SVF_MODE_NAMES_BY_ID: [&str; 8] = [
    "LP6", "LP12", "LP24", "BP12", "BP24", "HP24", "NOTCH", "PHASER",
];

/// PHASER over LP24: measured 2026-09-28, bench ROUTING SVF row, rev V at
/// 480 MHz (500 − 1 OP's 483). That row never takes `saturate`'s divide;
/// SVF HOT and LP24 HOT do, not yet read.
const SVF_PHASER: u32 = 17;
/// BP24 and HP24 over LP24: provisional, for BP24's extra multiply and the
/// loop's own layout, until the bench reads each mode.
const SVF_PAD: u32 = 2;

/// Which filter model (spec § Data model). A variant lands with its model
/// (#123–#127); until then only the SVF exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum FilterKind {
    #[default]
    Svf = 0,
}

impl DiskCode for FilterKind {
    fn disk_code(self) -> u8 {
        match self {
            FilterKind::Svf => 0,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            FilterKind::Svf => "SVF",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(FilterKind::Svf),
            _ => None,
        }
    }
}

/// KIND's names, as it steps the built kinds.
pub static KIND_NAMES: [&str; 1] = ["SVF"];

impl FilterKind {
    /// The kinds built, as KIND steps them.
    pub const BUILT: [FilterKind; 1] = [FilterKind::Svf];

    /// Never empty; `[0]` is the default (spec § 7).
    pub const fn modes(self) -> &'static [FilterMode] {
        match self {
            FilterKind::Svf => &SVF_MODES,
        }
    }

    /// NOTE → CUTOFF on a new Sound (spec § 2).
    pub const fn key_default(self) -> i8 {
        match self {
            FilterKind::Svf => 0,
        }
    }

    /// Cycles per sample over LP24 (spec § CPU), never below 0: the engine
    /// terms were solved from bench rows that already run the SVF at LP24.
    /// `recost` re-prices at every note-on, so a mode bills its own delta.
    pub const fn cost(self, mode: FilterMode) -> Cost {
        match self {
            FilterKind::Svf => Cost(match mode {
                FilterMode::Phaser => SVF_PHASER,
                FilterMode::Bp24 | FilterMode::Hp24 => SVF_PAD,
                // LP24 is the baseline; LP6, LP12, BP12 and NOTCH run less.
                FilterMode::Lp24
                | FilterMode::Lp6
                | FilterMode::Lp12
                | FilterMode::Bp12
                | FilterMode::Notch => 0,
            }),
        }
    }

    pub fn from_index(v: f32) -> Self {
        pick(&Self::BUILT, v)
    }
}

/// `p` with KIND `new`: MODE stays if `new` has it, else becomes `new`'s
/// default; nothing else moves (spec § 7).
pub fn kind_change(p: FilterParams, new: FilterKind) -> FilterParams {
    let mut q = p;
    q.set_kind_raw(new);
    if !new.modes().contains(&p.mode()) {
        q.set_mode(new.modes()[0]);
    }
    q
}

/// Octaves a full CUTOFF route moves (spec § 3).
pub const CUTOFF_OCTAVES: f32 = 10.0;

/// The cutoff a route sum `sum` gives from `base` (spec § 3): Σ = 0 is
/// `base` bit for bit; the filter clamps to 0.497·fs as well.
pub fn routed_cutoff(base: f32, sum: f32) -> f32 {
    if sum == 0.0 {
        return base;
    }
    crate::params::FILTER_SPECS[0].offset(base, sum)
}

/// Sample `i`'s coefficient on a ramp from `from` by `step` a sample: the
/// block's last sample lands on the new `g` (#53).
#[inline(always)]
pub fn g_at(from: f32, step: f32, i: usize) -> f32 {
    from + step * (i + 1) as f32
}
