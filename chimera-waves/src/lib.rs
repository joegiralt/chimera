//! Recipes for the Algo engine's wave tables (ADR 0023) and their render.
//! Pure and `no_std`: the firmware renders the tables at boot (ADR 0122);
//! with `emit`, `chimera-engine`'s build script writes them as Rust for
//! the host.

#![no_std]

#[cfg(feature = "emit")]
extern crate std;

use core::f64::consts::{PI, TAU};
use libm::{cos, exp, round, sin, tanh};

pub const WAVE_LEN: usize = 256;
pub const MIPS: usize = 8;
pub const WAVE_COUNT: usize = RECIPES.len();
/// One period and a guard copy of its first sample.
pub type Table = [i16; WAVE_LEN + 1];
pub type Waves = [[Table; MIPS]; WAVE_COUNT];
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
    /// The label the UI shows.
    pub name: &'static str,
    /// The wave's frozen disk ident (ADR 0045): one token, a literal of its
    /// own so a relabel can't move it. Append-only, like the entry.
    pub ident: &'static str,
    pub shape: Shape,
    pub keeps_dc: bool,
}

fn w2(t: f64) -> f64 {
    sin(TAU * t).signum() * (1.0 - cos(TAU * t).abs())
}

fn first_half(t: f64, f: fn(f64) -> f64) -> f64 {
    if t < 0.5 { f(2.0 * t) } else { 0.0 }
}

const fn tx(name: &'static str, ident: &'static str, f: fn(f64) -> f64) -> Recipe {
    Recipe {
        name,
        ident,
        shape: Shape::Time(f),
        keeps_dc: true,
    }
}

const fn classic(name: &'static str, ident: &'static str, shape: Shape) -> Recipe {
    Recipe {
        name,
        ident,
        shape,
        keeps_dc: false,
    }
}

/// Append-only: a recipe's index is its wave's disk code (ADR 0045), frozen
/// with its name in `chimera-core/tests/fixtures/disk_codes_v1.txt`. Never
/// reorder or remove.
pub const RECIPES: [Recipe; 16] = [
    tx("W1", "W1", |t| sin(TAU * t)),
    tx("W2", "W2", w2),
    tx("W3", "W3", |t| sin(TAU * t).max(0.0)),
    tx("W4", "W4", |t| w2(t).max(0.0)),
    tx("W5", "W5", |t| first_half(t, |u| sin(TAU * u))),
    tx("W6", "W6", |t| first_half(t, w2)),
    tx("W7", "W7", |t| first_half(t, |u| sin(TAU * u).abs())),
    tx("W8", "W8", |t| first_half(t, |u| w2(u).abs())),
    classic(
        "TRI",
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
    classic("SAW", "SAW", Shape::Time(|t| 2.0 * ((t + 0.5) % 1.0) - 1.0)),
    classic(
        "SQR",
        "SQR",
        Shape::Time(|t| if t < 0.5 { 1.0 } else { -1.0 }),
    ),
    classic(
        "P25",
        "P25",
        Shape::Time(|t| if t < 0.25 { 1.0 } else { -1.0 }),
    ),
    classic(
        "P12",
        "P12",
        Shape::Time(|t| if t < 0.125 { 1.0 } else { -1.0 }),
    ),
    classic(
        "TSAW",
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
        "RSQR",
        Shape::Time(|t| tanh(4.0 * sin(TAU * t)) / tanh(4.0)),
    ),
    classic(
        "SSAW",
        "SSAW",
        Shape::Sines(|k| {
            let sign = if k % 2 == 1 { 1.0 } else { -1.0 };
            sign * 2.0 / (PI * k as f64) * exp(-(k as f64 - 1.0) / 12.0)
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
            // Sampled at step centres, so a jump on a step edge splits
            // evenly. Steps n and N-1-n mirror about t = 1/2, where every
            // harmonic's cosine is the same and its sine flips: one pass
            // over the first half. Harmonic k's phasor is the fundamental's
            // turned k times: two trig calls per pair, not 508.
            let t = |n: usize| (n as f64 + 0.5) / ANALYSIS_LEN as f64;
            for n in 0..ANALYSIS_LEN / 2 {
                let (v, w) = (f(t(n)), f(t(ANALYSIS_LEN - 1 - n)));
                s.dc += v + w;
                let (even, odd) = (v + w, v - w);
                let (s1, c1) = (sin(TAU * t(n)), cos(TAU * t(n)));
                let (mut sk, mut ck) = (s1, c1);
                for k in 1..=MAX_HARMONIC {
                    s.cos[k] += even * ck;
                    s.sin[k] += odd * sk;
                    (ck, sk) = (ck * c1 - sk * s1, sk * c1 + ck * s1);
                }
            }
            s.dc /= ANALYSIS_LEN as f64;
            for k in 1..=MAX_HARMONIC {
                s.cos[k] = 2.0 * s.cos[k] / ANALYSIS_LEN as f64;
                s.sin[k] = 2.0 * s.sin[k] / ANALYSIS_LEN as f64;
            }
        }
    }
    s
}

pub fn mip_harmonics(mip: usize) -> usize {
    (128 >> mip).min(MAX_HARMONIC)
}

/// One period's cosine at each table step.
fn cosines() -> [f64; WAVE_LEN] {
    core::array::from_fn(|j| cos(TAU * j as f64 / WAVE_LEN as f64))
}

fn mip_sample(
    s: &Spectrum,
    keeps_dc: bool,
    mip: usize,
    n: usize,
    cosines: &[f64; WAVE_LEN],
) -> f64 {
    let mut v = if keeps_dc { s.dc } else { 0.0 };
    for k in 1..=mip_harmonics(mip) {
        let j = k * n % WAVE_LEN;
        // sin x = cos(x - quarter period).
        let (c, sn) = (cosines[j], cosines[(j + 3 * WAVE_LEN / 4) % WAVE_LEN]);
        v += s.cos[k] * c + s.sin[k] * sn;
    }
    v
}

/// Every mip, scaled by one factor so the loudest sample of any mip is
/// full scale: the level does not change from mip to mip. Each table ends
/// with a guard copy of sample 0, so a sample and its successor are always
/// adjacent.
pub fn render_into(r: &Recipe, out: &mut [Table; MIPS]) {
    let s = spectrum(r.shape);
    let cosines = cosines();
    let at = |m, n| mip_sample(&s, r.keeps_dc, m, n, &cosines);
    let mut peak = 0.0f64;
    for m in 0..MIPS {
        for n in 0..WAVE_LEN {
            peak = peak.max(at(m, n).abs());
        }
    }
    for (m, table) in out.iter_mut().enumerate() {
        for (n, v) in table[..WAVE_LEN].iter_mut().enumerate() {
            *v = round(at(m, n) / peak * 32767.0) as i16;
        }
        table[WAVE_LEN] = table[0];
    }
}

/// Every wave's tables, in `RECIPES` order.
pub fn render_all(out: &mut Waves) {
    for (r, w) in RECIPES.iter().zip(out) {
        render_into(r, w);
    }
}

/// `render_into` by value, for tests and tools.
pub fn render(r: &Recipe) -> [Table; MIPS] {
    let mut out = [[0; WAVE_LEN + 1]; MIPS];
    render_into(r, &mut out);
    out
}

/// The Rust source `chimera-engine` includes: `WAVE_NAMES`, `WAVE_IDENTS`
/// and, with `tables`, `WAVES`.
#[cfg(feature = "emit")]
pub fn emit_rust(tables: bool) -> std::string::String {
    use core::fmt::Write;
    use std::format;
    use std::string::{String, ToString};
    use std::vec::Vec;

    let mut out = String::new();
    let names: Vec<String> = RECIPES.iter().map(|r| format!("{:?}", r.name)).collect();
    let idents: Vec<String> = RECIPES.iter().map(|r| format!("{:?}", r.ident)).collect();
    let _ = writeln!(
        out,
        "pub static WAVE_IDENTS: [&str; {}] = [{}];",
        RECIPES.len(),
        idents.join(", ")
    );
    // Names that match the idents share their table: one copy in flash.
    let _ = if names == idents {
        writeln!(
            out,
            "pub static WAVE_NAMES: &[&str; {}] = &WAVE_IDENTS;",
            RECIPES.len()
        )
    } else {
        writeln!(
            out,
            "pub static WAVE_NAMES: &[&str; {}] = &[{}];",
            RECIPES.len(),
            names.join(", ")
        )
    };
    if !tables {
        return out;
    }
    let _ = writeln!(
        out,
        "pub static WAVES: [[[i16; {}]; {MIPS}]; {}] = [",
        WAVE_LEN + 1,
        RECIPES.len()
    );
    for r in &RECIPES {
        let _ = writeln!(out, "    [");
        for mip in render(r) {
            let row: Vec<String> = mip.iter().map(i16::to_string).collect();
            let _ = writeln!(out, "        [{}],", row.join(", "));
        }
        let _ = writeln!(out, "    ],");
    }
    let _ = writeln!(out, "];");
    out
}
