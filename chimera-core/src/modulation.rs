//! Compact modulation state shared between UI and audio thread.
//!
//! Built only from the destination registry (`from_registry`) or the UI's
//! matrix (`sync_from_matrix`), both of which admit only modulatable
//! addresses (spec §4). The audio ISR reads routes + source values to compute
//! per-destination offsets.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::algo::plan::OPS;
use crate::mod_path::{LABEL_LEN, ModDestRegistry};
use crate::params::FilterParams;
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
