//! ORBIT's chord ring: key, segments over the loop, harmony per segment.

use chimera_core::orbit::chords::{Bars, ChordRing, ScaleKind, Segs, harmony, segment_at};
use chimera_core::orbit::clock::Phase;
use chimera_theory::{Chord, Degree, PitchClass, Quality, Scale};

fn bars(n: u8) -> Bars {
    Bars::new(n).unwrap()
}

fn segs(n: u8) -> Segs {
    Segs::new(n).unwrap()
}

#[test]
fn scale_kind_names_the_theory_scales() {
    let x = Scale::custom(&[]);
    for i in 0..10 {
        assert_eq!(ScaleKind::ALL[i].scale(x), Scale::ALL[i]);
    }
    assert_eq!(ScaleKind::Custom.scale(x), x);
}

#[test]
fn segments_split_the_loop_evenly() {
    let half = Phase::new(1 << 31);
    let (b, s) = (bars(2), segs(4));
    assert_eq!(segment_at(0, Phase::ZERO, b, s), 0);
    assert_eq!(segment_at(0, half, b, s), 1);
    assert_eq!(segment_at(1, Phase::ZERO, b, s), 2);
    assert_eq!(segment_at(1, half, b, s), 3);
    assert_eq!(segment_at(1, Phase::new(u32::MAX), b, s), 3);

    let (b, s) = (bars(1), segs(3));
    for k in 1..3u64 {
        let edge = (k << 32).div_ceil(3) as u32;
        assert_eq!(segment_at(0, Phase::new(edge - 1), b, s), k as usize - 1);
        assert_eq!(segment_at(0, Phase::new(edge), b, s), k as usize);
    }
}

#[test]
fn harmony_takes_the_played_chord_else_the_triad() {
    let mut c = ChordRing::NEUTRAL;
    c.tonic = PitchClass::A;
    c.scale = ScaleKind::Minor;
    c.segs = segs(4);
    c.degrees[..4].copy_from_slice(&[Degree::I, Degree::IV, Degree::V, Degree::I]);
    let dm = Chord::new(PitchClass::D, Quality::Min);
    assert_eq!(harmony(&c, 1).chord, dm);
    assert_eq!(harmony(&c, 1).key, c.key());

    let am_c = Chord::new(PitchClass::A, Quality::Min)
        .over(PitchClass::C)
        .unwrap();
    c.played[1] = Some(am_c);
    assert_eq!(harmony(&c, 1).chord, am_c);
    assert_eq!(
        harmony(&c, 0).chord,
        Chord::new(PitchClass::A, Quality::Min)
    );
}

#[test]
fn a_degree_past_a_small_set_wraps() {
    let mut c = ChordRing::NEUTRAL;
    c.tonic = PitchClass::A;
    c.scale = ScaleKind::PentatonicMinor;
    c.degrees[0] = Degree::VI;
    let h = harmony(&c, 0);
    assert_eq!(h.chord, h.key.triad(Degree::I));
}
