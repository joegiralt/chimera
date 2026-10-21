//! The CEM3320 from its own circuit, not from the 2040, wired as the
//! Oberheim OB-8's 4-pole (#720, branch `cem3320-lab`, 19c87c8e).
//!
//! The cell is Curtis's ganged log/antilog multiplier (US 4,004,141 fig. 3,
//! the data sheet's "current-in, current-out" cell): each stage's input pin
//! is a diode to ground (the log side); a transistor whose base sits on that
//! diode and whose emitter shares the exponential converter's node carries
//! `G·I_D` (the antilog side); a pnp mirror of the grounded-base reference
//! transistor sources `G·I_REF` into the same collector node, the pole cap
//! and the buffer. So `I_OUT = G·(I_REF − I_D)` with `I_D ≥ 0`: linear in
//! current, starving one way. The resonance cell is the data sheet's
//! "traditional transconductance" pair, here degenerated (fitted to fig. 6),
//! its tail the pin-9 current, its output into stage 1's diode node.
//!
//! - `chip`: `Chip` (`OB8`, `DATASHEET`), its maps, the device functions.
//! - `reference`: that circuit, Shockley/Ebers–Moll devices, trapezoidal
//!   caps, Newton on all 13 node voltages per sample in f64, 8×
//!   (`render_ref`).
//! - `fw`: `Fw`, the same equations cut to what an M7 can run at 1×.
//! - `stimuli`, `measure`: the lab's phrases, cases and measures.
//!
//! What shipped: nothing. The lab found the chip nulls with today's 2040
//! REV2 at playing levels; what differs (the louder asymmetric ring, the
//! OB-8's CX16 squeal at high cutoff, the lopsided clip) was left as a
//! product call (`docs/cem3320_ob8/`). Provenance: the CES data sheet
//! (1980), US 4,004,141, the OB-8 service manual (voice board sheet 3,
//! 1682B); textbook device equations, Zavalishin (TPT); our own code.

pub mod chip;
pub mod fw;
pub mod measure;
pub mod reference;
pub mod stimuli;

#[cfg(test)]
mod tests;

use crate::util::harness::{Case, ac, pad_run};
use crate::{Controls, Entry, Voice};
use chip::Chip;
use fw::Fw;
use reference::{OS_REF, render_ref};

/// RES → loop gain, the house law on every filter the labs heard.
pub fn k_of(res: f64) -> f64 {
    crate::ssm2040_prophet::k_of(res)
}

/// A render as the lab heard it: the OB-8 chip, padded at rest, through the
/// house 15.9 Hz output blocker.
fn ob8(x: &[f64], c: &[Controls], f: impl FnOnce(&Chip, &Case) -> Vec<f64>) -> Vec<f64> {
    ac(&pad_run(&Case::of(x, c), |c| Ok(f(&Chip::OB8, c))).unwrap_or_default())
}

struct Reference;

impl Voice for Reference {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        ob8(x, c, |ch, c| render_ref(ch, c, OS_REF, 1.0))
    }
}

struct Firmware(usize);

impl Voice for Firmware {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        ob8(x, c, |ch, c| Fw::new(self.0).render(ch, c))
    }
}

pub fn models() -> Vec<Entry> {
    vec![
        Entry {
            name: "cem3320_ob8/reference",
            about: "OB-8 4-pole CEM3320 from US 4,004,141's cell, 13-node Newton at 8× (f64), slow",
            make: || Box::new(Reference),
        },
        Entry {
            name: "cem3320_ob8/fw",
            about: "the firmware candidate: the same cell at 1×, closed-form stages, polynomial OTA (f32)",
            make: || Box::new(Firmware(1)),
        },
        Entry {
            name: "cem3320_ob8/fw2x",
            about: "the firmware candidate at 2×: the lab's check that 1× is enough (within 0.1 dB)",
            make: || Box::new(Firmware(2)),
        },
    ]
}
