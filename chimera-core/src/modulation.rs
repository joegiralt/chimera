//! Compact modulation state shared between UI and audio thread.
//!
//! Built only from the destination registry (`from_registry`) or the UI's
//! matrix (`sync_from_matrix`), both of which admit only modulatable
//! addresses (spec §4). The audio ISR reads routes + source values to compute
//! per-destination offsets.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::pizza::PizzaParams;
use crate::mod_path::{legacy_to_addr, ModDestRegistry};
use crate::preset::ChainType;
use crate::ui::mod_grid::MatrixState;

pub const MAX_MOD_SOURCES: usize = 8;
pub const MAX_MOD_DESTS: usize = 16;

/// Fills unused dest slots; never read (only `d < num_dests` is).
const UNUSED: ParamAddr = ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE);

/// Compact modulation state shared between UI and audio thread.
#[derive(Clone, Debug)]
pub struct ModState {
    num_sources: usize,
    num_dests: usize,
    dests: [ParamAddr; MAX_MOD_DESTS],
    /// amounts[source][dest], -127 to +127
    amounts: [[i8; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
}

impl ModState {
    /// No sources, no destinations.
    pub const fn new() -> Self {
        Self {
            num_sources: 0,
            num_dests: 0,
            dests: [UNUSED; MAX_MOD_DESTS],
            amounts: [[0; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
        }
    }

    pub fn num_sources(&self) -> usize {
        self.num_sources
    }

    pub fn num_dests(&self) -> usize {
        self.num_dests
    }

    /// Destination `d` (`d < num_dests()`).
    pub fn dest(&self, d: usize) -> ParamAddr {
        self.dests[d]
    }

    /// Amount from `source` to destination `dest`; 0 when out of range.
    pub fn amount(&self, source: usize, dest: usize) -> i8 {
        if source < self.num_sources && dest < self.num_dests {
            self.amounts[source][dest]
        } else {
            0
        }
    }

    /// Sum of `source_value * amount / 127` over all sources for dest `d`.
    /// Same order and arithmetic as the old `compute_offset`.
    pub fn sum_for(&self, d: usize, source_values: &[f32; MAX_MOD_SOURCES]) -> f32 {
        let mut total = 0.0f32;
        for si in 0..self.num_sources {
            let amt = self.amounts[si][d];
            if amt != 0 {
                total += source_values[si] * (amt as f32 / 127.0);
            }
        }
        total
    }

    /// Offset for `addr`, or 0.0 if it is not a destination (UI display).
    pub fn offset_for(&self, addr: ParamAddr, source_values: &[f32; MAX_MOD_SOURCES]) -> f32 {
        (0..self.num_dests)
            .find(|&d| self.dests[d] == addr)
            .map_or(0.0, |d| self.sum_for(d, source_values))
    }

    /// Destinations from the registry (in order, at most `MAX_MOD_DESTS`),
    /// all amounts zero. `num_sources` is clamped to `MAX_MOD_SOURCES`.
    pub fn from_registry(registry: &ModDestRegistry, chain: ChainType, num_sources: usize) -> Self {
        let mut ms = Self::new();
        ms.num_sources = num_sources.min(MAX_MOD_SOURCES);
        for i in 0..registry.len() {
            let Some(entry) = registry.get(i) else { continue };
            ms.push_dest(legacy_to_addr(chain, entry.path));
        }
        ms
    }

    /// Set one amount. Ignored when `source` or `dest` is out of range.
    pub fn set_amount(&mut self, source: usize, dest: usize, amount: i8) {
        if source < self.num_sources && dest < self.num_dests {
            self.amounts[source][dest] = amount;
        }
    }

    /// Copy routing from the UI's matrix. Keeps only modulatable
    /// destinations (at most `MAX_MOD_DESTS`, amounts moved with their dest)
    /// and at most `MAX_MOD_SOURCES` sources.
    pub fn sync_from_matrix(&mut self, matrix: &MatrixState, chain: ChainType) {
        *self = Self::new();
        self.num_sources = matrix.num_sources.min(MAX_MOD_SOURCES);
        for di in 0..matrix.num_dests {
            let addr = matrix.dests[di].as_ref().and_then(|d| legacy_to_addr(chain, d.path));
            if self.push_dest(addr) {
                let d = self.num_dests - 1;
                for si in 0..self.num_sources {
                    self.amounts[si][d] = matrix.amounts[si][di];
                }
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
