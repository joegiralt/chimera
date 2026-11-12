//! HARMONIC engine (0.4), host prototype: the design of
//! docs/superpowers/specs/2026-11-12-harmonic-engine-design.md grown from
//! `macro_osc::harmonic` (24 partials by phasor and recurrence).
//!
//! A bank of up to 64 recursive sine partials (Vicanek) with Plaits'
//! Nyquist taper; a 5×5 map of spectra made by formulas, stored as 8-bit
//! log amplitudes and mixed bilinearly; DENSITY (a Grids-style threshold,
//! level held); AIR (noise that follows the spectrum); SUB (½ and ¼); four
//! module slots from TILT, ODD/EVEN, FORMANT, STRETCH, COMB, PLUCK; four
//! corner variations of the module knobs morphed by the same X/Y; PATH;
//! ENV → X; VARIETY; and a plain ADSR standing in for the voice chain.
//!
//! BRIGHT (`Bright`, after the owner's "darker than Loom"): a brighter map
//! and taper, PHASE (aligned to Schroeder), BRIGHT TAIL (`tail`: VA's saw
//! through CLEAN above the last partial) and LOG spacing on low notes.
//! `Bright::ON` is the default; `Bright::DARK` the first prototype.
//!
//! `harmonic-render OUTDIR`: the audition groups as mono 48 kHz 24-bit
//! WAVs at one fixed gain (`GAIN`), never normalised, and `manifest.json`
//! with each file's levels and a host cost estimate.

mod map;
mod modules;
mod renders;
mod tail;
mod voice;

use crate::drums::parts::{B, Blk, FS};
use crate::macro_osc::Out;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Instant;

pub use modules::ModuleKind;
pub use voice::{Adsr, Bright, PathShape, Sound, SubKind, SubOctave, SubWave, Voice};

/// The most partials a build may have.
pub const MAX: usize = 160;

/// A build's partial count: a multiple of 8, at most `MAX`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partials(usize);

impl Partials {
    pub const fn new(n: usize) -> Self {
        assert!(n > 0 && n.is_multiple_of(8) && n <= MAX);
        Self(n)
    }

    pub const fn get(self) -> usize {
        self.0
    }

    pub const P32: Self = Self::new(32);
    pub const P48: Self = Self::new(48);
    pub const P64: Self = Self::new(64);
    /// C3-up: the whole audio range from C3 (152 needed there).
    pub const P160: Self = Self::new(160);
}

/// The one render gain for every file: a voice sits at VA's oscillator
/// RMS (−14 dBFS) times this (−30 dBFS a voice at sustain). Set once so
/// the loudest render (VOWEL PAD, five notes) peaks under −6 dBFS; held,
/// so every level difference survives.
pub const GAIN: f32 = 0.16;

/// A note: MIDI number, start and gate length in seconds.
pub type Note = (u8, f32, f32);

/// `notes` of `sound` with `n` partials at `GAIN`; `auto(t, sound)` moves
/// controls at song time `t` (block rate).
pub fn render(
    n: Partials,
    sound: &Sound,
    notes: &[Note],
    auto: &dyn Fn(f32, &mut Sound),
) -> Vec<f32> {
    let end = notes.iter().map(|&(_, a, l)| a + l).fold(0.0, f32::max) + 6.0 * sound.amp.r + 0.05;
    let blocks = (end * FS / B as f32).ceil() as usize;
    let at_block = |s: f32| (s * FS / B as f32).round() as usize;
    let mut out = vec![0.0f32; blocks * B];
    let mut voices: Vec<(Voice, usize)> = Vec::new();
    let mut b: Blk = [0.0; B];
    for i in 0..blocks {
        let t = (i * B) as f32 / FS;
        let mut s = *sound;
        auto(t, &mut s);
        for (j, &(m, at, len)) in notes.iter().enumerate() {
            if at_block(at) == i {
                let f0 = chimera_core::dsp::note_to_freq(m);
                voices.push((Voice::new(n, f0, j as u32 + 1, &s), at_block(at + len)));
            }
        }
        let o = &mut out[i * B..(i + 1) * B];
        voices.retain_mut(|(v, off)| {
            if i >= *off {
                v.release();
            }
            let alive = v.block(&mut b, &s, t);
            for (o, x) in o.iter_mut().zip(&b) {
                *o += x * GAIN;
            }
            alive
        });
    }
    out
}

/// A Sound's controls for the manifest.
fn controls(n: Partials, s: &Sound) -> Value {
    let r = |v: f32| (v as f64 * 1000.0).round() / 1000.0;
    let slots: Vec<Value> = (0..voice::SLOTS)
        .filter(|&i| s.chain[i] != ModuleKind::None)
        .map(|i| {
            let k = s.chain[i];
            let at = |c: usize| -> Value {
                k.knobs()
                    .iter()
                    .enumerate()
                    .map(|(j, name)| (name.to_string(), json!(r(s.corners[c][i][j]))))
                    .collect::<serde_json::Map<_, _>>()
                    .into()
            };
            let same = (1..4).all(|c| s.corners[c][i] == s.corners[0][i]);
            if same {
                json!({"slot": i + 1, "kind": k.name(), "knobs": at(0)})
            } else {
                json!({"slot": i + 1, "kind": k.name(), "corners": {"x0y0": at(0), "x1y0": at(1), "x0y1": at(2), "x1y1": at(3)}})
            }
        })
        .collect();
    let mut v = json!({
        "partials": n.get(), "x": r(s.x), "y": r(s.y), "density": r(s.density),
        "air": r(s.air), "sub": r(s.sub), "variety": r(s.variety), "slots": slots,
        "amp": {"attack": r(s.amp.a), "decay": r(s.amp.d), "sustain": r(s.amp.s), "release": r(s.amp.r)},
    });
    v["bright"] = json!({"map": s.bright.map, "phase": r(s.bright.phase), "tail": s.bright.tail, "log": s.bright.log});
    if let SubKind::Va { wave, octave } = s.sub_kind {
        v["sub"] =
            json!({"kind": "VA", "wave": wave.name(), "octave": octave.name(), "level": r(s.sub)});
    }
    if s.path.shape != PathShape::Off {
        v["path"] = json!({"shape": s.path.shape.name(), "bars": r(s.path.bars), "size": r(s.path.size), "bpm": 120});
    }
    if s.env_x.amount != 0.0 {
        v["env_x"] = json!({"amount": r(s.env_x.amount), "decay_s": r(s.env_x.decay)});
    }
    v
}

pub fn run(dir: &Path) -> Result<(), String> {
    crate::drums::parts::flush_denormals();
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Out {
        dir,
        rows: Vec::new(),
    };
    let groups = renders::all(&mut out)?;
    let loudest = out
        .rows
        .iter()
        .filter_map(|r| r["peak_dbfs"].as_f64())
        .fold(f64::MIN, f64::max);
    let nodes = |bright: bool| -> Vec<Value> {
        let f = if bright {
            &map::FORMULAS_BRIGHT
        } else {
            &map::FORMULAS
        };
        (0..5)
            .flat_map(|r| {
                (0..5).map(move |c| {
                    json!({
                        "row": map::ROWS[r], "col": c, "formula": f[r][c],
                        "log_amp_bytes": map::Map::get(bright).node(r, c).bytes().to_vec(),
                        "centroid_partial_64": (map::centroid(bright, c as f32 / 4.0, r as f32 / 4.0, map::MAP_N) * 100.0).round() / 100.0,
                    })
                })
            })
            .collect()
    };
    let manifest = json!({
        "about": "Chimera HARMONIC engine, host prototype (chimera-utils harmonic-render): recursive sine partials (Vicanek) with Plaits' Nyquist taper, a 5x5 map of formula spectra as 8-bit log amplitudes, DENSITY, AIR, SUB, four module slots, corner variations, PATH, ENV->X, VARIETY; a plain ADSR stands in for the voice chain (no filter, no FX). 48 kHz mono 24-bit WAVs, never normalised.",
        "render_gain": {"gain": (GAIN as f64 * 1000.0).round() / 1000.0, "voice_rms_dbfs_at_sustain": (20.0 * (voice::RMS as f64 * GAIN as f64).log10() * 10.0).round() / 10.0, "loudest_peak_dbfs": loudest, "note": "every file at the same fixed gain; LEVEL holds each voice at VA's oscillator RMS (0.2) before the amp envelope"},
        "groups": groups,
        "map": nodes(false),
        "map_bright": nodes(true),
        "cpu_estimate": bench(),
        "files": out.rows,
    });
    let p = dir.join("manifest.json");
    std::fs::write(&p, serde_json::to_string_pretty(&manifest).unwrap())
        .map_err(|e| format!("{}: {e}", p.display()))?;
    println!(
        "{} files, loudest {loudest} dBFS, manifest {}",
        out.rows.len(),
        p.display()
    );
    Ok(())
}

/// Host ns a sample of one voice, held C1 (every partial under the taper
/// live), against ALGO's six-operator kernel, times ALGO's billed 770
/// cycles (as `macro-render`'s estimate).
fn bench() -> Value {
    let n_blocks = FS as usize / 2 / B;
    let time = |f: &mut dyn FnMut(&mut Blk, usize)| {
        let mut best = f64::MAX;
        for _ in 0..5 {
            let t = Instant::now();
            let mut b: Blk = [0.0; B];
            for i in 0..n_blocks {
                f(&mut b, i);
                std::hint::black_box(&b);
            }
            best = best.min(t.elapsed().as_nanos() as f64 / (n_blocks * B) as f64);
        }
        best
    };
    let mut algo = crate::drums::parts::Ops::new([0, 1, 2, 4, 8, 16], 1);
    let a = time(&mut |b, _| {
        algo.block(
            b,
            [110.0, 220.0, 330.0, 440.0, 550.0, 660.0],
            [0.8, 0.3, 0.3, 0.3, 0.3, 0.3],
            [0.0; 6],
        )
    });
    let r2 = |v: f64| (v * 100.0).round() / 100.0;
    let est = |ns: f64| (ns / a * 770.0).round();
    let saw = Sound::new(0.5, 0.75).with(|s| {
        s.amp = Adsr::new(0.001, 1.0, 1.0, 1.0);
        s.bright = Bright::DARK;
    });
    let full = saw
        .slot(0, ModuleKind::Tilt, [0.6, 0.2, 0.0])
        .slot(1, ModuleKind::Formant, [0.5, 0.4, 0.5])
        .slot(2, ModuleKind::Comb, [0.4, 0.3, 0.6])
        .slot(3, ModuleKind::Pluck, [0.9, 0.5, 0.0])
        .with(|s| {
            s.air = 0.3;
            s.sub = 0.5;
        });
    let stretch = saw.slot(0, ModuleKind::Stretch, [0.6, 0.0, 0.0]);
    let on = |s: Sound, b: Bright| s.with(|s| s.bright = b);
    let log = Bright {
        log: true,
        ..Bright::ON
    };
    let cases: [(&str, Sound, bool); 7] = [
        ("bare: map + DENSITY + bank (first prototype)", saw, false),
        (
            "full: TILT FORMANT COMB PLUCK, AIR 0.3, SUB 0.5 (first prototype)",
            full,
            false,
        ),
        ("STRETCH moving every block (k re-solved)", stretch, true),
        (
            "bare, bright defaults (BRIGHT TAIL on)",
            on(saw, Bright::ON),
            false,
        ),
        (
            "full, bright defaults (BRIGHT TAIL on)",
            on(full, Bright::ON),
            false,
        ),
        (
            "bare, bright + LOG (LOG replaces the tail)",
            on(saw, log),
            false,
        ),
        ("full, bright + LOG", on(full, log), false),
    ];
    let mut rows = Vec::new();
    for (name, s, moving) in cases {
        let mut per = serde_json::Map::new();
        for n in [Partials::P32, Partials::P48, Partials::P64] {
            let mut v = Voice::new(n, 32.70, 1, &s);
            let ns = time(&mut |b, i| {
                let mut s = s;
                if moving {
                    s.set_knob(0, 0, 0.55 + 0.1 * (i % 16) as f32 / 16.0);
                }
                v.block(b, &s, i as f32 * B as f32 / FS);
            });
            per.insert(
                n.get().to_string(),
                json!({"host_ns_per_sample": r2(ns), "est_m7_cycles_per_sample": est(ns)}),
            );
        }
        rows.push(json!({"case": name, "partials": per}));
    }
    let mut ctl = serde_json::Map::new();
    for n in [Partials::P32, Partials::P48, Partials::P64] {
        let vary = voice::Vary::default();
        let ns = time(&mut |_, i| {
            std::hint::black_box(voice::frame(n, &full, 32.70, &vary, i as f32, i as f32));
        });
        ctl.insert(
            n.get().to_string(),
            json!({"host_ns_per_sample": r2(ns), "est_m7_cycles_per_sample": est(ns)}),
        );
    }
    rows.push(json!({"case": "of which the control frame alone (full case, once a block)", "partials": ctl}));
    // C3-up: 160 partials, every one live at C3 (20.9 kHz under the
    // taper's 21.1), no tail, no LOG; the VA SUB beside
    let c3 = Bright {
        tail: false,
        log: false,
        ..Bright::ON
    };
    let va = |s: Sound| {
        s.with(|s| {
            s.sub = 0.5;
            s.sub_kind = SubKind::Va {
                wave: SubWave::Square,
                octave: SubOctave::Down1,
            };
        })
    };
    let bare3 = on(saw, c3).with(|s| s.sub = 0.0);
    let full3 = on(full, c3);
    let mut c3_rows = Vec::new();
    for (name, n, s) in [
        ("64 partials, first prototype, bare", Partials::P64, saw),
        ("160 partials, bare", Partials::P160, bare3),
        (
            "160 partials, bare + VA SUB (square -1)",
            Partials::P160,
            va(bare3),
        ),
        (
            "160 partials, full chain (TILT FORMANT COMB PLUCK, AIR 0.3) + VA SUB",
            Partials::P160,
            va(full3),
        ),
    ] {
        let mut v = Voice::new(n, 130.81, 1, &s);
        let ns = time(&mut |b, i| {
            v.block(b, &s, i as f32 * B as f32 / FS);
        });
        c3_rows.push(json!({"case": name, "host_ns_per_sample": r2(ns), "est_m7_cycles_per_sample": est(ns), "four_voices": est(ns) * 4.0}));
    }
    let mut proto = crate::macro_osc::Harmonic::new();
    let p = time(&mut |b, _| {
        crate::macro_osc::Model::block(
            &mut proto,
            b,
            32.70,
            crate::macro_osc::Ctl::new(0.3, 0.7, 0.0),
        )
    });
    json!({
        "estimate": true,
        "method": "one voice on a held C1 (every partial below the taper live), host ns/sample over 0.5 s, best of five, denormals flushed; est_m7 = ratio to ALGO's six-operator kernel times its billed 770 cycles. Read the ratios, not the digits: the unit bench sets the real figures. Excludes the voice chain (filter, ENVs, FX), billed by its own terms.",
        "algo6_host_ns_per_sample": r2(a),
        "cases": rows,
        "c3_up_at_c3": c3_rows,
        "reference_macro_osc_harmonic_24_chebyshev": {"host_ns_per_sample": r2(p), "est_m7_cycles_per_sample": est(p)},
    })
}

#[cfg(test)]
mod tests {
    use super::voice::{SLOTS, Vary, frame};
    use super::*;
    use rustfft::{FftPlanner, num_complex::Complex};

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn finite_and_under_full_scale_at_the_control_corners() {
        use ModuleKind::*;
        let chains = [
            [Tilt, OddEven, Formant, Comb],
            [Stretch, Pluck, Formant, Tilt],
        ];
        for corner in 0..32u32 {
            let f = |bit: u32| ((corner >> bit) & 1) as f32;
            for chain in chains {
                for knob in [0.0, 1.0] {
                    let mut s = Sound::new(f(0), f(1));
                    (s.density, s.air, s.sub) = (f(2), f(3), f(4));
                    for (i, k) in chain.iter().enumerate() {
                        s = s.slot(i, *k, [knob; 3]);
                    }
                    for m in [24, 60, 108] {
                        let x = render(Partials::P64, &s, &[(m, 0.0, 0.3)], &|_, _| {});
                        let pk = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                        assert!(x.iter().all(|v| v.is_finite()), "{s:?} {m}");
                        assert!(pk <= 1.0, "{s:?} {m}: peak {pk}");
                    }
                }
            }
        }
    }

    #[test]
    fn no_partial_at_or_above_045_fs() {
        for m in (24..=120).step_by(4) {
            let f0 = chimera_core::dsp::note_to_freq(m);
            for (st, log) in [0.0, 0.5, 0.8, 1.0]
                .into_iter()
                .flat_map(|st| [(st, false), (st, true)])
            {
                let s = Sound::new(1.0, 0.75)
                    .slot(0, ModuleKind::Stretch, [st, 0.0, 0.0])
                    .with(|s| {
                        s.sub = 1.0;
                        s.bright.log = log;
                    });
                let fr = frame(Partials::P64, &s, f0, &Vary::default(), 0.0, 0.0);
                for (hz, a) in fr.hz.iter().zip(&fr.amp) {
                    assert!(
                        *a == 0.0 || *hz < 0.45 * FS,
                        "note {m} st {st}: {hz} Hz at {a}"
                    );
                }
            }
        }
        // and no energy above it in a held note's spectrum
        for m in [60, 84, 96, 103] {
            let s = Sound::new(1.0, 0.75).with(|s| s.amp = Adsr::new(0.001, 1.0, 1.0, 0.1));
            let x = render(Partials::P64, &s, &[(m, 0.0, 1.0)], &|_, _| {});
            let n = 1 << 14;
            let x = &x[B * 64..B * 64 + n];
            let mut buf: Vec<Complex<f32>> = x
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let w = 0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / n as f32).cos();
                    Complex::new(s * w, 0.0)
                })
                .collect();
            FftPlanner::new().plan_fft_forward(n).process(&mut buf);
            let (mut lo, mut hi) = (0.0f64, 0.0f64);
            for (i, v) in buf[..n / 2].iter().enumerate() {
                let e = v.norm_sqr() as f64;
                if i as f32 * FS / n as f32 > 0.46 * FS {
                    hi += e;
                } else {
                    lo += e;
                }
            }
            let db = 10.0 * (hi / lo).log10();
            assert!(db < -80.0, "note {m}: {db:.1} dB above 0.46 fs");
        }
    }

    #[test]
    fn density_holds_rms_within_1_5_db() {
        for (x, y) in [
            (0.0, 0.0),
            (0.5, 0.25),
            (1.0, 0.5),
            (0.5, 0.75),
            (0.25, 1.0),
        ] {
            let level = |d: f32| {
                let s = Sound::new(x, y).with(|s| {
                    s.density = d;
                    s.amp = Adsr::new(0.001, 1.0, 1.0, 0.1);
                });
                let v = render(Partials::P64, &s, &[(48, 0.0, 1.0)], &|_, _| {});
                20.0 * rms(&v[24_000..48_000]).log10()
            };
            let full = level(1.0);
            for i in 0..10 {
                let d = level(i as f32 / 10.0);
                assert!(
                    (d - full).abs() <= 1.5,
                    "({x},{y}) DENSITY {i}/10: {d:.2} vs {full:.2}"
                );
            }
        }
    }

    /// |X| of a held note's Hann-windowed 2^15 samples from 0.5 s.
    fn spectrum(s: &Sound, m: u8) -> Vec<f32> {
        let s = s.with(|s| s.amp = Adsr::new(0.001, 1.0, 1.0, 0.1));
        let x = render(Partials::P64, &s, &[(m, 0.0, 1.5)], &|_, _| {});
        let n = 1 << 15;
        let mut buf: Vec<Complex<f32>> = x[24_000..24_000 + n]
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let w = 0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / n as f32).cos();
                Complex::new(s * w, 0.0)
            })
            .collect();
        FftPlanner::new().plan_fft_forward(n).process(&mut buf);
        buf[..n / 2].iter().map(|v| v.norm()).collect()
    }

    /// The level at `hz`: the strongest bin within ±2.
    fn at(sp: &[f32], hz: f32) -> f32 {
        let b = (hz * (1 << 15) as f32 / FS).round() as usize;
        20.0 * sp[b - 2..=b + 2]
            .iter()
            .fold(0.0f32, |m, &v| m.max(v))
            .log10()
    }

    /// BRIGHT TAIL's skirt never doubles a partial under N: with the tail
    /// on, every partial under N moves by the same LEVEL factor (the tail
    /// takes its share of the held RMS), within 0.5 dB.
    #[test]
    fn tail_leaves_partials_under_n_untouched() {
        for (x, y) in [(1.0, 0.75), (0.5, 0.75), (1.0, 0.25), (0.6, 0.5)] {
            for phase in [0.0, 1.0] {
                let on = Sound::new(x, y).with(|s| {
                    s.bright = Bright {
                        phase,
                        ..Bright::ON
                    }
                });
                let off = on.with(|s| s.bright.tail = false);
                for m in [24, 36] {
                    let f0 = chimera_core::dsp::note_to_freq(m);
                    let (a, b) = (spectrum(&on, m), spectrum(&off, m));
                    let top = (1..64)
                        .map(|k| at(&b, k as f32 * f0))
                        .fold(f32::MIN, f32::max);
                    let d: Vec<(usize, f32)> = (1..64)
                        .filter(|&k| at(&b, k as f32 * f0) > top - 60.0)
                        .map(|k| (k, at(&a, k as f32 * f0) - at(&b, k as f32 * f0)))
                        .collect();
                    let lvl = d[0].1;
                    for &(k, v) in &d {
                        assert!(
                            (v - lvl).abs() <= 0.5,
                            "({x},{y}) phase {phase} note {m} partial {k}: {v:.2} dB against {lvl:.2}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn aligned_phase_attacks_harder_than_schroeder() {
        for (x, y) in [(1.0, 0.75), (0.5, 0.75), (1.0, 0.25)] {
            let peak = |phase: f32| {
                let s = Sound::new(x, y).with(|s| {
                    s.bright.phase = phase;
                    s.amp = Adsr::new(0.001, 0.3, 0.8, 0.1);
                });
                let v = render(Partials::P64, &s, &[(48, 0.0, 0.2)], &|_, _| {});
                v[..1440].iter().fold(0.0f32, |m, v| m.max(v.abs()))
            };
            let (a, s) = (peak(0.0), peak(1.0));
            assert!(a > 1.1 * s, "({x},{y}): aligned {a} Schroeder {s}");
        }
    }

    /// A C1 with the defaults (the tail on) has a spectrum at 12 kHz; the
    /// first prototype stopped at 2 kHz.
    #[test]
    fn c1_reaches_12_khz() {
        for (x, y) in [(1.0, 0.75), (0.5, 0.75), (1.0, 0.25)] {
            let s = Sound::new(x, y);
            let band = |sp: &[f32]| {
                (11_800..12_200)
                    .step_by(20)
                    .map(|hz| at(sp, hz as f32))
                    .fold(f32::MIN, f32::max)
            };
            let new = band(&spectrum(&s, 24));
            let old = band(&spectrum(&s.with(|s| s.bright = Bright::DARK), 24));
            assert!(new >= old + 30.0, "({x},{y}): {new:.1} dB against {old:.1}");
        }
    }

    /// C3-up: 160 partials stay under 0.45·fs at every note and leave
    /// nothing above 0.46·fs; with each VA SUB, finite and under full
    /// scale, and LEVEL still holds the RMS within 1.5 dB of no SUB.
    #[test]
    fn c3_up_is_clean() {
        let c3 = Bright {
            tail: false,
            log: false,
            ..Bright::ON
        };
        let s = Sound::new(1.0, 0.75).with(|s| {
            s.bright = c3;
            s.amp = Adsr::new(0.001, 1.0, 1.0, 0.1);
        });
        for m in (24..=120).step_by(6) {
            let f0 = chimera_core::dsp::note_to_freq(m);
            let fr = frame(Partials::P160, &s, f0, &Vary::default(), 0.0, 0.0);
            for (hz, a) in fr.hz.iter().zip(&fr.amp) {
                assert!(*a == 0.0 || *hz < 0.45 * FS, "note {m}: {hz} Hz");
            }
        }
        for m in [48, 60, 84] {
            let x = render(Partials::P160, &s, &[(m, 0.0, 1.0)], &|_, _| {});
            let n = 1 << 14;
            let mut buf: Vec<Complex<f32>> = x[B * 64..B * 64 + n]
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let w = 0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / n as f32).cos();
                    Complex::new(s * w, 0.0)
                })
                .collect();
            FftPlanner::new().plan_fft_forward(n).process(&mut buf);
            let (mut lo, mut hi) = (0.0f64, 0.0f64);
            for (i, v) in buf[..n / 2].iter().enumerate() {
                if i as f32 * FS / n as f32 > 0.46 * FS {
                    hi += v.norm_sqr() as f64;
                } else {
                    lo += v.norm_sqr() as f64;
                }
            }
            let db = 10.0 * (hi / lo).log10();
            assert!(db < -80.0, "note {m}: {db:.1} dB above 0.46 fs");
        }
        let level = |s: &Sound, m: u8| {
            let v = render(Partials::P160, s, &[(m, 0.0, 1.0)], &|_, _| {});
            assert!(v.iter().all(|v| v.is_finite()));
            assert!(v.iter().all(|v| v.abs() <= 1.0));
            20.0 * rms(&v[24_000..48_000]).log10()
        };
        for m in [24, 48, 72] {
            let none = level(&s, m);
            for w in SubWave::ALL {
                for o in [SubOctave::Down1, SubOctave::Down2] {
                    let with = s.with(|s| {
                        s.sub = 1.0;
                        s.sub_kind = SubKind::Va { wave: w, octave: o };
                    });
                    let d = level(&with, m) - none;
                    assert!(d.abs() <= 1.5, "note {m} {w:?} {o:?}: {d:.2} dB");
                }
            }
        }
    }

    #[test]
    fn variety_never_moves_the_fundamental() {
        let s = Sound::new(0.5, 0.5)
            .slot(0, ModuleKind::Stretch, [0.8, 0.5, 0.0])
            .slot(1, ModuleKind::Formant, [0.5, 0.5, 0.5])
            .slot(2, ModuleKind::Tilt, [0.5, 0.5, 0.0])
            .with(|s| s.variety = 1.0);
        for m in [36, 60, 84] {
            let f0 = chimera_core::dsp::note_to_freq(m);
            let base = frame(Partials::P64, &s, f0, &Vary::default(), 0.0, 0.0);
            for seed in 1..300 {
                let v = Vary::latch(seed, 1.0);
                assert!(v.x != 0.0, "VARIETY varies something");
                let fr = frame(Partials::P64, &s, f0, &v, 0.0, 0.0);
                assert_eq!(fr.hz[2], f0, "note {m} seed {seed}: the fundamental");
                assert_eq!(fr.hz, base.hz, "note {m} seed {seed}: a partial moved");
            }
        }
        assert_eq!(SLOTS, 4);
    }
}
