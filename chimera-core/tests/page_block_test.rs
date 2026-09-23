//! Legacy (`PageId`) pages — Mixer, System, Demo — after moving onto
//! `Block` specs and `ParamAddr` bindings. Parity tests pin today's steps.

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::params::{EnvParams, FilterParams, FmOpParams, OutParams, ParamSnapshot};
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
    assert_eq!(p.fm.operators[2].feedback, 2.0);
    PageId::EnvAux.apply_encoder(3, -128, &mut p);
    assert_eq!(p.envelopes[2].release, 0.001);
}

#[test]
fn out_encoders_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Mixer.apply_encoder(0, -8, &mut p);
    assert_eq!(p.out.volume, 0.8 - 8.0 / 128.0);
    PageId::Master.apply_encoder(1, 1, &mut p);
    assert_eq!(p.out.pan, 2.0 / 128.0);
}

/// The mixer page keeps its placeholder bars for unbound slots.
#[test]
fn mixer_read_values_keep_placeholders() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Mixer.read_values(&p), [0.8, 0.5, 0.5, 0.0, 0.5, 0.0]);
}

#[test]
fn fx_encoders_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Delay.apply_encoder(0, 2, &mut p);
    assert_eq!(p.delay.time_ms, 375.0 + 2.0 * 8.0);
    PageId::Chorus.apply_encoder(0, 5, &mut p);
    assert_eq!(p.chorus.mode, 3);
    PageId::MixReverb.apply_encoder(0, 5, &mut p);
    assert_eq!(p.reverb.reverb_type, 2);
    PageId::MixReverb.apply_encoder(4, -1, &mut p);
    assert_eq!(p.reverb.mix, 0.0);
    PageId::MixReverb.apply_encoder(4, 1, &mut p); // Efx MIX slot, +1 off the floor
    assert_eq!(p.reverb.mix, 1.0 / 128.0);
    PageId::Delay.snap_encoder(5, 1, &mut p);
    assert_eq!(p.delay.mix, 100.0 / 127.0);
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
        Some(ParamAddr::new(BlockRef::FmOp(Op::A), FmOpParams::FEEDBACK))
    );
    assert_eq!(PageId::DemoShapes.binding(1), Some(ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF)));
    assert_eq!(PageId::Master.binding(0), Some(ParamAddr::new(BlockRef::Out, OutParams::VOLUME)));
    assert_eq!(PageId::Mixer.binding(2), None);
    assert_eq!(PageId::DemoMatrix.binding(0), None);
    // Spec §5: System has its own page with no editable params.
    for i in 0..6 {
        assert_eq!(PageId::System.binding(i), None);
    }
}

/// Every bound slot of every legacy page resolves to a spec.
#[test]
fn every_legacy_binding_has_a_spec() {
    let pages = [
        PageId::Mixer, PageId::Chorus, PageId::Delay, PageId::MixReverb, PageId::Master,
        PageId::EnvAmp, PageId::EnvFilter, PageId::EnvAux, PageId::DemoWaves, PageId::DemoShapes,
        PageId::DemoMotion, PageId::DemoFm, PageId::DemoMatrix, PageId::System,
    ];
    for page in pages {
        for i in 0..6 {
            if let Some(a) = page.binding(i) {
                assert!(a.spec().is_some(), "{page:?} slot {i}: {a:?} has no spec");
            }
        }
    }
}
