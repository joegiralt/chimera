use crate::block::DiskCode;
use crate::dsp::modulator::pick;
use crate::hw::BLOCK_SIZE;
use crate::hw::Cost;
use crate::params::{EngineType, FilterParams};
use crate::taps::TapLane;
use coef::exp2_poly;

pub mod clean;
pub mod coef;
pub mod dirty;
pub mod k2040;
pub mod level;
#[cfg(any(test, feature = "probe", feature = "filter-probe"))]
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

/// The moving path's bill, once a block, over the steady one (plan
/// § Measured, Task 15, bench ELF at 1879169d, 2026-10-07): CLEAN LP4's
/// moving loop 64 instructions, 2 `vdiv`, with `ramps`' fill (8.5) and
/// the block (3.61, 0.016 `vdiv`) ceil5(76.1 × 1.606 + 12 × 2.02) = 150,
/// less steady LP4's 41 + 3.25 = 75: 75. NOTCH2 moves for 90 − 35 = 55;
/// the one-stage modes run less.
pub const CLEAN_MOVE_COST: Cost = Cost(75);
/// The 2040's moving path (ADR 0096's +12 instructions, 2 `vdiv`), with
/// `ramps`' fill by adding (6.25): REV2 80 + 6.25 + 2.31 block
/// instructions, 2.02 `vdiv`, 170 less steady 110 = 60; EXP 94 + 6.25 +
/// 2.34, 190 − 135 = 55. Billed the larger: 60 (1879169d, 2026-10-07).
pub const K2040_MOVE_COST: Cost = Cost(60);
/// LEVEL's clipper on CLEAN and DIRTY, billed while it can run: the
/// fading loop 20.0 instructions a sample, 0 `vdiv`, ceil5(20 × 1.606) =
/// 35 (held in, 16.62: 30), over the gate shut's 0 at ½ (1879169d,
/// 2026-10-07). The 2040 takes LEVEL into its stages: its steady count.
pub const FILTER_LEVEL_COST: Cost = Cost(35);

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

    /// The TYPE a new Sound on `e` starts with. VA (2040) adds its arm when
    /// that engine lands.
    pub const fn default_for(e: EngineType) -> FilterKind {
        match e {
            EngineType::Algo | EngineType::Modal | EngineType::Tape => FilterKind::Svf,
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

    pub fn from_index(v: f32) -> Self {
        pick(&Self::BUILT, v)
    }

    /// The steady bill, cycles a sample over `OLD_LP24` (95), never below
    /// 0: the engine terms carry LP24 (plan § Measured, Task 15, counted
    /// with the block at 1879169d). CLEAN: LP4, BP4 and HP4 41 + 3.25
    /// instructions, 75 (80 with LEVEL's gain); NOTCH2 18 + 3.06, 35; the
    /// one-stage modes less: 0. The 2040 by VOICE (`K2040Voice::cost`):
    /// REV2 110, EXP 135. DIRTY 30.25 + `ramps`' 6.25 + 4.08, 70, every
    /// block and mode: 0. No mode bills over LP24, so the mode isn't asked.
    pub const fn cost(self, voice: K2040Voice) -> Cost {
        match self {
            FilterKind::Svf => Cost::ZERO,
            FilterKind::K2040 => voice.cost(),
            FilterKind::Dirty => dirty::DIRTY_COST,
        }
    }

    /// What a moving block adds, once: a CUTOFF or RES route, or a FREQ or
    /// Q tap. DIRTY's one loop already reads the arrays on every block.
    pub const fn move_cost(self) -> Cost {
        match self {
            FilterKind::Svf => CLEAN_MOVE_COST,
            FilterKind::K2040 => K2040_MOVE_COST,
            FilterKind::Dirty => Cost::ZERO,
        }
    }

    /// LEVEL's clipper, while it can run; the 2040 has none.
    pub const fn level_cost(self) -> Cost {
        match self {
            FilterKind::Svf | FilterKind::Dirty => FILTER_LEVEL_COST,
            FilterKind::K2040 => Cost::ZERO,
        }
    }
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

/// A moving block's `g` and `k` (DIRTY's `f` and `q`) for each sample,
/// each `(from, step)` ramped by `g_at` (CLEAN) or by adding (the 2040 and
/// DIRTY), as each type always ramped. Out of line: one copy for every
/// mode's loop, which reads the arrays.
#[inline(never)]
pub(crate) fn ramps(
    gs: &mut [f32; BLOCK_SIZE],
    ks: &mut [f32; BLOCK_SIZE],
    g: (f32, f32),
    k: (f32, f32),
    by_adding: bool,
) {
    // `t` counts samples in `f32`, exact to 2^24: `g_at`'s `i + 1` without
    // a conversion a sample.
    let (mut a, mut b, mut t) = (g.0, k.0, 0.0f32);
    for (gi, ki) in gs.iter_mut().zip(ks.iter_mut()) {
        t += 1.0;
        (a, b) = if by_adding {
            (a + g.1, b + k.1)
        } else {
            (g.0 + g.1 * t, k.0 + k.1 * t)
        };
        (*gi, *ki) = (a, b);
    }
}

/// A tapped block's per-sample FREQ and Q (spec § 6): `g·exp2_poly(4·freq)`
/// held to `g`'s range, and `k + dk·q`, RES moved by the lane (every type's
/// `k` is linear in RES), held to RES 0..1's. A FREQ or Q tap makes the
/// block a moving one; an absent lane reads zero amounts. Both lanes read
/// one block's `TapRows`, as the voice builds them.
#[derive(Clone, Copy)]
pub(crate) struct TapShape<'a> {
    rows: &'a [[f32; BLOCK_SIZE]; 2],
    /// Each op's amount, `(from, step)`: FREQ's × 4 (octaves), Q's × `dk`.
    freq: ([f32; 2], [f32; 2]),
    q: ([f32; 2], [f32; 2]),
    g: (f32, f32),
    k: (f32, f32),
}

impl<'a> TapShape<'a> {
    /// `None` without a FREQ or Q tap; `k_of` is the type's RES → `k`.
    #[inline(always)]
    pub(crate) fn new(mods: &FilterMods<'a>, g: (f32, f32), k_of: fn(f32) -> f32) -> Option<Self> {
        let rows = mods.freq.or(mods.q)?.rows;
        debug_assert!(
            [mods.freq, mods.q]
                .iter()
                .flatten()
                .all(|l| core::ptr::eq(l.rows, rows))
        );
        let (k0, k1) = (k_of(0.0), k_of(1.0));
        let scaled = |l: Option<TapLane>, by: f32| {
            l.map_or(([0.0; 2], [0.0; 2]), |l| {
                (l.from.map(|a| a * by), l.step().map(|d| d * by))
            })
        };
        Some(Self {
            rows,
            freq: scaled(mods.freq, 4.0),
            q: scaled(mods.q, k1 - k0),
            g,
            k: (k0.min(k1), k0.max(k1)),
        })
    }

    /// A moving block's per-sample `g` and `k`, the ramp's, through the
    /// taps. One copy serves every type and mode: the types' moving loops
    /// read the arrays, so a tap adds no loop of its own to each.
    #[inline(never)]
    pub(crate) fn apply(&self, g: &mut [f32; BLOCK_SIZE], k: &mut [f32; BLOCK_SIZE]) {
        let ((mut fa, df), (mut qa, dq)) = (self.freq, self.q);
        let [r0, r1] = self.rows;
        let (gr, kr) = (self.g, self.k);
        for (n, (g, k)) in g.iter_mut().zip(k.iter_mut()).enumerate() {
            fa = [fa[0] + df[0], fa[1] + df[1]];
            qa = [qa[0] + dq[0], qa[1] + dq[1]];
            let (x0, x1) = (r0[n], r1[n]);
            *g = hold(*g * exp2_poly(fa[0] * x0 + fa[1] * x1), gr);
            *k = hold(*k + qa[0] * x0 + qa[1] * x1, kr);
        }
    }
}

/// `x` held to `r` (a NaN to `r.0`, as `clampf`), by compare and select:
/// `max`/`min` against bounds read from memory cost twice the instructions.
#[inline(always)]
fn hold(x: f32, r: (f32, f32)) -> f32 {
    let x = if x >= r.0 { x } else { r.0 };
    if x <= r.1 { x } else { r.1 }
}

/// A FREQ tap's `g` range: FREQ's floor (20 Hz) to 0.45·fs, short of
/// `g_of`'s 0.497 so the tap's top octave stays clear of Nyquist.
#[inline(always)]
pub(crate) fn tap_g_range(fs: u32) -> (f32, f32) {
    (coef::g_of(20.0, fs as f32), coef::tan_accurate(0.45))
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
