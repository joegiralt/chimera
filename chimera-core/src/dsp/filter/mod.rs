use crate::block::DiskCode;
use crate::dsp::modulator::pick;
use crate::hw::Cost;
use crate::params::FilterParams;

pub mod clean;
pub mod coef;
pub mod dirty;
pub mod k2040;
pub mod level;
#[cfg(any(test, feature = "probe"))]
pub mod probe;

pub use clean::Clean;
pub use dirty::Dirty;
pub use k2040::{K2040, K2040Voice, ResElement};
pub use level::{
    FilterMods, LevelRamp, LevelStage, clip_amount, clipped, level_from_drive, level_gain,
    translate_drive,
};

/// The filter modes. The discriminants are the old `mode` byte (#111), so
/// every Sound keeps its mode (LP24 = 2); new ones append (ADR 0009).
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
    /// DIRTY's HP2; in no CLEAN list. Code 9, the routing spec's.
    Hp12 = 9,
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
            FilterMode::Hp12 => 9,
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
            FilterMode::Hp12 => "HP12",
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
            9 => Some(FilterMode::Hp12),
            _ => None,
        }
    }
}

impl FilterMode {
    /// Every mode, in discriminant order.
    pub const ALL: [FilterMode; 9] = [
        FilterMode::Lp6,
        FilterMode::Lp12,
        FilterMode::Lp24,
        FilterMode::Bp12,
        FilterMode::Bp24,
        FilterMode::Hp24,
        FilterMode::Notch,
        FilterMode::Phaser,
        FilterMode::Hp12,
    ];
}

/// CLEAN's modes as its MODE knob steps them (spec § 1). Its index is the
/// discriminant, which CLEAN's `run::<M>` reads back.
pub(crate) const CLEAN_MODES: [FilterMode; 8] = [
    FilterMode::Lp6,
    FilterMode::Lp12,
    FilterMode::Lp24,
    FilterMode::Bp12,
    FilterMode::Bp24,
    FilterMode::Hp24,
    FilterMode::Notch,
    FilterMode::Phaser,
];

/// `CLEAN_MODES`' screen names: what each does, the Cascadia's words. The
/// disk idents stay the old ones.
const CLEAN_LABELS: [&str; 8] = ["LP1", "LP2", "LP4", "BP2", "BP4", "HP4", "NOTCH1", "NOTCH2"];

/// PHASER over LP24: measured 2026-09-28, bench ROUTING SVF row, rev V at
/// 480 MHz (500 − 1 OP's 483). That row never takes `saturate`'s divide;
/// SVF HOT and LP24 HOT do, not yet read.
const SVF_PHASER: u32 = 17;
/// BP24 and HP24 over LP24: provisional, for BP24's extra multiply and the
/// loop's own layout, until the bench reads each mode.
const SVF_PAD: u32 = 2;

/// The 2040's one mode.
const K2040_MODES: [FilterMode; 1] = [FilterMode::Lp24];
const K2040_LABELS: [&str; 1] = ["LP4"];

/// DIRTY's modes, in knob order (spec § 1): LP2 the default.
const DIRTY_MODES: [FilterMode; 4] = [
    FilterMode::Lp12,
    FilterMode::Bp12,
    FilterMode::Hp12,
    FilterMode::Notch,
];
/// DIRTY's NOTCH1 notches at FREQ, Chamberlin's own (CLEAN's at 2 × FREQ).
const DIRTY_LABELS: [&str; 4] = ["LP2", "BP2", "HP2", "NOTCH1"];

/// Which filter model, its TYPE (spec § 1). The discriminants are the disk
/// codes: append-only, gaps kept for the kinds that never landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum FilterKind {
    /// CLEAN, the Cascadia-style SVF; its disk ident stays "SVF".
    #[default]
    Svf = 0,
    /// 2040, the SSM2040 four-pole (spec § 3).
    K2040 = 4,
    /// DIRTY, the Chamberlin SVF at ×2 (spec § 4).
    Dirty = 6,
}

impl DiskCode for FilterKind {
    fn disk_code(self) -> u8 {
        match self {
            FilterKind::Svf => 0,
            FilterKind::K2040 => 4,
            FilterKind::Dirty => 6,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            FilterKind::Svf => "SVF",
            FilterKind::K2040 => "2040",
            FilterKind::Dirty => "DIRTY",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(FilterKind::Svf),
            4 => Some(FilterKind::K2040),
            6 => Some(FilterKind::Dirty),
            _ => None,
        }
    }
}

/// TYPE's names, as it steps the built kinds.
pub static KIND_NAMES: [&str; 3] = ["CLEAN", "2040", "DIRTY"];

const _: () = assert!(KIND_NAMES.len() == FilterKind::BUILT.len());

impl FilterKind {
    /// The kinds built, as TYPE steps them.
    pub const BUILT: [FilterKind; 3] = [FilterKind::Svf, FilterKind::K2040, FilterKind::Dirty];

    /// Never empty, in knob order (spec § 1).
    pub const fn modes(self) -> &'static [FilterMode] {
        match self {
            FilterKind::Svf => &CLEAN_MODES,
            FilterKind::K2040 => &K2040_MODES,
            FilterKind::Dirty => &DIRTY_MODES,
        }
    }

    /// Where a new Sound, a KIND change that loses MODE and an unknown code
    /// land; always in `modes()`.
    pub const fn default_mode(self) -> FilterMode {
        match self {
            FilterKind::Svf | FilterKind::K2040 => FilterMode::Lp24,
            FilterKind::Dirty => FilterMode::Lp12,
        }
    }

    /// `modes()`' screen names, in their order.
    pub const fn mode_labels(self) -> &'static [&'static str] {
        match self {
            FilterKind::Svf => &CLEAN_LABELS,
            FilterKind::K2040 => &K2040_LABELS,
            FilterKind::Dirty => &DIRTY_LABELS,
        }
    }

    /// `m`'s screen name on this kind; its disk ident if the kind lacks it.
    pub fn mode_label(self, m: FilterMode) -> &'static str {
        self.modes()
            .iter()
            .position(|&x| x == m)
            .map_or(m.disk_ident(), |i| self.mode_labels()[i])
    }

    /// NOTE → CUTOFF on a new Sound (spec § 2).
    pub const fn key_default(self) -> i8 {
        match self {
            FilterKind::Svf | FilterKind::K2040 | FilterKind::Dirty => 0,
        }
    }

    pub fn from_index(v: f32) -> Self {
        pick(&Self::BUILT, v)
    }
}

/// CLEAN's bill by mode, cycles a sample over LP24 (spec § CPU), never
/// below 0: the engine terms were solved from bench rows that already run
/// the SVF at LP24. `recost` re-prices at every note-on, so a mode bills
/// its own delta. Each kind's bill is `FilterParams::filter_cost`.
pub const fn clean_cost(mode: FilterMode) -> Cost {
    Cost(match mode {
        FilterMode::Phaser => SVF_PHASER,
        // HP12 is DIRTY's; on CLEAN it would run HP4.
        FilterMode::Bp24 | FilterMode::Hp24 | FilterMode::Hp12 => SVF_PAD,
        // LP24 is the baseline; LP6, LP12, BP12 and NOTCH run less.
        FilterMode::Lp24
        | FilterMode::Lp6
        | FilterMode::Lp12
        | FilterMode::Bp12
        | FilterMode::Notch => 0,
    })
}

/// `p` with KIND `new`: MODE stays if `new` has it, else becomes `new`'s
/// default; nothing else moves (spec § 7).
pub fn kind_change(p: FilterParams, new: FilterKind) -> FilterParams {
    let mut q = p;
    q.set_kind_raw(new);
    if !new.modes().contains(&p.mode()) {
        q.set_mode(new.default_mode());
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

/// A voice's filter: every built TYPE's state, the one its params name run
/// each block. Until Task 10's crossfade, a TYPE change starts the incoming
/// type from rest.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    clean: Clean,
    k2040: K2040,
    dirty: Dirty,
    /// The last block's TYPE, MODE and VOICE; `None` before the first.
    /// Task 10's crossfade triggers on a change of any of them.
    last: Option<(FilterKind, FilterMode, K2040Voice)>,
}

impl Filter {
    pub fn new() -> Self {
        Self::default()
    }

    /// One block through `p`'s TYPE: its clip for the LED.
    pub fn process(
        &mut self,
        buf: &mut [f32; crate::hw::BLOCK_SIZE],
        p: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> crate::meter::Clip {
        let kind = p.kind();
        if self
            .last
            .replace((kind, p.mode(), p.voice))
            .is_some_and(|(k, ..)| k != kind)
        {
            match kind {
                FilterKind::Svf => self.clean.reset(),
                FilterKind::K2040 => self.k2040.reset(),
                FilterKind::Dirty => self.dirty.reset(),
            }
        }
        match kind {
            FilterKind::Svf => self.clean.process(buf, p, fs, mods),
            FilterKind::K2040 => self.k2040.process(buf, p, fs, mods),
            FilterKind::Dirty => self.dirty.process(buf, p, fs, mods),
        }
    }

    /// Forget the ramps: the next block starts unramped.
    pub fn hold(&mut self) {
        self.clean.hold();
        self.k2040.hold();
        self.dirty.hold();
    }

    /// A fresh voice's filter: every type at rest and unramped.
    pub fn reset(&mut self) {
        self.clean.reset();
        self.k2040.reset();
        self.dirty.reset();
        self.last = None;
    }

    /// The running type still rings (the voice-end fade, #374).
    pub fn rings(&self) -> bool {
        match self.last {
            Some((FilterKind::Svf, ..)) => self.clean.rings(),
            Some((FilterKind::K2040, ..)) => self.k2040.rings(),
            Some((FilterKind::Dirty, ..)) => self.dirty.rings(),
            None => false,
        }
    }

    /// LEVEL's clipper runs next block: CLEAN's or DIRTY's; the 2040 has
    /// none.
    pub fn level_running(&self) -> bool {
        match self.last {
            Some((FilterKind::Svf, ..)) => self.clean.level_running(),
            Some((FilterKind::Dirty, ..)) => self.dirty.level_running(),
            _ => false,
        }
    }

    /// The running type's last `g` (`None`: no ramp next block).
    #[cfg(test)]
    pub(crate) fn last_g(&self) -> Option<f32> {
        match self.last {
            Some((FilterKind::K2040, ..)) => self.k2040.last_g(),
            Some((FilterKind::Dirty, ..)) => self.dirty.last_f(),
            _ => self.clean.last_g(),
        }
    }
}
