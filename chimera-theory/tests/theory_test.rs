use chimera_theory::{
    Chord, Degree, Interval, Key, Note, PitchClass, Quality, Scale, SnapTo, Stack, TABLE_BYTES,
    TIE, Tie, snap,
};

fn n(x: u8) -> Note {
    Note::new(x).unwrap()
}

fn name(c: Chord) -> String {
    format!("{c}")
}

#[test]
fn note_is_checked() {
    assert!(Note::new(127).is_some());
    assert!(Note::new(128).is_none());
    assert_eq!(n(69).pc(), PitchClass::A);
    assert_eq!(n(60).pc(), PitchClass::C);
}

#[test]
fn pitch_class_arithmetic_wraps() {
    assert_eq!(PitchClass::A + Interval::MINOR_THIRD, PitchClass::C);
    assert_eq!(PitchClass::B + Interval::FIFTH, PitchClass::Fs);
    assert_eq!(PitchClass::C - PitchClass::A, Interval::MINOR_THIRD);
    assert_eq!(PitchClass::A - PitchClass::C, Interval::MAJOR_SIXTH);
    for p in PitchClass::ALL {
        for i in Interval::ALL {
            assert_eq!((p + i) - p, i);
        }
    }
}

#[test]
fn note_names_c4_is_60() {
    assert_eq!(format!("{}", n(60)), "C4");
    assert_eq!(format!("{}", n(57)), "A3");
    assert_eq!(format!("{}", n(69)), "A4");
    assert_eq!(format!("{}", n(61)), "C#4");
    assert_eq!(format!("{}", n(0)), "C-1");
    assert_eq!(format!("{}", n(127)), "G9");
}

#[test]
fn scales() {
    assert_eq!(Scale::MAJOR.degree_count(), 7);
    assert_eq!(Scale::PENTATONIC_MAJOR.degree_count(), 5);
    assert_eq!(Scale::PENTATONIC_MINOR.degree_count(), 5);
    for s in Scale::ALL {
        assert!(s.contains(Interval::UNISON));
    }
    assert!(Scale::MAJOR.contains(Interval::MAJOR_THIRD));
    assert!(!Scale::MAJOR.contains(Interval::MINOR_THIRD));
    assert!(Scale::DORIAN.contains(Interval::MAJOR_SIXTH));
    assert!(Scale::MIXOLYDIAN.contains(Interval::MINOR_SEVENTH));
    assert!(Scale::HARMONIC_MINOR.contains(Interval::MAJOR_SEVENTH));
    let a_minor = Key::new(PitchClass::A, Scale::MINOR);
    let c_major = Key::new(PitchClass::C, Scale::MAJOR);
    for p in PitchClass::ALL {
        assert_eq!(a_minor.contains(p), c_major.contains(p), "{p}");
    }
}

#[test]
fn major_diatonic_triads_and_sevenths() {
    let c = Key::new(PitchClass::C, Scale::MAJOR);
    let triads: Vec<_> = Degree::ALL
        .iter()
        .map(|&d| name(c.chord(d, Stack::Triad)))
        .collect();
    assert_eq!(triads, ["C", "Dm", "Em", "F", "G", "Am", "Bdim"]);
    let sevenths: Vec<_> = Degree::ALL
        .iter()
        .map(|&d| name(c.chord(d, Stack::Seventh)))
        .collect();
    assert_eq!(
        sevenths,
        ["Cmaj7", "Dm7", "Em7", "Fmaj7", "G7", "Am7", "Bm7b5"]
    );
}

#[test]
fn harmonic_minor_degrees() {
    let a = Key::new(PitchClass::A, Scale::HARMONIC_MINOR);
    assert_eq!(name(a.chord(Degree::I, Stack::Seventh)), "AmMaj7");
    assert_eq!(name(a.chord(Degree::III, Stack::Triad)), "Caug");
    assert_eq!(name(a.chord(Degree::V, Stack::Seventh)), "E7");
    assert_eq!(name(a.chord(Degree::VII, Stack::Seventh)), "G#dim7");
}

#[test]
fn heptatonic_degrees_name_exact_chords() {
    for s in Scale::ALL.into_iter().filter(|s| s.degree_count() == 7) {
        for d in Degree::ALL {
            for st in [Stack::Triad, Stack::Seventh] {
                let c = Key::new(PitchClass::D, s).chord(d, st);
                let want = if st == Stack::Triad { 3 } else { 4 };
                assert_eq!(c.quality().tones().len(), want, "{s:?} {d:?}");
            }
        }
    }
}

#[test]
fn pentatonic_stacks_every_other_note() {
    // A C D E G, every other note: I = A D G, II = C E A, III = D G C.
    // Quartal stacks take the nearest quality.
    let a = Key::new(PitchClass::A, Scale::PENTATONIC_MINOR);
    assert_eq!(name(a.chord(Degree::I, Stack::Triad)), "A7sus4");
    assert_eq!(name(a.chord(Degree::II, Stack::Triad)), "C6");
    assert_eq!(name(a.chord(Degree::III, Stack::Triad)), "D7sus4");
    assert_eq!(
        a.chord(Degree::VI, Stack::Triad),
        a.chord(Degree::I, Stack::Triad),
        "degrees wrap in a 5-note set"
    );
}

#[test]
fn custom_pentatonic_sized_set() {
    let s = Scale::custom(0b10_1001_0101);
    assert_eq!(s, Scale::PENTATONIC_MAJOR);
    let c = Key::new(PitchClass::C, s);
    assert_eq!(c.degree_count(), 5);
    assert_eq!(
        c.chord(Degree::VI, Stack::Triad),
        c.chord(Degree::I, Stack::Triad)
    );
    assert_eq!(
        c.chord(Degree::VII, Stack::Seventh),
        c.chord(Degree::II, Stack::Seventh)
    );
}

#[test]
fn custom_forces_the_root_and_drops_high_bits() {
    let s = Scale::custom(0xF000 | (1 << 7));
    assert!(s.contains(Interval::UNISON));
    assert_eq!(s.degree_count(), 2);
    assert_eq!(Scale::custom(0).degree_count(), 1);
}

#[test]
fn custom_two_note_set() {
    // Root and fifth on C: every degree lands on C or G.
    let c = Key::new(PitchClass::C, Scale::custom(1 << 7));
    assert_eq!(c.degree_count(), 2);
    assert_eq!(
        c.chord(Degree::I, Stack::Triad),
        Chord::new(PitchClass::C, Quality::Maj)
    );
    assert_eq!(
        c.chord(Degree::II, Stack::Triad),
        Chord::new(PitchClass::G, Quality::Maj)
    );
    assert_eq!(c.chord(Degree::III, Stack::Seventh).root(), PitchClass::C);
    assert_eq!(c.chord(Degree::IV, Stack::Seventh).root(), PitchClass::G);
}

#[test]
fn custom_chromatic_set() {
    let c = Key::new(PitchClass::C, Scale::custom(0xFFF));
    assert_eq!(c.degree_count(), 12);
    for d in Degree::ALL {
        assert_eq!(c.chord(d, Stack::Triad).root(), PitchClass::ALL[d as usize]);
    }
    for x in 0..=127 {
        assert_eq!(snap(n(x), SnapTo::Scale(c)), n(x));
    }
}

#[test]
fn snap_to_a_custom_set() {
    // D and A only.
    let da = SnapTo::Scale(Key::new(PitchClass::D, Scale::custom(1 << 7)));
    assert_eq!(snap(n(60), da), n(62), "C: D 2 up, A 3 down");
    assert_eq!(snap(n(64), da), n(62), "E: D 2 down, A 5 up");
    assert_eq!(snap(n(66), da), n(69), "F#: A 3 up, D 4 down");
    assert_eq!(snap(n(69), da), n(69));
    // One-note set: always within a tritone, ties up.
    let d = SnapTo::Scale(Key::new(PitchClass::D, Scale::custom(0)));
    assert_eq!(snap(n(68), d), n(62), "a tritone either way: down");
    assert_eq!(snap(n(69), d), n(74));
}

#[test]
fn numerals_follow_quality() {
    let a = Key::new(PitchClass::A, Scale::MINOR);
    assert_eq!(
        format!("{}", a.degree_label(Degree::IV, Stack::Triad)),
        "iv"
    );
    assert_eq!(
        format!("{}", a.degree_label(Degree::III, Stack::Triad)),
        "III"
    );
    assert_eq!(
        format!("{}", a.degree_label(Degree::II, Stack::Triad)),
        "iio"
    );
    let c = Key::new(PitchClass::C, Scale::MAJOR);
    assert_eq!(
        format!("{}", c.degree_label(Degree::V, Stack::Seventh)),
        "V7"
    );
    assert_eq!(
        format!("{}", c.degree_label(Degree::VII, Stack::Seventh)),
        "vii7b5"
    );
    let h = Key::new(PitchClass::A, Scale::HARMONIC_MINOR);
    assert_eq!(
        format!("{}", h.degree_label(Degree::III, Stack::Triad)),
        "III+"
    );
}

#[test]
fn chord_tones_are_derived() {
    let e7 = Chord::new(PitchClass::E, Quality::Dom7);
    assert_eq!(name(e7), "E7");
    let tones: Vec<_> = e7.tones().collect();
    assert_eq!(
        tones,
        [PitchClass::E, PitchClass::Gs, PitchClass::B, PitchClass::D]
    );
    for q in Quality::ALL {
        let t = q.tones().len();
        assert!((3..=4).contains(&t), "{q:?}");
    }
}

#[test]
fn played_chords_map_to_the_nearest_quality() {
    let e7 = Chord::new(PitchClass::E, Quality::Dom7);
    // Any voicing over an E bass: E B D G#.
    assert_eq!(Chord::from_notes(&[n(52), n(59), n(62), n(68)]), Some(e7));
    // The root is the lowest note, as played.
    assert_eq!(
        Chord::from_notes(&[n(56), n(59), n(62), n(64)]).map(|c| c.root()),
        Some(PitchClass::Gs)
    );
    assert_eq!(
        Chord::from_notes(&[n(62), n(67), n(69)]),
        Some(Chord::new(PitchClass::D, Quality::Sus4))
    );
    // Octave doublings collapse.
    assert_eq!(
        Chord::from_notes(&[n(48), n(60), n(64), n(67), n(72)]),
        Some(Chord::new(PitchClass::C, Quality::Maj))
    );
    // Two classes: a fifth reads as a major triad on the bass.
    assert_eq!(
        Chord::from_notes(&[n(57), n(64)]),
        Some(Chord::new(PitchClass::A, Quality::Maj))
    );
    // Under two classes: nothing.
    assert_eq!(Chord::from_notes(&[]), None);
    assert_eq!(Chord::from_notes(&[n(60)]), None);
    assert_eq!(Chord::from_notes(&[n(48), n(60), n(72)]), None);
    // A cluster still gives a chord on its bass.
    let c = Chord::from_notes(&[n(60), n(61), n(62)]).unwrap();
    assert_eq!(c.root(), PitchClass::C);
}

#[test]
fn scale_always_holds_its_root() {
    assert_eq!(Scale::custom(0).degree_count(), 1);
    let m = Scale::MAJOR;
    assert_eq!(m.toggle(Interval::UNISON), m, "the root can't be cleared");
    for i in &Interval::ALL[1..] {
        let t = m.toggle(*i);
        assert_ne!(t.contains(*i), m.contains(*i));
        assert_eq!(t.toggle(*i), m);
    }
}

#[test]
fn snap_ties_go_down() {
    assert_eq!(TIE, Tie::Down);
    // D against {C, E}: C.
    let c = SnapTo::Chord(Chord::new(PitchClass::C, Quality::Maj));
    assert_eq!(snap(n(62), c), n(60));
    assert_eq!(snap(n(74), c), n(72));
}

#[test]
fn snap_to_scale() {
    let c = SnapTo::Scale(Key::new(PitchClass::C, Scale::MAJOR));
    assert_eq!(snap(n(60), c), n(60));
    assert_eq!(snap(n(64), c), n(64));
    assert_eq!(snap(n(61), c), n(60), "C# ties between C and D");
    assert_eq!(snap(n(66), c), n(65), "F# ties between F and G");
}

#[test]
fn snap_to_chord() {
    let am = SnapTo::Chord(Chord::new(PitchClass::A, Quality::Min));
    assert_eq!(snap(n(60), am), n(60));
    assert_eq!(snap(n(62), am), n(60), "D: C 2 down, E 2 up");
    assert_eq!(snap(n(65), am), n(64));
    assert_eq!(snap(n(66), am), n(64), "F#: 2 to E, 3 to A");
    assert_eq!(snap(n(67), am), n(69));
}

#[test]
fn snap_stays_in_midi_range() {
    // B dim: B D F. 127 (G9): F9 is 2 down, B9 is out of range.
    let bdim = SnapTo::Chord(Chord::new(PitchClass::B, Quality::Dim));
    assert_eq!(snap(n(127), bdim), n(125));
    // 0 (C-1): B is 1 down, off the bottom; D is 2 up.
    assert_eq!(snap(n(0), bdim), n(2));
    // A tie at the bottom has no down: in D major, 0 (C) is 1 from B
    // (-1, out) and 1 from C# (1).
    let d = SnapTo::Scale(Key::new(PitchClass::D, Scale::MAJOR));
    assert_eq!(snap(n(0), d), n(1));
    assert_eq!(snap(n(12), d), n(11), "same tie with room below goes down");
}

/// The rule written out plainly: nearest in-range note in the set, the
/// lower on a tie.
fn brute(x: u8, to: SnapTo) -> u8 {
    (0..=127u8)
        .filter(|&y| to.contains(n(y).pc()))
        .min_by_key(|&y| ((y as i16 - x as i16).abs(), y))
        .unwrap()
}

#[test]
fn snap_is_pinned_to_brute_force() {
    for x in 0..=127 {
        for p in PitchClass::ALL {
            let mut sets: Vec<SnapTo> = Scale::ALL
                .iter()
                .map(|&s| SnapTo::Scale(Key::new(p, s)))
                .collect();
            sets.extend(
                Quality::ALL
                    .iter()
                    .map(|&q| SnapTo::Chord(Chord::new(p, q))),
            );
            for mask in [0, 1 << 7, 0b10_1001_0101, 0xFFF, 0b1111_0000_0000] {
                sets.push(SnapTo::Scale(Key::new(p, Scale::custom(mask))));
            }
            for to in sets {
                assert_eq!(snap(n(x), to).get(), brute(x, to), "{x} {to:?}");
            }
        }
    }
}

#[test]
fn tables_stay_small() {
    const { assert!(TABLE_BYTES <= 512) };
    println!("theory tables: {TABLE_BYTES} B");
}
