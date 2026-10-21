//! The WALK listening page's takes (ADR 0202), through the whole
//! instrument, raw (ab-method: no normalisation), 32-bit float stereo:
//! `PAGE_OUT=dir cargo test -p chimera-desktop --test tape_play_listen -- --ignored --nocapture`

mod tape_render;

use chimera_core::block::Block as _;
use chimera_core::dsp::tape_engine::machine::{direction_defaults, env_defaults};
use chimera_core::dsp::tape_engine::params::Drift;
use chimera_core::dsp::tape_engine::{
    Direction, FADE_HOLD, TapeEngineParams as P, TapeMachine, Tron,
};
use chimera_core::preset::Sound;
use chimera_core::storage::{Check, Decode, Framer, SoundDecoder};
use std::fmt::Write as _;
use std::path::PathBuf;
use tape_render::*;

fn root(b: &chimera_desktop::import::tape::Built) -> u8 {
    ((b.head.root + 50) / 100) as u8
}

/// `s` turned to `d` as the UI turns DIRECTION.
fn direction(s: &Sound, d: Direction) -> Sound {
    let mut s = s.clone();
    let was = s.params.tape.direction;
    s.params.tape.direction = d;
    let w = direction_defaults(was, d);
    for &(id, v) in w {
        s.params.tape.set(id, v);
    }
    if !w.is_empty() {
        for &(id, v) in env_defaults(TapeMachine::Walk, d) {
            s.params.envelopes[0].set(id, v);
        }
    }
    s
}

fn decode(bytes: &[u8]) -> Sound {
    let mut s = Sound::neutral(chimera_core::params::EngineType::Algo);
    for apply in [false, true] {
        let mut d = SoundDecoder::new(&mut s);
        let mut f = Framer::new(bytes.len() as u32).unwrap();
        f.push(bytes, &mut |e| if apply { d.apply(e) } else { d.event(e) })
            .unwrap();
        f.finish().unwrap();
        if apply { d.commit() } else { d.end() }.unwrap();
    }
    s
}

/// The fixture's old file for `case`.
fn old(case: &str) -> Sound {
    let line = include_str!("fixtures/tape_legacy.txt")
        .lines()
        .find(|l| l.starts_with(&format!("{case}|")))
        .unwrap();
    let hex = line.split('|').nth(3).unwrap();
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    decode(&bytes)
}

/// Peak and RMS (over the louder half of 0.1 s frames) of an interleaved
/// render, dBFS.
fn stats(x: &[f32]) -> (f32, f32) {
    let m = mono(x);
    let peak = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    let mut f = frames(&m, 100.0);
    f.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let h = &f[..f.len().div_ceil(2)];
    (
        db(peak),
        db((h.iter().map(|v| v * v).sum::<f32>() / h.len() as f32).sqrt()),
    )
}

#[test]
#[ignore]
fn takes() {
    let dir = PathBuf::from(std::env::var("PAGE_OUT").expect("PAGE_OUT"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut log = String::new();
    let mut take = |name: &str,
                    s: &Sound,
                    b: &chimera_desktop::import::tape::Built,
                    ev: &Events,
                    secs: f32| {
        let x = render(s, b, ev, secs);
        write_wav(&dir.join(format!("{name}.wav")), &x, 2);
        let (pk, lv) = stats(&x);
        writeln!(log, "{name}\t{pk:.1}\t{lv:.1}\t{:016x}", hash(&x)).unwrap();
    };

    // Each DIRECTION on BELL HIT's bell (a decay: REVERSE swells), the
    // phrase at its root.
    let (bell, b) = factory("BELL HIT").unwrap();
    let r = root(b);
    for (slug, d) in [
        ("fwd", Direction::Forward),
        ("reverse", Direction::Reverse),
        ("loop", Direction::Loop),
        ("ping", Direction::Ping),
    ] {
        take(
            &format!("dir_bell_{slug}"),
            &direction(&bell, d),
            b,
            &phrase(r),
            4.5,
        );
    }
    // And on PAD LOOP's chord.
    let (pad, b) = factory("PAD LOOP").unwrap();
    let r = root(b);
    for (slug, d) in [
        ("fwd", Direction::Forward),
        ("reverse", Direction::Reverse),
        ("loop", Direction::Loop),
        ("ping", Direction::Ping),
    ] {
        take(
            &format!("dir_pad_{slug}"),
            &direction(&pad, d),
            b,
            &phrase(r),
            4.5,
        );
    }

    // DRIFT on a repeated note: TRON BOWED's 4 s strip, FORWARD, a quarter
    // of it a note; and on VOICE LOOP's loop.
    let (bowed, b) = factory("TRON BOWED").unwrap();
    let r = root(b);
    let mut one = bowed.clone();
    one.params.tape.tron = Tron::Off;
    one.params.tape.size = 0.75;
    one.params.tape.drag = 0.3;
    let reps: Events = (0..12)
        .flat_map(|k| {
            let t = 0.4 * k as f32;
            [(t, r, true), (t + 0.32, r, false)]
        })
        .collect();
    for w in [0.0, 0.1, 0.3, 1.0] {
        let mut s = one.clone();
        s.params.tape.drift = Drift::of(w);
        take(
            &format!("drift_bowed_{:03}", (w * 100.0) as u32),
            &s,
            b,
            &reps,
            5.2,
        );
    }
    if let Some((voice, b)) = factory("VOICE LOOP") {
        let r = root(b);
        let reps: Events = (0..10)
            .flat_map(|k| {
                let t = 0.45 * k as f32;
                [(t, r, true), (t + 0.4, r, false)]
            })
            .collect();
        for w in [0.0, 0.3, 1.0] {
            let mut s = voice.clone();
            (s.params.tape.drift, s.params.tape.size) = (Drift::of(w), 0.5);
            take(
                &format!("drift_voice_{:03}", (w * 100.0) as u32),
                &s,
                b,
                &reps,
                4.8,
            );
        }
    }

    // TRON off and on: a chord at the root held 10 s (OFF ends at the
    // strip's end, 4 s; ON goes round until its 8 s tape runs out), a low
    // chord held 10 s, then chords let go at 1 s, 2.5 s and 3.5 s (the pad).
    let mut off = bowed.clone();
    off.params.tape.tron = Tron::Off;
    off.params.tape.drag = FADE_HOLD;
    let held = chord(&[r, r + 4, r + 7], 0.0, 10.0);
    let low = chord(&[r - 24, r - 19, r - 15], 0.0, 10.0);
    let mut ups = chord(&[r - 12, r - 5, r], 0.0, 1.0);
    ups.extend(chord(&[r - 9, r - 5, r - 2], 1.5, 2.5));
    ups.extend(chord(&[r - 12, r - 7, r - 3], 3.0, 3.5));
    for (slug, s) in [("off", &off), ("on", &bowed)] {
        take(&format!("tron_held_{slug}"), s, b, &held, 10.5);
        take(&format!("tron_low_{slug}"), s, b, &low, 10.5);
        take(&format!("tron_keyups_{slug}"), s, b, &ups, 6.0);
    }

    // Old Sounds after the merge: each fixture file from before WALK,
    // loaded now (the `before` takes are the base tree's).
    for (slug, case, src) in [
        ("oneshot", "factory EPIANO HIT", "EPIANO C4"),
        ("loop", "factory PAD LOOP", "PAD CHORD"),
        ("ping", "factory PAD PINGPONG", "PAD CHORD"),
        ("tron", "factory TRON BOWED", "TRON BOWED"),
    ] {
        let b = source(src).unwrap();
        take(
            &format!("old_{slug}_after"),
            &old(case),
            b,
            &phrase(root(b)),
            4.5,
        );
    }
    print!("{log}");
    std::fs::write(dir.join("takes.tsv"), log).unwrap();
    let _ = P::default();
}
