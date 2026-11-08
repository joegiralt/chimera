use chimera_theory::tuning::{BUILT_IN, KbmError, Keymap, Scale, SclError, offsets};
use proptest::prelude::*;

fn scl(text: &str) -> Scale {
    Scale::parse(text.as_bytes()).unwrap()
}

const TET: &str = "! 12-TET\n\
12-tone equal temperament\n 12\n!\n 100.0\n 200.\n 300.0\n 400.0\n 500.0\n 600.0\n\
 700.0\n 800.0\n 900.0\n 1000.0\n 1100.0\n 2/1\n";

#[test]
fn twelve_tet_is_identity() {
    let t = offsets(&scl(TET), None);
    assert!(t.iter().all(|&o| o == 0.0), "{t:?}");
}

#[test]
fn ratios_and_cents_read_as_scala_says() {
    let s = scl("!\nJust fifth\n2\n3/2\n2\n");
    assert_eq!(s.len(), 2);
    assert!((s.cents(1) - 701.955).abs() < 1e-3);
    assert_eq!(s.cents(2), 1200.0);
    // Two steps an octave from middle C: 60 C, 61 G, 62 C'.
    let t = offsets(&s, None);
    assert_eq!(t[60], 0.0);
    assert!((t[61] - (67.01955 - 61.0)).abs() < 1e-4);
    assert!((t[62] - (72.0 - 62.0)).abs() < 1e-4);
    assert!((t[59] - (67.01955 - 12.0 - 59.0)).abs() < 1e-4);
}

#[test]
fn comments_blank_description_and_trailing_text() {
    let s = scl("! a\n! b\n\n 3 ! steps\n100.0 cents\n! mid\n5/4 major third\n1200.0\n");
    assert_eq!(s.len(), 3);
    assert!((s.cents(2) - 386.3137).abs() < 1e-3);
}

#[test]
fn malformed_scales_are_refused() {
    for (text, e) in [
        ("", SclError::NoCount),
        ("desc\n", SclError::NoCount),
        ("desc\nx\n", SclError::BadCount),
        ("desc\n0\n", SclError::BadCount),
        ("desc\n129\n", SclError::BadCount),
        ("desc\n2\n100.0\n", SclError::TooFew),
        ("desc\n1\nabc\n", SclError::BadPitch),
        ("desc\n1\n3/0\n", SclError::BadPitch),
        ("desc\n1\n-3/2\n", SclError::BadPitch),
        ("desc\n1\n0/2\n", SclError::BadPitch),
        ("desc\n1\n0.0\n", SclError::BadPeriod),
        ("desc\n1\n-1200.0\n", SclError::BadPeriod),
        ("desc\n1\n1e400.\n", SclError::BadPitch),
    ] {
        assert_eq!(Scale::parse(text.as_bytes()).err(), Some(e), "{text:?}");
    }
}

#[test]
fn every_built_in_parses() {
    for (name, text) in ["JUST", "PYTHAG", "MEANTONE", "WERCK"].iter().zip(BUILT_IN) {
        let s = Scale::parse(text).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(s.len(), 12, "{name}");
        assert!((s.cents(12) - 1200.0).abs() < 1e-9, "{name}");
        let t = offsets(&s, None);
        // C is C in every one; no step strays 30 cents.
        assert_eq!(t[60], 0.0, "{name}");
        assert!(t.iter().all(|o| o.abs() < 0.3), "{name}");
    }
}

/// No map plays `Keymap::LINEAR`, bit for bit: `None` keeps no image of it.
#[test]
fn no_map_is_linear() {
    let mut scales: Vec<Scale> = BUILT_IN.iter().map(|t| Scale::parse(t).unwrap()).collect();
    scales.push(scl(TET));
    for s in &scales {
        let a = offsets(s, None).map(f32::to_bits);
        let b = offsets(s, Some(&Keymap::LINEAR)).map(f32::to_bits);
        assert_eq!(a, b);
    }
}

const KBM_A440: &str = "! map\n12\n0\n127\n60\n69\n440.0\n12\n\
! the keys\n0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n";

#[test]
fn a_full_keymap_on_twelve_tet_is_identity() {
    let k = Keymap::parse(KBM_A440.as_bytes()).unwrap();
    let t = offsets(&scl(TET), Some(&k));
    assert!(t.iter().all(|o| o.abs() < 1e-5), "{t:?}");
}

#[test]
fn reference_pitch_moves_every_key() {
    let k = Keymap::parse(b"0\n0\n127\n60\n69\n432.0\n0\n").unwrap();
    let t = offsets(&scl(TET), Some(&k));
    let st = 12.0 * libm::log2(432.0f64 / 440.0) as f32;
    // Key 0 stays on the MIDI range.
    assert!(t[1..].iter().all(|o| (o - st).abs() < 1e-4), "{t:?}");
    assert_eq!(t[0], 0.0);
}

#[test]
fn unmapped_and_out_of_range_keys_stay_twelve_tet() {
    // Seven white keys of a 7-step scale; the black keys unmapped.
    let k =
        Keymap::parse(b"12\n48\n84\n60\n60\n261.6255653\n7\n0\nx\n1\nx\n2\n3\nx\n4\nx\n5\nx\n6\n")
            .unwrap();
    let s = scl("seven\n7\n200.\n400.\n500.\n700.\n900.\n1100.\n1200.\n");
    let t = offsets(&s, Some(&k));
    for key in [61, 63, 66, 68, 70, 47, 85, 0, 127] {
        assert_eq!(t[key], 0.0, "{key}");
    }
    for key in [60, 62, 64, 65, 67, 69, 71, 72, 48, 84] {
        assert!(t[key].abs() < 1e-4, "{key}: {}", t[key]);
    }
}

#[test]
fn malformed_keymaps_are_refused() {
    for (text, e) in [
        ("", KbmError::Short),
        ("12\n0\n127\n60\n69\n440.0\n12\n0\n", KbmError::Short),
        ("200\n0\n127\n60\n69\n440.0\n12\n", KbmError::BadValue),
        ("0\n0\n127\n60\n69\n-440.0\n0\n", KbmError::BadValue),
        ("0\n0\n127\n60\n200\n440.0\n0\n", KbmError::BadValue),
        ("1\n0\n127\n60\n69\n440.0\n1\ny\n", KbmError::BadValue),
    ] {
        assert_eq!(Keymap::parse(text.as_bytes()).err(), Some(e), "{text:?}");
    }
}

proptest! {
    /// Any bytes: refused or a table of finite offsets that keep every key
    /// on the MIDI range.
    #[test]
    fn any_bytes_are_safe(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        if let Ok(s) = Scale::parse(&bytes) {
            for (k, o) in offsets(&s, None).iter().enumerate() {
                prop_assert!(o.is_finite());
                prop_assert!((0.0..=127.0).contains(&(k as f32 + o)));
            }
        }
        let _ = Keymap::parse(&bytes);
    }

    /// Scale-shaped text with any pitches and any keymap: never panics,
    /// always in range.
    #[test]
    fn any_scale_and_map_stay_in_range(
        pitches in proptest::collection::vec(-5000.0f64..5000.0, 1..20),
        middle in 0u8..128,
        reference in 0u8..128,
        hz in 1.0f64..20000.0,
        map in proptest::collection::vec(proptest::option::of(0u8..40), 0..20),
    ) {
        let mut text = format!("p\n{}\n", pitches.len());
        for c in &pitches {
            text += &format!("{c:.3}\n");
        }
        let Ok(s) = Scale::parse(text.as_bytes()) else { return Ok(()) };
        let mut kbm = format!("{}\n0\n127\n{middle}\n{reference}\n{hz}\n{}\n", map.len(), pitches.len());
        for m in &map {
            kbm += &m.map_or("x".into(), |d| d.to_string());
            kbm += "\n";
        }
        let k = Keymap::parse(kbm.as_bytes()).unwrap();
        for (key, o) in offsets(&s, Some(&k)).iter().enumerate() {
            prop_assert!(o.is_finite());
            prop_assert!((0.0..=127.0).contains(&(key as f32 + o)));
        }
    }
}
