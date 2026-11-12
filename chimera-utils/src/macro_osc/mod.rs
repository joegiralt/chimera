//! Macro oscillator KINDs for VA (design note 2026-11-12): host prototypes
//! of HARMONIC, SWARM and PARTICLE, after Mutable Instruments Plaits'
//! macro oscillator (MIT). Our own code from its published structure and
//! behaviour; no code, tables or constants copied. Each model takes
//! HARMONICS, TIMBRE and MORPH in 0..1 and a pitch, and plays one voice's
//! oscillator: no filter, no amp but the harness's envelope.
//!
//! `macro-render OUTDIR`: per model, the listening phrases (sustained C3,
//! a three-note stab, a four-note bass line, a slow pad swell) at three
//! named settings, and one 4 s sweep per control (the others at ½). Mono
//! 48 kHz 24-bit WAVs at their real levels, never normalised: one fixed
//! gain per model (`Kind::gain`) leaves at least −6 dBFS of headroom on
//! its loudest render. `manifest.json` beside them, with a host cost estimate.

mod harmonic;
mod particle;
mod swarm;

use crate::drums::parts::{self, B, Blk, FS};
use crate::reference::write_pcm24;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Instant;

pub use harmonic::Harmonic;
pub use particle::Particle;
pub use swarm::Swarm;

/// The three macro controls, each 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ctl {
    pub harmonics: f32,
    pub timbre: f32,
    pub morph: f32,
}

impl Ctl {
    pub const fn new(harmonics: f32, timbre: f32, morph: f32) -> Self {
        Self {
            harmonics,
            timbre,
            morph,
        }
    }

    const MID: Self = Self::new(0.5, 0.5, 0.5);
}

/// One voice's macro oscillator.
pub trait Model {
    /// A block at `hz` with controls `c`, held for the block: a model eases
    /// toward them inside it, so a sweep never steps.
    fn block(&mut self, out: &mut Blk, hz: f32, c: Ctl);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Harmonic,
    Swarm,
    Particle,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Harmonic, Kind::Swarm, Kind::Particle];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Harmonic => "HARMONIC",
            Kind::Swarm => "SWARM",
            Kind::Particle => "PARTICLE",
        }
    }

    /// The fixed render gain: the model's loudest render (a pad, four
    /// voices) peaks near −6 dBFS. Measured once on these renders and held,
    /// so the files keep every level difference. PARTICLE's is held lower
    /// by its densest corner (TIMBRE, MORPH and HARMONICS at 1), which must
    /// stay under full scale: its renders peak near −10 dBFS.
    pub fn gain(self) -> f32 {
        match self {
            Kind::Harmonic => 0.28,
            Kind::Swarm => 0.47,
            Kind::Particle => 1.0,
        }
    }

    pub fn build(self, seed: u32) -> Box<dyn Model> {
        match self {
            Kind::Harmonic => Box::new(Harmonic::new()),
            Kind::Swarm => Box::new(Swarm::new(seed)),
            Kind::Particle => Box::new(Particle::new(seed)),
        }
    }

    /// Named settings: (name, controls, what to listen for).
    fn settings(self) -> [(&'static str, Ctl, &'static str); 3] {
        match self {
            Kind::Harmonic => [
                (
                    "classic",
                    Ctl::new(0.3, 0.7, 0.0),
                    "a wide bump on the low partials, every harmonic: a soft, round saw-like tone",
                ),
                (
                    "mid",
                    Ctl::new(0.55, 0.35, 0.5),
                    "odd partials only, a narrower bump around the 5th-7th: hollow, reedy, vowel-ish",
                ),
                (
                    "extreme",
                    Ctl::new(0.9, 0.05, 1.0),
                    "drawbar partials (1 2 3 4 6 8 10 12 16) under a needle bump at the top: a glassy whistle",
                ),
            ],
            Kind::Swarm => [
                (
                    "classic",
                    Ctl::new(0.3, 0.0, 0.0),
                    "eight saws about ±30 cents apart, steady: a supersaw-like chorus with slow drift",
                ),
                (
                    "mid",
                    Ctl::new(0.5, 0.4, 0.5),
                    "±1.5 semitones of triangles in overlapping grains: a shimmering, moving cloud",
                ),
                (
                    "extreme",
                    Ctl::new(0.9, 0.9, 1.0),
                    "sine grains scattered over ±9 semitones, sparse and short: a granular haze, pitch barely held",
                ),
            ],
            Kind::Particle => [
                (
                    "classic",
                    Ctl::new(0.05, 0.75, 0.85),
                    "dense dust through sharp band-passes at the note: a pitched, crackling ring",
                ),
                (
                    "mid",
                    Ctl::new(0.4, 0.5, 0.5),
                    "moderate dust, band-passes scattered over an octave or so, medium Q: wooden droplets",
                ),
                (
                    "extreme",
                    Ctl::new(1.0, 0.35, 0.0),
                    "sparse dust through the all-pass diffuser: unpitched clicks smeared into short reverberant splashes",
                ),
            ],
        }
    }
}

/// A linear attack, an exponential decay to sustain, an exponential
/// release (time constants in seconds).
#[derive(Clone, Copy)]
struct Env {
    a: f32,
    d: f32,
    s: f32,
    r: f32,
}

/// A note: MIDI number, start and gate length in seconds.
type Note = (u8, f32, f32);

struct Phrase {
    name: &'static str,
    notes: &'static [Note],
    env: Env,
    about: &'static str,
}

const PHRASES: [Phrase; 4] = [
    Phrase {
        name: "sustain",
        notes: &[(48, 0.0, 2.0)],
        env: Env {
            a: 0.005,
            d: 0.3,
            s: 0.8,
            r: 0.3,
        },
        about: "C3 held 2 s",
    },
    Phrase {
        name: "stab",
        notes: &[(48, 0.05, 0.22), (51, 0.05, 0.22), (55, 0.05, 0.22)],
        env: Env {
            a: 0.002,
            d: 0.12,
            s: 0.5,
            r: 0.12,
        },
        about: "C minor stab (C3 Eb3 G3), 220 ms",
    },
    Phrase {
        name: "bass",
        notes: &[
            (36, 0.0, 0.2),
            (36, 0.25, 0.2),
            (39, 0.5, 0.2),
            (31, 0.75, 0.35),
        ],
        env: Env {
            a: 0.002,
            d: 0.1,
            s: 0.6,
            r: 0.06,
        },
        about: "bass line C2 C2 Eb2 G1 in eighths at 120 BPM",
    },
    Phrase {
        name: "pad",
        notes: &[
            (48, 0.0, 3.2),
            (51, 0.0, 3.2),
            (55, 0.0, 3.2),
            (58, 0.0, 3.2),
        ],
        env: Env {
            a: 1.5,
            d: 0.5,
            s: 1.0,
            r: 1.2,
        },
        about: "Cm7 pad (C3 Eb3 G3 Bb3), 1.5 s swell, 1.2 s release",
    },
];

/// One note of `kind` into `out` at `at` seconds, at its render gain:
/// `ctl(t)` the controls at the note's time `t`.
#[allow(clippy::too_many_arguments)]
fn note(
    kind: Kind,
    seed: u32,
    midi: u8,
    at: f32,
    len: f32,
    env: Env,
    ctl: &dyn Fn(f32) -> Ctl,
    out: &mut Vec<f32>,
) {
    let mut m = kind.build(seed);
    let hz = chimera_core::dsp::note_to_freq(midi);
    let g = kind.gain();
    let total = len + 6.0 * env.r;
    let n = (total * FS / B as f32).ceil() as usize;
    let start = (at * FS) as usize;
    out.resize(out.len().max(start + n * B), 0.0);
    let (dk, rk) = ((-1.0 / (env.d * FS)).exp(), (-1.0 / (env.r * FS)).exp());
    let mut e = 0.0f32;
    let mut b: Blk = [0.0; B];
    for i in 0..n {
        m.block(&mut b, hz, ctl(i as f32 * B as f32 / FS));
        for (s, x) in b.iter().enumerate() {
            let t = (i * B + s) as f32 / FS;
            e = if t >= len {
                e * rk
            } else if t < env.a {
                t / env.a
            } else {
                env.s + (e.max(env.s) - env.s) * dk
            };
            out[start + i * B + s] += x * e * g;
        }
    }
}

struct Out<'a> {
    dir: &'a Path,
    rows: Vec<Value>,
}

impl Out<'_> {
    fn write(&mut self, name: &str, x: &[f32], mut row: Value) -> Result<(), String> {
        let x64: Vec<f64> = x.iter().map(|&s| s as f64).collect();
        write_pcm24(&self.dir.join(name), &x64, FS as u32)?;
        let pk = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let ms = x.iter().map(|s| (s * s) as f64).sum::<f64>() / x.len().max(1) as f64;
        let db = |v: f64| (20.0 * v.max(1e-9).log10() * 10.0).round() / 10.0;
        row["file"] = json!(name);
        row["peak_dbfs"] = json!(db(pk as f64));
        row["rms_dbfs"] = json!(db(ms.sqrt()));
        row["seconds"] = json!((x.len() as f64 / FS as f64 * 1000.0).round() / 1000.0);
        if pk > 1.0 {
            eprintln!("over full scale: {name} peaks at {pk}");
        }
        self.rows.push(row);
        Ok(())
    }
}

fn ctl_json(c: Ctl) -> Value {
    let r = |v: f32| (v * 100.0).round() / 100.0;
    json!({"harmonics": r(c.harmonics), "timbre": r(c.timbre), "morph": r(c.morph)})
}

pub fn run(dir: &Path) -> Result<(), String> {
    parts::flush_denormals();
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Out {
        dir,
        rows: Vec::new(),
    };
    for kind in Kind::ALL {
        let slug = kind.name().to_ascii_lowercase();
        for (set, c, listen) in kind.settings() {
            for ph in &PHRASES {
                let mut x = Vec::new();
                for (i, &(midi, at, len)) in ph.notes.iter().enumerate() {
                    note(kind, 1 + i as u32, midi, at, len, ph.env, &|_| c, &mut x);
                }
                let row = json!({
                    "kind": "phrase", "model": kind.name(), "setting": set, "phrase": ph.name,
                    "controls": ctl_json(c), "description": format!("{}: {}", ph.about, listen),
                });
                out.write(&format!("{slug}_{set}_{}.wav", ph.name), &x, row)?;
            }
        }
        for (ci, ctl) in ["harmonics", "timbre", "morph"].iter().enumerate() {
            let sweep = move |t: f32| {
                let v = (t / 4.0).min(1.0);
                let mut c = Ctl::MID;
                *[&mut c.harmonics, &mut c.timbre, &mut c.morph][ci] = v;
                c
            };
            let mut x = Vec::new();
            let env = Env {
                a: 0.01,
                d: 0.1,
                s: 1.0,
                r: 0.2,
            };
            note(kind, 7, 48, 0.0, 4.0, env, &sweep, &mut x);
            let row = json!({
                "kind": "sweep", "model": kind.name(), "setting": format!("sweep-{ctl}"),
                "phrase": "C3 held 4 s", "controls": ctl_json(Ctl::MID), "sweep": {"control": ctl, "from": 0.0, "to": 1.0, "seconds": 4.0},
                "description": format!("{} {} swept 0 to 1 over 4 s on a held C3, the others at 0.5", kind.name(), ctl.to_ascii_uppercase()),
            });
            out.write(&format!("{slug}_sweep_{ctl}.wav"), &x, row)?;
        }
    }
    let manifest = json!({
        "about": "Chimera macro oscillator KINDs, host prototypes (chimera-utils macro-render): HARMONIC (additive), SWARM (8 drifting VA oscillators), PARTICLE (dust through CLEAN band-passes and an all-pass diffuser), after Plaits' structure, our own code. 48 kHz mono 24-bit WAVs at raw levels, never normalised; each model has one fixed render gain. Oscillator only: no filter; a simple ADSR per note.",
        "render_gain": Kind::ALL.iter().map(|k| (k.name().to_string(), json!(k.gain()))).collect::<serde_json::Map<_, _>>(),
        "cpu_estimate": bench(),
        "files": out.rows,
    });
    let p = dir.join("manifest.json");
    std::fs::write(&p, serde_json::to_string_pretty(&manifest).unwrap())
        .map_err(|e| format!("{}: {e}", p.display()))?;
    println!("{} files, manifest {}", out.rows.len(), p.display());
    Ok(())
}

/// Host ns a sample of each model at its classic and mid settings on C3,
/// against ALGO's kernel with six chained operators; the ratio times
/// ALGO's billed 770 cycles (as `drum-render`'s estimate).
fn bench() -> Value {
    let n = FS as usize / 2 / B;
    let time = |f: &mut dyn FnMut(&mut Blk)| {
        let mut best = f64::MAX;
        for _ in 0..5 {
            let t = Instant::now();
            let mut b: Blk = [0.0; B];
            for _ in 0..n {
                f(&mut b);
                std::hint::black_box(&b);
            }
            best = best.min(t.elapsed().as_nanos() as f64 / (n * B) as f64);
        }
        best
    };
    let mut algo = parts::Ops::new([0, 1, 2, 4, 8, 16], 1);
    let a = time(&mut |b| {
        algo.block(
            b,
            [110.0, 220.0, 330.0, 440.0, 550.0, 660.0],
            [0.8, 0.3, 0.3, 0.3, 0.3, 0.3],
            [0.0; 6],
        )
    });
    let r2 = |v: f64| (v * 100.0).round() / 100.0;
    let mut rows = serde_json::Map::new();
    for kind in Kind::ALL {
        for (set, c, _) in &kind.settings()[..2] {
            let mut m = kind.build(1);
            let ns = time(&mut |b| m.block(b, 130.81, *c));
            rows.insert(
                format!("{} {set}", kind.name()),
                json!({"host_ns_per_sample": r2(ns), "ratio_vs_algo6": r2(ns / a), "est_cycles_per_sample": (ns / a * 770.0).round()}),
            );
        }
    }
    json!({
        "estimate": true,
        "method": "host ns/sample over 0.5 s on C3, best of five, denormals flushed, vs ALGO's kernel with six chained operators, times ALGO's billed 770 cycles; read the ratios, not the digits",
        "algo6_host_ns_per_sample": r2(a),
        "models": rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `secs` of one voice at `hz`, controls `c`, render gain applied.
    pub fn play(kind: Kind, hz: f32, c: Ctl, secs: f32) -> Vec<f32> {
        let mut m = kind.build(3);
        let mut x = Vec::new();
        let mut b: Blk = [0.0; B];
        for _ in 0..(secs * FS) as usize / B {
            m.block(&mut b, hz, c);
            x.extend(b.iter().map(|s| s * kind.gain()));
        }
        x
    }

    #[test]
    fn every_model_finite_and_under_full_scale_at_the_extremes() {
        for kind in Kind::ALL {
            for corner in 0..8 {
                let f = |bit: u32| ((corner >> bit) & 1) as f32;
                let c = Ctl::new(f(0), f(1), f(2));
                for hz in [32.7, 261.6, 4186.0] {
                    let x = play(kind, hz, c, 1.0);
                    let pk = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                    assert!(
                        x.iter().all(|s| s.is_finite()),
                        "{} {c:?} {hz}",
                        kind.name()
                    );
                    assert!(pk <= 1.0, "{} {c:?} {hz} Hz peaks {pk}", kind.name());
                }
            }
        }
    }
}
