//! The typed binding (ADR 0217, spec §§ 4.2, 6.1-6.2).
#![cfg(feature = "midi-map")]

use chimera_core::midi_map::{Cc14Msb, CcKind, Feedback, Input, NrpnKind, default_for};
use chimera_core::params::EngineType;
use chimera_midi::ctl::{Ctl, FreeCc, Sel};
use chimera_midi::take::Takeover;
use chimera_midi::value::{U7, U14};

fn cc_ctl(n: u8) -> Ctl {
    Ctl::Cc {
        cc: FreeCc::new(n).unwrap(),
        value: U7::new(5).unwrap(),
    }
}

#[test]
fn default_map_is_jump_without_echo() {
    for e in [
        EngineType::Algo,
        EngineType::Modal,
        EngineType::Va,
        EngineType::Tape,
    ] {
        for b in default_for(e).entries() {
            assert!(
                matches!(b.input, Input::Cc(_, CcKind::Absolute(Takeover::Jump))),
                "{e:?}"
            );
            assert_eq!(b.feedback, Feedback::Off, "{e:?}");
        }
    }
}

#[test]
fn input_kinds_never_offer_incdec_on_cc() {
    let cc = Input::cc(74).unwrap();
    let all: Vec<Input> = cc.kinds().collect();
    assert_eq!(all.len(), 9, "3 takeovers, 4 relatives, 2 buttons");
    assert!(all.contains(&cc));
    assert!(
        all.iter()
            .all(|k| matches!(k, Input::Cc(c, _) if c.get() == 74))
    );
    let n = Input::Nrpn(U14::new(3).unwrap(), NrpnKind::Absolute(Takeover::Jump));
    let nr: Vec<Input> = n.kinds().collect();
    assert!(
        nr.iter()
            .any(|k| matches!(k, Input::Nrpn(_, NrpnKind::IncDec)))
    );
    assert_eq!(nr.len(), 10);
    assert!(
        nr.iter()
            .all(|k| matches!(k, Input::Nrpn(a, _) if a.get() == 3))
    );
}

#[test]
fn pressure_only_absolute() {
    let p = Input::Pressure(Takeover::Jump);
    let all: Vec<Input> = p.kinds().collect();
    assert_eq!(all.len(), 3);
    assert!(all.iter().all(|k| matches!(k, Input::Pressure(_))));
    let m = Input::Cc14(Cc14Msb::new(2).unwrap(), Takeover::Scale);
    let all: Vec<Input> = m.kinds().collect();
    assert_eq!(all.len(), 3);
    assert!(
        all.iter()
            .all(|k| matches!(k, Input::Cc14(c, _) if c.get() == 2))
    );
}

#[test]
fn cc14_msb_refuses_0_6_and_32_up() {
    for n in 0..=255u8 {
        let ok = matches!(n, 1..=5 | 7..=31);
        assert_eq!(Cc14Msb::new(n).is_some(), ok, "{n}");
    }
}

#[test]
fn heard_matches_cc14_lsb_half() {
    let i = Input::Cc14(Cc14Msb::new(2).unwrap(), Takeover::Jump);
    assert!(i.heard(&cc_ctl(2)));
    assert!(i.heard(&cc_ctl(34)));
    assert!(!i.heard(&cc_ctl(3)));
    assert!(!i.heard(&cc_ctl(35)));
    let c = Input::cc(2).unwrap();
    assert!(c.heard(&cc_ctl(2)) && !c.heard(&cc_ctl(34)));
    let n = U14::new(9).unwrap();
    let nr = Input::Nrpn(n, NrpnKind::IncDec);
    assert!(nr.heard(&Ctl::Data {
        sel: Sel::Nrpn(n),
        value: chimera_midi::ctl::DataValue::Coarse(U7::ZERO)
    }));
    assert!(!nr.heard(&Ctl::Data {
        sel: Sel::Rpn(n),
        value: chimera_midi::ctl::DataValue::Coarse(U7::ZERO)
    }));
    assert!(Input::Pressure(Takeover::Jump).heard(&Ctl::Pressure(U7::ZERO)));
}
