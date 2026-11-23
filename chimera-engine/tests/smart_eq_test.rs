// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SMART EQ's settings (ADR 0296): ROLE a Part, SMART EQ and AMOUNT on the
//! master, each a typed code; the rest state is zero bytes.

use chimera_engine::block::Block;
use chimera_engine::dsp::comp::CompParams;
use chimera_engine::part::PartParams;
use chimera_engine::part_eq::PartEqs;
use chimera_engine::part_eq::smart::{
    Amount, Band, CutDb, CutView, Role, SmartEq, SmartState, Switch,
};

#[test]
fn roles_rank_front_over_mid_over_back() {
    for r in Role::ALL {
        assert_eq!(Role::from_code(r.code()), Some(r));
        assert_eq!(Role::from_f32(f32::from(r.code())), r);
        assert_eq!(r.depth_over(r).get(), 0.0, "{r:?} never cuts its own");
    }
    assert_eq!(Role::from_code(3), None);
    assert_eq!(Role::Front.depth_over(Role::Back).get(), 1.0);
    assert!(Role::Front.depth_over(Role::Mid).get() > 0.0);
    assert!(Role::Mid.depth_over(Role::Back).get() > 0.0);
    for r in Role::ALL {
        assert_eq!(r.depth_over(Role::Front).get(), 0.0, "FRONT is never cut");
    }
    assert_eq!(Role::default(), Role::Mid);
}

#[test]
fn the_smart_byte_holds_switch_and_amount_and_every_byte_decodes() {
    for b in 0..=255u8 {
        let a = Amount::from_code(b & 0x7f).unwrap();
        for s in Switch::ALL {
            let e = SmartEq::new(s, a);
            assert_eq!((e.switch(), e.amount()), (s, a));
        }
    }
    assert_eq!(SmartEq::default(), SmartEq::OFF);
    assert_eq!(SmartEq::OFF.switch(), Switch::Off);
    assert!(!SmartEq::OFF.listens());
    assert!(!SmartEq::new(Switch::On, Amount::ZERO).listens());
    assert!(SmartEq::new(Switch::On, Amount::DEFAULT).listens());
    assert_eq!(Amount::from_code(128), None);
    assert_eq!(Amount::from_f32(f32::NAN), Amount::ZERO);
    assert_eq!(Amount::from_f32(500.0).code(), Amount::MAX);
}

#[test]
fn settings_ride_the_mix_blocks_without_growing_them() {
    assert_eq!(core::mem::size_of::<PartParams>(), 20);
    assert_eq!(core::mem::size_of::<CompParams>(), 32);
    let mut p = PartParams::for_part(0);
    assert_eq!(p.role, Role::Mid);
    for r in Role::ALL {
        p.write(PartParams::ROLE, f32::from(r.code()));
        assert_eq!((p.role, p.get(PartParams::ROLE)), (r, f32::from(r.code())));
        assert_eq!(p.enum_ident(PartParams::ROLE), Some(r.ident()));
    }
    assert!(p.set_enum_code(PartParams::ROLE, Role::Front.code()));
    assert!(!p.set_enum_code(PartParams::ROLE, 9));
    assert_eq!(p.role, Role::Front);

    let mut c = CompParams::DEFAULT;
    assert_eq!(c.smart, SmartEq::OFF);
    c.write(CompParams::SMART_EQ, 1.0);
    c.write(CompParams::AMOUNT, 100.0);
    assert_eq!(
        c.smart,
        SmartEq::new(Switch::On, Amount::from_code(100).unwrap())
    );
    assert_eq!(c.get(CompParams::AMOUNT), 100.0);
    assert_eq!(c.enum_ident(CompParams::SMART_EQ), Some("ON"));
    assert_eq!(c.sanitised().smart, c.smart);
}

#[test]
fn rest_is_zero_bytes() {
    // ITCM's boot zeroing must leave SMART EQ at rest (ADR 0286).
    let rest = PartEqs::REST;
    // SAFETY: `PartEqs` is plain `f32`s, `u8` enums and arrays of them;
    // reading its bytes as `u8` is defined for any value (padding aside,
    // which a const holds as written here: no padding bytes in these).
    let bytes = unsafe {
        core::slice::from_raw_parts(
            (&rest.smart as *const SmartState).cast::<u8>(),
            core::mem::size_of::<SmartState>(),
        )
    };
    assert!(bytes.iter().all(|&b| b == 0));
    assert!(!rest.any_live());
}

#[test]
fn the_view_packs_four_cuts_to_an_eighth_of_a_db_up_to_the_deepest() {
    let cuts = [CutDb::new(4.0), CutDb::ZERO, CutDb::new(1.5), CutDb::MAX];
    let v = CutView::of(&cuts);
    for b in Band::ALL {
        assert!((v.cut(b).get() - cuts[b.index()].get()).abs() <= 1.0 / 16.0);
    }
    assert_eq!(v.deepest(), (Band::High, CutDb::MAX));
    assert_eq!(CutDb::new(f32::NAN), CutDb::ZERO);
    assert_eq!(CutDb::new(40.0), CutDb::MAX);
    // The deepest cut reads whole: no wrap at the old 7.97 dB.
    assert_eq!(v.cut(Band::High), CutDb::MAX);
    assert_eq!(CutDb::MAX.get(), 27.0);
}
