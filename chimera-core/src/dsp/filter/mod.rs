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
            FilterKind::Dirty => &dirty::DIRTY_MODES,
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

/// What a block runs: TYPE, MODE and, on the 2040, VOICE. What a type
/// ignores is normalised away (the 2040's MODE, the others' VOICE), so
/// changing it changes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Setting {
    kind: FilterKind,
    mode: FilterMode,
    voice: K2040Voice,
}

impl Setting {
    fn of(p: &FilterParams) -> Self {
        match p.kind() {
            FilterKind::K2040 => Setting {
                kind: FilterKind::K2040,
                mode: FilterKind::K2040.default_mode(),
                voice: p.voice,
            },
            kind => Setting {
                kind,
                mode: p.mode(),
                voice: K2040Voice::default(),
            },
        }
    }

    /// `p` running this setting; FREQ, RES and LEVEL its own.
    fn on(self, p: &FilterParams) -> FilterParams {
        let mut q = *p;
        q.set_kind_raw(self.kind);
        q.set_mode(self.mode);
        q.voice = self.voice;
        q
    }
}

/// What a crossfade runs the old setting on (spec § 1).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    /// TYPE: the old type on its own state, zeroed when the fade ends.
    Kind,
    /// CLEAN's MODE: the old mode on a stack copy of `Clean`; between two
    /// cascades the copy's stage 2 is carried, since each runs it on its
    /// own input.
    CleanMode { stage2: bool },
    /// DIRTY's MODE: its states run alike in every mode, so a stack copy
    /// is exact.
    DirtyMode,
    /// The 2040's VOICE: the old voice on a stack copy, its states carried.
    Voice,
}

impl Change {
    fn of(from: Setting, to: Setting) -> Self {
        match to.kind {
            _ if from.kind != to.kind => Change::Kind,
            FilterKind::Svf => Change::CleanMode {
                stage2: clean::cascade(from.mode) && clean::cascade(to.mode),
            },
            FilterKind::Dirty => Change::DirtyMode,
            FilterKind::K2040 => Change::Voice,
        }
    }
}

/// A crossfade under way: the setting it leaves, which of its two blocks
/// is next, and the old side's states a stack copy can't keep
/// (`Change::CleanMode`'s stage 2, `Change::Voice`'s stages).
#[derive(Clone, Copy, Debug)]
struct Fade {
    from: Setting,
    half: Half,
    carry: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Half {
    First,
    Second,
}

/// The crossfade's length, in samples: two blocks (spec § 1).
pub const FADE_SAMPLES: usize = 2 * crate::hw::BLOCK_SIZE;

/// A voice's filter: every built TYPE's state, the one its params name run
/// each block. A change of TYPE, MODE or VOICE warm-starts the incoming
/// setting and crossfades from the old one over two blocks, never snapped
/// (spec § 1); one arriving mid-fade waits for its end.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    clean: Clean,
    k2040: K2040,
    dirty: Dirty,
    /// The setting the last block ran (a fade's destination); `None` before
    /// the first.
    last: Option<Setting>,
    /// No `pending`: the params hold the latest setting, so a change made
    /// mid-fade is read again when the fade ends, and the last one wins.
    fade: Option<Fade>,
}

impl Filter {
    pub fn new() -> Self {
        Self::default()
    }

    /// One block through `p`'s setting, or a crossfade block into it: the
    /// incoming setting's clip for the LED. The crossfade's blocks are
    /// not billed (spec § 1).
    pub fn process(
        &mut self,
        buf: &mut [f32; crate::hw::BLOCK_SIZE],
        p: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> crate::meter::Clip {
        let want = Setting::of(p);
        let to = match (self.fade, self.last) {
            (Some(_), Some(to)) => to,
            (_, Some(from)) if from != want => {
                self.start(from, want, p.resonance);
                want
            }
            _ => want,
        };
        self.last = Some(to);
        let q = to.on(p);
        match self.fade {
            None => self.run(to, buf, &q, fs, mods),
            Some(f) => self.crossfade(f, to, buf, p, fs, mods),
        }
    }

    fn run(
        &mut self,
        s: Setting,
        buf: &mut [f32; crate::hw::BLOCK_SIZE],
        q: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> crate::meter::Clip {
        match s.kind {
            FilterKind::Svf => self.clean.process(buf, q, fs, mods),
            FilterKind::K2040 => self.k2040.process(buf, q, fs, mods),
            FilterKind::Dirty => self.dirty.process(buf, q, fs, mods),
        }
    }

    /// A fade's start: the incoming setting warm-started from the outgoing
    /// one's outputs (spec § 1), and what the old side carries.
    fn start(&mut self, from: Setting, to: Setting, res: f32) {
        let mut carry = [0.0; 4];
        match Change::of(from, to) {
            Change::Kind => {
                let (lp, bp) = match from.kind {
                    FilterKind::Svf => self.clean.outputs(from.mode),
                    FilterKind::K2040 => (self.k2040.y4(from.voice, res), 0.0),
                    FilterKind::Dirty => self.dirty.outputs(),
                };
                match to.kind {
                    FilterKind::Svf => {
                        self.clean.seed(lp, bp);
                        self.clean.enter(to.mode);
                    }
                    FilterKind::K2040 => self.k2040.seed(lp, to.voice, res),
                    FilterKind::Dirty => self.dirty.seed(lp, bp),
                }
            }
            Change::CleanMode { stage2 } => {
                if stage2 {
                    let [a, b] = self.clean.stage2();
                    carry = [a, b, 0.0, 0.0];
                }
                self.clean.shift(from.mode, to.mode);
            }
            Change::DirtyMode => {}
            Change::Voice => {
                carry = self.k2040.stages();
                self.k2040.rescale(from.voice, to.voice);
            }
        }
        self.fade = Some(Fade {
            from,
            half: Half::First,
            carry,
        });
    }

    /// One of the fade's two blocks: the old setting on a copy of the
    /// input, the new one on `buf`, crossfaded linearly.
    fn crossfade(
        &mut self,
        mut f: Fade,
        to: Setting,
        buf: &mut [f32; crate::hw::BLOCK_SIZE],
        p: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> crate::meter::Clip {
        let (po, q) = (f.from.on(p), to.on(p));
        let mut old = *buf;
        let change = Change::of(f.from, to);
        let mut copy = None;
        match change {
            Change::Kind => {
                self.run(f.from, &mut old, &po, fs, mods);
            }
            Change::CleanMode { stage2 } => {
                let mut c = self.clean.clone();
                if stage2 {
                    c.set_stage2([f.carry[0], f.carry[1]]);
                }
                c.process(&mut old, &po, fs, mods);
                copy = Some(c);
            }
            Change::DirtyMode => {
                self.dirty.clone().process(&mut old, &po, fs, mods);
            }
            Change::Voice => {
                let mut c = self.k2040.clone();
                c.set_stages(f.carry);
                c.process(&mut old, &po, fs, mods);
                f.carry = c.stages();
            }
        }
        let clip = self.run(to, buf, &q, fs, mods);
        if let Some(c) = copy {
            self.clean.adopt(&c, to.mode);
            let [a, b] = c.stage2();
            f.carry = [a, b, 0.0, 0.0];
        }
        let base = match f.half {
            Half::First => 0,
            Half::Second => crate::hw::BLOCK_SIZE,
        };
        for (i, (y, o)) in buf.iter_mut().zip(&old).enumerate() {
            let w = (base + i + 1) as f32 * (1.0 / FADE_SAMPLES as f32);
            *y = o + (*y - o) * w;
        }
        self.fade = match f.half {
            Half::First => Some(Fade {
                half: Half::Second,
                ..f
            }),
            Half::Second => {
                self.finish(change, f.from, to);
                None
            }
        };
        clip
    }

    /// The fade's end: the state left behind zeroed, clean the next time it
    /// is entered (spec § 1).
    fn finish(&mut self, change: Change, from: Setting, to: Setting) {
        if change == Change::Kind {
            match from.kind {
                FilterKind::Svf => self.clean.reset(),
                FilterKind::K2040 => self.k2040.reset(),
                FilterKind::Dirty => self.dirty.reset(),
            }
        }
        if to.kind == FilterKind::Svf {
            self.clean.zero_unused(to.mode);
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
        (self.last, self.fade) = (None, None);
    }

    /// The running type still rings (the voice-end fade, #374).
    pub fn rings(&self) -> bool {
        match self.last {
            Some(s) => match s.kind {
                FilterKind::Svf => self.clean.rings(),
                FilterKind::K2040 => self.k2040.rings(s.voice),
                FilterKind::Dirty => self.dirty.rings(),
            },
            None => false,
        }
    }

    /// LEVEL's clipper runs next block: CLEAN's or DIRTY's; the 2040 has
    /// none.
    pub fn level_running(&self) -> bool {
        match self.last.map(|s| s.kind) {
            Some(FilterKind::Svf) => self.clean.level_running(),
            Some(FilterKind::Dirty) => self.dirty.level_running(),
            _ => false,
        }
    }

    /// The running type's last `g` (`None`: no ramp next block).
    #[cfg(test)]
    pub(crate) fn last_g(&self) -> Option<f32> {
        match self.last.map(|s| s.kind) {
            Some(FilterKind::K2040) => self.k2040.last_g(),
            Some(FilterKind::Dirty) => self.dirty.last_f(),
            _ => self.clean.last_g(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hw::{BLOCK_SIZE, SAMPLE_RATE};

    /// The type a fade leaves is zeroed when it ends, its states and ramps,
    /// so it is clean the next time it is entered (spec § 1).
    #[test]
    fn a_type_left_is_zeroed_when_its_fade_ends() {
        let mut p = FilterParams::default();
        (p.cutoff, p.resonance) = (800.0, 0.9);
        let mut f = Filter::new();
        let block = |f: &mut Filter, p: &FilterParams, b: usize| {
            let mut buf: [f32; BLOCK_SIZE] =
                core::array::from_fn(|i| 0.5 * (((b * BLOCK_SIZE + i) % 300) as f32 / 150.0 - 1.0));
            f.process(&mut buf, p, SAMPLE_RATE, &FilterMods::NONE);
        };
        for (from, to) in [
            (FilterKind::K2040, FilterKind::Svf),
            (FilterKind::Svf, FilterKind::Dirty),
            (FilterKind::Dirty, FilterKind::K2040),
        ] {
            p.set_kind(from);
            for b in 0..8 {
                block(&mut f, &p, b);
            }
            p.set_kind(to);
            for b in 0..2 {
                block(&mut f, &p, b);
                // Mid-fade, the old type still runs.
                if b == 0 {
                    assert!(match from {
                        FilterKind::Svf => f.clean.rings(),
                        FilterKind::K2040 => f.k2040.rings(p.voice),
                        FilterKind::Dirty => f.dirty.rings(),
                    });
                }
            }
            let (rings, ramped) = match from {
                FilterKind::Svf => (f.clean.rings(), f.clean.last_g().is_some()),
                FilterKind::K2040 => (f.k2040.rings(p.voice), f.k2040.last_g().is_some()),
                FilterKind::Dirty => (f.dirty.rings(), f.dirty.last_f().is_some()),
            };
            assert!(!rings && !ramped, "{from:?} → {to:?}");
            assert!(f.rings(), "{to:?} runs");
        }
    }
}
