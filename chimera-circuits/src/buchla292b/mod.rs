//! The Buchla 292B low-pass gate as a circuit. From the lpg-circuit lab
//! (`lpg_circuit.rs`, `lpg_gate.rs`); `docs/lpg/` has the research, the
//! validation tables and the gate checks.
//!
//! Two models of the same circuit:
//! - `reference::run_ref`: f64, 8× oversampled, nodal Newton each step.
//!   The 292B's audio path and LED driver (Buchla Associates drawing
//!   B21208, 10/12/70, rev. 7-76) with the 292C's third switch position
//!   (COMBO); a VTL5C3 vactrol (`crate::vtl5c3`, Najnudel et al., DAFx-23).
//! - `fw::Fw`: f32 at 48 kHz, the shape a firmware voice could run: the same
//!   driver and vactrol ODE once every 16 samples, the audio core as Parker
//!   & D'Angelo's topology-preserving trapezoidal solve (DAFx-13).
//!
//! As voices: a gate rise strikes (the gate checks' 1 ms/10 ms ping × 15 V
//! × velocity into CV), CUTOFF sets OFFSET (the volts whose settled −3 dB
//! is CUTOFF, 0–15 V), RES is ignored (the 292B has none: Q1's fixed
//! follower gain is its "a"). Default mode FILT (LOWPASS), the 292B's own;
//! the cell the VTL5C3, as the lab settled on.

pub mod core;
pub mod driver;
pub mod fw;
pub mod parts;
pub mod reference;
pub mod stim;
#[cfg(test)]
mod tests;

use crate::vtl5c3::{Light, VTL5C3};
use crate::{Controls, Entry, Voice};
use parts::{B292, Mode};

/// Volts at AUD IN for a digital 1.0 (a 5 V p-p modular level; our choice).
pub const VREF: f64 = 2.5;
/// CV volts for a strike at velocity 1 (a Buchla envelope's 15 V).
pub const CV_FS: f64 = 15.0;

/// CUTOFF → OFFSET: the settled −3 dB at each `driver::static_table` volt.
struct OffsetMap {
    /// (volts, ln −3 dB), the −3 dB never falling.
    v: Vec<(f64, f64)>,
}

impl OffsetMap {
    fn new(mode: Mode, light: &Light) -> Self {
        let a = core::Core::follower_gain(&B292);
        let mut top = f64::MIN;
        let v = driver::static_table()
            .iter()
            .enumerate()
            .map(|(k, &i)| {
                let f = core::f3(&B292, mode, light.r_static(i), a).ln();
                top = top.max(f);
                (k as f64 * driver::TABLE_DV, top)
            })
            .collect();
        OffsetMap { v }
    }

    /// The lowest OFFSET whose −3 dB reaches `hz` (log-interpolated).
    fn volts(&self, hz: f64) -> f64 {
        let l = hz.max(1.0).ln();
        match self.v.iter().position(|&(_, f)| f >= l) {
            None => self.v[self.v.len() - 1].0,
            Some(0) => 0.0,
            Some(k) => {
                let ((v0, f0), (v1, f1)) = (self.v[k - 1], self.v[k]);
                if f1 > f0 {
                    v0 + (v1 - v0) * (l - f0) / (f1 - f0)
                } else {
                    v1
                }
            }
        }
    }
}

/// CV (volts, the strikes) and OFFSET (volts, from CUTOFF), a sample each.
fn cv_off(map: &OffsetMap, c: &[Controls]) -> (Vec<f64>, Vec<f64>) {
    let cv = stim::strikes(c).into_iter().map(|s| s * CV_FS).collect();
    let mut last = (f64::NAN, 0.0);
    let off = c
        .iter()
        .map(|c| {
            if c.cutoff != last.0 {
                last = (c.cutoff, map.volts(c.cutoff));
            }
            last.1
        })
        .collect();
    (cv, off)
}

struct Reference {
    mode: Mode,
    light: Light,
    map: OffsetMap,
}

impl Voice for Reference {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        let (cv, off) = cv_off(&self.map, c);
        reference::run_ref(B292, self.mode, &self.light, x, &cv, &off, 1.0, None)
    }
}

struct Firmware {
    fw: fw::Fw,
    map: OffsetMap,
}

impl Voice for Firmware {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        let (cv, off) = cv_off(&self.map, c);
        let xf: Vec<f32> = x.iter().map(|&v| v as f32).collect();
        self.fw
            .run(&xf, &cv, &off, 1.0)
            .into_iter()
            .map(f64::from)
            .collect()
    }
}

fn reference(mode: Mode) -> Box<dyn Voice> {
    let light = Light::new(VTL5C3);
    let map = OffsetMap::new(mode, &light);
    Box::new(Reference { mode, light, map })
}

fn firmware(mode: Mode) -> Box<dyn Voice> {
    let light = Light::new(VTL5C3);
    let map = OffsetMap::new(mode, &light);
    Box::new(Firmware {
        fw: fw::Fw::new(&B292, mode, &light),
        map,
    })
}

pub fn models() -> Vec<Entry> {
    vec![
        Entry {
            name: "buchla292b/reference",
            about: "292B circuit, f64 8x Newton, FILT (LOWPASS, the default), VTL5C3; gate rise strikes, CUTOFF sets OFFSET",
            make: || reference(Mode::Lowpass),
        },
        Entry {
            name: "buchla292b/reference-gate",
            about: "292B circuit reference in GATE (the 15k shunt: a VCA that closes)",
            make: || reference(Mode::Gate),
        },
        Entry {
            name: "buchla292b/reference-combo",
            about: "292B circuit reference in the 292C's COMBO (-6 dB/oct, leaks -20 dB shut)",
            make: || reference(Mode::Combo),
        },
        Entry {
            name: "buchla292b/fw",
            about: "292B firmware shape: f32 48 kHz TPT core, driver + vactrol every 16, FILT (LOWPASS), VTL5C3",
            make: || firmware(Mode::Lowpass),
        },
        Entry {
            name: "buchla292b/fw-gate",
            about: "292B firmware shape in GATE",
            make: || firmware(Mode::Gate),
        },
        Entry {
            name: "buchla292b/fw-combo",
            about: "292B firmware shape in the 292C's COMBO",
            make: || firmware(Mode::Combo),
        },
    ]
}
