//! The filter types' desktop QA (plan Task 19), on the real mix path.
//!
//! `per_pair_levels` (ignored): every TYPE, MODE and 2040 VOICE at RES 0,
//! ½ and 1 on a held saw: finite, under the ceiling, every voice freed after
//! the release; prints the RMS/peak table, flagging pairs more than 6 dB from
//! CLEAN LP4 at RES 0. `demo_render` writes the owner's short-sound takes to
//! `$FILTER_QA_WAV` when it is set; `ladder_demo_render` LADDER's A/B takes
//! to `$LADDER_DEMO_WAV`.

mod common;

use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::{FilterKind, K2040Voice, KIND_NAMES};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::params::EngineType;
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::taps::TapOp;
use chimera_hal::BLOCK_SIZE;
use common::sweep::{BPS, Bench, Patch, Tape, db, within_ceiling};

fn saw() -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo = AlgoParams::single(WaveId::SAW);
    s
}

fn filtered(mut s: Sound, kind: FilterKind, voice: K2040Voice, cutoff: f32, res: f32) -> Sound {
    let f = &mut s.params.filter;
    f.set_kind(kind);
    f.voice = voice;
    (f.cutoff, f.resonance) = (cutoff, res);
    s
}

fn bench(s: Sound) -> Bench {
    let mut b = Bench::new(Patch::AlgoInit, PartParams::default(), FxParams::default());
    b.edit_sound(|x| *x = s);
    b
}

/// `notes` (note, on block, off block) over `blocks`.
fn play(b: &mut Bench, notes: &[(u8, usize, usize)], blocks: usize) -> Tape {
    let mut t = Tape::default();
    for i in 0..blocks {
        for &(n, on, off) in notes {
            if i == on {
                b.note(n, Some(100));
            }
            if i == off {
                b.note(n, None);
            }
        }
        b.render(&mut t);
    }
    t
}

#[test]
#[ignore = "QA table: cargo test --release --test filter_qa_test -- --ignored --nocapture"]
fn per_pair_levels() {
    let (on, off, end) = (BPS * 3 / 10, BPS * 3 / 2, BPS * 6);
    let mut rows = Vec::new();
    for kind in FilterKind::BUILT {
        let voices: &[K2040Voice] = if kind == FilterKind::K2040 {
            &K2040Voice::ALL
        } else {
            &[K2040Voice::Rev2]
        };
        for &voice in voices {
            for (mi, &mode) in kind.modes().iter().enumerate() {
                for res in [0.0, 0.5, 1.0] {
                    let mut s = filtered(saw(), kind, voice, 1000.0, res);
                    assert!(s.params.filter.set_mode(mode));
                    let mut b = bench(s);
                    let t = play(&mut b, &[(48, 0, off)], end);
                    let held = &t.bus[on * BLOCK_SIZE..off * BLOCK_SIZE];
                    let name = match kind {
                        FilterKind::K2040 => format!("2040 {:?}", voice).to_uppercase(),
                        _ => KIND_NAMES[FilterKind::BUILT.iter().position(|&x| x == kind).unwrap()]
                            .into(),
                    };
                    rows.push((
                        format!("{name} {} RES {res}", kind.mode_labels()[mi]),
                        db(common::rms(held)),
                        db(t.peak(0)),
                        t.finite() && within_ceiling(t.peak(0)),
                        b.all_free(),
                        t.freed(off).map(|blk| blk as f32 / BPS as f32),
                    ));
                }
            }
        }
    }
    let base = rows[rows
        .iter()
        .position(|r| r.0 == "CLEAN LP4 RES 0")
        .expect("CLEAN LP4")]
    .1;
    println!(
        "| pair | RMS dB | Δ CLEAN LP4 | peak dBFS | ok | freed s |\n|---|---|---|---|---|---|"
    );
    for (n, r, p, ok, free, fs) in &rows {
        let flag = if (r - base).abs() > 6.0 { " ⚑" } else { "" };
        println!(
            "| {n} | {r:.1} | {:+.1}{flag} | {p:.1} | {} | {} |",
            r - base,
            if *ok { "ok" } else { "FAIL" },
            if *free {
                fs.map_or("?".into(), |s| format!("{s:.2}"))
            } else {
                "STUCK".into()
            },
        );
    }
    let bad: Vec<_> = rows.iter().filter(|r| !r.3 || !r.4).map(|r| &r.0).collect();
    assert!(
        bad.is_empty(),
        "non-finite, over the ceiling or stuck: {bad:?}"
    );
}

/// ALGO's taps (OP5 and OP6 into FREQ, Q, VCA and FOLD) on every type, on
/// MORPH PAD (six operators sounding): at ±100 % from the note-on, and
/// jumped 0 → 100 % mid-note. Finite, under the ceiling, freed, and the
/// jump judged by the sweep's click rule.
#[test]
#[ignore = "QA: cargo test --release --test filter_qa_test -- --ignored --nocapture"]
fn taps_on_algo() {
    let (jump, off, end) = (BPS, BPS * 2, BPS * 6);
    let mut bad = Vec::new();
    let mut clicks = Vec::new();
    let mut n = 0;
    for (kind, voice) in [
        (FilterKind::Svf, K2040Voice::Rev2),
        (FilterKind::K2040, K2040Voice::Rev2),
        (FilterKind::K2040, K2040Voice::Exp),
        (FilterKind::Dirty, K2040Voice::Rev2),
        (FilterKind::Ladder, K2040Voice::Rev2),
    ] {
        for op in [TapOp::Op5, TapOp::Op6] {
            for col in FixedCol::ALL {
                let mut held: [Vec<f32>; 3] = Default::default();
                for (ai, amt) in [127i8, -127, 0].into_iter().enumerate() {
                    let mut s = Patch::Factory(6).sound();
                    s.params.filter.set_kind(kind);
                    (s.params.filter.voice, s.params.filter.resonance) = (voice, 0.6);
                    s.params.taps.set(op, col, amt);
                    let mut b = bench(s);
                    let mut t = Tape::default();
                    for i in 0..end {
                        match i {
                            0 => b.note(48, Some(100)),
                            _ if i == jump && amt == 0 => {
                                b.edit_sound(|s| s.params.taps.set(op, col, 127))
                            }
                            _ if i == off => b.note(48, None),
                            _ => {}
                        }
                        b.render(&mut t);
                    }
                    n += 1;
                    held[ai] = t.pair(0, BPS / 2, jump).to_vec();
                    let label = format!("{kind:?} {voice:?} {op:?}→{col:?} {amt}");
                    if !(t.finite() && within_ceiling(t.peak(0)) && b.all_free()) {
                        bad.push(label.clone());
                    }
                    if amt == 0
                        && let Some(r) = t.click(0, jump, (0, off), true)
                    {
                        clicks.push(format!("{label}→127 click ×{r:.1}"));
                    }
                }
                // The route reaches the voice: ±100 % changes what is heard
                // (the 0 run is untapped until its jump).
                let moved = |a: &[f32]| db(common::rms_diff(a, &held[2]) / common::rms(&held[2]));
                let (up, down) = (moved(&held[0]), moved(&held[1]));
                println!("{kind:?} {voice:?} {op:?}→{col:?}: ±100 % moves {up:.1} / {down:.1} dB");
                if up.max(down) < -60.0 {
                    bad.push(format!("{kind:?} {voice:?} {op:?}→{col:?} inert"));
                }
            }
        }
    }
    println!("taps: {n} runs; clicks {clicks:?}");
    assert!(
        bad.is_empty(),
        "non-finite, over the ceiling or stuck: {bad:?}"
    );
}

fn op(wave: WaveId, coarse: u8, env: [u8; 5], feedback: u8) -> AlgoOpParams {
    let [ar, d1r, d1l, d2r, rr] = env;
    AlgoOpParams {
        wave: wave.get(),
        coarse,
        level: 99,
        ar,
        d1r,
        d1l,
        d2r,
        rr,
        feedback,
        ..AlgoOpParams::default()
    }
}

fn voice_of(o: AlgoOpParams) -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo = AlgoParams::single(WaveId::W1);
    s.params.algo.ops[0] = o;
    s
}

/// The short-sound set: (name, Sound, cutoff, notes, seconds).
type Stim = (&'static str, Sound, f32, Vec<(u8, usize, usize)>, f32);

fn stimuli() -> Vec<Stim> {
    let b = |s: f32| (s * BPS as f32) as usize;
    let hats = (0..8)
        .map(|i| (78, b(0.125 * i as f32), b(0.125 * i as f32 + 0.03)))
        .collect();
    vec![
        (
            "bass-hit",
            voice_of(op(WaveId::SAW, 4, [31, 14, 0, 0, 10], 0)),
            600.0,
            vec![(36, 0, b(0.4))],
            2.0,
        ),
        (
            "chord-stab",
            voice_of(op(WaveId::SAW, 4, [31, 12, 0, 0, 9], 0)),
            1500.0,
            vec![
                (57, 0, b(0.25)),
                (60, 0, b(0.25)),
                (64, 0, b(0.25)),
                (67, 0, b(0.25)),
            ],
            2.0,
        ),
        (
            "short-sine",
            voice_of(op(WaveId::W1, 4, [31, 0, 15, 0, 10], 0)),
            2000.0,
            vec![(72, 0, b(0.15))],
            1.0,
        ),
        (
            "long-sine",
            voice_of(op(WaveId::W1, 4, [31, 0, 15, 0, 6], 0)),
            2000.0,
            vec![(60, 0, b(2.0))],
            3.5,
        ),
        (
            "hats",
            voice_of(op(WaveId::SQR, 33, [31, 26, 0, 0, 15], 7)),
            8000.0,
            hats,
            1.5,
        ),
    ]
}

#[test]
fn demo_render() {
    let Some(dir) = std::env::var_os("FILTER_QA_WAV") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("the WAV dir");
    let types = [
        ("clean", FilterKind::Svf, K2040Voice::Rev2),
        ("2040-rev2", FilterKind::K2040, K2040Voice::Rev2),
        ("2040-exp", FilterKind::K2040, K2040Voice::Exp),
        ("dirty", FilterKind::Dirty, K2040Voice::Rev2),
    ];
    for (stim, s, cutoff, notes, secs) in stimuli() {
        for (tname, kind, voice) in types {
            let mut b = bench(filtered(s.clone(), kind, voice, cutoff, 0.5));
            let t = play(&mut b, &notes, (secs * BPS as f32) as usize);
            assert!(t.finite(), "{stim} {tname}: non-finite");
            let name = format!("{stim}_{tname}.wav");
            write_wav(&dir.join(&name), &t.out[0]);
            println!(
                "{name} peak {:.1} dBFS free {}",
                db(t.peak(0)),
                b.all_free()
            );
        }
    }
}

/// LADDER's A/B set (ADR 0104): bass hit, chord stab, an acid line and a
/// Moog-style bassline, each through CLEAN, the 2040's two voices, DIRTY
/// and LADDER at the same FREQ, RES and LEVEL ½, the types' default modes.
/// The two lines sweep FREQ from ENV 1 (A 10 ms, no sustain).
#[test]
fn ladder_demo_render() {
    let Some(dir) = std::env::var_os("LADDER_DEMO_WAV") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("the WAV dir");
    let types = [
        ("clean", FilterKind::Svf, K2040Voice::Rev2),
        ("2040-rev2", FilterKind::K2040, K2040Voice::Rev2),
        ("2040-exp", FilterKind::K2040, K2040Voice::Exp),
        ("dirty", FilterKind::Dirty, K2040Voice::Rev2),
        ("ladder", FilterKind::Ladder, K2040Voice::Rev2),
    ];
    let b = |s: f32| (s * BPS as f32) as usize;
    // 16ths at 130 BPM: (step, note).
    let sixteenth = 60.0 / 130.0 / 4.0;
    let line = |steps: &[(usize, u8)], gate: f32| -> Vec<(u8, usize, usize)> {
        steps
            .iter()
            .map(|&(i, n)| {
                let t = i as f32 * sixteenth;
                (n, b(t), b(t + gate * sixteenth))
            })
            .collect()
    };
    let acid = line(
        &[
            (0, 36),
            (2, 48),
            (3, 36),
            (4, 39),
            (6, 36),
            (7, 46),
            (8, 36),
            (10, 41),
            (11, 36),
            (12, 48),
            (14, 43),
            (15, 36),
        ],
        0.6,
    );
    let moog = line(
        &[
            (0, 31),
            (3, 31),
            (6, 43),
            (8, 34),
            (11, 34),
            (12, 36),
            (14, 38),
        ],
        0.9,
    );
    let swept = |mut s: Sound, decay: f32, amount: i8| {
        let e = &mut s.params.envelopes[0];
        (e.decay, e.sustain) = (decay, 0.0);
        s.mod_state
            .set_fixed(ModSource::Env1.index(), FixedCol::Freq, amount);
        s
    };
    let saw_op = || voice_of(op(WaveId::SAW, 4, [31, 0, 15, 0, 12], 0));
    type Take = (&'static str, Sound, f32, f32, Vec<(u8, usize, usize)>, f32);
    let set: Vec<Take> = vec![
        (
            "bass-hit",
            voice_of(op(WaveId::SAW, 4, [31, 14, 0, 0, 10], 0)),
            600.0,
            0.5,
            vec![(36, 0, b(0.4))],
            2.0,
        ),
        (
            "chord-stab",
            voice_of(op(WaveId::SAW, 4, [31, 12, 0, 0, 9], 0)),
            1500.0,
            0.5,
            vec![
                (57, 0, b(0.25)),
                (60, 0, b(0.25)),
                (64, 0, b(0.25)),
                (67, 0, b(0.25)),
            ],
            2.0,
        ),
        (
            "acid-line",
            swept(saw_op(), 0.42, 38),
            350.0,
            0.85,
            acid,
            2.2,
        ),
        ("moog-bass", swept(saw_op(), 0.5, 30), 300.0, 0.4, moog, 2.2),
    ];
    for (stim, s, cutoff, res, notes, secs) in set {
        for (tname, kind, voice) in types {
            let mut bn = bench(filtered(s.clone(), kind, voice, cutoff, res));
            let t = play(&mut bn, &notes, (secs * BPS as f32) as usize);
            assert!(t.finite(), "{stim} {tname}: non-finite");
            let name = format!("{stim}_{tname}.wav");
            write_wav(&dir.join(&name), &t.out[0]);
            println!(
                "{name} peak {:.1} dBFS rms {:.1} dBFS free {}",
                db(t.peak(0)),
                db(common::rms(&t.out[0])),
                bn.all_free()
            );
        }
    }
}

/// 48 kHz 16-bit stereo PCM.
fn write_wav(path: &std::path::Path, stereo: &[f32]) {
    let data = (stereo.len() * 2) as u32;
    let sr = common::SR;
    let mut v = Vec::with_capacity(44 + data as usize);
    for c in [
        &b"RIFF"[..],
        &(36 + data).to_le_bytes(),
        b"WAVEfmt ",
        &16u32.to_le_bytes(),
    ] {
        v.extend_from_slice(c);
    }
    for h in [1u16, 2] {
        v.extend_from_slice(&h.to_le_bytes());
    }
    v.extend_from_slice(&sr.to_le_bytes());
    v.extend_from_slice(&(sr * 4).to_le_bytes());
    v.extend_from_slice(&4u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    for &x in stereo {
        v.extend_from_slice(
            &((chimera_core::dsp::clampf(x, -1.0, 1.0) * 32767.0).round() as i16).to_le_bytes(),
        );
    }
    std::fs::write(path, v).expect("the WAV");
}
