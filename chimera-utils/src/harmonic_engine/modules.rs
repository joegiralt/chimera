//! The module chain's six kinds, each with the design's law, at block rate.
//! Cells are generic (three knobs in 0..1 a slot); `Module` is the typed
//! view built from them: a knob belongs to its kind.
//!
//! TILT, ODD/EVEN, FORMANT and COMB are multiplicative masks, so slot order
//! never matters. STRETCH moves frequencies (B's of several slots add),
//! PLUCK decay rates (several slots add their rates).

use crate::drums::parts::expo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleKind {
    None,
    Tilt,
    OddEven,
    Formant,
    Stretch,
    Comb,
    Pluck,
}

impl ModuleKind {
    pub fn name(self) -> &'static str {
        match self {
            ModuleKind::None => "NONE",
            ModuleKind::Tilt => "TILT",
            ModuleKind::OddEven => "ODD/EVEN",
            ModuleKind::Formant => "FORMANT",
            ModuleKind::Stretch => "STRETCH",
            ModuleKind::Comb => "COMB",
            ModuleKind::Pluck => "PLUCK",
        }
    }

    pub fn knobs(self) -> &'static [&'static str] {
        match self {
            ModuleKind::None => &[],
            ModuleKind::Tilt => &["SLOPE", "PIVOT"],
            ModuleKind::OddEven => &["BALANCE"],
            ModuleKind::Formant => &["FREQ", "WIDTH", "PEAKS"],
            ModuleKind::Stretch => &["AMOUNT", "KEY"],
            ModuleKind::Comb => &["SPACING", "OFFSET", "DEPTH"],
            ModuleKind::Pluck => &["DECAY", "SLOPE"],
        }
    }
}

/// STRETCH's largest B: the 2nd partial ~7 % sharp, the 5th near 7.3×.
const B_MAX: f32 = 0.05;
/// Compression: B's floor keeps 1 + 2Bk² > 0 to the 64th (monotonic).
const B_MIN: f32 = -0.9 / (2.0 * 64.0 * 64.0);
const FORMANT_FLOOR: f32 = 0.0316; // −30 dB
const FORMANT_RATIOS: [f32; 3] = [1.0, 1.6, 2.6];
/// KEY's reference: B is full from middle C up.
const KEY_REF_HZ: f32 = 261.63;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Module {
    None,
    /// dB/oct, and the partial number left untouched
    Tilt {
        slope: f32,
        pivot: f32,
    },
    /// −1..1: > 0 thins the evens, < 0 the odds above the 1st
    OddEven {
        balance: f32,
    },
    /// Hz of the first peak, σ in octaves, 1..3 peaks (fractional: the
    /// next peak fades in)
    Formant {
        hz: f32,
        sigma: f32,
        peaks: f32,
    },
    /// the stiff-string B, and KEY 0..1 (how much low notes scale it down)
    Stretch {
        b: f32,
        key: f32,
    },
    /// spacing in partials, offset in cycles, depth 0..1
    Comb {
        spacing: f32,
        offset: f32,
        depth: f32,
    },
    /// T60 of the fundamental in s (`f32::INFINITY` off), slope 0..2
    Pluck {
        decay: f32,
        slope: f32,
    },
}

impl Module {
    /// The typed view of a slot's cells (knobs 0..1).
    pub fn from_cells(kind: ModuleKind, k: [f32; 3]) -> Self {
        let k = k.map(|v| v.clamp(0.0, 1.0));
        let bip = |v: f32| 2.0 * v - 1.0;
        match kind {
            ModuleKind::None => Module::None,
            ModuleKind::Tilt => {
                // curved: fine near flat (knob ½), −12 or +6 dB/oct at the ends
                let u = bip(k[0]);
                let slope = if u < 0.0 { -12.0 * u * u } else { 6.0 * u * u };
                Module::Tilt {
                    slope,
                    pivot: expo(k[1], 1.0, 16.0),
                }
            }
            ModuleKind::OddEven => Module::OddEven { balance: bip(k[0]) },
            ModuleKind::Formant => Module::Formant {
                hz: expo(k[0], 150.0, 5000.0),
                sigma: expo(k[1], 0.08, 1.0),
                peaks: 1.0 + 2.0 * k[2],
            },
            ModuleKind::Stretch => {
                let u = bip(k[0]);
                Module::Stretch {
                    b: if u < 0.0 {
                        B_MIN * u * u
                    } else {
                        B_MAX * u * u
                    },
                    key: k[1],
                }
            }
            ModuleKind::Comb => Module::Comb {
                spacing: expo(k[0], 2.0, 16.0),
                offset: k[1],
                depth: k[2],
            },
            ModuleKind::Pluck => Module::Pluck {
                decay: if k[0] >= 0.999 {
                    f32::INFINITY
                } else {
                    expo(k[0], 0.05, 8.0)
                },
                slope: 2.0 * k[1],
            },
        }
    }

    /// The amplitude mask for partial `k` (1-based harmonic number) at
    /// `hz`, the note's fundamental `f0`. 1 for the non-mask kinds.
    pub fn gain(&self, k: usize, hz: f32, f0: f32) -> f32 {
        match *self {
            Module::Tilt { slope, pivot } => (hz / (pivot * f0)).powf(slope / 6.0206),
            Module::OddEven { balance } => {
                if k.is_multiple_of(2) && balance > 0.0 {
                    1.0 - balance
                } else if k % 2 == 1 && k > 1 && balance < 0.0 {
                    1.0 + balance
                } else {
                    1.0
                }
            }
            Module::Formant {
                hz: fc,
                sigma,
                peaks,
            } => {
                let bumps: f32 = FORMANT_RATIOS
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let w = (peaks - i as f32).clamp(0.0, 1.0);
                        let d = (hz / (fc * r)).log2() / sigma;
                        w * (-0.5 * d * d).exp()
                    })
                    .sum();
                FORMANT_FLOOR + (1.0 - FORMANT_FLOOR) * bumps.min(1.0)
            }
            Module::Comb {
                spacing,
                offset,
                depth,
            } => {
                let c = (core::f32::consts::TAU * (k as f32 / spacing + offset)).cos();
                1.0 - depth * (0.5 + 0.5 * c)
            }
            Module::None | Module::Stretch { .. } | Module::Pluck { .. } => 1.0,
        }
    }

    /// STRETCH's B at the note `f0`, after KEY; 0 for other kinds.
    pub fn stretch_b(&self, f0: f32) -> f32 {
        match *self {
            Module::Stretch { b, key } => b * (f0 / KEY_REF_HZ).min(1.0).powf(2.0 * key),
            _ => 0.0,
        }
    }

    /// PLUCK's decay rate for a partial at `ratio` = f / f0, in 1 / T60
    /// (1/s); 0 for other kinds or PLUCK off.
    pub fn decay_rate(&self, ratio: f32) -> f32 {
        match *self {
            Module::Pluck { decay, slope } if decay.is_finite() => ratio.powf(slope) / decay,
            _ => 0.0,
        }
    }
}

/// The stiff-string ratio of harmonic `k` under `b`, the fundamental held
/// at 1: k·√(1 + b·k²) / √(1 + b).
pub fn stretch_ratio(k: usize, b: f32) -> f32 {
    let k = k as f32;
    k * ((1.0 + b * k * k).max(1e-6) / (1.0 + b)).sqrt()
}
