// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Chimera's circuit models, kept (ADR 0196): every filter and gate the
//! circuit labs built, shipped or not, by circuit. Each module holds the
//! f64 circuit or reference solver, the firmware-shaped model derived from
//! it, and its validation as tests; `docs/<circuit>/` holds the lab's
//! research and notes.
//!
//! Host only. No firmware crate depends on this one; the models need
//! nothing of Chimera's, so they can move to a bigger processor or a
//! plugin as they are. `chimera-utils circuit render` auditions any of
//! them (see `README.md`).

// The workspace bans float `clamp` for the firmware's flash (it links float
// formatting); this crate never ships in it.
#![allow(clippy::disallowed_methods)]

pub mod util;

pub mod buchla292b;
pub mod cem3320_ob8;
pub mod plaits_lpg;
pub mod sem1a;
pub mod ssm2040_kobol;
pub mod ssm2040_prophet;
pub mod vtl5c3;

/// The one rate the models' 1× versions run at: Chimera's.
pub const FS: f64 = 48_000.0;

/// What a harness hands a model at each sample: the knobs as the synth's
/// filter page has them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Controls {
    /// CUTOFF, Hz. A gate with no cutoff knob of its own (an LPG) reads it
    /// as its offset: how far open it rests.
    pub cutoff: f64,
    /// RES, the knob's 0..1.
    pub res: f64,
    /// The note's gate: 0 between notes, else its velocity, 0..1. A filter
    /// ignores it; an LPG strikes on its rise.
    pub gate: f64,
}

/// A model run offline, the way every lab rendered: the whole input at
/// once, so an oversampled reference can resample it in one go.
pub trait Voice: Send {
    /// `x` at 48 kHz (1.0 = the model's full scale), `c[i]` the controls at
    /// sample `i` (as long as `x`); the output at 48 kHz, as long as `x`,
    /// at the model's own level: never normalised.
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64>;
}

/// One model the render tool can build.
pub struct Entry {
    /// Its name on the command line: `<circuit>/<model>`.
    pub name: &'static str,
    /// One line: what it is and what it was for.
    pub about: &'static str,
    pub make: fn() -> Box<dyn Voice>,
}

/// Every model that renders as a `Voice`, by circuit.
pub fn models() -> Vec<Entry> {
    [
        ssm2040_prophet::models(),
        ssm2040_kobol::models(),
        cem3320_ob8::models(),
        sem1a::models(),
        buchla292b::models(),
        vtl5c3::models(),
        plaits_lpg::models(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The model named `name`, built.
pub fn make(name: &str) -> Option<Box<dyn Voice>> {
    models()
        .into_iter()
        .find(|e| e.name == name)
        .map(|e| (e.make)())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_has_a_unique_name_and_renders_finite() {
        let all = models();
        for (i, e) in all.iter().enumerate() {
            assert!(
                all[..i].iter().all(|o| o.name != e.name),
                "{} twice",
                e.name
            );
            let c = [Controls {
                cutoff: 1000.0,
                res: 0.5,
                gate: 1.0,
            }; 4800];
            let x: Vec<f64> = (0..4800)
                .map(|i| if i % 100 < 50 { 0.5 } else { -0.5 })
                .collect();
            let y = (e.make)().render(&x, &c);
            assert_eq!(y.len(), x.len(), "{}", e.name);
            assert!(y.iter().all(|s| s.is_finite()), "{} blew up", e.name);
            assert!(y.iter().any(|s| *s != 0.0), "{} is silent", e.name);
        }
    }
}
