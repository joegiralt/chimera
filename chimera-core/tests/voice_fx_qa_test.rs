//! The voice FX slot's desktop QA (issue #614, Task 14): the types are level
//! matched at their normal points, and every pot sweeps without a click.

mod common;

use chimera_core::dsp::voice_fx::params::{FxType, Place};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::{Rig, SR, VEL, step};

const TYPES: [FxType; 3] = [FxType::Soft, FxType::Fuzz, FxType::Fold];
const PLACES: [Place; 2] = [Place::Pre, Place::Post];
const SECONDS: usize = 2;
const BLOCKS: usize = SECONDS * SR as usize / BLOCK_SIZE;

/// The ledger's normal P1 for each type.
fn normal_p1(t: FxType) -> f32 {
    match t {
        FxType::Fuzz => 0.5,
        FxType::Fold => 0.4,
        _ => 0.5,
    }
}

/// `e`'s INIT with the slot on `t` at `p`, MIX 1. `sine`: VA as one steady
/// sine (INIT's detuned saws beat and edge on their own).
fn init_as(e: EngineType, t: FxType, p: Place, sine: bool) -> ParamSnapshot {
    let mut s = Sound::init(e).params;
    if sine && e == EngineType::Va {
        // A steady sine: INIT's detuned saws beat and edge on their own.
        (s.va.wave, s.va.fine, s.va.stability) = ([0.0; 2], [0.0; 2], 1.0);
    }
    s.fx.set_type(t);
    s.fx.place = p;
    s.fx.mix = 1.0;
    s
}

/// `note` held for `BLOCKS` blocks, `edit(block, params)` before each.
fn play(mut p: ParamSnapshot, note: u8, edit: impl Fn(usize, &mut ParamSnapshot)) -> Vec<f32> {
    let mut v = Rig::new(SR);
    let mods = ModState::new();
    v.note_on(
        MidiNote::new(note).unwrap(),
        Velocity::new(VEL).unwrap(),
        &p,
    );
    let mut out = Vec::with_capacity(BLOCKS * BLOCK_SIZE);
    for b in 0..BLOCKS {
        edit(b, &mut p);
        let mut blk = [0.0; BLOCK_SIZE];
        v.render_glided(&mut blk, &p, &mods);
        out.extend_from_slice(&blk);
    }
    out
}

fn rms_db(x: &[f32]) -> f32 {
    10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).log10()
}

fn init(e: EngineType, t: FxType, p: Place) -> ParamSnapshot {
    init_as(e, t, p, true)
}

/// The note's own onset (blocks) is not the slot's: skipped.
const ATTACK: usize = 40;

/// The sweep adds no click: the steepest step the desktop's output stage
/// hears (`step`) after the onset is within 1.25x of the steepest of the
/// same sound held at the sweep's stops, or under `clicks`' own 0.15. A
/// hard-clipped wave is steep held still (FUZZ at VOLUME 1 peaks near 2),
/// so `clicks` alone would flag the held sound, not the sweep.
fn assert_no_click(label: &str, sweep: &[f32], stops: &[Vec<f32>]) {
    let after = |y: &[f32]| step(&y[ATTACK * BLOCK_SIZE..]);
    let held = stops.iter().map(|y| after(y)).fold(0.15, f32::max);
    let s = after(sweep);
    assert!(
        s <= 1.25 * held,
        "{label}: sweep steps {s:.3}, held {held:.3}"
    );
}

/// 0 up to 1 and back over the render.
fn tri(b: usize) -> f32 {
    let t = b as f32 / BLOCKS as f32;
    1.0 - (2.0 * t - 1.0).abs()
}

/// The largest dB step between neighbouring 64-sample block RMSs, skipping
/// the first `skip` blocks (the note's own attack).
fn max_block_step_db(x: &[f32], skip: usize) -> (f32, usize) {
    let db: Vec<f32> = x
        .chunks(BLOCK_SIZE)
        .skip(skip)
        .map(|c| rms_db(c).max(-120.0))
        .collect();
    db.windows(2)
        .enumerate()
        .map(|(i, w)| ((w[1] - w[0]).abs(), i + skip))
        .fold((0.0, 0), |m, x| if x.0 > m.0 { x } else { m })
}

#[test]
fn each_type_at_defaults_matches_off_within_1_5_db() {
    let mut bad = Vec::new();
    for sine in [false, true] {
        let mut off = init_as(EngineType::Va, FxType::Off, Place::Pre, sine);
        off.filter.level = 0.5;
        let dry = rms_db(&play(off, 60, |_, _| {}));
        for t in TYPES {
            for place in PLACES {
                let mut p = init_as(EngineType::Va, t, place, sine);
                p.filter.level = 0.5;
                p.fx.pots = [normal_p1(t), 0.5, 0.5];
                let d = rms_db(&play(p, 60, |_, _| {})) - dry;
                println!(
                    "{} {t:?} {place:?}: {d:+.2} dB",
                    if sine { "sine" } else { "INIT" }
                );
                // FUZZ keeps RF 4's bound over the 1.5 dB (MAKE_UP in fuzz.rs).
                let tol = if t == FxType::Fuzz { 1.75 } else { 1.5 };
                if !sine && d.abs() > tol {
                    bad.push(format!("{t:?} {place:?} {d:+.2}"));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "off by more than 1.5 dB (FUZZ 1.75): {bad:?}"
    );
}

#[test]
fn input_level_sweeps_smoothly_under_every_type() {
    for t in TYPES {
        for place in PLACES {
            let p = init(EngineType::Va, t, place);
            let sweep = |b: usize, p: &mut ParamSnapshot| p.filter.level = b as f32 / BLOCKS as f32;
            let y = play(p.clone(), 60, sweep);
            let stops: Vec<_> = [0.0, 0.5, 1.0]
                .map(|l| play(p.clone(), 60, |_, p| p.filter.level = l))
                .into();
            assert_no_click(&format!("{t:?} {place:?}"), &y, &stops);
            // F#6: four periods a block, so the block RMS doesn't ripple.
            let (step, at) = max_block_step_db(&play(p, 90, sweep), ATTACK);
            println!("{t:?} {place:?}: step {step:.3} dB at block {at}");
            assert!(step <= 0.5, "{t:?} {place:?}: {step:.2} dB at block {at}");
        }
    }
}

#[test]
fn every_fx_pot_sweeps_smoothly() {
    for e in [EngineType::Va, EngineType::Modal] {
        for t in TYPES {
            for place in PLACES {
                for pot in 0..4 {
                    let p = init(e, t, place);
                    let set = |p: &mut ParamSnapshot, v: f32| match pot {
                        3 => p.fx.mix = v,
                        i => p.fx.pots[i] = v,
                    };
                    let y = play(p.clone(), 60, |b, p| set(p, tri(b)));
                    let stops: Vec<_> = [0.0, 0.5, 1.0]
                        .map(|v| play(p.clone(), 60, |_, p| set(p, v)))
                        .into();
                    assert_no_click(&format!("{e:?} {t:?} {place:?} pot {pot}"), &y, &stops);
                }
            }
        }
    }
}

/// A mono 24-bit PCM WAV's samples (the A/B files; a 44-byte header).
fn pcm24(path: &str) -> Vec<f32> {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    b[44..]
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0)
        .collect()
}

/// The alias floor of each `sine_long` pair under `AB_DIRS` (`chimera-utils
/// ab`'s output folders, comma-separated): `cargo test -- --ignored
/// --nocapture ab_alias_figures`. A report, not a gate.
#[test]
#[ignore = "reads the A/B ledger's WAVs (AB_DIRS)"]
fn ab_alias_figures() {
    let Ok(dirs) = std::env::var("AB_DIRS") else {
        return;
    };
    for d in dirs.split(',') {
        for side in ["ref", "chimera"] {
            let x = pcm24(&format!("{d}/sine_long_{side}.wav"));
            let db = common::spectrum::alias_db(&x[4800..], SR as f32, &[220.0]);
            println!("ALIAS {d} {side}: {db:.1} dB");
        }
    }
}
