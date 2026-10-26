use chimera_engine::midi_in::Cc;
use chimera_midi::ctl::FreeCc;

#[test]
fn free_cc_matches_todays_reserved_set() {
    for n in 0..=119u8 {
        assert_eq!(
            FreeCc::new(n).is_some(),
            !Cc::new(n).unwrap().is_reserved(),
            "cc {n}"
        );
    }
}
