// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The audition groups. Short musical phrases (a held note, a chord stab,
//! a bass line, a slow pad swell), per the listening rules.

use super::voice::{EnvX, PathCtl};
use super::{
    Adsr, Bright, ModuleKind as K, Note, Out, Partials, PathShape, Sound, SubKind, SubOctave,
    SubWave, controls, render,
};
use serde_json::{Value, json};

const HELD: Adsr = Adsr::new(0.005, 0.3, 0.8, 0.3);
const STAB: Adsr = Adsr::new(0.002, 0.12, 0.5, 0.12);
const BASS: Adsr = Adsr::new(0.002, 0.15, 0.6, 0.06);
const PAD: Adsr = Adsr::new(1.5, 0.5, 1.0, 1.2);
const PLUCKED: Adsr = Adsr::new(0.002, 0.35, 0.0, 0.15);

const ROWS: [&str; 5] = ["bell", "organ", "reed", "brass", "voice"];

fn row_y(r: usize) -> f32 {
    r as f32 / 4.0
}

fn chord(ms: &[u8], at: f32, len: f32) -> Vec<Note> {
    ms.iter().map(|&m| (m, at, len)).collect()
}

fn cm_stab() -> Vec<Note> {
    chord(&[48, 51, 55], 0.05, 0.22)
}

fn cm7_pad(len: f32) -> Vec<Note> {
    chord(&[48, 51, 55, 58], 0.0, len)
}

/// C2 C2 Eb2 G1 in eighths at 120 BPM, twice.
fn bass_line() -> Vec<Note> {
    let mut v = Vec::new();
    for bar in 0..2 {
        let t = bar as f32 * 1.0;
        v.extend([
            (36, t, 0.2),
            (36, t + 0.25, 0.2),
            (39, t + 0.5, 0.2),
            (31, t + 0.75, 0.2),
        ]);
    }
    v
}

/// The FORMANT FREQ knob for `hz`.
fn fk(hz: f32) -> f32 {
    (hz / 150.0).ln() / (5000.0f32 / 150.0).ln()
}

struct G<'a, 'b> {
    out: &'a mut Out<'b>,
    group: &'static str,
    bright: Bright,
}

impl G<'_, '_> {
    #[allow(clippy::too_many_arguments)]
    fn file(
        &mut self,
        name: &str,
        n: Partials,
        s: &Sound,
        notes: &[Note],
        auto: &dyn Fn(f32, &mut Sound),
        sweep: Option<Value>,
        about: &str,
    ) -> Result<(), String> {
        let s = &s.with(|s| s.bright = self.bright);
        let x = render(n, s, notes, auto);
        let mut c = controls(n, s);
        if let Some(sw) = sweep {
            c["sweep"] = sw;
        }
        let row = json!({"group": self.group, "description": about, "controls": c});
        self.out
            .write(&format!("{}_{name}.wav", self.group), &x, row)
    }
}

fn sweep_json(what: &str, from: f32, to: f32, secs: f32) -> Value {
    json!({"control": what, "from": from, "to": to, "seconds": secs})
}

pub fn all(out: &mut Out) -> Result<Value, String> {
    let n64 = Partials::P64;
    let mut groups = serde_json::Map::new();
    let mut add = |id: &str, d: &str| {
        groups.insert(id.into(), json!(d));
    };

    // 1: partial count
    add(
        "1-partials",
        "Partial count A/B: one bright ORGAN and one buzzy BRASS patch on a C1 bass and a held C3, at 32, 48 and 64 partials; level held, so only the top changes.",
    );
    let mut g = G {
        out,
        group: "1-partials",
        bright: Bright::DARK,
    };
    for (pname, s) in [
        ("organ", Sound::new(1.0, 0.25)),
        ("brass", Sound::new(1.0, 0.75)),
    ] {
        for (nname, notes, env) in [
            (
                "c1bass",
                vec![(24u8, 0.0, 0.4), (24, 0.5, 0.4), (24, 1.0, 1.0)],
                BASS,
            ),
            ("c3held", vec![(48u8, 0.0, 2.0)], HELD),
        ] {
            for n in [Partials::P32, Partials::P48, Partials::P64] {
                let s = s.with(|s| s.amp = env);
                g.file(
                    &format!("{pname}_{nname}_{}", n.get()),
                    n,
                    &s,
                    &notes,
                    &|_, _| {},
                    None,
                    &format!(
                        "{} {} at {} partials",
                        if pname == "organ" {
                            "bright ORGAN (full drawbars and mixtures)"
                        } else {
                            "buzzy BRASS (k^-0.3, bump at the 14th)"
                        },
                        if nname == "c1bass" {
                            "C1 bass, three hits"
                        } else {
                            "C3 held 2 s"
                        },
                        n.get()
                    ),
                )?;
            }
        }
    }

    // 2: the map
    add(
        "2-map",
        "The map: each row (BELL, ORGAN, REED, BRASS/BOWED, VOICE) at X 0, 0.5 and 1 on a held C3 (1.5 s); then an 8 s X sweep along each row and Y sweeps down the field at X 0.25 and 0.75.",
    );
    let mut g = G {
        out: g.out,
        group: "2-map",
        bright: Bright::DARK,
    };
    for (r, row) in ROWS.iter().enumerate() {
        for (xn, x) in [("xlo", 0.0), ("xmid", 0.5), ("xhi", 1.0)] {
            let s = Sound::new(x, row_y(r));
            g.file(
                &format!("{row}_{xn}"),
                n64,
                &s,
                &[(48, 0.0, 1.5)],
                &|_, _| {},
                None,
                &format!("{} row at X {x}: held C3 1.5 s", super::map::ROWS[r]),
            )?;
        }
    }
    for (r, row) in ROWS.iter().enumerate() {
        let s = Sound::new(0.0, row_y(r));
        g.file(
            &format!("sweep_x_{row}"),
            n64,
            &s,
            &[(48, 0.0, 8.0)],
            &|t, s| s.x = (t / 8.0).min(1.0),
            Some(sweep_json("x", 0.0, 1.0, 8.0)),
            &format!(
                "X swept dark to bright along the {} row over 8 s, held C3",
                super::map::ROWS[r]
            ),
        )?;
    }
    for x in [0.25, 0.75] {
        let s = Sound::new(x, 0.0);
        g.file(
            &format!("sweep_y_x{}", (x * 100.0) as u32),
            n64,
            &s,
            &[(48, 0.0, 8.0)],
            &|t, s| s.y = (t / 8.0).min(1.0),
            Some(sweep_json("y", 0.0, 1.0, 8.0)),
            &format!("Y swept BELL to VOICE at X {x} over 8 s, held C3"),
        )?;
    }

    // 3: DENSITY
    add(
        "3-density",
        "DENSITY: swept 0 to 1 on a Cm7 pad (strongest partials first, level held), then a C minor stab at 0.1, 0.35 and 1.",
    );
    let mut g = G {
        out: g.out,
        group: "3-density",
        bright: Bright::DARK,
    };
    let pad = Sound::new(0.6, 0.75).with(|s| {
        s.amp = Adsr::new(0.8, 0.5, 1.0, 1.2);
        s.density = 0.0;
    });
    g.file(
        "sweep_pad",
        n64,
        &pad,
        &cm7_pad(8.0),
        &|t, s| s.density = (t / 7.5).min(1.0),
        Some(sweep_json("density", 0.0, 1.0, 7.5)),
        "DENSITY 0 to 1 over 7.5 s on a Cm7 pad (BRASS/BOWED, X 0.6)",
    )?;
    for d in [0.1, 0.35, 1.0] {
        let s = Sound::new(0.6, 0.75).with(|s| {
            s.amp = STAB;
            s.density = d;
        });
        g.file(
            &format!("stab_{}", (d * 100.0) as u32),
            n64,
            &s,
            &cm_stab(),
            &|_, _| {},
            None,
            &format!("C minor stab at DENSITY {d}"),
        )?;
    }

    // 4: modules
    add(
        "4-modules",
        "Each module alone on a saw (BRASS/BOWED X 0.5), its main knob swept 0 to 1 on a held C3 over 6 s (PLUCK: DECAY stepped over eight notes); then FORMANT+FORMANT (two vowel peaks) and STRETCH->PLUCK (a struck bell).",
    );
    let mut g = G {
        out: g.out,
        group: "4-modules",
        bright: Bright::DARK,
    };
    let saw = Sound::new(0.5, 0.75).with(|s| s.amp = HELD);
    let sweeps: [(&str, K, [f32; 3], &str); 5] = [
        (
            "tilt",
            K::Tilt,
            [0.0, 0.0, 0.0],
            "TILT SLOPE (-12 dB/oct to +6, pivot on the fundamental)",
        ),
        (
            "oddeven",
            K::OddEven,
            [0.0; 3],
            "ODD/EVEN BALANCE (odds thinned: octave-ish, to evens thinned: hollow)",
        ),
        (
            "formant",
            K::Formant,
            [0.0, 0.35, 0.5],
            "FORMANT FREQ 150 Hz to 5 kHz (two peaks, width 0.35)",
        ),
        (
            "stretch",
            K::Stretch,
            [0.0, 0.0, 0.0],
            "STRETCH AMOUNT (compressed, harmonic at 0.5, to stiff-bar stretch)",
        ),
        (
            "comb",
            K::Comb,
            [0.35, 0.0, 0.85],
            "COMB OFFSET, one full cycle (spacing ~4 partials, depth 0.85): the partial-domain phaser",
        ),
    ];
    for (name, kind, k, about) in sweeps {
        let knob = if kind == K::Comb { 1 } else { 0 };
        let s = saw.slot(0, kind, k);
        g.file(
            &format!("sweep_{name}"),
            n64,
            &s,
            &[(48, 0.0, 6.0)],
            &move |t, s| s.set_knob(0, knob, (t / 6.0).min(1.0)),
            Some(sweep_json(kind.knobs()[knob], 0.0, 1.0, 6.0)),
            &format!("{about}, held C3 6 s"),
        )?;
    }
    let notes: Vec<Note> = (0..8).map(|i| (48, i as f32 * 0.75, 0.6)).collect();
    let s = saw.slot(0, K::Pluck, [0.0, 0.5, 0.0]);
    g.file(
        "steps_pluck",
        n64,
        &s,
        &notes,
        &|t, s| s.set_knob(0, 0, ((t / 0.75).floor() / 7.0).min(1.0)),
        Some(json!({"control": "DECAY", "steps": [0.0, 0.143, 0.286, 0.429, 0.571, 0.714, 0.857, 1.0], "per_note": true})),
        "PLUCK DECAY stepped from 0.05 s to off over eight C3 notes (SLOPE 0.5: highs die first)",
    )?;
    let ff =
        saw.slot(0, K::Formant, [fk(500.0), 0.2, 0.0])
            .slot(1, K::Formant, [fk(1500.0), 0.2, 0.0]);
    let melody: Vec<Note> = [48u8, 51, 55, 60]
        .iter()
        .enumerate()
        .map(|(i, &m)| (m, i as f32 * 0.6, 0.55))
        .collect();
    g.file(
        "formant_formant",
        n64,
        &ff,
        &melody,
        &|_, _| {},
        None,
        "FORMANT + FORMANT: single peaks at 500 Hz and 1.5 kHz (two vowel regions, fixed in Hz), C3 Eb3 G3 C4",
    )?;
    let bell = Sound::new(0.5, 0.0)
        .slot(0, K::Stretch, [0.8, 0.3, 0.0])
        .slot(1, K::Pluck, [0.6, 0.6, 0.0])
        .with(|s| s.amp = Adsr::new(0.001, 1.0, 1.0, 2.0));
    let strikes: Vec<Note> = [72u8, 79, 84, 75]
        .iter()
        .enumerate()
        .map(|(i, &m)| (m, i as f32 * 0.6, 0.5))
        .collect();
    g.file(
        "stretch_pluck",
        n64,
        &bell,
        &strikes,
        &|_, _| {},
        None,
        "STRETCH -> PLUCK: BELL row stretched like a stiff bar, highs decaying first: a struck bell, C5 G5 C6 Eb5",
    )?;

    // 5: Loom-style morph
    add(
        "5-morph",
        "Loom-style morph: four corner variations of TILT, FORMANT, COMB, ODD/EVEN moved by PATH (CIRCLE over 2 bars on a pad, LINE and FIGURE-8 too; ZIGZAG at 1/4 bar on a pulsing chord), and ENV->X on a plucked bass line.",
    );
    let mut g = G {
        out: g.out,
        group: "5-morph",
        bright: Bright::DARK,
    };
    let corners = Sound::new(0.5, 0.5)
        .slot(0, K::Tilt, [0.3, 0.0, 0.0])
        .slot(1, K::Formant, [fk(400.0), 0.4, 0.0])
        .slot(2, K::Comb, [0.3, 0.0, 0.0])
        .slot(3, K::OddEven, [0.5, 0.0, 0.0])
        .corner(1, 0, [0.75, 0.2, 0.0])
        .corner(1, 1, [fk(2000.0), 0.3, 0.5])
        .corner(1, 3, [0.9, 0.0, 0.0])
        .corner(2, 0, [0.5, 0.0, 0.0])
        .corner(2, 1, [fk(800.0), 0.25, 1.0])
        .corner(2, 2, [0.4, 0.25, 0.8])
        .corner(3, 0, [0.8, 0.0, 0.0])
        .corner(3, 1, [fk(3000.0), 0.5, 0.0])
        .corner(3, 2, [0.2, 0.6, 0.9])
        .corner(3, 3, [0.2, 0.0, 0.0]);
    for (shape, bars, len) in [
        (PathShape::Circle, 2.0, 8.0),
        (PathShape::Line, 1.0, 6.0),
        (PathShape::Figure8, 2.0, 8.0),
    ] {
        let s = corners.with(|s| {
            s.amp = Adsr::new(1.0, 0.5, 1.0, 1.2);
            s.path = PathCtl {
                shape,
                bars,
                size: 0.4,
            };
        });
        g.file(
            &format!("pad_{}", shape.name().to_ascii_lowercase()),
            n64,
            &s,
            &cm7_pad(len),
            &|_, _| {},
            None,
            &format!(
                "corners + PATH {} over {bars} bar(s) at 120 BPM, size 0.4, on a Cm7 pad ({len} s)",
                shape.name()
            ),
        )?;
    }
    let pulses: Vec<Note> = (0..16)
        .flat_map(|i| chord(&[48, 51, 55, 58], i as f32 * 0.25, 0.16))
        .collect();
    let s = corners.with(|s| {
        s.amp = STAB;
        s.path = PathCtl {
            shape: PathShape::Zigzag,
            bars: 0.25,
            size: 0.45,
        };
    });
    g.file(
        "pulse_zigzag",
        n64,
        &s,
        &pulses,
        &|_, _| {},
        None,
        "corners + PATH ZIGZAG at 1/4 bar, size 0.45, on a Cm7 chord pulsing in eighths for 2 bars",
    )?;
    let s = Sound::new(0.05, 0.75).with(|s| {
        s.amp = PLUCKED;
        s.env_x = EnvX {
            amount: 0.85,
            decay: 0.12,
        };
    });
    g.file(
        "envx_bass",
        n64,
        &s,
        &bass_line(),
        &|_, _| {},
        None,
        "ENV->X 0.85 (decay 0.12 s) on BRASS/BOWED from X 0.05: each note opens bright and closes, a plucked bass line",
    )?;

    // 6: AIR and SUB
    add(
        "6-air-sub",
        "AIR (noise shaped by the spectrum) and SUB (partials at 1/2 and 1/4) off and on, on a Cm7 pad and a bass line.",
    );
    let mut g = G {
        out: g.out,
        group: "6-air-sub",
        bright: Bright::DARK,
    };
    let air_pad = Sound::new(0.5, 1.0).with(|s| s.amp = PAD);
    let air_bass = Sound::new(0.6, 0.5).with(|s| s.amp = BASS);
    for (ctl, v) in [("air", 0.7), ("sub", 0.8)] {
        for on in [false, true] {
            let set = |s: Sound| {
                s.with(|s| {
                    if on {
                        if ctl == "air" { s.air = v } else { s.sub = v }
                    }
                })
            };
            let tag = if on { "on" } else { "off" };
            g.file(
                &format!("{ctl}_{tag}_pad"),
                n64,
                &set(air_pad),
                &cm7_pad(3.2),
                &|_, _| {},
                None,
                &format!(
                    "{} {} on a Cm7 pad (VOICE row, vowel A)",
                    ctl.to_ascii_uppercase(),
                    if on { v } else { 0.0 }
                ),
            )?;
            g.file(
                &format!("{ctl}_{tag}_bass"),
                n64,
                &set(air_bass),
                &bass_line(),
                &|_, _| {},
                None,
                &format!(
                    "{} {} on a bass line (REED row, X 0.6)",
                    ctl.to_ascii_uppercase(),
                    if on { v } else { 0.0 }
                ),
            )?;
        }
    }

    // 7: VARIETY
    add(
        "7-variety",
        "VARIETY 0 against 0.5 on eight repeated C minor stabs: per-note offsets on X, Y, DENSITY, AIR and the slot knobs, never pitch.",
    );
    let mut g = G {
        out: g.out,
        group: "7-variety",
        bright: Bright::DARK,
    };
    let hits: Vec<Note> = (0..8)
        .flat_map(|i| chord(&[48, 51, 55], i as f32 * 0.5, 0.22))
        .collect();
    for v in [0.0, 0.5] {
        let s = Sound::new(0.5, 0.5)
            .slot(0, K::Formant, [fk(900.0), 0.4, 0.3])
            .slot(1, K::Tilt, [0.55, 0.2, 0.0])
            .with(|s| {
                s.amp = STAB;
                s.density = 0.6;
                s.air = 0.15;
                s.variety = v;
            });
        g.file(
            &format!("stabs_{}", (v * 100.0) as u32),
            n64,
            &s,
            &hits,
            &|_, _| {},
            None,
            &format!("eight C minor stabs at VARIETY {v} (REED row, FORMANT 900 Hz, TILT)"),
        )?;
    }

    // 8: sound sketches
    add(
        "8-sketches",
        "Sound sketches: finished-sounding patches named like factory Sounds, each a 4-6 s phrase.",
    );
    let mut g = G {
        out: g.out,
        group: "8-sketches",
        bright: Bright::DARK,
    };
    let sk = sketches();
    for k in &sk {
        g.file(k.name, n64, &k.sound, &k.notes, &|_, _| {}, None, k.about)?;
    }

    // 9: brighter
    add(
        "9-brighter",
        "Brighter (owner: \"darker than Loom\"): each sketch as an A/B, _dark the first prototype (regenerated identically to group 8) and _bright the new defaults (brighter map and taper, aligned phases, BRIGHT TAIL; LOG off). Then single-feature A/Bs on a buzzy BRASS C1 bass and a bright ORGAN C3 held: dark, map only (brighter map + taper), phase only (aligned), tail only, log only (log-spaced partials), and all (every option; at C1 and C3 LOG covers the top, so the tail rests). Same fixed gain; LEVEL holds a voice's RMS, the tail inside it, so bright is not louder by design: compare peaks and RMS in the files' rows.",
    );
    let mut g = G {
        out: g.out,
        group: "9-brighter",
        bright: Bright::DARK,
    };
    for k in &sk {
        for (tag, b) in [("dark", Bright::DARK), ("bright", Bright::ON)] {
            g.bright = b;
            g.file(
                &format!("{}_{tag}", k.name),
                n64,
                &k.sound,
                &k.notes,
                &|_, _| {},
                None,
                &format!("{} [{tag}]", k.about),
            )?;
        }
    }
    let d = Bright::DARK;
    let variants: [(&str, Bright); 6] = [
        ("dark", d),
        ("map", Bright { map: true, ..d }),
        ("phase", Bright { phase: 0.0, ..d }),
        ("tail", Bright { tail: true, ..d }),
        ("log", Bright { log: true, ..d }),
        (
            "all",
            Bright {
                log: true,
                ..Bright::ON
            },
        ),
    ];
    for (pname, sound, nname, notes, env, about) in [
        (
            "brass",
            Sound::new(1.0, 0.75),
            "c1bass",
            vec![(24u8, 0.0, 0.4), (24, 0.5, 0.4), (24, 1.0, 1.0)],
            BASS,
            "buzzy BRASS (X 1, Y 0.75), C1 bass, three hits",
        ),
        (
            "organ",
            Sound::new(1.0, 0.25),
            "c3held",
            vec![(48u8, 0.0, 2.0)],
            HELD,
            "bright ORGAN (X 1, Y 0.25), C3 held 2 s",
        ),
    ] {
        for (tag, b) in variants {
            g.bright = b;
            g.file(
                &format!("{pname}_{nname}_{tag}"),
                n64,
                &sound.with(|s| s.amp = env),
                &notes,
                &|_, _| {},
                None,
                &format!("{about} [{tag}]"),
            )?;
        }
    }
    // 10: C3-up
    add(
        "10-c3-up",
        "C3-up (owner: \"bright from C3 up\"): 160 exact partials, the whole audio range from C3 up; below C3 the same 160 top out lower (darker, no cutoff). Brighter map and taper, aligned phases; no tail, no LOG. SUB is VA's band-limited oscillator (SINE, TRIANGLE or SQUARE, -1 or -2 octaves, LEVEL) in place of the 1/2 and 1/4 partials. The six sketches with a SUB set per sketch; a melody from C1 to C7 (hear the darkening below C3); each SUB wave at -1 and -2 under a held C3 and a bass line. Same fixed gain.",
    );
    let mut g = G {
        out: g.out,
        group: "10-c3-up",
        bright: Bright {
            tail: false,
            log: false,
            ..Bright::ON
        },
    };
    let n160 = Partials::P160;
    let va = |s: Sound, wave: SubWave, octave: SubOctave, level: f32| {
        s.with(|s| {
            s.sub = level;
            s.sub_kind = SubKind::Va { wave, octave };
        })
    };
    use SubOctave::{Down1, Down2};
    use SubWave::{Sine, Square, Triangle};
    let subs: [(SubWave, SubOctave, f32, &str); 6] = [
        (Sine, Down1, 0.35, "a 16' sine under the registration"),
        (Sine, Down1, 0.4, "a -1 sine under the vowels"),
        (Sine, Down1, 0.25, "a -1 sine: the bell's hum note"),
        (Square, Down1, 0.5, "a -1 square under the reed"),
        (Triangle, Down1, 0.3, "a -1 triangle thickening the stabs"),
        (Triangle, Down1, 0.25, "a -1 triangle under the strings"),
    ];
    for (k, (w, o, l, why)) in sketches().iter().zip(subs) {
        g.file(
            k.name,
            n160,
            &va(k.sound, w, o, l),
            &k.notes,
            &|_, _| {},
            None,
            &format!(
                "{} [160 partials; SUB {} {} at {l}: {why}]",
                k.about,
                w.name(),
                o.name()
            ),
        )?;
    }
    let mel: Vec<Note> = (0..6u8)
        .flat_map(|o| {
            [0u8, 3, 7, 10]
                .into_iter()
                .enumerate()
                .map(move |(i, d)| (24 + 12 * o + d, (o as usize * 4 + i) as f32 * 0.3, 0.27))
        })
        .chain([(96u8, 7.2, 0.8)])
        .collect();
    g.file(
        "melody_c1_c7",
        n160,
        &Sound::new(0.8, 0.75).with(|s| s.amp = Adsr::new(0.003, 0.2, 0.7, 0.15)),
        &mel,
        &|_, _| {},
        None,
        "a Cm7 arpeggio climbing from C1 to C7 at 160 partials (BRASS/BOWED X 0.8, no SUB): full range from C3 up, gracefully darker below",
    )?;
    let base = Sound::new(0.5, 0.75);
    for w in SubWave::ALL {
        for o in [Down1, Down2] {
            let tag = format!("sub_{}_{}", w.name().to_ascii_lowercase(), &o.name()[1..]);
            g.file(
                &format!("{tag}_c3held"),
                n160,
                &va(base.with(|s| s.amp = HELD), w, o, 0.6),
                &[(48, 0.0, 2.0)],
                &|_, _| {},
                None,
                &format!(
                    "SUB {} {} at 0.6 under a held C3 (BRASS/BOWED X 0.5), 160 partials",
                    w.name(),
                    o.name()
                ),
            )?;
            g.file(
                &format!("{tag}_bass"),
                n160,
                &va(base.with(|s| s.amp = BASS), w, o, 0.6),
                &bass_line(),
                &|_, _| {},
                None,
                &format!(
                    "SUB {} {} at 0.6 under a bass line (BRASS/BOWED X 0.5), 160 partials",
                    w.name(),
                    o.name()
                ),
            )?;
        }
    }
    Ok(Value::Object(groups))
}

/// A sound sketch: a finished-sounding patch and its phrase.
pub struct Sketch {
    pub name: &'static str,
    pub sound: Sound,
    pub notes: Vec<Note>,
    pub about: &'static str,
}

pub fn sketches() -> Vec<Sketch> {
    let organ_env = Adsr::new(0.005, 0.1, 1.0, 0.15);
    let prog: Vec<Note> = [
        chord(&[48, 51, 55, 58], 0.0, 1.9),
        chord(&[44, 48, 51, 55], 2.0, 1.9),
        chord(&[46, 50, 53, 58], 4.0, 1.0),
    ]
    .concat();
    let glass = Sound::new(0.85, 0.25)
        .slot(0, K::Tilt, [0.6, 0.3, 0.0])
        .with(|s| {
            s.amp = organ_env;
            s.air = 0.1;
        });
    let vowel = Sound::new(0.3, 0.9).with(|s| {
        s.amp = Adsr::new(1.2, 0.5, 1.0, 1.2);
        s.air = 0.35;
        s.path = PathCtl {
            shape: PathShape::Circle,
            bars: 2.0,
            size: 0.25,
        };
    });
    let bell_row = Sound::new(0.55, 0.0)
        .slot(0, K::Stretch, [0.78, 0.5, 0.0])
        .slot(1, K::Pluck, [0.55, 0.6, 0.0])
        .with(|s| s.amp = Adsr::new(0.001, 1.0, 1.0, 1.2));
    let tune: Vec<Note> = [72u8, 79, 75, 82, 84, 79, 75, 72]
        .iter()
        .enumerate()
        .map(|(i, &m)| (m, i as f32 * 0.4, 0.3))
        .collect();
    let reed_bass = Sound::new(0.35, 0.5)
        .slot(0, K::OddEven, [0.85, 0.0, 0.0])
        .slot(1, K::Pluck, [0.62, 0.5, 0.0])
        .with(|s| {
            s.amp = Adsr::new(0.002, 0.2, 0.6, 0.08);
            s.sub = 0.5;
            s.env_x = EnvX {
                amount: 0.5,
                decay: 0.2,
            };
        });
    let line: Vec<Note> = [
        36u8, 36, 39, 36, 43, 41, 39, 34, 36, 36, 39, 36, 43, 46, 48, 43,
    ]
    .iter()
    .enumerate()
    .map(|(i, &m)| (m, i as f32 * 0.25, 0.18))
    .collect();
    let brass = Sound::new(0.55, 0.75)
        .slot(0, K::Formant, [fk(1100.0), 0.5, 0.3])
        .with(|s| {
            s.amp = Adsr::new(0.03, 0.3, 0.7, 0.15);
            s.env_x = EnvX {
                amount: 0.35,
                decay: 0.25,
            };
        });
    let stabs: Vec<Note> = [
        chord(&[48, 51, 55], 0.0, 0.3),
        chord(&[48, 51, 55], 0.5, 0.15),
        chord(&[48, 51, 55], 0.75, 0.6),
        chord(&[44, 48, 51], 1.5, 0.3),
        chord(&[46, 50, 53], 2.0, 0.3),
        chord(&[46, 50, 53], 2.5, 0.15),
        chord(&[48, 51, 55], 2.75, 1.2),
    ]
    .concat();
    let comb = Sound::new(0.35, 0.7)
        .slot(0, K::Comb, [0.4, 0.0, 0.7])
        .corner(1, 0, [0.4, 1.0, 0.7])
        .corner(3, 0, [0.4, 1.0, 0.7])
        .with(|s| {
            s.amp = Adsr::new(0.6, 0.5, 1.0, 1.5);
            s.air = 0.2;
            s.path = PathCtl {
                shape: PathShape::Line,
                bars: 2.0,
                size: 0.3,
            };
        });
    let sk = |name, sound, notes, about| Sketch {
        name,
        sound,
        notes,
        about,
    };
    vec![
        sk(
            "glass_organ",
            glass,
            prog,
            "GLASS ORGAN: ORGAN near full registration, tilted bright, a breath of AIR; Cm7, Abmaj7, Bb",
        ),
        sk(
            "vowel_pad",
            vowel,
            chord(&[48, 55, 58, 62, 63], 0.0, 4.5),
            "VOWEL PAD: the VOICE row circling U-O-A over 2 bars with AIR; Cm9 swell",
        ),
        sk(
            "bell_row",
            bell_row,
            tune,
            "BELL ROW: BELL spectra stretched like bars, highs dying first; a C minor tune",
        ),
        sk(
            "reed_bass",
            reed_bass,
            line,
            "REED BASS: hollow REED (odds only), ENV->X snap, SUB under it; two bars of eighths",
        ),
        sk(
            "brass_stabs",
            brass,
            stabs,
            "BRASS STABS: BRASS/BOWED with a fixed 1.1 kHz formant and ENV->X bite; a stab riff",
        ),
        sk(
            "comb_strings",
            comb,
            chord(&[41, 48, 55, 56, 63], 0.0, 4.0),
            "COMB STRINGS: bowed spectra through a COMB whose OFFSET rides PATH LINE across X: a slow phaser; Fm(add9)",
        ),
    ]
}
