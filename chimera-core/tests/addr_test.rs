//! Semantic addresses (spec §2) and `ParamSnapshot::block(_mut)`.

use chimera_core::addr::{BlockRef, Blocks, Op, OpOutOfRange, ParamAddr};
use chimera_core::preset::Performance;
use chimera_core::dsp::pizza::PizzaParams;
use chimera_core::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FolderParams, OutParams, ParamSnapshot};

#[test]
fn op_rejects_out_of_range() {
    for (i, op) in Op::ALL.iter().enumerate() {
        assert_eq!(Op::try_from(i as u8), Ok(*op));
        assert_eq!(op.index(), i);
    }
    assert_eq!(Op::try_from(4), Err(OpOutOfRange(4)));
    assert_eq!(Op::try_from(255), Err(OpOutOfRange(255)));
}

#[test]
fn op_nudge_clamps() {
    assert_eq!(Op::A.nudged(-1), Op::A);
    assert_eq!(Op::A.nudged(2), Op::C);
    assert_eq!(Op::C.nudged(127), Op::D);
    assert_eq!(Op::D.nudged(-128), Op::A);
}

#[test]
fn block_ref_all_has_no_duplicates() {
    for (i, b) in BlockRef::ALL.iter().enumerate() {
        assert!(!BlockRef::ALL[..i].contains(b), "{b:?} listed twice");
    }
}

/// `block()` hands out the instance whose spec table `BlockRef::specs`
/// names; a Part view resolves every address, a Sound all but the FX.
#[test]
fn block_and_specs_agree() {
    let mut perf = Performance::new();
    let part = perf.edit(0);
    for b in BlockRef::ALL {
        let blk = part.block(b).expect("a Part resolves every block");
        assert!(core::ptr::eq(blk.specs(), b.specs()), "{b:?}");
        let fx = matches!(b, BlockRef::Chorus | BlockRef::Delay | BlockRef::Reverb);
        assert_eq!(ParamSnapshot::default().block(b).is_some(), !fx, "{b:?}");
    }
}

#[test]
fn block_mut_reaches_the_named_instance() {
    let mut p = ParamSnapshot::default();
    p.block_mut(BlockRef::FmOp(Op::C)).unwrap().set(FmOpParams::LEVEL, 42.0);
    assert_eq!(p.fm.operators[2].level, 42.0);
    p.block_mut(BlockRef::FilterEnv).unwrap().set(EnvParams::ATTACK, 2.0);
    assert_eq!(p.envelopes[1].attack, 2.0);
    assert_eq!(p.envelopes[0].attack, 0.01);
    p.block_mut(BlockRef::Out).unwrap().set(OutParams::VOLUME, 0.25);
    assert_eq!(p.out.volume, 0.25);
    assert_eq!(p.block(BlockRef::Out).unwrap().get(OutParams::VOLUME), 0.25);
}

/// Spec §4: exactly these are modulatable (read by `Voice` per block).
#[test]
fn modulatable_addresses_are_exactly_the_spec_list() {
    let mut want = vec![
        ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE),
        ParamAddr::new(BlockRef::Pizza, PizzaParams::CRUSH),
        ParamAddr::new(BlockRef::Pizza, PizzaParams::LEVEL),
        ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE),
        ParamAddr::new(BlockRef::Drive, DriveParams::TONE),
        ParamAddr::new(BlockRef::Drive, DriveParams::MIX),
        ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF),
        ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE),
        ParamAddr::new(BlockRef::Filter, FilterParams::DRIVE),
        ParamAddr::new(BlockRef::Folder, FolderParams::FOLD),
        ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY),
        ParamAddr::new(BlockRef::Folder, FolderParams::MIX),
        ParamAddr::new(BlockRef::AmpEnv, EnvParams::ATTACK),
        ParamAddr::new(BlockRef::AmpEnv, EnvParams::DECAY),
        ParamAddr::new(BlockRef::AmpEnv, EnvParams::SUSTAIN),
        ParamAddr::new(BlockRef::AmpEnv, EnvParams::RELEASE),
        ParamAddr::new(BlockRef::Out, OutParams::VOLUME),
    ];
    for op in Op::ALL {
        want.push(ParamAddr::new(BlockRef::FmOp(op), FmOpParams::LEVEL));
        want.push(ParamAddr::new(BlockRef::FmOp(op), FmOpParams::FEEDBACK));
    }
    let got: Vec<ParamAddr> = BlockRef::ALL
        .iter()
        .flat_map(|&b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
        .filter(|a| a.modulatable())
        .collect();
    assert_eq!(got.len(), want.len());
    for a in &want {
        assert!(got.contains(a), "{a:?} should be modulatable");
    }
}

#[test]
fn unknown_param_has_no_spec_and_is_not_modulatable() {
    let a = ParamAddr::new(BlockRef::Pizza, chimera_core::block::ParamId(99));
    assert!(a.spec().is_none());
    assert!(!a.modulatable());
}
