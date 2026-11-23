// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The added scope's demos (design note, "Added scope", 2026-11-10):
//! ACCENT routing, RETRIG, kit macros and pad-to-pad modulation, rendered
//! on the host prototypes for listening. Each is a law over the machines'
//! knobs at the hit, as the unit would latch them.

use super::machines::{Id, Knobs, build, spec};
use super::parts::{B, Blk, FS, Flt};
use super::{Out, STEP, add_at, hit, knob_map, knobs, lanes, run_hit, settings, slug};
use chimera_core::dsp::filter::FilterMode;
use chimera_core::dsp::voice_fx::soft;
use serde_json::json;

pub fn run(out: &mut Out) -> Result<(), String> {
    accent(out)?;
    retrig(out)?;
    macros(out)?;
    pad_mod(out)
}

/// The knob named `name` on `id`, if it has one.
fn knob(id: Id, name: &str) -> Option<usize> {
    spec(id).knobs.iter().position(|n| *n == name)
}

/// One hit with its knobs moved first.
fn hit_k(id: Id, k: Knobs, vel: f32, seed: u32) -> Vec<f32> {
    run_hit(build(id, k, vel, seed), 5.0)
}

// -------------------------------------------------------------- accent

/// ACCENT: velocity moves DECAY, TONE (or HARDNESS) and PITCH by up to
/// these at 127, from 80 (the routes' centre, so a middling hit is the
/// pad as set).
const ACCENT: [(&str, f32); 4] = [
    ("DECAY", 0.25),
    ("TONE", 0.3),
    ("HARDNESS", 0.3),
    ("PITCH", 0.06),
];

fn accented(id: Id, mut k: Knobs, vel: f32) -> Knobs {
    let d = (vel - 80.0 / 127.0) / (1.0 - 80.0 / 127.0);
    for (name, depth) in ACCENT {
        if let Some(i) = knob(id, name) {
            k[i] = (k[i] + depth * d).clamp(0.0, 1.0);
        }
    }
    k
}

fn accent(out: &mut Out) -> Result<(), String> {
    for id in [Id::ToneSnare, Id::BodyTom, Id::MetalHat, Id::FmKick] {
        let (sname, k, _) = settings(id)[0];
        let kn = knobs(id, k);
        for routed in [false, true] {
            let mut x = Vec::new();
            for (i, v) in [40u8, 80, 127].into_iter().enumerate() {
                let vel = v as f32 / 127.0;
                let kk = if routed { accented(id, kn, vel) } else { kn };
                add_at(
                    &mut x,
                    (i as f32 * 0.75 * FS) as usize,
                    &hit_k(id, kk, vel, 3 + i as u32),
                    1.0,
                );
            }
            x.resize((2.4 * FS) as usize, 0.0);
            let tag = if routed { "routed" } else { "level-only" };
            out.write(
                &format!("accent_{}_{tag}.wav", slug(spec(id).name)),
                &x,
                json!({
                    "kind": "accent", "family": "DEMO", "machine": spec(id).name, "setting": sname,
                    "knobs": knob_map(id, k), "velocities": [40, 80, 127], "routed": routed,
                    "routes": if routed { json!(ACCENT.iter().filter(|(n, _)| knob(id, n).is_some()).map(|(n, d)| json!({"knob": n, "depth_at_127": d})).collect::<Vec<_>>()) } else { json!([]) },
                    "listen_for": if routed { "velocity 40, 80, 127 moving DECAY, TONE/HARDNESS and PITCH: soft hits shorter, darker, lower; the accent longer, brighter, a touch higher" } else { "velocity 40, 80, 127 with nothing routed: only what the machine's own velocity does (mostly level)" },
                }),
            )?;
        }
    }
    Ok(())
}

// -------------------------------------------------------------- retrig

/// RETRIG from one note: hits every `rate` s for `length` s, velocity
/// ramped from `1 − ramp` (ramp > 0, up) or to it (ramp < 0, down). CUT
/// within the pad: each retrig cuts the last with a 3 ms fade.
fn retrigged(id: Id, k: Knobs, vel: f32, rate: f32, length: f32, ramp: f32) -> (Vec<f32>, usize) {
    let n = ((length / rate).floor() as usize + 1).max(2);
    let mut x: Vec<f32> = Vec::new();
    let fade = (0.003 * FS) as usize;
    for i in 0..n {
        let t = i as f32 / (n - 1) as f32;
        let v = if ramp >= 0.0 {
            1.0 - ramp * (1.0 - t)
        } else {
            1.0 + ramp * t
        };
        let at = (i as f32 * rate * FS) as usize;
        for (j, s) in x.iter_mut().enumerate().skip(at) {
            *s *= 1.0 - ((j - at) as f32 / fade as f32).min(1.0);
        }
        add_at(&mut x, at, &hit_k(id, k, vel * v, 40 + i as u32), 1.0);
    }
    (x, n)
}

fn retrig(out: &mut Out) -> Result<(), String> {
    let beat = 4.0 * STEP;
    // Name, pad, RATE (s and as shown), LENGTH (s), ramp, what to hear.
    type Roll = (&'static str, Id, f32, &'static str, f32, f32, &'static str);
    let cases: [Roll; 6] = [
        (
            "snare-roll-32nd-up",
            Id::ToneSnare,
            beat / 8.0,
            "1/32",
            beat * 2.0,
            0.8,
            "a two-beat snare roll in 32nds, crescendo",
        ),
        (
            "snare-roll-48th-down",
            Id::FmSnare,
            beat / 12.0,
            "1/48",
            beat * 1.5,
            -0.8,
            "a 1/48 roll (32nd triplets), dying away",
        ),
        (
            "snare-roll-64th-flat",
            Id::BodySnare,
            beat / 16.0,
            "1/64",
            beat,
            0.0,
            "a buzz roll in 64ths, even",
        ),
        (
            "hat-flam",
            Id::MetalHat,
            0.022,
            "22 ms",
            0.022,
            0.6,
            "a flam: a soft grace hit 22 ms before the main one",
        ),
        (
            "hat-roll-32nd-down",
            Id::MetalHat,
            beat / 8.0,
            "1/32",
            beat,
            -0.7,
            "a closed hat ratchet over a beat, falling",
        ),
        (
            "clap-flam",
            Id::ToneClap,
            0.03,
            "30 ms",
            0.03,
            0.5,
            "a doubled clap 30 ms apart",
        ),
    ];
    for (tag, id, rate, rname, length, ramp, listen) in cases {
        let (sname, k, _) = settings(id)[0];
        let (mut x, n) = retrigged(id, knobs(id, k), 1.0, rate, length, ramp);
        x.resize(x.len().max((length + 0.6) as usize * FS as usize), 0.0);
        out.write(
            &format!("retrig_{tag}.wav"),
            &x,
            json!({
                "kind": "retrig", "family": "DEMO", "machine": spec(id).name, "setting": sname,
                "knobs": knob_map(id, k), "rate": rname, "rate_s": rate, "length_s": length, "ramp": ramp,
                "hits": n, "bpm": 120, "listen_for": listen,
            }),
        )?;
    }
    Ok(())
}

// --------------------------------------------------------------- macros

/// PITCH's span in octaves on each kit machine, for TUNE's semitones.
fn octaves(id: Id) -> Option<f32> {
    match id {
        Id::ToneKick => Some(2.0),
        Id::FmSnare => Some((400.0f32 / 120.0).log2()),
        Id::MetalHat => Some(4.0),
        Id::BodyTom => Some((400.0f32 / 60.0).log2()),
        Id::ToneClap => Some(3.0),
        _ => None,
    }
}

/// The kit's lanes with kit macros at each hit: DECAY scale, TUNE
/// semitones and DRIVE (0..1) as functions of the time (0..1 over the
/// four bars).
fn kit_with(m: impl Fn(f32) -> (f32, f32, f32)) -> Vec<f32> {
    let mut x = Vec::new();
    let bars = 64.0 * STEP;
    for (_, id, set, steps, gain) in lanes() {
        let (_, k, _) = settings(id)[set];
        let kn = knobs(id, k);
        for (i, c) in steps.chars().enumerate() {
            let vel = match c {
                'X' => 1.0,
                'x' => 0.7,
                _ => continue,
            };
            let t = i as f32 * STEP;
            let (dscale, st, _) = m(t / bars);
            let mut kk = kn;
            if let Some(d) = knob(id, "DECAY") {
                kk[d] = (kk[d] * dscale).clamp(0.0, 1.0);
            }
            let p = knob(id, "PITCH").or_else(|| knob(id, "TONE").filter(|_| id == Id::ToneClap));
            if let (Some(p), Some(o)) = (p, octaves(id)) {
                kk[p] = (kk[p] + st / 12.0 / o).clamp(0.0, 1.0);
            }
            add_at(
                &mut x,
                (t * FS) as usize,
                &hit_k(id, kk, vel, 700 + i as u32),
                gain,
            );
        }
    }
    x.resize(x.len().max((bars * FS) as usize), 0.0);
    // DRIVE: the kit bus through SOFT, per sample from the time.
    let n = x.len() as f32;
    for (i, v) in x.iter_mut().enumerate() {
        let (_, _, d) = m((i as f32 / n).min(1.0));
        if d > 0.0 {
            *v = 0.8 * soft::curve(*v, [d, 0.5, soft::UNITY_LEVEL]);
        }
    }
    x
}

fn macros(out: &mut Out) -> Result<(), String> {
    type Law = fn(f32) -> (f32, f32, f32);
    let cases: [(&str, Law, &str); 3] = [
        (
            "decay",
            |t| (0.4 + 1.2 * t, 0.0, 0.0),
            "kit DECAY from 0.4x to 1.6x over four bars: every pad from tight to long at once",
        ),
        (
            "tune",
            |t| (1.0, -7.0 + 14.0 * t, 0.0),
            "kit TUNE from -7 to +7 semitones over four bars: the whole kit transposed, each pad by its own PITCH law",
        ),
        (
            "drive",
            |t| (1.0, 0.0, t),
            "kit DRIVE from 0 to full over four bars: SOFT on the kit's bus",
        ),
    ];
    for (tag, law, listen) in cases {
        let x = kit_with(law);
        out.write(
            &format!("kit-macro_{tag}.wav"),
            &x,
            json!({
                "kind": "kit-macro", "family": "DEMO", "machine": "KIT", "setting": format!("808ISH, {tag} macro"),
                "knobs": {}, "macro": tag.to_ascii_uppercase(), "bpm": 120, "listen_for": listen,
            }),
        )?;
    }
    Ok(())
}

// ------------------------------------------------------------- pad mod

/// A lane's hits: every hit's start and velocity.
fn hits(steps: &str) -> Vec<(usize, f32)> {
    steps
        .chars()
        .enumerate()
        .filter_map(|(i, c)| match c {
            'X' => Some(((i as f32 * STEP * FS) as usize, 1.0)),
            'x' => Some(((i as f32 * STEP * FS) as usize, 0.7)),
            _ => None,
        })
        .collect()
}

fn lane(id: Id, set: usize, steps: &str, len: usize) -> Vec<f32> {
    let (_, k, _) = settings(id)[set];
    let mut x = vec![0.0; len];
    for (n, (at, v)) in hits(steps).into_iter().enumerate() {
        add_at(&mut x, at, &hit(id, k, v, 300 + n as u32, 5.0), 1.0);
    }
    x.truncate(len);
    x
}

/// A source pad's envelope as a mod source: 1 at each hit (times its
/// velocity), falling to 1/e in `tau`.
fn env(steps: &str, tau: f32, len: usize) -> Vec<f32> {
    let k = (-1.0 / (tau * FS)).exp();
    let starts = hits(steps);
    let mut e = vec![0.0f32; len];
    let mut v = 0.0f32;
    let mut j = 0;
    for (i, s) in e.iter_mut().enumerate() {
        while j < starts.len() && starts[j].0 == i {
            v = v.max(starts[j].1);
            j += 1;
        }
        *s = v;
        v *= k;
    }
    e
}

fn pad_mod(out: &mut Out) -> Result<(), String> {
    let len = (32.0 * STEP * FS) as usize;
    // Kick ducking hats: the kick's envelope, −depth on the hats' level.
    let kick = "X...x...X...x...X...x...X...x.x.";
    let hat = "x.X.x.X.x.X.x.X.x.X.x.X.x.X.x.X.";
    let k = lane(Id::ToneKick, 0, kick, len);
    let h = lane(Id::MetalHat, 1, hat, len);
    let e = env(kick, 0.12, len);
    for on in [false, true] {
        let x: Vec<f32> = (0..len)
            .map(|i| 0.7 * k[i] + 0.5 * h[i] * if on { 1.0 - 0.85 * e[i] } else { 1.0 })
            .collect();
        let tag = if on { "on" } else { "off" };
        out.write(
            &format!("pad-mod_kick-ducks-hats_{tag}.wav"),
            &x,
            json!({
                "kind": "pad-mod", "family": "DEMO", "machine": "TONE KICK > METAL HAT", "setting": "kick envelope to hat level",
                "knobs": {}, "source": "KICK pad envelope (1/e 120 ms)", "destination": "OPEN HAT pad LEVEL", "depth": if on { -0.85 } else { 0.0 }, "bpm": 120,
                "listen_for": if on { "the open hats pumping: pushed down at each kick, swelling back between" } else { "the same kick and hats, no route: the reference" },
            }),
        )?;
    }
    // Snare envelope opening a tom's filter.
    let snare = "....X.......X.......X.......X...";
    let tom = "X.x.x.X.x.x.X.x.X.x.x.X.x.x.X.x.";
    let s = lane(Id::FmSnare, 0, snare, len);
    let t = lane(Id::BodyTom, 0, tom, len);
    let e = env(snare, 0.25, len);
    for on in [false, true] {
        let mut f = Flt::new(
            chimera_core::dsp::filter::FilterKind::Svf,
            FilterMode::Lp12,
            250.0,
            0.3,
        );
        let mut y = vec![0.0f32; len.div_ceil(B) * B];
        y[..len].copy_from_slice(&t);
        for (bi, b) in y.as_chunks_mut::<B>().0.iter_mut().enumerate() {
            let m = if on { e[(bi * B).min(len - 1)] } else { 0.0 };
            let blk: &mut Blk = b;
            f.run(blk, 250.0 * (5.0 * m).exp2());
        }
        let x: Vec<f32> = (0..len).map(|i| 0.55 * s[i] + 0.8 * y[i]).collect();
        let tag = if on { "on" } else { "off" };
        out.write(
            &format!("pad-mod_snare-opens-tom-filter_{tag}.wav"),
            &x,
            json!({
                "kind": "pad-mod", "family": "DEMO", "machine": "FM SNARE > BODY TOM", "setting": "snare envelope to tom cutoff",
                "knobs": {}, "source": "SNARE pad envelope (1/e 250 ms)", "destination": "TOM pad FILTER cutoff (CLEAN LP12 at 250 Hz)", "depth_octaves": if on { 5.0 } else { 0.0 }, "bpm": 120,
                "listen_for": if on { "the dull toms flaring bright on each snare, closing again after" } else { "the toms through a fixed dark low-pass under the snare: the reference" },
            }),
        )?;
    }
    Ok(())
}
