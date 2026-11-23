// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Loudness through the real `Instrument` (ADR 0109, #411, #193): the
//! level table, the targets, and the ceiling every scene stays under.

mod common;

use chimera_core::dsp::SendLevel;
use chimera_core::dsp::limiter::CEILING;
use chimera_core::dsp::tape_engine::TapeModel;
use chimera_core::params::EngineType;
use chimera_core::project::{PartId, Performance};
use common::scene::*;

const HOLD: usize = 750;
const TOTAL: usize = 800;
const C4: u8 = 60;
const CHORD: [u8; 4] = [48, 55, 60, 64];

fn engines() -> Vec<(String, Performance, Option<Vec<u8>>)> {
    let mut v = vec![
        ("ALGO INIT".to_string(), init_perf(EngineType::Algo), None),
        ("MODAL INIT".to_string(), init_perf(EngineType::Modal), None),
        ("VA INIT".to_string(), init_perf(EngineType::Va), None),
    ];
    let mut t = init_perf(EngineType::Tape);
    tape_model(&mut t, 0, TapeModel::Walk);
    v.push(("TAPE INIT (LOOP)".to_string(), t, Some(chimera_source(0))));
    for i in 0..8 {
        let mut p = Performance::new();
        let s = chimera_core::factory::factory_sound(i).unwrap();
        let name = s.name.as_str().to_string();
        **p.edit(PartId::ALL[0]).sound = s;
        v.push((format!("F{i} {name}"), p, None));
    }
    v
}

/// Six Parts, three pairs, a four-note chord each at velocity 127, every
/// send 0.6, the FX on: factory Sounds 0..5.
pub fn dense() -> (Performance, Vec<(usize, u8)>) {
    let mut p = Performance::new();
    let mut notes = Vec::new();
    for k in 0..6 {
        let s = chimera_core::factory::factory_sound(k).unwrap();
        let e = p.edit(PartId::ALL[k]);
        **e.sound = s;
        e.mix.channel = ch(k as u8);
        e.sound.params.out.sends = [SendLevel::new(0.6); 3];
        e.mix.output = chimera_core::part::DacPair::ALL[k % 3];
        for n in CHORD {
            notes.push((k, n + 12 * (k as u8 % 2)));
        }
    }
    (p, notes)
}

#[test]
#[ignore = "prints the loudness table"]
fn loudness_table() {
    println!("| sound | note | peak dBFS | rms dBFS | pre-limiter pk | max GR dB |");
    for (name, perf, src) in engines() {
        for (what, notes, vel) in [
            ("C4 v100", vec![(0usize, C4)], 100u8),
            ("chord4 v100", CHORD.iter().map(|&n| (0, n)).collect(), 100),
        ] {
            let r = run(&perf, &notes, vel, HOLD, TOTAL, src.as_deref());
            println!(
                "| {name} | {what} | {:.1} | {:.1} | {:.2} | {:.1} |",
                db(r.peak(2)),
                db(r.rms(2)),
                r.pre.iter().fold(0.0f32, |m, &x| m.max(x)),
                r.gr.iter().skip(2).fold(0.0f32, |m, &x| m.min(x))
            );
        }
    }
    let (p, notes) = dense();
    let r = run(&p, &notes, 127, HOLD, TOTAL, None);
    println!(
        "| DENSE 6 parts | 24 notes v127 | {:.1} | {:.1} | {:.2} | {:.1} |",
        db(r.peak(2)),
        db(r.rms(2)),
        r.pre.iter().fold(0.0f32, |m, &x| m.max(x)),
        r.gr.iter().skip(2).fold(0.0f32, |m, &x| m.min(x))
    );
}

fn peak_of(perf: &Performance, notes: &[(usize, u8)], vel: u8, src: Option<&[u8]>) -> f32 {
    run(perf, notes, vel, HOLD, TOTAL, src).peak(0)
}

/// #411: a lone ALGO INIT note lands at −12..−8 dBFS peak; every INIT
/// note is at least that loud and under the ceiling.
#[test]
fn a_single_init_note_reaches_the_target() {
    let algo = db(peak_of(&init_perf(EngineType::Algo), &[(0, C4)], 100, None));
    println!("ALGO INIT C4 v100: {algo:.1} dBFS");
    assert!((-12.5..=-8.0).contains(&algo), "{algo}");
    for (name, perf, src) in engines().into_iter().take(4) {
        let p = db(peak_of(&perf, &[(0, C4)], 100, src.as_deref()));
        assert!((-12.5..=db(CEILING)).contains(&p), "{name}: {p:.1}");
    }
}

/// #193: INIT through REV SEND 0.5 (mix 0.5, time 0.7) and at MORPH B
/// stay at or under full scale (they read 1.55 and 1.27 before the limiter
/// was last in the chain; ADR 0109 keeps it so with the gain up).
#[test]
fn issue_193_scenes_stay_under_full_scale() {
    let mut rev = init_perf(EngineType::Algo);
    rev.fx.reverb.mix = 0.5;
    rev.fx.reverb.time = 0.7;
    rev.edit(PartId::ALL[0]).sound.params.out.sends[2] = SendLevel::new(0.5);
    let mut morph = init_perf(EngineType::Algo);
    morph.edit(PartId::ALL[0]).sound.params.algo.morph = 127;
    for (what, perf) in [("REV 0.5", rev), ("MORPH B", morph)] {
        for notes in [vec![(0, C4)], CHORD.iter().map(|&n| (0, n)).collect()] {
            let p = peak_of(&perf, &notes, 127, None);
            assert!(p <= CEILING, "{what}: {p}");
        }
    }
}

/// Six Parts, 24 notes, sends up, FX on: at or under the ceiling.
#[test]
fn the_dense_all_parts_scene_stays_under_full_scale() {
    let (p, notes) = dense();
    let r = run(&p, &notes, 127, HOLD, TOTAL, None);
    assert!(r.peak(0) <= CEILING, "{}", r.peak(0));
    assert!(r.out.iter().flatten().all(|x| x.is_finite()));
}

/// The standard deviation, dB, of `e`'s level in windows of 16 blocks
/// (21 ms) over `range`.
fn envelope_sd(e: &[f32], range: core::ops::Range<usize>) -> f32 {
    let v: Vec<f32> = e[range]
        .chunks(16)
        .map(|w| db((w.iter().sum::<f32>() / w.len() as f32).sqrt()))
        .collect();
    let mean = v.iter().sum::<f32>() / v.len() as f32;
    (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt()
}

/// A sustained chord does not pump: on a held ALGO INIT chord, four notes
/// at velocity 100 and eight at 127, the level out moves no more in time
/// than the level in does (21 ms windows, plus 0.15 dB, under a level step's threshold), and the limiter
/// is working (over 1 dB of reduction).
#[test]
fn a_sustained_chord_does_not_pump() {
    let wide: [u8; 8] = [36, 43, 48, 52, 55, 60, 64, 72];
    for (notes, vel) in [(CHORD.to_vec(), 100), (wide.to_vec(), 127)] {
        let notes: Vec<_> = notes.into_iter().map(|n| (0, n)).collect();
        let r = run(&init_perf(EngineType::Algo), &notes, vel, HOLD, TOTAL, None);
        let held = 300..HOLD - 50;
        let mean = r.gr[held.clone()].iter().sum::<f32>() / held.len() as f32;
        let (sd_in, sd_out) = (
            envelope_sd(&r.e_in, held.clone()),
            envelope_sd(&r.e_out, held),
        );
        println!(
            "{} notes: GR {mean:.2} dB, level sd in {sd_in:.3} out {sd_out:.3} dB",
            notes.len()
        );
        assert!(mean < -1.0, "the chord is limited ({mean})");
        assert!(
            sd_out <= sd_in + 0.15,
            "{} notes: {sd_out} vs {sd_in}",
            notes.len()
        );
    }
}

/// Silence stays silence.
#[test]
fn silence_stays_silence() {
    let r = run(&init_perf(EngineType::Algo), &[], 100, HOLD, 200, None);
    assert!(r.out.iter().flatten().all(|&x| x == 0.0));
}

/// The static curve: a steady level in comes out as itself up to the
/// knee, then rises smoothly (never down, slope under 1) toward the
/// ceiling and never past it.
#[test]
fn the_knee_is_monotonic_smooth_and_bounded() {
    use chimera_core::dsp::limiter::{DacBlocks, KNEE, Limiter, OUTPUT_GAIN};
    let out_at = |o: f32| {
        let mut lim = Box::new(Limiter::new());
        let mut dac = Box::new(DacBlocks::new());
        for _ in 0..60 {
            for pair in dac.mix().iter_mut() {
                pair.fill(o / OUTPUT_GAIN);
            }
            lim.process(&mut dac, 48_000);
        }
        dac.out()[0][5]
    };
    let mut prev = (0.0f32, 0.0f32);
    for i in 1..=300 {
        let o = i as f32 * 0.01;
        let y = out_at(o);
        assert!(y <= CEILING, "{o}: {y}");
        if o <= KNEE {
            assert!((y - o).abs() < 1e-5, "{o}: {y} under the knee");
        }
        let slope = (y - prev.1) / (o - prev.0);
        assert!((0.0..=1.0001).contains(&slope), "{o}: slope {slope}");
        prev = (o, y);
    }
}

fn wav24(path: &std::path::Path, x: &[f32]) {
    let n = x.len() as u32 * 3;
    let mut b = Vec::with_capacity(44 + n as usize);
    b.extend(b"RIFF");
    b.extend((36 + n).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend([1, 0, 2, 0]);
    b.extend(48_000u32.to_le_bytes());
    b.extend((48_000u32 * 6).to_le_bytes());
    b.extend([6, 0, 24, 0]);
    b.extend(b"data");
    b.extend(n.to_le_bytes());
    for &s in x {
        let v = (chimera_core::dsp::clampf(s, -1.0, 1.0) * 8_388_607.0).round() as i32;
        b.extend(&v.to_le_bytes()[..3]);
    }
    std::fs::write(path, b).unwrap();
}

/// The A/B takes (unnormalised 24-bit WAVs, pair 1 only) into
/// `$LOUDNESS_DEMO`, named `<scene>_<$LOUDNESS_TAG>.wav`.
#[test]
#[ignore = "writes the demo takes"]
fn render_demo() {
    let dir = std::path::PathBuf::from(std::env::var("LOUDNESS_DEMO").unwrap());
    let tag = std::env::var("LOUDNESS_TAG").unwrap();
    let sound = |i: usize| {
        let mut p = Performance::new();
        **p.edit(PartId::ALL[0]).sound = chimera_core::factory::factory_sound(i).unwrap();
        p
    };
    let (mut dense, notes) = dense();
    for k in 0..6 {
        dense.edit(PartId::ALL[k]).mix.output = chimera_core::part::DacPair::P1;
    }
    let takes = [
        ("bass-hit", sound(5), vec![(0, 36)], 110u8, 300, 1000),
        (
            "chord-stab",
            sound(1),
            CHORD.iter().map(|&n| (0, n)).collect(),
            110,
            190,
            1100,
        ),
        ("dense-all-parts", dense, notes, 127, 700, 1500),
    ];
    for (name, perf, notes, vel, hold, total) in takes {
        let r = run(&perf, &notes, vel, hold, total, None);
        wav24(&dir.join(format!("{name}_{tag}.wav")), &r.out[0]);
        println!("{name} {tag}: peak {:.1} dBFS", db(r.peak(0)));
    }
}
