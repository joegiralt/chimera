//! ALGO's MORPH is smooth, never steppy (owner UAT 2026-10-08, #417;
//! ADR 0111). Turns go through the real path: the panel's acceleration,
//! `Block::nudge`, a publish, the Instrument. What is measured is the MORPH
//! each block plays, as a fraction of its range, and the output's level.

mod common;

use chimera_core::block::Block;
use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::morph::Morph;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::params::EngineType;
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use common::part_rig::Rig;
use common::sweep::{Bench, Patch, Tape};
use common::turn::{FAST_DETENT_MS, MS_PER_BLOCK, fast_turn, slow_turn};

/// MORPH's stored range, ticks.
const RANGE: f32 = Morph::STORED_MAX;

/// A held ALGO INIT-style Sound on `a` → `b`, MORPH `m`.
fn algo(a: AlgoId, b: AlgoId, m: u8) -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    s.params
        .algo
        .try_set_algs(a, s.params.algo.algs().1)
        .unwrap();
    s.params
        .algo
        .try_set_algs(s.params.algo.algs().0, b)
        .unwrap();
    s.params.algo.morph = m;
    s.params.envelopes[0].sustain = 1.0;
    s
}

/// Held `s` over `ticks.len()` blocks, MORPH nudged by each block's ticks:
/// the MORPH each block played (a fraction of the range) and the output.
fn turn(s: Sound, ticks: &[i8]) -> (Vec<f32>, Tape) {
    let mut b = Bench::new(Patch::AlgoInit, PartParams::default(), FxParams::OFF);
    b.edit_sound(|x| *x = s);
    b.note(57, Some(100));
    let mut t = Tape::default();
    let m = ticks
        .iter()
        .map(|&d| {
            if d != 0 {
                b.edit_sound(|x| x.params.algo.nudge(AlgoParams::MORPH, d));
            }
            b.render(&mut t);
            b.played_morph() / RANGE
        })
        .collect();
    (m, t)
}

/// The largest block-to-block move, fractions of the range a millisecond.
fn max_per_ms(m: &[f32]) -> f32 {
    m.windows(2)
        .map(|w| (w[1] - w[0]).abs() / MS_PER_BLOCK)
        .fold(0.0, f32::max)
}

/// A fast detent asks for 10 ticks (10/127 of the range) every 30 ms; the
/// glide may run at most 2× the hand's own rate. A step is a whole detent in
/// one block, ~23× it.
#[test]
fn a_fast_morph_turn_glides() {
    let (m, _) = turn(algo(AlgoId::T1, AlgoId::A1, 0), &fast_turn());
    let worst = max_per_ms(&m);
    eprintln!(
        "MORPH fast: worst {worst:.5}/ms ({:.4} a block)",
        worst * MS_PER_BLOCK
    );
    assert!(
        m.iter().cloned().fold(0.0, f32::max) > 0.5,
        "the turn moved nothing"
    );
    assert!(worst <= 2.0 * 10.0 / RANGE / FAST_DETENT_MS, "{worst}/ms");
}

/// One tick alone (1/127) must spread so no block moves more than 1/127
/// over a few blocks: a slow turn has no step anywhere.
#[test]
fn a_slow_morph_turn_never_steps() {
    let (m, _) = turn(algo(AlgoId::T1, AlgoId::A1, 0), &slow_turn());
    let worst = max_per_ms(&m);
    eprintln!(
        "MORPH slow: worst {worst:.5}/ms ({:.5} a block)",
        worst * MS_PER_BLOCK
    );
    assert!(*m.last().unwrap() > 0.03, "the turn moved nothing");
    assert!(worst * MS_PER_BLOCK <= 0.25 / RANGE, "{worst}/ms");
}

/// A Sound loaded over a sounding voice lands on its MORPH, the next block:
/// no 90 ms sweep from the old Sound's (the voice still rings).
#[test]
fn a_load_under_a_sounding_voice_lands() {
    let mut r = Rig::new([
        algo(AlgoId::T1, AlgoId::A1, 120),
        algo(AlgoId::T1, AlgoId::A1, 0),
    ]);
    r.note(0, true);
    for _ in 0..20 {
        r.morph_block();
    }
    r.note(0, false);
    r.load(0);
    let want = f32::from(r.sound(0).params.algo.morph);
    assert_ne!(want, 120.0);
    assert_eq!(r.morph_block(), [want]);
}

/// A turn made while a Part is silent is where its next note starts.
#[test]
fn a_turn_on_a_silent_part_lands_on_the_next_note() {
    let mut r = Rig::new([
        algo(AlgoId::T1, AlgoId::A1, 0),
        algo(AlgoId::T1, AlgoId::A1, 0),
    ]);
    r.render();
    r.sound(0).params.algo.morph = 90;
    r.render();
    r.note(0, true);
    assert_eq!(r.morph_block(), [90.0]);
}

/// A stored MORPH steady for a while is played bit for bit, at both ends
/// and past them (a raw write of 255 clamps to B).
#[test]
fn a_settled_morph_is_the_stored_one() {
    for m in [0u8, 1, 64, 126, 127, 255] {
        let mut r = Rig::new([
            algo(AlgoId::T1, AlgoId::A1, 0),
            algo(AlgoId::T1, AlgoId::A1, 0),
        ]);
        r.note(0, true);
        r.morph_block();
        r.sound(0).params.algo.morph = m;
        let last = (0..400).map(|_| r.morph_block()[0]).last().unwrap();
        assert_eq!(last, f32::from(m), "{m}");
    }
}
