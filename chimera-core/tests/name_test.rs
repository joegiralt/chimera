use chimera_core::name::{Name, NameError, SoundName};

#[test]
fn accepts_the_charset() {
    let n = Name::<16>::new("DUB-042 a").unwrap();
    assert_eq!(n.as_str(), "DUB-042 a");
    assert_eq!(SoundName::new("Z").unwrap().as_str(), "Z");
    assert!(Name::<16>::new("ABCDEFGHIJKLMNOP").is_ok());
}

#[test]
fn rejects() {
    assert_eq!(Name::<16>::new(""), Err(NameError::Empty));
    assert_eq!(
        Name::<16>::new("ABCDEFGHIJKLMNOPQ"),
        Err(NameError::TooLong)
    );
    assert_eq!(Name::<16>::new("(init)"), Err(NameError::BadChar(b'(')));
    assert!(matches!(
        Name::<16>::new("\u{e9}"),
        Err(NameError::BadChar(_))
    ));
    for s in [" A", "A ", "   "] {
        assert_eq!(Name::<16>::new(s), Err(NameError::EdgeSpace), "{s:?}");
    }
}

#[test]
fn padded_round_trip() {
    let n = Name::<16>::new("ACID 303").unwrap();
    let p = n.padded();
    assert_eq!(&p[..8], b"ACID 303");
    assert!(p[8..].iter().all(|&b| b == 0));
    assert_eq!(Name::from_padded(&p), Ok(n));

    let full = Name::<16>::new("ABCDEFGHIJKLMNOP").unwrap();
    assert_eq!(Name::from_padded(&full.padded()), Ok(full));
    assert_eq!(Name::<16>::from_padded(&[0; 16]), Err(NameError::Empty));
}

#[test]
fn from_padded_rejects_a_gap() {
    let mut p = [0u8; 16];
    p[0] = b'A';
    p[2] = b'B';
    assert_eq!(Name::from_padded(&p), Err(NameError::BadChar(0)));

    let mut p = [0u8; 16];
    p[..2].copy_from_slice(b"A ");
    assert_eq!(Name::from_padded(&p), Err(NameError::EdgeSpace));
}
