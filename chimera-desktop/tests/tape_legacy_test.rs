// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Old-machine Sounds load as WALK and sound the same (ADR 0203): each
//! file `fixtures/tape_legacy.txt` holds, written before WALK, decodes and
//! renders through the whole instrument bit for bit as it did then, but
//! where the fixture says why not. And TRON is heard: on a held chord its
//! tape runs out and key up lifts the pad, where TRON off plays on.

mod tape_render;

use chimera_core::dsp::tape_engine::{TapeMachine, Tron, Way};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::{Check, Decode, Framer, SoundDecoder};
use tape_render::*;

fn decode(bytes: &[u8]) -> Sound {
    let mut s = Sound::neutral(EngineType::Algo);
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

struct Case {
    name: String,
    source: String,
    same: bool,
    bytes: Vec<u8>,
    hash: u64,
}

fn cases() -> Vec<Case> {
    let text = include_str!("fixtures/tape_legacy.txt");
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('|').collect();
            let bytes = (0..f[3].len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&f[3][i..i + 2], 16).unwrap())
                .collect();
            Case {
                name: f[0].into(),
                source: f[1].into(),
                same: f[2] == "same",
                bytes,
                hash: u64::from_str_radix(f[4], 16).unwrap(),
            }
        })
        .collect()
}

fn root(b: &chimera_desktop::import::tape::Built) -> u8 {
    ((b.head.root + 50) / 100) as u8
}

#[test]
fn old_machine_sounds_render_bit_identically() {
    let mut checked = 0;
    for c in cases() {
        let Some(b) = source(&c.source) else {
            eprintln!("{}: its CC0 original is absent, skipped", c.name);
            continue;
        };
        let s = decode(&c.bytes);
        let got = hash(&render(&s, b, &phrase(root(b)), 4.5));
        if c.same {
            assert_eq!(got, c.hash, "{}: renders as before PLAY", c.name);
        } else {
            assert_ne!(got, c.hash, "{}: the fixture says it differs", c.name);
        }
        checked += 1;
    }
    assert!(checked >= 45, "{checked}");
}

/// Each factory TAPE Sound, re-pointed to WALK and built now, renders as
/// its file from before WALK did. STRETCH's three are rebuilt on GRAIN
/// with ONE's ENV 1 (ADR 0204), so they are not WALK's to keep.
#[test]
fn factory_sounds_on_play_render_as_before() {
    let mut checked = 0;
    for c in cases() {
        let Some(name) = c.name.strip_prefix("factory ").filter(|_| c.same) else {
            continue;
        };
        let Some((s, b)) = factory(name) else {
            continue;
        };
        let rebuilt = match s.params.tape.machine {
            TapeMachine::Stretch => true,
            TapeMachine::Walk | TapeMachine::Scrub | TapeMachine::Wave => false,
        };
        if rebuilt {
            continue;
        }
        assert_eq!(
            hash(&render(&s, b, &phrase(root(b)), 4.5)),
            c.hash,
            "{name}"
        );
        checked += 1;
    }
    assert!(checked >= 23, "{checked}");
}

/// What each old machine loads as: a factory Sound as its recipe now
/// names it (WALK with its DIRECTION and TRON), the others by the machine
/// they were made on.
#[test]
fn old_machines_load_as_play_with_their_direction() {
    let (r, _) = set();
    for c in cases() {
        let t = decode(&c.bytes).params.tape;
        let want = match c.name.strip_prefix("factory ") {
            Some(n) => {
                let s = r.sounds.iter().find(|s| s.name.as_str() == n).unwrap();
                (s.machine, s.direction, s.tron)
            }
            None => {
                let w = match c.name.split(',').next().unwrap() {
                    "ONE" => Way::One,
                    "LOOP" => Way::Loop,
                    "PING" => Way::Ping,
                    "TRON" => Way::Tron,
                    o => panic!("{o}"),
                };
                w.parts()
            }
        };
        if want.0 == TapeMachine::Walk {
            assert_eq!((t.machine, t.direction, t.tron), want, "{}", c.name);
        } else {
            assert_eq!(t.machine, want.0, "{}", c.name);
        }
    }
}

/// TRON BOWED (a 4 s strip at its root, LENGTH 1: an 8 s tape) on a low
/// chord held 10 s, then on one let go at 1.5 s: ON, the strip comes
/// round until the tape runs out at 8 s while held, and its pad lifts at
/// key up; OFF (FORWARD at HOLD), it plays to END either way, slower below
/// the root.
#[test]
fn tron_is_heard_on_a_held_chord() {
    let Some((on, b)) = factory("TRON BOWED") else {
        return;
    };
    assert_eq!(on.params.tape.way(), Way::Tron);
    let mut off = on.clone();
    off.params.tape.tron = Tron::Off;
    off.params.tape.drag = chimera_core::dsp::tape_engine::FADE_HOLD;
    let r = root(b);
    let held = chord(&[r - 24, r - 19, r - 15], 0.0, 10.0);
    let (a, z) = (
        mono(&render(&on, b, &held, 11.0)),
        mono(&render(&off, b, &held, 11.0)),
    );
    assert!(
        level_in(&a, 7.0, 7.8) > -30.0,
        "ON: still on tape at 7 s, {}",
        level_in(&a, 7.0, 7.8)
    );
    assert!(
        level_in(&a, 8.2, 9.5) < -60.0,
        "ON: run out at 8 s, {}",
        level_in(&a, 8.2, 9.5)
    );
    assert!(
        level_in(&z, 8.2, 9.5) > -30.0,
        "OFF plays on, {}",
        level_in(&z, 8.2, 9.5)
    );
    // At the root the strip ends at 4 s, OFF; ON goes round to 8 s.
    let root_chord = chord(&[r, r + 4, r + 7], 0.0, 10.0);
    let (a, z) = (
        mono(&render(&on, b, &root_chord, 9.0)),
        mono(&render(&off, b, &root_chord, 9.0)),
    );
    assert!(
        level_in(&a, 5.0, 7.5) > -30.0,
        "ON: round again, {}",
        level_in(&a, 5.0, 7.5)
    );
    assert!(
        level_in(&z, 5.0, 7.5) < -60.0,
        "OFF: ended at END, {}",
        level_in(&z, 5.0, 7.5)
    );
    let up = chord(&[r - 12, r - 7, r - 3], 0.0, 1.5);
    let (a, z) = (
        mono(&render(&on, b, &up, 3.0)),
        mono(&render(&off, b, &up, 3.0)),
    );
    assert!(
        level_in(&a, 1.7, 2.5) < -60.0,
        "ON: the pad lifts, {}",
        level_in(&a, 1.7, 2.5)
    );
    assert!(
        level_in(&z, 1.7, 2.5) > -30.0,
        "OFF holds, {}",
        level_in(&z, 1.7, 2.5)
    );
}
