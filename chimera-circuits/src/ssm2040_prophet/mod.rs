//! The Prophet-5 rev 1/2 filter at component level (#711, branch
//! `circuit-2040`): an SSM 2040 wired as the Prophet's Voice schematic SD411
//! (tech manual, Jan 1980).
//!
//! Stage 1's input node takes the mixer bus through R476 200 K (the bus sits
//! behind R460 100 K, so 300 K in all), its own output through R480 10 K and
//! R475 200 Ω to ground; stages 2–4 are 10 K/10 K/200 Ω; 1 nF caps. U479
//! (TL082) turns OUT 4 into −10 × through R461 10 K/R478 100 K; half an SSM
//! 2020 (U490, "RES FDBK VCA") turns that back into a current into stage 1's
//! node. Each stage is the data sheet's OTA, `I = I_c·tanh((V₊ − V₋)/2V_T)`
//! on the attenuated node, into its cap, then a unity buffer. Offsets, the
//! ±1 V swing (data sheet hint a) and the 2020's mild bend are modelled as
//! noted on `Circuit`.
//!
//! - `reference`: the parts, ngspice's netlist (the lab's reference) and
//!   `Model`, the same node equations by TR-BDF2 and Newton, oversampled.
//! - `fw`: `Fast`, the firmware-shaped candidates (`d` at 2×, `e` at 1×).
//!
//! What shipped instead: today's REV2 stayed (the circuit measured the same
//! at the Prophet's own level, and `e` cost a voice); the per-voice spread
//! the lab heard on chords shipped as STABILITY (ADR 0168).
//! Provenance: data sheets and the schematic, Zavalishin (TPT), our own
//! code; `docs/ssm2040_prophet/`. ngspice is a lab tool and never ships.

pub mod fw;
pub mod reference;

#[cfg(test)]
mod tests;

use crate::util::harness::{Case, ac, pad_run};
use crate::{Controls, Entry, Voice};
use fw::{Fast, Shaper};
use reference::{Circuit, Model};

/// RES as Chimera reads it, to the loop gain: the same law on all three.
pub fn k_of(res: f64) -> f64 {
    res * 4.0 / 0.95
}

/// A Prophet render as the lab heard it: padded at rest, through the
/// circuit's 15.9 Hz output coupling (C413/R471).
fn prophet(x: &[f64], c: &[Controls], f: impl FnOnce(&Case) -> Vec<f64>) -> Vec<f64> {
    ac(&pad_run(&Case::of(x, c), |c| Ok(f(c))).unwrap_or_default())
}

struct Reference;

impl Voice for Reference {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        prophet(x, c, |c| Model::new(Circuit::PROPHET, 2, 8).render(c))
    }
}

struct Candidate(usize, Shaper);

impl Voice for Candidate {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        prophet(x, c, |c| {
            let mut f = Fast::new(&Circuit::PROPHET, self.0);
            f.shaper = self.1;
            f.render(c)
        })
    }
}

pub fn models() -> Vec<Entry> {
    vec![
        Entry {
            name: "ssm2040_prophet/reference",
            about: "Prophet-5 rev 1/2 SSM 2040 + SSM 2020 loop, Newton at 2× (f64): ngspice's twin",
            make: || Box::new(Reference),
        },
        Entry {
            name: "ssm2040_prophet/d",
            about: "candidate d: REV2's structure on the circuit's numbers, 2×, Padé tanh (f32)",
            make: || Box::new(Candidate(2, Shaper::Pade)),
        },
        Entry {
            name: "ssm2040_prophet/e",
            about: "candidate e: the same at 1× with the divide-free quintic tanh (f32), the one near budget",
            make: || Box::new(Candidate(1, Shaper::Quintic)),
        },
    ]
}
