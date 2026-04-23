//! Compact modulation state shared between UI and audio thread.
//!
//! The UI thread writes mod routes via `sync_from_matrix()`.
//! The audio ISR reads routes + source values to compute per-param offsets.

use crate::ui::mod_grid::MatrixState;

pub const MAX_MOD_SOURCES: usize = 8;
pub const MAX_MOD_DESTS: usize = 16;

/// Compact modulation state shared between UI and audio thread.
#[derive(Clone, Debug)]
pub struct ModState {
    pub num_sources: usize,
    pub num_dests: usize,
    /// (block_idx, param_idx) for each dest slot
    pub dests: [(u8, u8); MAX_MOD_DESTS],
    /// amounts[source][dest], -127 to +127
    pub amounts: [[i8; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
}

impl ModState {
    pub const fn new() -> Self {
        Self {
            num_sources: 0,
            num_dests: 0,
            dests: [(0, 0); MAX_MOD_DESTS],
            amounts: [[0; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
        }
    }

    /// Compute the total modulation offset for a given (block_idx, param_idx) destination.
    /// Sums source_value * amount/127 across all sources routed to this dest.
    /// Returns 0.0 if no route exists.
    pub fn compute_offset(
        &self,
        source_values: &[f32; MAX_MOD_SOURCES],
        block_idx: u8,
        param_idx: u8,
    ) -> f32 {
        let mut total = 0.0f32;
        for di in 0..self.num_dests {
            let (bi, pi) = self.dests[di];
            if bi == block_idx && pi == param_idx {
                for si in 0..self.num_sources {
                    let amt = self.amounts[si][di];
                    if amt != 0 {
                        total += source_values[si] * (amt as f32 / 127.0);
                    }
                }
                return total;
            }
        }
        0.0
    }

    /// Copy modulation routing from the UI's MatrixState into this compact form.
    pub fn sync_from_matrix(&mut self, matrix: &MatrixState) {
        self.num_sources = matrix.num_sources;
        self.num_dests = matrix.num_dests;

        for di in 0..MAX_MOD_DESTS {
            if di < matrix.num_dests {
                if let Some(dest) = &matrix.dests[di] {
                    self.dests[di] = (dest.block_idx, dest.param_idx);
                } else {
                    self.dests[di] = (0, 0);
                }
            } else {
                self.dests[di] = (0, 0);
            }
        }

        // Copy amounts — MatrixState has [MAX_SOURCES][MAX_DESTS], we have [MAX_MOD_SOURCES][MAX_MOD_DESTS]
        for si in 0..MAX_MOD_SOURCES {
            for di in 0..MAX_MOD_DESTS {
                if si < matrix.num_sources && di < matrix.num_dests {
                    self.amounts[si][di] = matrix.amounts[si][di];
                } else {
                    self.amounts[si][di] = 0;
                }
            }
        }
    }
}

impl Default for ModState {
    fn default() -> Self {
        Self::new()
    }
}
