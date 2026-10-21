//! Records `fixtures/tape_legacy.txt` on the code before PLAY (ADR 0200):
//! each old-machine Sound as its file's bytes and its render's hash. Run
//! once, at origin/tape-streaming 0d026a9a:
//! `RECORD=1 cargo test -p chimera-desktop --test tape_legacy_record -- --ignored`

mod tape_render;

use chimera_core::block::Block as _;
use chimera_core::dsp::tape_engine::machine::{defaults, env_defaults};
use chimera_core::dsp::tape_engine::{Retrig, TapeEngineParams as P, TapeMachine};
use chimera_core::preset::Sound;
use chimera_core::storage::{FileKind, Generation, Header, encode_sound, write_file};
use chimera_hal::store::{ByteSink, StoreError};
use std::fmt::Write as _;
use tape_render::*;

struct VecSink(Vec<u8>);
impl ByteSink for VecSink {
    fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

fn encode(s: &Sound) -> Vec<u8> {
    let h = Header { kind: FileKind::Sound, generation: Generation::FIRST, name: Some(s.name) };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| encode_sound(s, w)).unwrap();
    sink.0
}

/// What the UI writes on a MODEL turn.
fn turn(s: &Sound, m: TapeMachine) -> Sound {
    let mut s = s.clone();
    s.params.tape.set_machine(m);
    for (id, v) in defaults(m) {
        s.params.tape.set(id, v);
    }
    for &(id, v) in env_defaults(m) {
        s.params.envelopes[0].set(id, v);
    }
    s
}

fn with(s: &Sound, set: &[(chimera_core::block::ParamId, f32)]) -> Sound {
    let mut s = s.clone();
    for &(id, v) in set {
        s.params.tape.set(id, v);
    }
    s
}

#[test]
#[ignore]
fn record() {
    if std::env::var_os("RECORD").is_none() {
        return;
    }
    let (r, _) = set();
    // (case, Sound, source, expect)
    let mut cases: Vec<(String, Sound, &str, &str)> = Vec::new();
    for s in &r.sounds {
        let Some((snd, _)) = factory(s.name.as_str()) else { continue };
        cases.push((format!("factory {}", s.name.as_str()), snd, s.source.as_str(), "same"));
    }
    let f = |n: &str| factory(n).unwrap().0;
    cases.extend([
        (
            "ONE, POSITION SIZE MOTION it never read".into(),
            with(&f("EPIANO HIT"), &[(P::POSITION, 0.4), (P::SIZE, 0.5), (P::MOTION, 0.3)]),
            "EPIANO C4",
            "same",
        ),
        ("ONE, FADE short of HOLD".into(), with(&f("TX BASS HIT"), &[(P::DRAG, 0.4)]), "TX BASS C2", "same"),
        ("ONE, RETRIG CUT".into(), with(&f("KIT"), &[(P::RETRIG, Retrig::Cut as u8 as f32)]), "KIT 16", "same"),
        (
            "ONE, END before START".into(),
            with(&f("EPIANO HIT"), &[(P::START, 0.8), (P::END, 0.1)]),
            "EPIANO C4",
            "same",
        ),
        (
            "LOOP, POSITION SIZE XFADE".into(),
            with(&f("PAD LOOP"), &[(P::POSITION, 0.3), (P::SIZE, 0.6), (P::XFADE, 0.3)]),
            "PAD CHORD",
            "same",
        ),
        (
            "LOOP, END before START".into(),
            with(&f("BRASS LOOP"), &[(P::START, 0.9), (P::END, 0.1)]),
            "BRASS C4",
            "same",
        ),
        (
            "LOOP, the old WALK".into(),
            with(&f("PAD LOOP"), &[(P::MOTION, 0.4), (P::SIZE, 0.5)]),
            "PAD CHORD",
            "differs: WALK is now a random step at each note (owner), not a drift in time",
        ),
        (
            "PING, POSITION SIZE".into(),
            with(&f("PAD PINGPONG"), &[(P::POSITION, 0.5), (P::SIZE, 0.5)]),
            "PAD CHORD",
            "same",
        ),
        ("TRON, LENGTH 0.5".into(), with(&f("TRON BOWED"), &[(P::SIZE, 0.5)]), "TRON BOWED", "same"),
        (
            "TRON, POSITION it never read".into(),
            with(&f("TRON BOWED"), &[(P::POSITION, 0.5), (P::MOTION, -0.5)]),
            "TRON BOWED",
            "same",
        ),
        (
            "TRON, END before START".into(),
            with(&f("TRON BOWED"), &[(P::START, 0.9), (P::END, 0.2)]),
            "TRON BOWED",
            "same",
        ),
        ("TRON, turned from EPIANO HIT".into(), turn(&f("EPIANO HIT"), TapeMachine::Tron), "EPIANO C4", "same"),
        ("TRON, FADE 0.7".into(), with(&f("TRON BOWED"), &[(P::DRAG, 0.7)]), "TRON BOWED", "same"),
        ("LOOP, turned from EPIANO HIT".into(), turn(&f("EPIANO HIT"), TapeMachine::Loop), "EPIANO C4", "same"),
        ("PING, turned from SINE LOOP".into(), turn(&f("SINE LOOP"), TapeMachine::Ping), "SINE C4", "same"),
        ("ONE, turned from PAD LOOP".into(), turn(&f("PAD LOOP"), TapeMachine::One), "PAD CHORD", "same"),
    ]);
    let mut out = String::from(
        "# Old-machine Sounds before PLAY (ADR 0200): case|source|expect|file hex|render hash.\n\
         # Recorded by tape_legacy_record.rs at origin/tape-streaming 0d026a9a: the phrase at\n\
         # the source's root, 4.5 s, the DAC's first pair after the limiter.\n",
    );
    for (name, snd, src, expect) in cases {
        let b = source(src).unwrap();
        let root = ((b.head.root + 50) / 100) as u8;
        let h = hash(&render(&snd, b, &phrase(root), 4.5));
        let hex: String = encode(&snd).iter().map(|b| format!("{b:02x}")).collect();
        writeln!(out, "{name}|{src}|{expect}|{hex}|{h:016x}").unwrap();
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tape_legacy.txt");
    std::fs::write(path, out).unwrap();
}

/// Today's TRON against ONE (the same Sound turned), on held chords with
/// key-ups: levels after the key-up and where each ends.
#[test]
#[ignore]
fn tron_against_one() {
    let (tron, b) = factory("TRON BOWED").unwrap();
    let one = turn(&tron, TapeMachine::One);
    let r = ((b.head.root + 50) / 100) as u8;
    for (slug, ev, secs) in [
        ("keyup 1.5 s", chord(&[r - 7, r - 3, r], 0.0, 1.5), 6.0),
        ("held 10 s", chord(&[r - 19, r - 15, r - 12], 0.0, 10.0), 12.0),
    ] {
        for (w, s) in [("TRON", &tron), ("ONE", &one)] {
            let x = mono(&render(s, b, &ev, secs));
            let f = frames(&x, 100.0);
            let end = f.iter().rposition(|&v| db(v) > -60.0).map_or(0.0, |i| (i + 1) as f32 * 0.1);
            let line: Vec<String> = f.iter().map(|&v| format!("{:.0}", db(v).max(-99.0))).collect();
            println!("{slug} {w}: end {end:.1} s, hash {:016x}\n  {}", hash(&x), line.join(" "));
        }
    }
    println!("TRON tape {:?}\nONE tape {:?}", tron.params.tape, one.params.tape);
    println!("TRON env1 {:?}\nONE env1 {:?}", tron.params.envelopes[0], one.params.envelopes[0]);
}
