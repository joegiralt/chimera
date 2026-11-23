// After L. Köper, M. Holters, F. Esqueda, J. D. Parker, DAFx20in22 (CC BY 4.0; see THIRD_PARTY.md)
// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Oberheim SEM-1A VCF (#720): a 12 dB OTA state-variable with a
//! diode-damped resonance and a LP–notch–HP MODE pot. `docs/sem1a/` holds
//! the labs' research, notes and validation tables.
//!
//! - `reference`: the VCF sheet as built, one nodal system of 15 unknowns,
//!   Newton in f64; run at ×8 (`sem-circuit-lab`).
//! - `fw`: the firmware model derived from it, f32 at 48 kHz; shipped as
//!   filter KIND SEM (ADR 0192). `export` reproduces the shipped constants
//!   and fixtures.
//! - `validate`: the schematic's algebra for `k` and the small-signal
//!   measures the reference is checked with.
//! - `ota_svf`: the first lab's simplified equations (`sem-lab`), superseded
//!   by the circuit model, kept.
//! - `stimuli`: both labs' listening set and probes.
//!
//! The slow checks (×8 sweeps, the A/B sets) are `#[ignore]`d: run them
//! with `cargo test -p chimera-circuits --release -- --ignored`.

pub mod export;
pub mod fw;
pub mod ota_svf;
pub mod reference;
pub mod stimuli;
pub mod validate;

use crate::util::measure::{bands, db, rms_db, tone};
use crate::{Controls, Entry, Voice};

/// The engine's block (`chimera_hal::BLOCK_SIZE`): where the labs read
/// the controls and ramped them across.
pub const BLOCK: usize = 64;

/// The reference's oversampling: ×8, 384 kHz.
pub const OS: usize = 8;

/// The circuit models' case from the harness's controls: LP (MODE 0),
/// LEVEL 0 dB.
fn fw_case(x: &[f64], c: &[Controls]) -> fw::Case {
    fw::Case {
        x: x.to_vec(),
        fc: c.iter().map(|c| c.cutoff).collect(),
        res: c.iter().map(|c| c.res).collect(),
        morph: vec![0.0; x.len()],
        bp: false,
        drive: 1.0,
    }
}

/// The first lab's case: LP, LEVEL 0 dB, on `floor`.
fn old_case(x: &[f64], c: &[Controls], floor: f64) -> ota_svf::Case {
    ota_svf::Case {
        x: x.to_vec(),
        fc: c.iter().map(|c| c.cutoff).collect(),
        res: c.iter().map(|c| c.res).collect(),
        morph: vec![0.0; x.len()],
        bp: false,
        drive: 1.0,
        floor,
    }
}

struct Reference;
impl Voice for Reference {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        fw::render_ref(&reference::Parts::default(), &fw_case(x, c), OS)
    }
}

struct Fw {
    exact: bool,
}
impl Voice for Fw {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        fw::render(&reference::Parts::default(), &fw_case(x, c), self.exact)
    }
}

struct OtaSvf {
    floor: f64,
}
impl Voice for OtaSvf {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        ota_svf::Sem::new().render(&old_case(x, c, self.floor))
    }
}

struct OtaSvfRef;
impl Voice for OtaSvfRef {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        ota_svf::Model::new(4).render(&old_case(x, c, ota_svf::K_MIN))
    }
}

pub fn models() -> Vec<Entry> {
    vec![
        Entry {
            name: "sem1a/reference",
            about: "SEM-1A VCF as built: 15-node Newton at x8, with the C15/C23 couplings; MODE at LP (slow)",
            make: || Box::new(Reference),
        },
        Entry {
            name: "sem1a/fw",
            about: "the firmware model derived from the reference, shipped as KIND SEM (ADR 0192), with the couplings; MODE at LP",
            make: || Box::new(Fw { exact: false }),
        },
        Entry {
            name: "sem1a/fw_exact",
            about: "fw with the followers folded into the solve (a divide a sample): measured no better, not shipped; MODE at LP",
            make: || Box::new(Fw { exact: true }),
        },
        Entry {
            name: "sem1a/ota_svf",
            about: "the first SEM lab's candidate: OTA SVF, CLEAN's RES law, invented Q 25 floor, 5 V a unit; superseded; MODE at LP",
            make: || {
                Box::new(OtaSvf {
                    floor: ota_svf::K_MIN,
                })
            },
        },
        Entry {
            name: "sem1a/ota_svf_osc",
            about: "ota_svf on its oscillating floor (k -0.06, held by the diodes): the self-oscillating SEM the circuit is not; MODE at LP",
            make: || {
                Box::new(OtaSvf {
                    floor: ota_svf::K_OSC,
                })
            },
        },
        Entry {
            name: "sem1a/ota_svf_ref",
            about: "the first lab's own reference: ota_svf's equations at x4 by Newton (circular, not the circuit); MODE at LP",
            make: || Box::new(OtaSvfRef),
        },
    ]
}

// ------------------------------------------------------------- measures

/// Mean |Δ| of third-octave levels over bands within 50 dB of the loudest
/// of `r`, after matching the two in RMS: the labs' LSD (the crate's
/// `measure::lsd` reads raw levels).
pub fn lsd_matched(x: &[f64], r: &[f64]) -> f64 {
    let off = rms_db(r) - rms_db(x);
    let (bx, br) = (bands(x), bands(r));
    let top = br.iter().cloned().fold(f64::MIN, f64::max);
    let d: Vec<f64> = bx
        .iter()
        .zip(&br)
        .filter(|(_, b)| **b > top - 50.0)
        .map(|(a, b)| (a + off - b).abs())
        .collect();
    d.iter().sum::<f64>() / d.len().max(1) as f64
}

/// Least-squares gain of `x` onto `r`, then the residual against `r`, dB.
pub fn null_ls(x: &[f64], r: &[f64]) -> f64 {
    let xr: f64 = x.iter().zip(r).map(|(a, b)| a * b).sum();
    let xx: f64 = x.iter().map(|a| a * a).sum();
    let g = xr / xx.max(1e-30);
    let d: Vec<f64> = x.iter().zip(r).map(|(a, b)| g * a - b).collect();
    rms_db(&d) - rms_db(r)
}

/// H1 in dB with `drive` divided out (the gain), H2 and H3 in dB re H1,
/// and THD (H2–H9) in %.
pub fn harm(y: &[f64], f: f64, drive: f64) -> (f64, f64, f64, f64) {
    let h1 = tone(y, f);
    let h = |k: f64| tone(y, f * k);
    let all: f64 = (2..=9).map(|k| h(k as f64).powi(2)).sum();
    (
        db(h1 / drive),
        db(h(2.0) / h1),
        db(h(3.0) / h1),
        100.0 * all.sqrt() / h1,
    )
}

/// The second half of `y`: what the labs' sine probes measured.
pub fn tail(y: &[f64]) -> &[f64] {
    &y[y.len() / 2..]
}

/// `f` over `items` on up to eight threads, in order.
#[cfg(test)]
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = items.len().div_ceil(8).max(1);
    std::thread::scope(|s| {
        let hs: Vec<_> = items
            .chunks(n)
            .map(|ch| s.spawn(|| ch.iter().map(&f).collect::<Vec<R>>()))
            .collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `BLOCK` is the engine's block.
    #[test]
    fn block_is_the_engines() {
        assert_eq!(BLOCK, chimera_hal::BLOCK_SIZE);
    }

    /// Every registered model renders a short saw finite and at a sane
    /// level: LP at 2 kHz, RES ½ passes a 110 Hz saw near unity.
    #[test]
    fn models_render() {
        let x = super::stimuli::saw(110.0, 4800, 0.0);
        let c = vec![
            Controls {
                cutoff: 2000.0,
                res: 0.5,
                gate: 1.0,
            };
            x.len()
        ];
        for e in models().iter().filter(|e| e.name != "sem1a/reference") {
            let y = (e.make)().render(&x, &c);
            let l = rms_db(&y[2400..]) - rms_db(&x[2400..]);
            assert!(l.abs() < 3.0, "{}: {l:.1} dB", e.name);
        }
    }
}
