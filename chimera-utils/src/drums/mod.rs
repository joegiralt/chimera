//! DRUM (0.4, #214): host prototypes of the design note's machines and of
//! the sample layer's COMBINE, rendered for the owner to pick the ship
//! list by ear (ruling 6, 2026-11-10). Built from chimera-engine's parts
//! (`parts`); never in a firmware build. Kept as model work.
//!
//! `drum-render OUTDIR`: per shipping machine its settings' single hits
//! (classic, mid, extreme, or named) and a two-bar pattern at 120 BPM; a
//! kit groove; COMBINE's modes on two hits; the clips at 12-bit, 8-bit
//! and 4-bit ADPCM; the three fixes before and after (`fixes`); ACCENT,
//! RETRIG, kit macros and pad-to-pad modulation (`demos`); VOX's beatbox
//! machines (consonant first; the formant first cut kept in `vox_formant`
//! for its before hits), a groove and a TRACT-per-hit demo (`vox`). 48 kHz
//! mono float WAVs at their real levels, `manifest.json` beside them, and
//! a host cost estimate per machine against ALGO's six-operator voice.

mod demos;
mod fixes;
mod machines;
pub(crate) mod parts;
mod sample;
mod vox;
mod vox_formant;
mod world;

use crate::reference::write_f32;
use machines::{Id, Knobs, SPECS, build, spec};
use parts::{B, Blk, FS};
use sample::{Depth, Dir, Mode, pad, store};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Instant;

/// A setting: its name, the six knobs (0..127) and what to listen for.
type Setting = (&'static str, &'static [u8], &'static str);

fn settings(id: Id) -> Vec<Setting> {
    use Id::*;
    match id {
        ToneKick => vec![
            (
                "classic",
                &[40, 80, 40, 40, 20, 10],
                "long round low boom, short downward chirp, a soft tick on top",
            ),
            (
                "mid",
                &[60, 50, 70, 30, 60, 50],
                "tighter punchy kick: more chirp, audible click, a little saturation",
            ),
            (
                "extreme",
                &[20, 120, 127, 100, 127, 127],
                "four-octave laser drop into a driven sub; check the drive's grit and the very long tail",
            ),
        ],
        ToneSnare => vec![
            (
                "classic",
                &[50, 40, 40, 80, 50, 70],
                "two-head tonal body under a mid noise snap; balance of tone and noise",
            ),
            (
                "mid",
                &[80, 30, 70, 100, 30, 100],
                "higher, brighter, snappier: short bright noise, pitch bump on the heads",
            ),
            (
                "extreme",
                &[20, 110, 127, 127, 127, 20],
                "low tuned heads with an octave bump and a long dark noise wash",
            ),
        ],
        ToneTom => vec![
            (
                "classic",
                &[50, 70, 30, 40, 20, 20],
                "classic analog floor tom: gentle downward sweep, round sine",
            ),
            (
                "mid",
                &[80, 50, 60, 20, 50, 60],
                "higher tom, faster sweep, a breath of noise, DAMP choking the tail",
            ),
            (
                "extreme",
                &[15, 127, 127, 127, 100, 0],
                "two-octave slow dive from a sub tom with noise: long and boomy",
            ),
        ],
        ToneClap => vec![
            (
                "classic",
                &[60, 50, 54, 50, 90, 30],
                "four quick claps then a band-passed room tail: the 808/909 hand clap shape",
            ),
            (
                "mid",
                &[90, 30, 36, 30, 60, 60],
                "tighter, brighter three-clap, more ring on the band-pass",
            ),
            (
                "extreme",
                &[30, 120, 127, 127, 127, 115],
                "eight slow claps into a long dark resonant tail; check the ringing pitch",
            ),
        ],
        ToneRim => vec![
            (
                "classic",
                &[60, 30, 50, 60, 60, 30],
                "short woody rim knock: two pinged modes and a click",
            ),
            (
                "mid",
                &[90, 50, 80, 30, 90, 60],
                "higher, ringier, more noise: closer to a clave/side-stick",
            ),
            (
                "extreme",
                &[20, 120, 127, 127, 127, 127],
                "low pinged modes ringing long with heavy click and noise: a metallic knock",
            ),
        ],
        ToneCowbell => vec![
            (
                "classic",
                &[55, 50, 60, 70, 40, 30],
                "two detuned pulses at about 1.48 apart through a band-pass: the 808 cowbell shape",
            ),
            (
                "mid",
                &[80, 30, 30, 90, 70, 70],
                "higher, narrower interval, more ring and drive: a clank",
            ),
            (
                "extreme",
                &[20, 120, 127, 30, 120, 127],
                "low wide-interval pulses, dark resonant band-pass, heavy drive, long",
            ),
        ],
        ToneCymbal => vec![
            (
                "classic",
                &[64, 80, 70, 30, 30, 40],
                "metallic ride/crash wash from the six squares, gentle shimmer",
            ),
            (
                "mid",
                &[80, 60, 90, 70, 60, 80],
                "brighter, more inharmonic, more resonant and moving",
            ),
            (
                "extreme",
                &[20, 127, 30, 127, 120, 127],
                "low dark gong-like wash with heavy shimmer; check the LFO's audible wobble",
            ),
        ],
        MetalHat => vec![
            (
                "classic",
                &[64, 20, 80, 0, 30, 15],
                "short closed hat from the 808 square set",
            ),
            (
                "mid",
                &[64, 75, 70, 40, 50, 30],
                "open hat: longer, a little spread and ring",
            ),
            (
                "extreme",
                &[0, 110, 20, 127, 120, 100],
                "two octaves down, dark and ringing: a trashy metal hit",
            ),
        ],
        FmKick => vec![
            (
                "classic",
                &[45, 70, 45, 50, 20, 30],
                "FM kick: ratio-1 modulator gives a short knocky attack over a swept sine",
            ),
            (
                "mid",
                &[60, 50, 60, 80, 50, 40],
                "ratio 2, more MOD: a brighter, harder attack",
            ),
            (
                "extreme",
                &[25, 110, 127, 127, 120, 100],
                "ratio 7, full MOD, long MOD DECAY, full sweep: metallic noisy boom; listen for steps in the sweep (the kernel's pitch is a block's)",
            ),
        ],
        FmSnare => vec![
            (
                "classic",
                &[60, 40, 90, 50, 70, 40],
                "FM body (ratio 1.41) under a high-passed noise: a Digitone-style snare",
            ),
            (
                "mid",
                &[90, 30, 110, 30, 100, 80],
                "higher, brighter, more noise: crack",
            ),
            (
                "extreme",
                &[30, 100, 60, 120, 127, 127],
                "ratio 7 full MOD: clangy metallic body and a long noise tail",
            ),
        ],
        FmHat => vec![
            (
                "classic",
                &[70, 20, 0, 50, 80, 40],
                "three-operator FM closed hat, high-passed",
            ),
            (
                "mid",
                &[90, 60, 50, 60, 100, 70],
                "open FM hat with tremolo flutter",
            ),
            (
                "extreme",
                &[30, 110, 127, 127, 127, 127],
                "full feedback and MOD, fast deep tremolo: noisy buzzing metal",
            ),
        ],
        BodyKick => vec![
            (
                "classic",
                &[40, 60, 40, 60, 40, 20],
                "MODAL bank struck: a physical kick with a pitch drop (TENSION)",
            ),
            (
                "mid",
                &[60, 40, 90, 20, 70, 40],
                "harder, shorter mallet: more attack overtones, damped",
            ),
            (
                "extreme",
                &[10, 127, 127, 0, 127, 0],
                "very low, very long, hard strike, an octave tension bend",
            ),
        ],
        BodySnare => vec![
            (
                "classic",
                &[60, 40, 80, 40, 90, 50],
                "struck membrane plus wires that follow the head",
            ),
            (
                "mid",
                &[90, 30, 110, 60, 110, 30],
                "tighter, higher, brighter wires",
            ),
            (
                "extreme",
                &[20, 110, 127, 127, 127, 120],
                "low ringing head with big bend and a long wire wash",
            ),
        ],
        BodyTom => vec![
            (
                "classic",
                &[50, 70, 60, 40, 20, 20],
                "struck membrane tom: inharmonic modes and a pitch drop",
            ),
            (
                "mid",
                &[85, 50, 90, 70, 50, 60],
                "higher tom, harder, struck off-centre (POSITION)",
            ),
            (
                "extreme",
                &[10, 127, 127, 127, 0, 127],
                "huge low tom, long ring, an octave bend",
            ),
        ],
        BodyBell => vec![
            (
                "classic",
                &[60, 80, 80, 50, 70, 10],
                "MODAL bank at bell structure: a struck bell, mid pitch",
            ),
            (
                "mid",
                &[90, 60, 100, 90, 90, 30],
                "higher, more inharmonic, brighter, shorter",
            ),
            (
                "extreme",
                &[20, 127, 127, 127, 127, 0],
                "low gong: full structure and brightness, very long",
            ),
        ],
        BodyShaker => vec![
            (
                "classic",
                &[80, 50, 40, 80, 30, 30],
                "random grains under one shake gesture, band-passed: a shaker",
            ),
            (
                "mid",
                &[100, 70, 60, 60, 80, 60],
                "denser, longer, peak later: a slower shake",
            ),
            (
                "extreme",
                &[30, 120, 127, 20, 120, 120],
                "sparse long grains, dark resonant band: rain stick",
            ),
        ],
        BodyWood => vec![
            (
                "rim",
                &[45, 40, 70, 0, 70, 50, 0, 64],
                "WOOD at rim: a pitched knock with the rim's ring and a stick tick (TONE RIM's gap)",
            ),
            (
                "sidestick",
                &[30, 30, 60, 40, 30, 70, 0, 64],
                "sidestick: lower, drier, a stick across the rim",
            ),
            (
                "clave",
                &[90, 50, 100, 85, 20, 20, 0, 64],
                "clave: the free bar's inharmonic partials, bright and short",
            ),
            (
                "woodblock",
                &[60, 45, 80, 127, 40, 30, 0, 64],
                "woodblock: hollow, the second mode close above the first",
            ),
            (
                "dual",
                &[85, 50, 100, 85, 20, 20, 60, 70],
                "clave with DUAL: a second strike a few ms on (TRX-CL's), TUNE moving the second mode",
            ),
        ],
        BodyHand => vec![
            (
                "conga-open",
                &[45, 70, 100, 64, 0, 30, 50, 50],
                "conga, OPEN tone: the palm's contact and skin noise, the palm lifts, the head rings",
            ),
            (
                "conga-slap",
                &[45, 70, 100, 64, 64, 30, 80, 50],
                "conga, SLAP: a short bright contact, the crack, fingers damping the upper modes, a flam on some hits",
            ),
            (
                "conga-mute",
                &[45, 70, 100, 64, 127, 30, 50, 50],
                "conga, MUTE: the palm stays on the head, a damped, skin-heavy thud",
            ),
            (
                "bongo",
                &[100, 45, 100, 10, 0, 40, 70, 60],
                "bongo, OPEN: small, high, tight, fingertip contact",
            ),
            (
                "timbale",
                &[85, 70, 60, 127, 0, 20, 90, 70],
                "timbale: the steel shell's ring over the head, a stick-and-hand mix (HAND 60)",
            ),
        ],
        MetalJingle => vec![
            (
                "classic",
                &[64, 50, 70, 30, 60, 50],
                "a tambourine hit: a burst of jingles from the metal bank, settling",
            ),
            (
                "shake",
                &[64, 90, 90, 100, 70, 60],
                "a long shake: jingles peaking later, then thinning",
            ),
            (
                "frame",
                &[30, 60, 50, 20, 40, 40, 90, 60],
                "a frame drum with jingles: HEAD's thump under fewer, darker jingles",
            ),
        ],
        PlainSine => vec![
            (
                "classic",
                &[20, 70, 30, 30, 0, 0],
                "plain swept sine: a sub kick",
            ),
            ("mid", &[60, 40, 50, 20, 0, 0], "a mid tom/blip"),
            ("extreme", &[100, 20, 127, 10, 0, 0], "a short high zap"),
        ],
        PlainNoise => vec![
            (
                "classic",
                &[30, 100, 0, 0, 0, 0],
                "short high-passed noise: a utility hat",
            ),
            ("mid", &[60, 50, 0, 0, 0, 0], "medium full-band noise burst"),
            (
                "extreme",
                &[127, 10, 0, 0, 0, 0],
                "long dark low-passed rumble",
            ),
        ],
        PlainImpulse => vec![
            (
                "classic",
                &[40, 30, 0, 0, 0, 0],
                "a short buzzy click train at a low pitch",
            ),
            ("mid", &[70, 50, 60, 30, 0, 0], "a swept pulse blip"),
            (
                "extreme",
                &[10, 127, 127, 127, 0, 0],
                "a slow-falling buzz, long: check the narrow pulse's aliasing",
            ),
        ],
        VoxKick => vec![
            (
                "classic",
                &[50, 40, 100, 30, 70, 0, 50, 40],
                "\"B\": lips burst open on held pressure: a low pop and its boom, a short dark breath burst, a flick of lip buzz; no voice",
            ),
            (
                "mid",
                &[40, 70, 90, 50, 60, 90, 40, 30],
                "\"Bm\": the same pop, then the lips close on a nasal \"m\" hum that sags a semitone",
            ),
            (
                "extreme",
                &[20, 120, 127, 110, 100, 60, 20, 110],
                "a deep, driven, long pop with lips buzzing hard and a hum under it: the biggest kick a mouth makes",
            ),
        ],
        VoxSnare => vec![
            (
                "classic",
                &[90, 90, 50, 10, 60, 50, 0, 64],
                "\"Pf\": a lip burst and straight into a flat, breathy \"f\"",
            ),
            (
                "mid",
                &[80, 100, 60, 110, 50, 50, 0, 64],
                "\"Psh\": the lip burst into a peaked \"sh\" (~3 kHz)",
            ),
            (
                "extreme",
                &[100, 100, 55, 60, 0, 80, 127, 64],
                "the inward K snare: a sharp velar click, then an in-breath that swells and hisses",
            ),
        ],
        VoxHat => vec![
            (
                "classic",
                &[90, 90, 15, 15, 30, 60, 64, 30],
                "\"ts\": a tongue-tip tick and a short bright \"s\" (closed)",
            ),
            (
                "mid",
                &[80, 100, 25, 120, 40, 50, 64, 30],
                "\"ch\": the tick into a lower, rounder \"sh\"-coloured hiss",
            ),
            (
                "extreme",
                &[70, 110, 110, 30, 60, 80, 64, 30],
                "open \"tsss\": the hiss swells and holds, turbulence moving in it",
            ),
        ],
        VoxClick => vec![
            (
                "classic",
                &[90, 110, 64, 0, 0, 60, 10, 30],
                "a palatal tongue click: one sharp \"tick\" ringing ~2.7 kHz",
            ),
            (
                "mid",
                &[64, 0, 30, 110, 70, 70, 0, 30],
                "\"K\": a velar burst, a second little burst, then a breath of \"kh\"",
            ),
            (
                "extreme",
                &[30, 120, 90, 80, 60, 30, 110, 70],
                "a hollow alveolar \"tok\" with a back \"k\" after: lips rounded, the cavity big",
            ),
        ],
        BodyBar | BodyPan | BodyGong | BodyTabla | BodySpring | LpgBongo | FilterPing => {
            world::settings(id)
        }
        VoxRoll => vec![
            (
                "classic",
                &[40, 60, 70, 0, 30, 50, 50, 30],
                "\"brrr\": a lip trill at ~35 Hz, loose and breathy",
            ),
            (
                "mid",
                &[60, 70, 80, 90, 20, 40, 40, 40],
                "a voiced lip-roll bass: the trill tighter (~45 Hz) with a hum under it",
            ),
            (
                "extreme",
                &[110, 110, 120, 60, 60, 90, 100, 100],
                "tense, fast (~70 Hz), driven, very loose: the lips nearly a buzz",
            ),
        ],
    }
}

/// Two bars of sixteenths: `X` accent, `x` a hit, `.` rest.
fn pattern(id: Id) -> &'static str {
    use Id::*;
    match id {
        ToneKick | FmKick | BodyKick | PlainSine => "X...x...X..x.x..X...x...X.x...x.",
        ToneSnare | FmSnare | BodySnare => "....X.......X.......X.......X..x",
        ToneTom | BodyTom => "X..x..X...x.x...X..x..X.x.xXxXx.",
        ToneClap => "....X.......X..x....X.......X.x.",
        ToneRim | PlainImpulse => "..x..x...x..x.x...x..x...x..x.xx",
        ToneCowbell => "X..x..x.X..x..x.X..x..x.X.x.x.x.",
        MetalHat | FmHat | PlainNoise => "XxxxXxxxXxxxXxxxXxxxXxxxXxxxXxXx",
        ToneCymbal => "X...............X...............",
        BodyBell => "X.....x.....X...X.....x...x.X...",
        BodyShaker | MetalJingle => "XxxxXxxxXxxxXxxxXxxxXxxxXxxxXxxx",
        BodyWood => "X..x..x...x.x...X..x..x...x.x...",
        BodyHand => "X..xX.x.X..xx.x.X..xX.x.X.xxX.x.",
        VoxKick => "X.....x...X.....X.....x.X...x...",
        VoxSnare => "....X.......X.......X.......X.x.",
        VoxHat => "x.X.x.X.x.X.x.xXx.X.x.X.x.X.x.Xx",
        VoxClick => "..x...x...x.x.....x..x..x...x.xx",
        VoxRoll => "X.......X...x...X.......X.x.x...",
        // The world batch plays its own melodic patterns (`world`).
        BodyBar | BodyPan | BodyGong | BodyTabla | BodySpring | LpgBongo | FilterPing => "",
    }
}

const STEP: f32 = 0.125; // a sixteenth at 120 BPM

/// A setting's knobs, MORE's at the machine's defaults where it leaves
/// them out, 0..1.
fn knobs(id: Id, k: &[u8]) -> Knobs {
    let more = spec(id).more;
    core::array::from_fn(|i| {
        k.get(i).copied().unwrap_or(more[(i.max(6) - 6).min(1)]) as f32 / 127.0
    })
}

/// One hit until it falls under −80 dB (after 30 ms) or `max` seconds.
fn hit(id: Id, k: &[u8], vel: f32, seed: u32, max: f32) -> Vec<f32> {
    run_hit(build(id, knobs(id, k), vel, seed), max)
}

/// A built hit run until it falls under −80 dB (after 30 ms) or `max` s.
fn run_hit(mut m: Box<dyn machines::Machine>, max: f32) -> Vec<f32> {
    let mut y = Vec::new();
    let mut quiet = 0;
    while y.len() < (max * FS) as usize {
        let mut b: Blk = [0.0; B];
        m.block(&mut b, None);
        let pk = b.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        y.extend_from_slice(&b);
        quiet = if pk < 1e-4 && y.len() > (0.03 * FS) as usize {
            quiet + 1
        } else {
            0
        };
        if quiet >= 16 {
            break;
        }
    }
    y
}

fn add_at(out: &mut Vec<f32>, at: usize, x: &[f32], gain: f32) {
    if out.len() < at + x.len() {
        out.resize(at + x.len(), 0.0);
    }
    for (o, v) in out[at..].iter_mut().zip(x) {
        *o += gain * v;
    }
}

fn play(id: Id, k: &[u8], steps: &str, gain: f32, seed: u32, out: &mut Vec<f32>) {
    for (i, c) in steps.chars().enumerate() {
        let vel = match c {
            'X' => 1.0,
            'x' => 0.7,
            _ => continue,
        };
        let h = hit(id, k, vel, seed.wrapping_add(i as u32 * 7919), 5.0);
        add_at(out, (i as f32 * STEP * FS) as usize, &h, gain);
    }
}

fn levels(x: &[f32]) -> (f64, f64) {
    let pk = x.iter().fold(0.0f32, |a, v| a.max(v.abs())) as f64;
    let ms = x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len().max(1) as f64;
    let db = |v: f64| {
        if v > 0.0 {
            (20.0 * v.log10() * 10.0).round() / 10.0
        } else {
            -999.0
        }
    };
    (db(pk), db(ms.sqrt()))
}

struct Out<'a> {
    dir: &'a Path,
    rows: Vec<Value>,
}

impl Out<'_> {
    fn write(&mut self, name: &str, x: &[f32], mut row: Value) -> Result<(), String> {
        write_f32(&self.dir.join(name), x, FS as u32)?;
        let (pk, rms) = levels(x);
        row["file"] = json!(name);
        row["peak_dbfs"] = json!(pk);
        row["rms_dbfs"] = json!(rms);
        row["seconds"] = json!((x.len() as f64 / FS as f64 * 1000.0).round() / 1000.0);
        if pk > 0.0 {
            eprintln!("over full scale: {name} peaks at {pk} dBFS");
        }
        self.rows.push(row);
        Ok(())
    }
}

fn knob_map(id: Id, k: &[u8]) -> Value {
    let s = spec(id);
    let kn = knobs(id, k);
    let mut m = serde_json::Map::new();
    for (n, v) in s.knobs.iter().zip(kn) {
        if *n != "-" {
            m.insert((*n).into(), json!((v * 127.0).round() as u8));
        }
    }
    Value::Object(m)
}

/// Rendered: the ship list, and VOX for the owner's first listen.
fn rendered(s: &machines::Spec) -> bool {
    s.ships || s.family == "VOX"
}

fn slug(name: &str) -> String {
    name.to_ascii_lowercase().replace(' ', "-")
}

pub fn run(dir: &Path) -> Result<(), String> {
    parts::flush_denormals();
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Out {
        dir,
        rows: Vec::new(),
    };

    for s in SPECS.iter().filter(|s| rendered(s)) {
        let sets = settings(s.id);
        for (si, (name, k, listen)) in sets.iter().enumerate() {
            let x = hit(s.id, k, 1.0, 1 + si as u32, 6.0);
            let row = json!({
                "kind": "hit", "machine": s.name, "family": s.family, "kernel": s.kernel,
                "setting": name, "knobs": knob_map(s.id, k), "velocity": 127, "listen_for": listen,
            });
            out.write(&format!("{}_{name}.wav", slug(s.name)), &x, row)?;
        }
        let (_, k, _) = sets[0];
        let steps = pattern(s.id);
        let mut x = Vec::new();
        play(s.id, k, steps, 1.0, 11, &mut x);
        x.resize(x.len().max((32.0 * STEP * FS) as usize), 0.0);
        let row = json!({
            "kind": "pattern", "machine": s.name, "family": s.family, "kernel": s.kernel,
            "setting": "classic", "knobs": knob_map(s.id, k), "steps": steps, "bpm": 120,
            "listen_for": "the classic setting in a two-bar groove: accents (X) at full velocity, others at 70 %; does it sit and repeat cleanly",
        });
        out.write(&format!("{}_pattern.wav", slug(s.name)), &x, row)?;
    }

    kit(&mut out)?;
    combine(&mut out)?;
    let fixes = fixes::run(&mut out)?;
    demos::run(&mut out)?;
    vox::run(&mut out)?;
    world::run(&mut out)?;

    let cost = bench();
    let manifest = json!({
        "about": "Chimera 0.4 DRUM host prototypes (chimera-utils drum-render). 48 kHz mono 32-bit float WAVs, raw levels, never normalised. Knobs 0-127 as the cells would show them. Machines are built from chimera-engine parts: VA NOISE and oscillator, CLEAN filter, ALGO kernel, MODAL BANK (16 modes) and its dispersion allpass, DRUM's two-pole mode bank, the Plaits-port LPG, the filter TYPEs, the FX slot's SOFT and FOLD curves, VA's combine, TAPE's Hermite read.",
        "cpu_estimate": cost,
        "fixes": fixes,
        "files": out.rows,
    });
    let p = dir.join("manifest.json");
    std::fs::write(&p, serde_json::to_string_pretty(&manifest).unwrap())
        .map_err(|e| format!("{}: {e}", p.display()))?;
    println!("{} files, manifest {}", out.rows.len(), p.display());
    Ok(())
}

/// A kit lane: pad, machine, setting, steps over four bars, level.
type Lane = (&'static str, Id, usize, &'static str, f32);

/// The mockup's 808ISH kit.
fn lanes() -> [Lane; 7] {
    [
        (
            "KICK",
            Id::ToneKick,
            0,
            "X.....x...X.....X.....x...X..x..X.....x...X.....X.x...x...X.....",
            0.7,
        ),
        (
            "SNARE",
            Id::FmSnare,
            0,
            "....X.......X.......X.......X.......X.......X.......X.....x.X...",
            0.55,
        ),
        (
            "CLAP",
            Id::ToneClap,
            0,
            "............X...............X...............X...............X...",
            0.45,
        ),
        (
            "CLOSED",
            Id::MetalHat,
            0,
            "x.x.X.x.x.x.X.x.x.x.X.x.x.x.X...x.x.X.x.x.x.X.x.x.x.X.x.x.......",
            0.45,
        ),
        (
            "OPEN",
            Id::MetalHat,
            1,
            "..............x...............x...............x.................",
            0.4,
        ),
        (
            "HIGH TOM",
            Id::BodyTom,
            1,
            "....................................................X..x........",
            0.6,
        ),
        (
            "LOW TOM",
            Id::BodyTom,
            0,
            "........................................................X..x..x.",
            0.6,
        ),
    ]
}

/// The kit as a four-bar groove; the open hat choked by the closed
/// (group C).
fn kit(out: &mut Out) -> Result<(), String> {
    let lanes = lanes();
    let closed: Vec<usize> = lanes[3]
        .3
        .char_indices()
        .filter(|(_, c)| *c != '.')
        .map(|(i, _)| i)
        .collect();
    let mut x = Vec::new();
    let mut pads = Vec::new();
    for (lane, id, set, steps, gain) in lanes {
        let (sname, k, _) = settings(id)[set];
        if lane == "OPEN" {
            for (i, c) in steps.chars().enumerate() {
                if c == '.' {
                    continue;
                }
                let mut h = hit(id, k, if c == 'X' { 1.0 } else { 0.7 }, 900 + i as u32, 5.0);
                // CHOKE C: the next closed hat cuts it, a 5 ms fade.
                if let Some(&j) = closed.iter().find(|&&j| j > i) {
                    let cut = ((j - i) as f32 * STEP * FS) as usize;
                    let fade = (0.005 * FS) as usize;
                    for (n, v) in h.iter_mut().enumerate().skip(cut) {
                        *v *= 1.0 - ((n - cut) as f32 / fade as f32).min(1.0);
                    }
                }
                add_at(&mut x, (i as f32 * STEP * FS) as usize, &h, gain);
            }
        } else {
            play(id, k, steps, gain, 500, &mut x);
        }
        pads.push(json!({"pad": lane, "machine": spec(id).name, "setting": sname, "knobs": knob_map(id, k), "level": gain, "steps": steps}));
    }
    x.resize(x.len().max((64.0 * STEP * FS) as usize), 0.0);
    let row = json!({
        "kind": "kit", "machine": "KIT", "family": "KIT", "setting": "808ISH groove", "knobs": {},
        "pads": pads, "bpm": 120,
        "listen_for": "a plausible kit of the machines at their mixer levels: TONE KICK, FM SNARE, TONE CLAP, METAL HAT closed/open (choked), BODY TOM fill in bar 4",
    });
    out.write("kit_groove.wav", &x, row)
}

fn combine(out: &mut Out) -> Result<(), String> {
    let snare = hit(Id::ToneSnare, settings(Id::ToneSnare)[0].1, 1.0, 3, 0.5);
    let raw = sample::clips(&snare);
    let clip = |name: &str, d: Depth| {
        let (_, _, x) = raw.iter().find(|c| c.0 == name).unwrap();
        store(x, d)
    };
    let amount = 0.7;
    // Two pads: a tom with the noise burst, a kick with the snare hit.
    let hits: [(&str, Id, &str, (f32, f32)); 2] = [
        ("tom-burst", Id::ToneTom, "noise-burst", (0.7, 0.6)),
        ("kick-snare", Id::ToneKick, "snare-hit", (0.7, 0.5)),
    ];
    for (tag, id, cname, lv) in hits {
        let (sname, k, _) = settings(id)[0];
        let c = clip(cname, Depth::B12);
        let secs = 1.2;
        let base = json!({
            "kind": "combine", "machine": spec(id).name, "family": "COMBINE", "setting": sname,
            "knobs": knob_map(id, k), "clip": cname, "depth": "12-bit", "clip_rate_hz": 16000,
            "clip_bytes": Depth::B12.bytes(c.q.len()), "levels": {"MACHINE": lv.0, "SAMPLE": lv.1}, "amount": amount,
        });
        let mut r = base.clone();
        r["mode"] = json!("MACHINE ALONE");
        r["listen_for"] = json!("the machine with no sample: the reference for every mode");
        out.write(
            &format!("combine_{tag}_machine.wav"),
            &pad(
                Some(build(id, knobs(id, k), 1.0, 5)),
                None,
                lv,
                Mode::Mix,
                0.0,
                secs,
            ),
            r,
        )?;
        let mut r = base.clone();
        r["mode"] = json!("SAMPLE ALONE");
        r["listen_for"] = json!("the clip alone (machine OFF), FORWARD at its root");
        out.write(
            &format!("combine_{tag}_sample.wav"),
            &pad(None, Some((&c, Dir::Forward)), lv, Mode::Mix, 0.0, secs),
            r,
        )?;
        for mode in Mode::ALL {
            let mut r = base.clone();
            r["mode"] = json!(mode.name());
            r["listen_for"] = json!(mode.listen());
            let x = pad(
                Some(build(id, knobs(id, k), 1.0, 5)),
                Some((&c, Dir::Forward)),
                lv,
                mode,
                amount,
                secs,
            );
            out.write(&format!("combine_{tag}_{}.wav", mode.slug()), &x, r)?;
        }
    }
    // Low bit: each clip alone at each depth, the snare's other
    // directions, and the kick pad at the low depths.
    for (cname, about, _) in &raw {
        for d in [Depth::B12, Depth::B8, Depth::Adpcm4] {
            let c = clip(cname, d);
            let row = json!({
                "kind": "sample", "machine": "OFF", "family": "SAMPLE", "setting": format!("{cname} {}", d.name()),
                "knobs": {}, "clip": cname, "clip_about": about, "depth": d.name(), "clip_rate_hz": 16000,
                "clip_samples": c.q.len(), "clip_bytes": d.bytes(c.q.len()), "direction": "FORWARD",
                "listen_for": match d {
                    Depth::B12 => "the clip as the kit pool stores it today (12-bit packed)",
                    Depth::B8 => "8-bit: a raised noise floor on the tail, 2/3 the bytes",
                    Depth::Adpcm4 => "4-bit ADPCM: 1/3 the bytes; the encoder's fitted first step and look-ahead keep the attack; listen for hiss on quiet tails",
                    Depth::Adpcm4First => "4-bit ADPCM from step 0 (the first prototype)",
                },
            });
            let tag = d.name().replace(' ', "-").to_ascii_lowercase();
            out.write(
                &format!("sample_{cname}_{tag}.wav"),
                &pad(
                    None,
                    Some((&c, Dir::Forward)),
                    (0.0, 1.0),
                    Mode::Mix,
                    0.0,
                    0.4,
                ),
                row,
            )?;
        }
    }
    let c = clip("snare-hit", Depth::B12);
    for (dir, dn) in [(Dir::Reverse, "REVERSE"), (Dir::PingPong, "PING-PONG")] {
        let row = json!({
            "kind": "sample", "machine": "OFF", "family": "SAMPLE", "setting": format!("snare-hit 12-bit {dn}"),
            "knobs": {}, "clip": "snare-hit", "depth": "12-bit", "direction": dn,
            "listen_for": "the head's other directions, once through",
        });
        out.write(
            &format!("sample_snare-hit_{}.wav", dn.to_ascii_lowercase()),
            &pad(None, Some((&c, dir)), (0.0, 1.0), Mode::Mix, 0.0, 0.6),
            row,
        )?;
    }
    let (_, k, _) = settings(Id::ToneKick)[0];
    for d in [Depth::B8, Depth::Adpcm4] {
        let c = clip("snare-hit", d);
        for mode in [Mode::Mix, Mode::Ring] {
            let row = json!({
                "kind": "combine", "machine": "TONE KICK", "family": "COMBINE", "setting": "classic",
                "knobs": knob_map(Id::ToneKick, k), "clip": "snare-hit", "depth": d.name(), "clip_rate_hz": 16000,
                "clip_bytes": d.bytes(c.q.len()), "mode": mode.name(), "amount": amount,
                "listen_for": "the kick pad with its clip at a low depth: does the loss matter inside a hit",
            });
            let tag = d.name().replace(' ', "-").to_ascii_lowercase();
            let x = pad(
                Some(build(Id::ToneKick, knobs(Id::ToneKick, k), 1.0, 5)),
                Some((&c, Dir::Forward)),
                (0.7, 0.5),
                mode,
                amount,
                1.2,
            );
            out.write(
                &format!("combine_kick-snare_{}_{tag}.wav", mode.slug()),
                &x,
                row,
            )?;
        }
    }
    Ok(())
}

/// Host ns a sample of each machine's classic hit (its first second,
/// built outside the clock), against ALGO's kernel playing six operators
/// in a chain; the ratio times ALGO's billed six-op cost (370 + 6 × 60 +
/// 5 × 8 = 770 cycles, ADR 0013's bench) is the estimate.
/// A fresh hit's block, for the bench.
type Run = Box<dyn FnMut(&mut Blk)>;

fn bench() -> Value {
    // A hit's first quarter second (its busy part), fresh each run, the
    // best of five.
    let n = FS as usize / 4 / B;
    let time = |mk: &mut dyn FnMut(u32) -> Run| {
        let mut best = f64::MAX;
        for r in 0..5 {
            let mut f = mk(r);
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
    let a = time(&mut |_| {
        let mut algo = parts::Ops::new([0, 1, 2, 4, 8, 16], 1);
        Box::new(move |b| {
            algo.block(
                b,
                [110.0, 220.0, 330.0, 440.0, 550.0, 660.0],
                [0.8, 0.3, 0.3, 0.3, 0.3, 0.3],
                [0.0; 6],
            )
        })
    });
    let mut rows = serde_json::Map::new();
    for s in SPECS
        .iter()
        .filter(|s| rendered(s) || world::IDS.contains(&s.id))
    {
        let k = knobs(s.id, settings(s.id)[0].1);
        let ns = time(&mut |r| {
            let mut m = build(s.id, k, 1.0, r);
            Box::new(move |b| m.block(b, None))
        });
        let ratio = ns / a;
        let mut row = json!({"kernel": s.kernel, "host_ns_per_sample": (ns * 100.0).round() / 100.0, "ratio_vs_algo6": (ratio * 100.0).round() / 100.0, "est_cycles_per_sample": (ratio * 770.0).round()});
        if let Some(c) = vox_formant::billed(s.id) {
            row["first_cut_algo_billed"] = c;
        }
        rows.insert(s.name.into(), row);
    }
    json!({
        "estimate": true,
        "method": "host ns/sample over the first 250 ms of the machine's classic hit (denormals flushed, as the M7 pays nothing for them) vs ALGO's kernel with six chained operators, times ALGO's billed 770 cycles; the prototypes' glue is per-sample and unoptimised (VA's oscillator ticked one sample at a time, the bank's cost is MODAL's own), so read the ratios, not the digits. Code count: MODAL BANK at 16 modes bills 400 + 16 x 45 = 1120 cycles alone",
        "algo6_host_ns_per_sample": (a * 100.0).round() / 100.0,
        "machines": rows,
    })
}
