//! Recipes for the Algo engine's wave tables (ADR 0023). Host-only:
//! `chimera-core`'s build script renders them into flash tables.

use std::f64::consts::{PI, TAU};
use std::fmt::Write;

pub const WAVE_LEN: usize = 256;
pub const MIPS: usize = 8;
/// Mip 0's top harmonic: the table's Nyquist (128) is left out.
pub const MAX_HARMONIC: usize = 127;
const ANALYSIS_LEN: usize = 4096;

#[derive(Clone, Copy)]
pub enum Shape {
    /// One period over phase `0.0..1.0`.
    Time(fn(f64) -> f64),
    /// Sine amplitude of harmonic `k` (1-based).
    Sines(fn(usize) -> f64),
}

#[derive(Clone, Copy)]
pub struct Recipe {
    pub name: &'static str,
    pub shape: Shape,
    pub keeps_dc: bool,
}

fn w2(t: f64) -> f64 {
    (TAU * t).sin().signum() * (1.0 - (TAU * t).cos().abs())
}

fn first_half(t: f64, f: fn(f64) -> f64) -> f64 {
    if t < 0.5 { f(2.0 * t) } else { 0.0 }
}

const fn tx(name: &'static str, f: fn(f64) -> f64) -> Recipe {
    Recipe {
        name,
        shape: Shape::Time(f),
        keeps_dc: true,
    }
}

const fn classic(name: &'static str, shape: Shape) -> Recipe {
    Recipe {
        name,
        shape,
        keeps_dc: false,
    }
}

pub const RECIPES: [Recipe; 16] = [
    tx("W1", |t| (TAU * t).sin()),
    tx("W2", w2),
    tx("W3", |t| (TAU * t).sin().max(0.0)),
    tx("W4", |t| w2(t).max(0.0)),
    tx("W5", |t| first_half(t, |u| (TAU * u).sin())),
    tx("W6", |t| first_half(t, w2)),
    tx("W7", |t| first_half(t, |u| (TAU * u).sin().abs())),
    tx("W8", |t| first_half(t, |u| w2(u).abs())),
    classic(
        "TRI",
        Shape::Time(|t| {
            if t < 0.25 {
                4.0 * t
            } else if t < 0.75 {
                2.0 - 4.0 * t
            } else {
                4.0 * t - 4.0
            }
        }),
    ),
    classic("SAW", Shape::Time(|t| 2.0 * ((t + 0.5) % 1.0) - 1.0)),
    classic("SQR", Shape::Time(|t| if t < 0.5 { 1.0 } else { -1.0 })),
    classic("P25", Shape::Time(|t| if t < 0.25 { 1.0 } else { -1.0 })),
    classic("P12", Shape::Time(|t| if t < 0.125 { 1.0 } else { -1.0 })),
    classic(
        "TSAW",
        Shape::Time(|t| {
            if t < 0.75 {
                -1.0 + t / 0.375
            } else {
                1.0 - (t - 0.75) / 0.125
            }
        }),
    ),
    classic(
        "RSQR",
        Shape::Time(|t| (4.0 * (TAU * t).sin()).tanh() / 4.0f64.tanh()),
    ),
    classic(
        "SSAW",
        Shape::Sines(|k| {
            let sign = if k % 2 == 1 { 1.0 } else { -1.0 };
            sign * 2.0 / (PI * k as f64) * (-(k as f64 - 1.0) / 12.0).exp()
        }),
    ),
];

/// One period's Fourier series: DC, then harmonics `1..=MAX_HARMONIC`.
pub struct Spectrum {
    pub dc: f64,
    pub cos: [f64; MAX_HARMONIC + 1],
    pub sin: [f64; MAX_HARMONIC + 1],
}

pub fn spectrum(shape: Shape) -> Spectrum {
    let mut s = Spectrum {
        dc: 0.0,
        cos: [0.0; MAX_HARMONIC + 1],
        sin: [0.0; MAX_HARMONIC + 1],
    };
    match shape {
        Shape::Sines(amp) => {
            for (k, b) in s.sin.iter_mut().enumerate().skip(1) {
                *b = amp(k);
            }
        }
        Shape::Time(f) => {
            // Sampled at step centres, so a jump on a step edge splits evenly.
            let t = |n: usize| (n as f64 + 0.5) / ANALYSIS_LEN as f64;
            let x: Vec<f64> = (0..ANALYSIS_LEN).map(|n| f(t(n))).collect();
            s.dc = x.iter().sum::<f64>() / ANALYSIS_LEN as f64;
            for k in 1..=MAX_HARMONIC {
                let (mut a, mut b) = (0.0, 0.0);
                for (n, v) in x.iter().enumerate() {
                    let ph = TAU * k as f64 * t(n);
                    a += v * ph.cos();
                    b += v * ph.sin();
                }
                s.cos[k] = 2.0 * a / ANALYSIS_LEN as f64;
                s.sin[k] = 2.0 * b / ANALYSIS_LEN as f64;
            }
        }
    }
    s
}

pub fn mip_harmonics(mip: usize) -> usize {
    (128 >> mip).min(MAX_HARMONIC)
}

pub fn render_mip(s: &Spectrum, keeps_dc: bool, mip: usize) -> [f64; WAVE_LEN] {
    core::array::from_fn(|n| {
        let mut v = if keeps_dc { s.dc } else { 0.0 };
        for k in 1..=mip_harmonics(mip) {
            let ph = TAU * (k * n) as f64 / WAVE_LEN as f64;
            v += s.cos[k] * ph.cos() + s.sin[k] * ph.sin();
        }
        v
    })
}

/// Every mip, scaled by one factor so the loudest sample of any mip is
/// full scale: the level does not change from mip to mip.
pub fn render(r: &Recipe) -> [[i16; WAVE_LEN]; MIPS] {
    let s = spectrum(r.shape);
    let mips: Vec<[f64; WAVE_LEN]> = (0..MIPS).map(|m| render_mip(&s, r.keeps_dc, m)).collect();
    let peak = mips.iter().flatten().fold(0.0f64, |p, v| p.max(v.abs()));
    core::array::from_fn(|m| core::array::from_fn(|n| (mips[m][n] / peak * 32767.0).round() as i16))
}

/// The Rust source `chimera-core` includes: `WAVE_NAMES` and `WAVES`.
pub fn emit_rust() -> String {
    let mut out = String::new();
    let names: Vec<String> = RECIPES.iter().map(|r| format!("{:?}", r.name)).collect();
    let _ = writeln!(
        out,
        "pub static WAVE_NAMES: [&str; {}] = [{}];",
        RECIPES.len(),
        names.join(", ")
    );
    let _ = writeln!(
        out,
        "pub static WAVES: [[[i16; {}]; {MIPS}]; {}] = [",
        WAVE_LEN + 1,
        RECIPES.len()
    );
    for r in &RECIPES {
        let _ = writeln!(out, "    [");
        for mip in render(r) {
            // A guard copy of sample 0 closes the period, so a sample and its
            // successor are always adjacent.
            let row: Vec<String> = mip.iter().chain(&mip[..1]).map(i16::to_string).collect();
            let _ = writeln!(out, "        [{}],", row.join(", "));
        }
        let _ = writeln!(out, "    ],");
    }
    let _ = writeln!(out, "];");
    out
}
