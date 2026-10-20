//! The voice FX block (ADR 0155): its frozen codes, PLACE's follow rule,
//! its defaults and its `Block` round trip.

use chimera_core::addr::BlockRef;
use chimera_core::block::{Block, DiskCode};
use chimera_core::dsp::voice_fx::params::{FxType, Place, VoiceFxParams, pot_level};
use chimera_core::params::ParamSnapshot;

#[test]
fn voice_fx_block_code_and_ident_are_frozen() {
    assert_eq!(BlockRef::VoiceFx.disk_code(), Some(36));
    assert_eq!(BlockRef::VoiceFx.disk_ident(), Some("VOICEFX"));
}

#[test]
fn fx_type_and_place_codes_are_frozen() {
    assert_eq!(FxType::LIST.map(|t| t.disk_code()), [0, 1, 2, 3]);
    assert_eq!(FxType::Fuzz.disk_ident(), "FUZZ");
    assert_eq!([Place::Pre, Place::Post].map(|p| p.disk_code()), [0, 1]);
    assert_eq!(FxType::from_disk_code(4), None); // DELAY's, not built
}

#[test]
fn picking_a_type_from_off_takes_its_place_later_ones_keep_yours() {
    let mut p = VoiceFxParams::default();
    p.set_type(FxType::Fold);
    assert_eq!(p.place, Place::Post);
    p.place = Place::Pre;
    p.set_type(FxType::Soft);
    assert_eq!(p.place, Place::Pre);
    // OFF onto OFF is no departure: PLACE stays as set.
    p.set_type(FxType::Off);
    p.place = Place::Post;
    p.set_type(FxType::Off);
    assert_eq!(p.place, Place::Post);
}

#[test]
fn defaults_are_off_pre_and_unity_level() {
    let p = ParamSnapshot::default().fx;
    assert_eq!(
        (p.ty(), p.place, p.pots, p.mix),
        (FxType::Off, Place::Pre, [0.0, 0.5, 0.5], 1.0)
    );
    assert_eq!(pot_level(0.5), 1.0);
}

#[test]
fn the_type_list_and_names_agree() {
    assert_eq!(
        FxType::LIST,
        [FxType::Off, FxType::Soft, FxType::Fuzz, FxType::Fold]
    );
    assert_eq!(FxType::NAMES, ["OFF", "SOFT", "FUZZ", "FOLD"]);
    for (i, t) in FxType::LIST.into_iter().enumerate() {
        assert_eq!(FxType::from_index(i as f32), t);
    }
    assert_eq!(FxType::from_index(-3.0), FxType::Off);
    assert_eq!(FxType::from_index(99.0), FxType::Fold);
    assert_eq!(FxType::Soft.default_place(), Place::Pre);
    assert_eq!(FxType::Fuzz.default_place(), Place::Pre);
    assert_eq!(FxType::Fold.default_place(), Place::Post);
    assert_eq!(FxType::Off.default_place(), Place::Pre);
}

#[test]
fn every_fx_param_round_trips_through_block() {
    let mut p = VoiceFxParams::default();
    for (id, v) in [
        (VoiceFxParams::P1, 0.25),
        (VoiceFxParams::P2, 0.75),
        (VoiceFxParams::P3, 0.125),
        (VoiceFxParams::MIX, 0.375),
    ] {
        p.set(id, v);
        assert_eq!(p.get(id), v);
    }
    assert_eq!((p.pots, p.mix), ([0.25, 0.75, 0.125], 0.375));

    for t in FxType::LIST {
        assert!(p.set_enum_code(VoiceFxParams::TYPE, t.disk_code()));
        assert_eq!(p.ty(), t);
        assert_eq!(p.enum_code(VoiceFxParams::TYPE), Some(t.disk_code()));
        assert_eq!(p.enum_ident(VoiceFxParams::TYPE), Some(t.disk_ident()));
    }
    assert!(!p.set_enum_code(VoiceFxParams::TYPE, 4));
    for pl in [Place::Pre, Place::Post] {
        assert!(p.set_enum_code(VoiceFxParams::PLACE, pl.disk_code()));
        assert_eq!(p.place, pl);
        assert_eq!(p.enum_code(VoiceFxParams::PLACE), Some(pl.disk_code()));
        assert_eq!(p.enum_ident(VoiceFxParams::PLACE), Some(pl.disk_ident()));
    }
    assert!(!p.set_enum_code(VoiceFxParams::PLACE, 2));

    // A load sets TYPE as stored: a sparse record leaves out a PLACE equal
    // to its base's, so FOLD PRE must not come back FOLD POST.
    let mut r = VoiceFxParams::default();
    assert!(r.set_enum_code(VoiceFxParams::TYPE, FxType::Fold.disk_code()));
    assert_eq!((r.ty(), r.place), (FxType::Fold, Place::Pre));

    // TYPE and PLACE through their indexes, as a knob turn sets them.
    let mut q = VoiceFxParams::default();
    q.set(VoiceFxParams::TYPE, 3.0);
    assert_eq!((q.ty(), q.place), (FxType::Fold, Place::Post));
    assert_eq!(q.get(VoiceFxParams::TYPE), 3.0);
    q.set(VoiceFxParams::TYPE, 2.0);
    assert_eq!((q.ty(), q.get(VoiceFxParams::TYPE)), (FxType::Fuzz, 2.0));
    q.set(VoiceFxParams::PLACE, 0.0);
    assert_eq!((q.place, q.get(VoiceFxParams::PLACE)), (Place::Pre, 0.0));
}
