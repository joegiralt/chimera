// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The RSF Kobol Expander's filter (branch `exp-circuit-lab`): an SSM 2040
//! in the data sheet's standard connection (10 K chain, 220 Ω legs, 1 nF,
//! R24 100 K in), its resonance through a CA3080 (CI5) fed from OUT 4 by
//! C6 4.7 µF and a 4.7 K/220 Ω divider, its output through a second
//! CA3080, the VCA (CI3). Nothing compensates the bass.
//!
//! - `circuit`: the parts (`Kobol`), the transistor-level netlist and
//!   `spice()`, ngspice's render: the lab's reference. It needs ngspice,
//!   so it is not a `Voice`; its tests are `#[ignore]`d.
//! - `fw`: EXP-C (`Exp2`), the firmware's 2040 structure rebuilt term by
//!   term from the circuit (f32, 1×), its `Offsets` and its degree-7
//!   `tanh_q`.
//! - `takes`: the measures, fits and stimuli the lab read a
//!   recorded Kobol with (the takes stay out of the repo: `KOBOL_DIR`).
//!
//! What shipped instead: the firmware's EXP voice (ADR 0105) kept its own
//! structure and took ADR 0191's make-up gain, fitted to sweep takes the
//! lab later found contaminated (an FM cable and a gain change); EXP-C
//! cost a voice and was never ported. `docs/ssm2040_kobol/` holds the
//! research, the validation and the open questions.
//!
//! Provenance: the RSF schematic and parts lists, the SSM 2040 and CA3080
//! data sheets, RCA AN6668, US 3,969,682 (facts only, cited), our own
//! code. ngspice is a lab tool and never ships.

pub mod circuit;
pub mod fw;
pub mod takes;

#[cfg(test)]
mod tests;

use crate::{Controls, Entry, FS, Voice};
use fw::{Exp2, K_MAX, Offsets};

/// RES as EXP-C reads it, to CI5's loop gain: the 3080 linear in its CV,
/// 0 → the Kobol's top (k 5.59). Onset sits at RES ≈ 0.72.
pub fn k_of(res: f64) -> f64 {
    f64::from(K_MAX) * res
}

/// EXP-C as the A/B rendered it: straight on the input, no padding (C6's
/// coupling is inside the model).
struct ExpC(Offsets);

impl Voice for ExpC {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        Exp2::new(self.0, FS as f32).render(x, &|i| c[i].cutoff, &|i| k_of(c[i].res))
    }
}

pub fn models() -> Vec<Entry> {
    vec![
        Entry {
            name: "ssm2040_kobol/exp_c",
            about: "EXP-C: the Kobol's 2040 + CA3080 loop in the firmware's structure, the owner's unit's offsets (f32, 1x): the A/B's",
            make: || Box::new(ExpC(Offsets::UNIT)),
        },
        Entry {
            name: "ssm2040_kobol/exp_c_typical",
            about: "EXP-C on the data sheets' typical offsets (the reference netlist's chip)",
            make: || Box::new(ExpC(Offsets::REFERENCE)),
        },
    ]
}
