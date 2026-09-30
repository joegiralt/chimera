use chimera_theory::{
    Chord, Degree, Interval, Key, Note, PcSet, PitchClass, Quality, Scale, SnapTo, Stack,
    TABLE_BYTES, TIE, Tie, snap,
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
                assert_eq!(c.tones().len(), want, "{s:?} {d:?}");
                let l = c.label();
                assert!(l.is_exact(), "{s:?} {d:?}");
                assert_eq!(l.root(), c.root(), "{s:?} {d:?}");
                assert_eq!(l.bass(), c.root(), "{s:?} {d:?}");
            }
        }
    }
}

#[test]
fn pentatonic_stacks_keep_exact_tones() {
    // A C D E G, every other note: I = A D G, II = C E A, III = D G C.
    use PitchClass::*;
    let a = Key::new(A, Scale::PENTATONIC_MINOR);
    let i = a.chord(Degree::I, Stack::Triad);
    assert_eq!(i.root(), A);
    assert_eq!(i.tones(), PcSet::of(&[A, D, G]));
    assert_eq!(i.tones().iter().collect::<Vec<_>>(), [D, G, A]);
    for x in 0..=127 {
        let pc = snap(n(x), SnapTo::Chord(i)).pc();
        assert!([A, D, G].contains(&pc), "{x}: {pc}");
    }
    assert_eq!(name(i), "Dsus4/A");
    assert_eq!(name(a.chord(Degree::II, Stack::Triad)), "Am/C");
    assert_eq!(name(a.chord(Degree::III, Stack::Triad)), "Gsus4/D");
    assert_eq!(
        format!("{}", a.degree_label(Degree::I, Stack::Triad)),
        "I?",
        "no quality on the degree's own note"
    );
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
    // Every other note of two wraps onto itself: one-tone stacks.
    let c = Key::new(PitchClass::C, Scale::custom(1 << 7));
    assert_eq!(c.degree_count(), 2);
    assert_eq!(
        c.chord(Degree::I, Stack::Triad).tones(),
        PcSet::of(&[PitchClass::C])
    );
    assert_eq!(
        c.chord(Degree::II, Stack::Triad).tones(),
        PcSet::of(&[PitchClass::G])
    );
    assert!(!c.chord(Degree::I, Stack::Triad).label().is_exact());
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
fn a_quality_chord_holds_its_tones() {
    let e7 = Chord::new(PitchClass::E, Quality::Dom7);
    assert_eq!(name(e7), "E7");
    let tones: Vec<_> = e7.tones().iter().collect();
    assert_eq!(
        tones,
        [PitchClass::D, PitchClass::E, PitchClass::Gs, PitchClass::B]
    );
    for q in Quality::ALL {
        let t = q.tones().len();
        assert!((3..=4).contains(&t), "{q:?}");
    }
}

#[test]
fn played_chords_keep_their_tones() {
    use PitchClass::*;
    let e7 = Chord::new(E, Quality::Dom7);
    // Any voicing over an E bass: E B D G#.
    assert_eq!(Chord::from_notes(&[n(52), n(59), n(62), n(68)]), Some(e7));
    // The lowest note is the bass; the label finds the root.
    let over_gs = Chord::from_notes(&[n(56), n(59), n(62), n(64)]).unwrap();
    assert_eq!(over_gs.root(), Gs);
    assert_eq!(over_gs.tones(), e7.tones());
    assert_eq!(name(over_gs), "E7/G#");
    assert_eq!(
        Chord::from_notes(&[n(62), n(67), n(69)]),
        Some(Chord::new(D, Quality::Sus4))
    );
    // Octave doublings collapse.
    assert_eq!(
        Chord::from_notes(&[n(48), n(60), n(64), n(67), n(72)]),
        Some(Chord::new(C, Quality::Maj))
    );
    // Two classes stay two: a bare fifth has no exact name.
    let fifth = Chord::from_notes(&[n(57), n(64)]).unwrap();
    assert_eq!(fifth.tones(), PcSet::of(&[A, E]));
    assert!(!fifth.label().is_exact());
    // Under two classes: nothing.
    assert_eq!(Chord::from_notes(&[]), None);
    assert_eq!(Chord::from_notes(&[n(60)]), None);
    assert_eq!(Chord::from_notes(&[n(48), n(60), n(72)]), None);
}

#[test]
fn c_e_a_reads_a_minor_over_c_and_snaps_to_its_own_tones() {
    use PitchClass::*;
    let c = Chord::from_notes(&[n(60), n(64), n(69)]).unwrap();
    assert_eq!(c.root(), C);
    assert_eq!(c.tones(), PcSet::of(&[C, E, A]));
    let l = c.label();
    assert!(l.is_exact());
    assert_eq!((l.root(), l.quality(), l.bass()), (A, Quality::Min, C));
    assert_eq!(name(c), "Am/C");
    for x in 0..=127 {
        let pc = snap(n(x), SnapTo::Chord(c)).pc();
        assert!([C, E, A].contains(&pc), "{x}: {pc}");
    }
    // G is 3 from E and 2 from A; a C6 would have kept it.
    assert_eq!(snap(n(67), SnapTo::Chord(c)), n(69));
}

#[test]
fn every_quality_names_itself_in_every_inversion() {
    for q in Quality::ALL {
        let bare = format!("{}", Chord::new(PitchClass::C, q));
        let suffix = &bare[1..];
        for r in PitchClass::ALL {
            let chord = Chord::new(r, q);
            for t in chord.tones().iter() {
                let inv = chord.over(t).unwrap();
                assert_eq!(inv.tones(), chord.tones());
                let want = if t == r {
                    format!("{r}{suffix}")
                } else {
                    format!("{r}{suffix}/{t}")
                };
                assert_eq!(name(inv), want, "{q:?} on {r} over {t}");
                assert!(inv.label().is_exact());
            }
        }
    }
    let c = Chord::new(PitchClass::C, Quality::Maj);
    assert_eq!(c.over(PitchClass::D), None, "the bass is a chord tone");
}

#[test]
fn played_inversions_name_the_quality() {
    // Played, only the notes are known: sets two qualities share (C6 and
    // Am7, Csus2 and Gsus4, aug, dim7) read as the first match from the
    // bass. Every other quality reads as itself over its bass.
    let shared = |set: PcSet| {
        Quality::ALL
            .iter()
            .flat_map(|&q| PitchClass::ALL.map(|r| Chord::new(r, q)))
            .filter(|c| c.tones() == set)
            .count()
            > 1
    };
    for q in Quality::ALL {
        let bare = format!("{}", Chord::new(PitchClass::C, q));
        let suffix = &bare[1..];
        for r in PitchClass::ALL {
            let tones: Vec<_> = Chord::new(r, q).tones().iter().collect();
            for &bass in &tones {
                let notes: Vec<Note> = core::iter::once(n(36 + bass as u8))
                    .chain(tones.iter().map(|&p| n(48 + p as u8)))
                    .collect();
                let c = Chord::from_notes(&notes).unwrap();
                let l = c.label();
                assert!(l.is_exact(), "{q:?} on {r} over {bass}");
                assert_eq!(l.bass(), bass);
                if !shared(c.tones()) {
                    let want = if bass == r {
                        format!("{r}{suffix}")
                    } else {
                        format!("{r}{suffix}/{bass}")
                    };
                    assert_eq!(name(c), want);
                }
            }
        }
    }
}

#[test]
fn a_cluster_gets_the_nearest_name_marked() {
    let c = Chord::from_notes(&[n(60), n(61), n(62)]).unwrap();
    assert_eq!(c.root(), PitchClass::C);
    assert_eq!(
        c.tones(),
        PcSet::of(&[PitchClass::C, PitchClass::Cs, PitchClass::D])
    );
    let l = c.label();
    assert!(!l.is_exact());
    assert!(name(c).ends_with('?'), "{}", name(c));
    // Snapping still uses the three played tones.
    for x in 0..=127 {
        assert!(c.contains(snap(n(x), SnapTo::Chord(c)).pc()));
    }
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
                let key = Key::new(p, Scale::custom(mask));
                sets.push(SnapTo::Scale(key));
                sets.push(SnapTo::Chord(key.chord(Degree::II, Stack::Seventh)));
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
