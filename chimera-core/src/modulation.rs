//! Compact modulation state shared between UI and audio thread.
//!
//! Built only from the destination registry (`from_registry`) or the UI's
//! matrix (`sync_from_matrix`), both of which admit only modulatable
//! addresses (spec §4). The audio ISR reads routes + source values to compute
//! per-destination offsets.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::algo::plan::OPS;
use crate::dsp::modulator::{EnvSlot, EnvType, FuncMode, LfoType};
use crate::hw::Cost;
use crate::mod_path::{LABEL_LEN, ModDestRegistry};
use crate::params::{EnvParams, FilterParams, ParamSnapshot};
use crate::ui::mod_grid::MatrixState;

pub const MAX_MOD_SOURCES: usize = 8;
pub const MAX_MOD_DESTS: usize = 16;

/// CUTOFF: the default routes' column, and the filter knobs' (spec § 2, § 6).
pub const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
/// CUTOFF's matrix column label, wherever the column is created.
pub const CUTOFF_LABEL: [u8; LABEL_LEN] = *b"FLTCUTOF";
/// The VCA: a hidden destination, the voice's output level (spec § 4).
pub const VCA: ParamAddr = ParamAddr::new(BlockRef::Out, crate::params::OutParams::VCA);

/// The matrix's source rows, in `Voice`'s order (spec § 2). Indices are
/// stored: 0 and 1 keep their old meaning (the envelope and the LFO).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ModSource {
    Env1 = 0,
    Lfo1 = 1,
    Env2 = 2,
    Env3 = 3,
    Lfo2 = 4,
    Lfo3 = 5,
    Vel = 6,
    Note = 7,
}

impl ModSource {
    pub const ALL: [ModSource; MAX_MOD_SOURCES] = [
        ModSource::Env1,
        ModSource::Lfo1,
        ModSource::Env2,
        ModSource::Env3,
        ModSource::Lfo2,
        ModSource::Lfo3,
        ModSource::Vel,
        ModSource::Note,
    ];

    /// The source ENV slot `s` feeds.
    pub const fn of_env(s: crate::dsp::modulator::EnvSlot) -> Self {
        [ModSource::Env1, ModSource::Env2, ModSource::Env3][s.index()]
    }

    /// The source LFO slot `s` feeds.
    pub const fn of_lfo(s: crate::dsp::modulator::LfoSlot) -> Self {
        [ModSource::Lfo1, ModSource::Lfo2, ModSource::Lfo3][s.index()]
    }

    pub const fn index(self) -> usize {
        self as usize
    }

    /// The ENV slot this source is, if any.
    pub const fn env_slot(self) -> Option<crate::dsp::modulator::EnvSlot> {
        use crate::dsp::modulator::EnvSlot;
        match self {
            ModSource::Env1 => Some(EnvSlot::Env1),
            ModSource::Env2 => Some(EnvSlot::Env2),
            ModSource::Env3 => Some(EnvSlot::Env3),
            _ => None,
        }
    }

    /// The matrix row's tag (≤ 3 characters, #15).
    pub const fn tag(self) -> &'static str {
        match self {
            ModSource::Env1 => "E1",
            ModSource::Lfo1 => "LF1",
            ModSource::Env2 => "E2",
            ModSource::Env3 => "E3",
            ModSource::Lfo2 => "LF2",
            ModSource::Lfo3 => "LF3",
            ModSource::Vel => "VEL",
            ModSource::Note => "NTE",
        }
    }
}

/// The NOTE source: `(note − 60) / 120`, clamped to −1..1, so a route at
/// 127 into CUTOFF tracks one octave per octave (spec § 2, § 3).
pub fn note_source(note: crate::MidiNote) -> f32 {
    ((note.get() as f32 - 60.0) / 120.0).clamp(-1.0, 1.0)
}

/// `amount / 127` for every amount (index `amount + 127`): the same f32 the
/// divide gives (const float arithmetic is IEEE), read instead of divided.
static AMOUNT_SCALE: [f32; 255] = {
    let mut t = [0.0f32; 255];
    let mut i = 0;
    while i < 255 {
        t[i] = (i as i32 - 127) as f32 / 127.0;
        i += 1;
    }
    t
};

/// `a / 127` (−128 reads as −127).
pub fn amount_scale(a: i8) -> f32 {
    AMOUNT_SCALE[(a.max(-127) as i32 + 127) as usize]
}

/// Fills unused dest slots; never read (only `d < num_dests` is).
const UNUSED: ParamAddr = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);

/// Compact modulation state shared between UI and audio thread.
#[derive(Clone, Debug)]
pub struct ModState {
    num_sources: usize,
    num_dests: usize,
    dests: [ParamAddr; MAX_MOD_DESTS],
    /// amounts[source][dest], -127 to +127
    amounts: [[i8; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
    /// Route presence (spec § 2): bit s of present[d] is set when source s
    /// routes to dest d, at any amount. The audio thread's sums ignore it.
    present: [u8; MAX_MOD_DESTS],
}

impl ModState {
    /// No sources, no destinations.
    pub const fn new() -> Self {
        Self {
            num_sources: 0,
            num_dests: 0,
            dests: [UNUSED; MAX_MOD_DESTS],
            amounts: [[0; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
            present: [0; MAX_MOD_DESTS],
        }
    }

    pub fn num_sources(&self) -> usize {
        self.num_sources
    }

    pub fn num_dests(&self) -> usize {
        self.num_dests
    }

    /// Destination `d` (`d < num_dests()`). Returns the unused sentinel
    /// address when `d` is out of range instead of panicking: this is called
    /// from the audio ISR (`Voice::render`), which must never hard-fault.
    pub fn dest(&self, d: usize) -> ParamAddr {
        self.dests.get(d).copied().unwrap_or(UNUSED)
    }

    /// Amount from `source` to destination `dest`; 0 when out of range.
    pub fn amount(&self, source: usize, dest: usize) -> i8 {
        if source < self.num_sources && dest < self.num_dests {
            self.amounts[source][dest]
        } else {
            0
        }
    }

    /// The present bits of destination `d` (0 out of range).
    pub fn present(&self, d: usize) -> u8 {
        if d < self.num_dests {
            self.present[d]
        } else {
            0
        }
    }

    /// The column of `addr`, if any.
    pub fn find(&self, addr: ParamAddr) -> Option<usize> {
        (0..self.num_dests).find(|&d| self.dests[d] == addr)
    }

    /// The present bits of the column of `addr` (0 without a column).
    pub fn routes_into(&self, addr: ParamAddr) -> u8 {
        self.find(addr).map_or(0, |d| self.present[d])
    }

    /// A route of nonzero amount into destination `d`: its sum can move it.
    pub fn moves(&self, d: usize) -> bool {
        (0..self.num_sources).any(|s| self.amount(s, d) != 0)
    }

    /// A route of nonzero amount into `addr`.
    pub fn moves_addr(&self, addr: ParamAddr) -> bool {
        self.find(addr).is_some_and(|d| self.moves(d))
    }

    /// Create (or set) the route `source → dest` at `amount`, 0 included.
    pub fn set_route(&mut self, source: usize, dest: usize, amount: i8) {
        if source < self.num_sources && dest < self.num_dests {
            self.amounts[source][dest] = amount;
            self.present[dest] |= 1 << source;
        }
    }

    /// Append `addr` as a column if it is modulatable and there is room.
    pub fn push(&mut self, addr: ParamAddr) -> Option<usize> {
        self.push_dest(Some(addr)).then(|| self.num_dests - 1)
    }

    /// Operators whose LEVEL has a route with a nonzero amount: a LEVEL of
    /// 0 there may still sound.
    pub fn algo_levels_routed(&self) -> [bool; OPS] {
        let mut routed = [false; OPS];
        for d in 0..self.num_dests.min(MAX_MOD_DESTS) {
            if let BlockRef::AlgoOp(op) = self.dests[d].block
                && self.dests[d].param == AlgoOpParams::LEVEL
                && (0..self.num_sources).any(|s| self.amounts[s][d] != 0)
            {
                routed[op.index()] = true;
            }
        }
        routed
    }

    /// Sum of `source_value * amount / 127` over all sources for dest `d`.
    /// Same order and arithmetic as the old `compute_offset`. Returns 0.0
    /// when `d` is out of range instead of panicking (see `dest`).
    pub fn sum_for(&self, d: usize, source_values: &[f32; MAX_MOD_SOURCES]) -> f32 {
        if d >= MAX_MOD_DESTS {
            return 0.0;
        }
        let mut total = 0.0f32;
        for si in 0..self.num_sources {
            let amt = self.amounts[si][d];
            if amt != 0 {
                total += source_values[si] * amount_scale(amt);
            }
        }
        total
    }

    /// Offset for `addr`, or 0.0 if it is not a destination (UI display).
    pub fn offset_for(&self, addr: ParamAddr, source_values: &[f32; MAX_MOD_SOURCES]) -> f32 {
        self.find(addr)
            .map_or(0.0, |d| self.sum_for(d, source_values))
    }

    /// Destinations from the registry (in order, at most `MAX_MOD_DESTS`),
    /// all amounts zero. `num_sources` is clamped to `MAX_MOD_SOURCES`.
    pub fn from_registry(registry: &ModDestRegistry, num_sources: usize) -> Self {
        let mut ms = Self::new();
        ms.num_sources = num_sources.min(MAX_MOD_SOURCES);
        for i in 0..registry.len() {
            let Some(entry) = registry.get(i) else {
                continue;
            };
            ms.push_dest(Some(entry.addr));
        }
        ms
    }

    /// Set one amount; a nonzero amount creates the route. Ignored when
    /// `source` or `dest` is out of range.
    pub fn set_amount(&mut self, source: usize, dest: usize, amount: i8) {
        if source < self.num_sources && dest < self.num_dests {
            self.amounts[source][dest] = amount;
            if amount != 0 {
                self.present[dest] |= 1 << source;
            }
        }
    }

    /// Copy routing from the UI's matrix. Keeps only modulatable
    /// destinations (at most `MAX_MOD_DESTS`, amounts moved with their dest)
    /// and at most `MAX_MOD_SOURCES` sources.
    pub fn sync_from_matrix(&mut self, matrix: &MatrixState) {
        *self = Self::new();
        self.num_sources = matrix.num_sources.min(MAX_MOD_SOURCES);
        for di in 0..matrix.num_dests {
            let addr = matrix.dests[di].map(|d| d.addr);
            if self.push_dest(addr) {
                let d = self.num_dests - 1;
                for si in 0..self.num_sources {
                    self.amounts[si][d] = matrix.amounts[si][di];
                }
                self.present[d] = matrix.present[di];
            }
        }
    }

    /// Append `addr` if it is modulatable and there is room.
    fn push_dest(&mut self, addr: Option<ParamAddr>) -> bool {
        match addr {
            Some(a) if a.modulatable() && self.num_dests < MAX_MOD_DESTS => {
                self.dests[self.num_dests] = a;
                self.num_dests += 1;
                true
            }
            _ => false,
        }
    }
}

impl Default for ModState {
    fn default() -> Self {
        Self::new()
    }
}

/// The modulator pool's cycles per sample (spec § CPU). Each term is
/// measured 2026-09-28 on the bench-t13c run, ROUTING rows, rev V at 480
/// MHz, rounded up, never below 1; the derivations are in the
/// filter-routing plan's `## Measured`.
pub struct ModRouting;

impl ModRouting {
    /// Six per-block modulators, the eight-row matrix sum and `fast_exp2`:
    /// 1 OP less its 436 before the pool.
    pub const BASE: Cost = Cost(47);
    /// An ENV slot of type A filling the VCA's buffer: A VCA less 1 OP and CLAMP.
    pub const ENV_A: Cost = Cost(28);
    /// Type B filling it: the costliest of B VCA, B LFO and B GLIDE (LFO
    /// FREE with a FORM change's glide always running) less 1 OP and CLAMP.
    pub const ENV_B: Cost = Cost(98);
    /// More for B in ENV mode with SHAPE off centre, or with a route into
    /// its SHAPE: B CURVE less B VCA.
    pub const CURVE: Cost = Cost(1);
    /// More for B in BURST mode, on top of `ENV_B`: BURST AD (above BURST
    /// CYC) less 1 OP, CLAMP and the steady B's costliest (B VCA, B LFO:
    /// 90), so BURST keeps B GLIDE's margin.
    pub const BURST: Cost = Cost(76);
    /// Each other VCA route's ramp: 2 VCA less VEL VCA.
    pub const OTHER: Cost = Cost(8);
    /// The VCA's clamp and multiply, with any route: VEL VCA less 1 OP and OTHER.
    pub const CLAMP: Cost = Cost(24);
    /// A route into an ENV slot's TIME, RISE, FALL or SHAPE rebuilds that
    /// slot's coefficients every block: billed once per slot with any such
    /// route, whether or not that slot feeds the VCA. The costlier of A
    /// SLIDE less A VCA and B SLIDE less B CURVE.
    pub const SLIDE: Cost = Cost(33);
    /// The first destination other than the VCA with a route of nonzero
    /// amount: its sum every block, and its offset. Measured: bench-t13d's
    /// 1 CUTOFF row (494) less that run's 1 OP (488) = 6; bench-t13c's
    /// 1 DEST row (486) less that run's 1 OP (483) = 3. Billed as the
    /// larger: 6.
    pub const DEST_FIRST: Cost = Cost(6);
    /// Each such destination after the first: what the MODS row leaves over
    /// every other term, over its five further destinations (0.2; the floor).
    pub const DEST: Cost = Cost(1);
    /// Each LFO slot of type FUNC, routed or not (every slot runs every
    /// block), over a CLASSIC one: FUNC LFO less 1 DEST.
    pub const FUNC: Cost = Cost(5);
    /// An ENV slot feeding the VCA with a route of nonzero amount into its
    /// LEVEL, whose peak then ramps across each block (`add_ramped`): A
    /// LEVEL less A VCA and `DEST_FIRST`.
    pub const LEVEL: Cost = Cost(10);

    pub fn cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        let slide = EnvSlot::ALL
            .iter()
            .filter(|&&slot| Self::slot_slides(mods, slot))
            .fold(Cost::ZERO, |a, _| a + Self::SLIDE);
        let func = p
            .lfos
            .iter()
            .filter(|l| l.lfo_type == LfoType::Func)
            .fold(Cost::ZERO, |a, _| a + Self::FUNC);
        Self::BASE + Self::vca_cost(p, mods) + slide + Self::dest_cost(mods) + func
    }

    /// `DEST_FIRST`, then `DEST` for each further destination other than
    /// the VCA with a route of nonzero amount. ENV slots' own count too:
    /// their sums run every block; SLIDE and LEVEL bill what they add.
    fn dest_cost(mods: &ModState) -> Cost {
        (0..mods.num_dests())
            .filter(|&d| mods.dest(d) != VCA && mods.moves(d))
            .enumerate()
            .fold(Cost::ZERO, |c, (i, _)| {
                c + if i == 0 { Self::DEST_FIRST } else { Self::DEST }
            })
    }

    /// The VCA's own routes: 0 without one, else `CLAMP` plus each source's
    /// per-sample term.
    fn vca_cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        let bits = mods.routes_into(VCA);
        if bits == 0 {
            return Cost::ZERO;
        }
        ModSource::ALL
            .iter()
            .filter(|s| bits & (1 << s.index()) != 0)
            .map(|s| match s.env_slot() {
                None => Self::OTHER,
                Some(slot) => Self::env_slot_cost(p, mods, slot) + Self::level_cost(mods, slot),
            })
            .fold(Self::CLAMP, |a, b| a + b)
    }

    /// `slot`'s per-sample term, feeding the VCA.
    fn env_slot_cost(p: &ParamSnapshot, mods: &ModState, slot: EnvSlot) -> Cost {
        let e = &p.envelopes[slot.index()];
        // A SHAPE route moves a centred SHAPE off 0.5: the divide runs.
        let shape_routed =
            mods.routes_into(ParamAddr::new(BlockRef::Env(slot), EnvParams::SHAPE)) != 0;
        match e.env_type {
            EnvType::A => Self::ENV_A,
            EnvType::B if e.func.mode == FuncMode::Burst => Self::ENV_B + Self::BURST,
            EnvType::B if e.func.mode == FuncMode::Env && (e.func.shape != 0.5 || shape_routed) => {
                Self::ENV_B + Self::CURVE
            }
            EnvType::B => Self::ENV_B,
        }
    }

    /// `LEVEL` for `slot`, feeding the VCA, with a moving route into its LEVEL.
    fn level_cost(mods: &ModState, slot: EnvSlot) -> Cost {
        if mods.moves_addr(ParamAddr::new(BlockRef::Env(slot), EnvParams::LEVEL)) {
            Self::LEVEL
        } else {
            Cost::ZERO
        }
    }

    /// Whether `slot` has a route into TIME, RISE, FALL or SHAPE: its
    /// coefficients rebuild every block, on top of any VCA term above.
    fn slot_slides(mods: &ModState, slot: EnvSlot) -> bool {
        [
            EnvParams::TIME,
            EnvParams::RISE,
            EnvParams::FALL,
            EnvParams::SHAPE,
        ]
        .into_iter()
        .any(|param| mods.routes_into(ParamAddr::new(BlockRef::Env(slot), param)) != 0)
    }
}
