//! The owner's SMEAR A/B renderer (reverb TYPEs spec § A/B):
//! `tools/ab/render.sh` drives it.
//!
//! - `stimuli` plays the strings (Modal BOWED), the pad (factory MORPH PAD)
//!   and an IDM phrase (those two over `stimuli.py`'s kick, snare and hat)
//!   on Chimera's own voices, into `$AB_DIR/stimuli/`.
//! - `render` runs `$AB_DIR/stimuli/$AB_STIMULUS.wav` through the real bus
//!   (`FxBus`), the reverb alone, QUAD at its defaults and MIX 1, at SMEAR
//!   `$AB_SMEAR`. It writes `<name>_s<smear>.wav` (16-bit stereo: the dry
//!   send on both sides plus the return, 6 dB down) and `<name>_s<smear>.txt`
//!   (the figures).
//!
//! - `render_bursts` (Task 8) runs the snare, kick, bowed and plucked
//!   strings and the IDM phrase through GATE and REVERSE, each at its
//!   starting knobs and two contrasting settings, into `$AB_OUT` (default
//!   `$AB_DIR/burst`): `<source>_<type>_<setting>.wav` and `manifest.json`.
//!
//! It sets the reverb through its `Block` ids, so it builds at the T4
//! reference (a2abe82f) too: id 5 (GRIT there, retired here) to 0; ids 11
//! and 12 (SMEAR, RATE here) that build ignores. `$AB_DIR` defaults to the
//! workspace's `target/ab`.

mod common;

use std::path::{Path, PathBuf};

use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::orbit::write_wav;
use common::rev::{pitch_per_pass_cents, run, tone};
use common::{Rig, SR};

fn dir() -> PathBuf {
    std::env::var_os("AB_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/ab"),
        PathBuf::from,
    )
}

/// A mono 32-bit float WAV's samples (`stimuli.py`'s format).
fn read_wav(path: &Path) -> Vec<f32> {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(&b[..4], b"RIFF");
    assert_eq!(u16::from_le_bytes([b[20], b[21]]), 3, "not float");
    assert_eq!(u16::from_le_bytes([b[22], b[23]]), 1, "not mono");
    let at = b.windows(4).position(|w| w == b"data").expect("no data") + 8;
    b[at..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&c| f32::from_le_bytes(c))
        .collect()
}

/// `x` as `read_wav` reads it.
fn write_float_wav(path: &Path, x: &[f32]) {
    let data = (x.len() * 4) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&SR.to_le_bytes());
    b.extend_from_slice(&(SR * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in x {
        b.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, b).expect("the WAV");
}

// ── the stimuli ──

/// One note: (start s, length s, MIDI note, velocity).
type Note = (f32, f32, u8, u8);

/// `notes` on `p`, one voice each, mono, `seconds` long.
fn play(p: &ParamSnapshot, notes: &[Note], seconds: f32) -> Vec<f32> {
    let mods = ModState::new();
    let blocks = (seconds * SR as f32) as usize / BLOCK_SIZE;
    let mut out = vec![0.0f32; blocks * BLOCK_SIZE];
    for &(start, len, note, vel) in notes {
        let (on, off) = (
            (start * SR as f32) as usize / BLOCK_SIZE,
            ((start + len) * SR as f32) as usize / BLOCK_SIZE,
        );
        let mut rig = Rig::new(SR);
        let mut block = [0.0; BLOCK_SIZE];
        for b in on..blocks {
            if b == on {
                let (n, v) = (MidiNote::new(note).unwrap(), Velocity::new(vel).unwrap());
                rig.note_on(n, v, p);
            }
            if b == off {
                rig.note_off();
            }
            rig.render(&mut block, p, &mods);
            for (o, s) in out[b * BLOCK_SIZE..].iter_mut().zip(&block) {
                *o += s;
            }
        }
    }
    out
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// `x` scaled to peak at `dbfs`.
fn peak_to(mut x: Vec<f32>, dbfs: f32) -> Vec<f32> {
    let g = 10f32.powf(dbfs / 20.0) / peak(&x);
    x.iter_mut().for_each(|s| *s *= g);
    x
}

fn plucked() -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Modal);
    p.modal.mode = ResonatorMode::String;
    p
}

fn bowed() -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Modal);
    p.modal.mode = ResonatorMode::Bowed;
    p
}

fn pad() -> ParamSnapshot {
    let s = chimera_core::factory::factory_sound(6).expect("MORPH PAD");
    assert_eq!(s.name.as_str(), "MORPH PAD");
    s.params
}

/// `x` from `at` seconds into `into`, scaled by `g`.
fn mix_in(into: &mut [f32], x: &[f32], at: f32, g: f32) {
    let i = (at * SR as f32) as usize;
    for (o, s) in into[i..].iter_mut().zip(x) {
        *o += g * s;
    }
}

/// An early-'90s IDM phrase, 12 s at 120 BPM (six bars): a broken beat on
/// the kick, snare and hat, a pad of four-bar chords and a bowed line over
/// it. Then 6 s of tail room.
fn idm(kick: &[f32], snare: &[f32], hat: &[f32]) -> Vec<f32> {
    const STEP: f32 = 0.125; // a 16th at 120 BPM
    let mut x = vec![0.0f32; 18 * SR as usize];
    // Per bar of 16 steps: kick, snare (velocity) and hat steps. Bars 3
    // and 6 break up, with a 32nd snare roll into the turnaround.
    let kicks: [&[usize]; 2] = [&[0, 3, 7, 10], &[0, 6, 9, 11, 14]];
    let snares: [&[(usize, f32)]; 2] = [
        &[(4, 1.0), (12, 1.0), (15, 0.35)],
        &[(4, 1.0), (7, 0.3), (12, 1.0), (13, 0.4)],
    ];
    let hats: &[usize] = &[2, 3, 6, 8, 10, 11, 14];
    for bar in 0..6 {
        let alt = (bar % 3 == 2) as usize;
        let t0 = bar as f32 * 16.0 * STEP;
        for &k in kicks[alt] {
            mix_in(&mut x, kick, t0 + k as f32 * STEP, 1.0);
        }
        for &(s, v) in snares[alt] {
            mix_in(&mut x, snare, t0 + s as f32 * STEP, v);
        }
        for &h in hats {
            mix_in(&mut x, hat, t0 + h as f32 * STEP, 0.7);
        }
        if bar == 5 {
            for r in 0..8 {
                let at = t0 + 14.0 * STEP + r as f32 * STEP / 4.0;
                mix_in(&mut x, snare, at, 0.25 + 0.08 * r as f32);
            }
        }
    }
    // Am9, Fmaj9 and Em9 under the beat, two bars each.
    let chords: [&[u8]; 3] = [
        &[57, 60, 64, 67, 71],
        &[53, 57, 60, 64, 67],
        &[52, 55, 59, 62, 66],
    ];
    let pad_notes: Vec<Note> = chords
        .iter()
        .enumerate()
        .flat_map(|(i, c)| c.iter().map(move |&n| (i as f32 * 4.0, 4.0, n, 80)))
        .collect();
    let pad = peak_to(play(&pad(), &pad_notes, 18.0), -14.0);
    mix_in(&mut x, &pad, 0.0, 1.0);
    // The line: half notes and a few quarters, E5 down to B4 and back.
    let line: &[(f32, f32, u8)] = &[
        (0.0, 1.0, 76),
        (1.0, 1.0, 74),
        (2.0, 2.0, 72),
        (4.0, 1.0, 77),
        (5.0, 0.5, 76),
        (5.5, 0.5, 74),
        (6.0, 2.0, 72),
        (8.0, 1.0, 71),
        (9.0, 1.0, 74),
        (10.0, 2.0, 76),
    ];
    let line: Vec<Note> = line.iter().map(|&(t, l, n)| (t, l, n, 90)).collect();
    let strings = peak_to(play(&bowed(), &line, 18.0), -16.0);
    mix_in(&mut x, &strings, 0.0, 1.0);
    peak_to(x, -6.0)
}

#[test]
#[ignore]
fn stimuli() {
    let d = dir().join("stimuli");
    let read = |n: &str| read_wav(&d.join(format!("{n}.wav")));
    let (kick, snare, hat) = (read("kick"), read("snare"), read("hat"));
    let trim = |x: Vec<f32>| -> Vec<f32> {
        let end = x.iter().rposition(|s| s.abs() > 1e-6).unwrap_or(0) + 1;
        x[..end].to_vec()
    };
    let (kick, snare, hat) = (trim(kick), trim(snare), trim(hat));
    // Strings and pad alone: a chord held 3 s, released, 12 s of room.
    let chord = |notes: &[u8]| -> Vec<Note> { notes.iter().map(|&n| (0.0, 3.0, n, 90)).collect() };
    let strings = peak_to(play(&bowed(), &chord(&[57, 64, 69, 72]), 15.0), -12.0);
    let pad = peak_to(play(&pad(), &chord(&[57, 60, 64, 67, 71]), 15.0), -12.0);
    // Plucks: an A minor arpeggio, an eighth apart at 120 BPM, then a chord.
    let arp: Vec<Note> = [57u8, 64, 69, 72, 76, 72, 69, 64]
        .iter()
        .enumerate()
        .map(|(i, &n)| (i as f32 * 0.25, 0.25, n, 100))
        .chain([57u8, 64, 69, 72].map(|n| (2.0, 1.0, n, 100)))
        .collect();
    let plucked = peak_to(play(&plucked(), &arp, 15.0), -12.0);
    for (name, x) in [
        ("strings", strings),
        ("plucked", plucked),
        ("pad", pad),
        ("idm", idm(&kick, &snare, &hat)),
    ] {
        assert!(x.iter().all(|s| s.is_finite()), "{name}");
        write_float_wav(&d.join(format!("{name}.wav")), &x);
    }
}

// ── the renders ──

/// A fresh bus with only the reverb on: QUAD's defaults, MIX 1, SMEAR
/// `smear` (through the ids, so either build takes it).
fn bus(smear: f32) -> (Box<FxBus>, FxParams) {
    let mut fx = FxParams::default();
    fx.reverb.mix = 1.0;
    fx.reverb.write(ParamId(5), 0.0);
    fx.reverb.write(ParamId(11), smear);
    (Box::new(FxBus::new()), fx)
}

fn tick(
    bus: &mut FxBus,
    fx: &FxParams,
    x: &[f32; BLOCK_SIZE],
) -> ([f32; BLOCK_SIZE], [f32; BLOCK_SIZE]) {
    let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
    sends[2] = *x;
    let mut ret = Stereo::SILENT;
    bus.process(&mut sends, fx, Tempo::DEFAULT, SR, &mut ret);
    (ret.l, ret.r)
}

fn dbfs(x: &[f32]) -> f32 {
    let ms = x.iter().map(|&s| s as f64 * s as f64).sum::<f64>() / x.len() as f64;
    (10.0 * (ms / 0.5).log10()) as f32
}

#[test]
#[ignore]
fn render() {
    let name = std::env::var("AB_STIMULUS").expect("AB_STIMULUS");
    let smear: f32 = std::env::var("AB_SMEAR")
        .expect("AB_SMEAR")
        .parse()
        .expect("SMEAR");
    let d = dir();
    let x = read_wav(&d.join("stimuli").join(format!("{name}.wav")));
    let (mut b, fx) = bus(smear);
    let mut out = Vec::with_capacity(2 * x.len());
    for blk in x.chunks(BLOCK_SIZE) {
        let mut send = [0.0; BLOCK_SIZE];
        send[..blk.len()].copy_from_slice(blk);
        let (l, r) = tick(&mut b, &fx, &send);
        for i in 0..BLOCK_SIZE {
            out.extend([send[i] + l[i], send[i] + r[i]]);
        }
    }
    assert!(out.iter().all(|s| s.is_finite()), "non-finite");
    let stem = format!("{name}_s{smear}");
    let wav = d.join(format!("{stem}.wav"));
    // 6 dB down: the send and a MIX 1 return together peak near 0 dBFS.
    let out: Vec<f32> = out.iter().map(|&s| 0.5 * s).collect();
    write_wav(&wav, &out);
    assert!(wav.exists());
    // The figures: the pitch spread on a 1 kHz probe's tail, per pass, in
    // cents from its own mean, and the render's peak and last 2 s. The
    // probe runs QUAD at SIZE 0 (a 0.338 s round trip, whose tail barely
    // beats) and TIME 1 (a long one), as `reverb_smear_test`'s bound does.
    let (mut b, mut fx) = bus(smear);
    (fx.reverb.time, fx.reverb.size) = (1.0, 0.0);
    let sine = tone(1_000.0, 0.25);
    let (l, _) = run(
        &mut |s| tick(&mut b, &fx, s),
        &mut |i| if i < SR as usize / 2 { sine(i) } else { 0.0 },
        10.0,
    );
    // Cut where the tail falls 40 dB: below, the LSBs scatter the crossings.
    let tail = &l[3 * SR as usize / 2..];
    let first = common::rms(&tail[..SR as usize / 100]);
    let end = tail
        .chunks(SR as usize / 100)
        .position(|w| common::rms(w) < 1e-2 * first)
        .map_or(tail.len(), |w| w * SR as usize / 100);
    let passes = pitch_per_pass_cents(&tail[..end], 1_000.0, 0.338);
    let mean = passes.iter().sum::<f32>() / passes.len() as f32;
    let spread = passes.iter().map(|c| (c - mean).abs()).fold(0.0, f32::max);
    std::fs::write(
        d.join(format!("{stem}.txt")),
        format!(
            "{name}\t{smear}\t{spread:.1}\t{:.1}\t{:.1}\n",
            20.0 * peak(&out).log10(),
            dbfs(&out[out.len() - 4 * SR as usize..]),
        ),
    )
    .expect("the figures");
}

/// GATE and REVERSE at their starting knobs and two contrasting settings,
/// each a (name, knobs over the starting ones) pair.
type Setting = (
    &'static str,
    &'static str,
    fn(&mut chimera_core::dsp::reverb::ReverbParams),
);

const BURST_SETTINGS: [(chimera_core::dsp::reverb::ReverbType, [Setting; 3]); 2] = {
    use chimera_core::dsp::reverb::ReverbType::{Gate, Reverse};
    [
        (
            Gate,
            [
                (
                    "default",
                    "starting knobs: LENGTH 177 ms, TAIL 0.25 s, SHAPE -4 dB",
                    |_| {},
                ),
                (
                    "fast",
                    "fast gate: LENGTH 150 ms, TAIL 0, SHAPE 0 (flat)",
                    |p| (p.size, p.time, p.shape) = (0.488, 0.0, 0.0),
                ),
                (
                    "dying",
                    "slow dying gate: LENGTH 600 ms, TAIL 0.8 s, SHAPE -18 dB, DENSITY 1",
                    |p| (p.size, p.time, p.shape, p.diffuse) = (1.0, 1.0, 1.0, 1.0),
                ),
            ],
        ),
        (
            Reverse,
            [
                (
                    "default",
                    "starting knobs: LENGTH 346 ms, REGEN 0, CURVE 24 dB",
                    |_| {},
                ),
                ("regen", "regenerating: REGEN 0.7 (-6 dB a pass)", |p| {
                    p.time = 0.7
                }),
                (
                    "swell",
                    "long dark swell: LENGTH 800 ms, CURVE 30 dB, TONE 4.4 kHz, DENSITY 1",
                    |p| (p.size, p.shape, p.tone, p.diffuse) = (1.0, 1.0, 0.6, 1.0),
                ),
            ],
        ),
    ]
};

#[test]
#[ignore]
fn render_bursts() {
    use chimera_core::dsp::reverb::KnobMemory;
    let d = dir();
    let out_dir = std::env::var_os("AB_OUT").map_or_else(|| d.join("burst"), PathBuf::from);
    std::fs::create_dir_all(&out_dir).expect("AB_OUT");
    let gain_db: f32 = std::env::var("AB_GAIN_DB").map_or(-9.0, |v| v.parse().expect("dB"));
    let gain = 10f32.powf(gain_db / 20.0);
    let mut manifest = Vec::new();
    for source in ["snare", "kick", "strings", "plucked", "idm"] {
        let x = read_wav(&d.join("stimuli").join(format!("{source}.wav")));
        for (kind, settings) in BURST_SETTINGS {
            let name = chimera_core::dsp::reverb::ReverbType::NAMES[kind.index()];
            for (tag, what, set) in settings {
                let mut fx = FxParams::default();
                fx.reverb = fx.reverb.with_type(kind, &KnobMemory::new());
                fx.reverb.mix = 1.0;
                set(&mut fx.reverb);
                let mut b = Box::new(FxBus::new());
                let mut out = Vec::with_capacity(2 * x.len());
                for blk in x.chunks(BLOCK_SIZE) {
                    let mut send = [0.0; BLOCK_SIZE];
                    send[..blk.len()].copy_from_slice(blk);
                    let (l, r) = tick(&mut b, &fx, &send);
                    for i in 0..BLOCK_SIZE {
                        out.extend([send[i] + l[i], send[i] + r[i]]);
                    }
                }
                // The dry send plus the return, `$AB_GAIN_DB` down (9 by
                // default: a held string through the dying gate peaks over
                // 0 dBFS at the SMEAR set's 6).
                let out: Vec<f32> = out.iter().map(|&s| gain * s).collect();
                assert!(out.iter().all(|s| s.is_finite()), "{source} {name} {tag}");
                let pk = 20.0 * peak(&out).log10();
                assert!(pk <= 0.0, "{source} {name} {tag} clips: {pk:.1} dBFS");
                let file = format!("{source}_{}_{tag}.wav", name.to_lowercase());
                write_wav(&out_dir.join(&file), &out);
                manifest.push(format!(
                    "  {{\"source\": \"{source}\", \"type\": \"{name}\", \"setting\": \"{what}\", \"file\": \"{file}\", \"peak_dbfs\": {pk:.1}}}"
                ));
            }
        }
    }
    std::fs::write(
        out_dir.join("manifest.json"),
        format!("[\n{}\n]\n", manifest.join(",\n")),
    )
    .expect("the manifest");
}
