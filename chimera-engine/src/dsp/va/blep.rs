//! 2-point polyBLEP and polyBLAMP kernels and the `Corrector` that applies
//! them (spec § Oscillator antialiasing). A better kernel (4-point
//! polyBLAMP, Hohnerlein et al., DAFx-17) replaces `blep`, `blamp` and
//! `LATENCY` here and nothing else.

use crate::dsp::provenance::{Patent, PatentLine, Provenance};

pub const PROVENANCE: &[Provenance] = &[
    Provenance::Textbook {
        cite: "Valimaki & Huovilainen, \"Antialiasing oscillators in subtractive synthesis\", \
               IEEE SPM 24(2), 2007 (2-point polyBLEP)",
    },
    Provenance::Textbook {
        cite: "Esqueda, Valimaki & Bilbao, \"Aliasing reduction in soft-clipping algorithms\", \
               EUSIPCO 2015 (2-point polyBLAMP)",
    },
    Provenance::Textbook {
        cite: "Esqueda, Valimaki & Bilbao, \"Rounding Corners with BLAMP\", DAFx-16, 2016 \
               (polyBLAMP at corners)",
    },
];

pub const PATENT: &[PatentLine] = &[
    PatentLine {
        technique: "2-point polyBLEP",
        patent: Patent::NotChecked,
    },
    PatentLine {
        technique: "2-point polyBLAMP",
        patent: Patent::NotChecked,
    },
];

pub const GPL_CHECK: &str = "clean room: papers [3][4][5] only; no GPL source read";

/// Step residuals for the sample before and the sample after an edge `t`
/// samples (0..1) before the later one.
#[inline]
pub fn blep(t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    [0.5 * t * t, -0.5 * u * u]
}

/// Slope-jump residuals, same convention as `blep`.
#[inline]
pub fn blamp(t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    [t * t * t / 6.0, u * u * u / 6.0]
}

#[cfg(any(test, feature = "test-support"))]
const EDGE_LOG: usize = 8;

/// Holds the previous naive sample so an edge can correct both sides.
#[derive(Clone, Copy, Debug)]
pub struct Corrector {
    held: f32,
    acc: f32,
    #[cfg(any(test, feature = "test-support"))]
    edges: [(f32, f32, f32); EDGE_LOG],
    #[cfg(any(test, feature = "test-support"))]
    n: usize,
}

impl Corrector {
    pub const LATENCY: usize = 1;

    pub const NEW: Self = Self {
        held: 0.0,
        acc: 0.0,
        #[cfg(any(test, feature = "test-support"))]
        edges: [(0.0, 0.0, 0.0); EDGE_LOG],
        #[cfg(any(test, feature = "test-support"))]
        n: 0,
    };

    /// An edge `t` samples before the next `push`'s sample: `step` is the
    /// output jump, `slope` the jump in output per sample. Corrections
    /// superpose.
    #[inline]
    pub fn edge(&mut self, t: f32, step: f32, slope: f32) {
        debug_assert!((0.0..=1.0).contains(&t));
        let (e, r) = (blep(t), blamp(t));
        self.held += step * e[0] + slope * r[0];
        self.acc += step * e[1] + slope * r[1];
        #[cfg(any(test, feature = "test-support"))]
        if self.n < EDGE_LOG {
            self.edges[self.n] = (t, step, slope);
            self.n += 1;
        }
    }

    /// The corrected sample, `LATENCY` samples late.
    #[inline]
    pub fn push(&mut self, naive: f32) -> f32 {
        let out = self.held;
        self.held = naive + self.acc;
        self.acc = 0.0;
        #[cfg(any(test, feature = "test-support"))]
        {
            self.n = 0;
        }
        out
    }

    pub fn clear(&mut self) {
        *self = Self::NEW;
    }

    /// The `(t, step, slope)` passed since the last `push`, up to 8.
    #[cfg(any(test, feature = "test-support"))]
    pub fn last_edges(&self) -> &[(f32, f32, f32)] {
        &self.edges[..self.n]
    }
}
