//! Legacy (`PageId`) pages — System, Demo — after moving onto
//! `Block` specs and `ParamAddr` bindings. Parity tests pin today's steps.

use chimera_core::addr::{BlockRef, Blocks, Op, ParamAddr};
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::params::{EnvParams, FilterParams, ParamSnapshot};
use chimera_core::ui::page::PageId;

#[test]
fn demo_pages_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::DemoWaves.apply_encoder(1, -2, &mut p);
    assert_eq!(p.drive.tone, 0.5 - 2.0 * (1.0 / 128.0));
    PageId::DemoWaves.apply_encoder(3, -1, &mut p);
    assert_eq!(p.folder.symmetry, 0.5 - 1.0 / 128.0);
    PageId::DemoShapes.apply_encoder(4, 3, &mut p);
    assert_eq!(p.filter.resonance, 3.0 / 128.0);
    PageId::DemoMotion.apply_encoder(4, 1, &mut p);
    assert_eq!(p.envelopes[1].attack, 0.01 + (10.0 - 0.001) / 128.0);
    PageId::DemoFm.apply_encoder(3, 2, &mut p);
    assert_eq!(p.algo.ops[2].feedback, 2);
    PageId::EnvAux.apply_encoder(3, -128, &mut p);
    assert_eq!(p.envelopes[2].release, 0.001);
}

/// A Sound carries no FX blocks any more.
#[test]
fn a_sound_has_no_fx_blocks() {
    let p = ParamSnapshot::default();
    for b in [
        BlockRef::Chorus,
        BlockRef::Delay,
        BlockRef::Reverb,
        BlockRef::Tape,
        BlockRef::Comp,
    ] {
        assert!(p.block(b).is_none(), "{b:?}");
    }
}

#[test]
fn legacy_bindings_name_semantic_addresses() {
    // Spec §5: Demo pages address envelopes[1] via FilterEnv.
    assert_eq!(
        PageId::DemoMotion.binding(4),
        Some(ParamAddr::new(BlockRef::FilterEnv, EnvParams::ATTACK))
    );
    assert_eq!(
        PageId::DemoFm.binding(1),
        Some(ParamAddr::new(
            BlockRef::AlgoOp(Op::A),
            AlgoOpParams::FEEDBACK
        ))
    );
    assert_eq!(
        PageId::DemoShapes.binding(1),
        Some(ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF))
    );
    assert_eq!(PageId::DemoMatrix.binding(0), None);
    // Spec §5: System has its own page with no editable params.
    for def in chimera_core::ui::block_registry::SYSTEM_CHAIN.blocks {
        for i in 0..6 {
            assert_eq!(PageId::System(def.def.id).binding(i), None);
        }
    }
}

/// Every bound slot of every legacy page resolves to a spec.
#[test]
fn every_legacy_binding_has_a_spec() {
    let pages = [
        PageId::EnvAmp,
        PageId::EnvFilter,
        PageId::EnvAux,
        PageId::DemoWaves,
        PageId::DemoShapes,
        PageId::DemoMotion,
        PageId::DemoFm,
        PageId::DemoMatrix,
    ];
    for page in pages {
        for i in 0..6 {
            if let Some(a) = page.binding(i) {
                assert!(a.spec().is_some(), "{page:?} slot {i}: {a:?} has no spec");
            }
        }
    }
}
