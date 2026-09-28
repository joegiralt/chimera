//! Semantic addresses (spec §2) and `ParamSnapshot::block(_mut)`.

use chimera_core::addr::{BlockRef, Blocks, Op, OpOutOfRange, ParamAddr};
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::modulator::EnvSlot;
use chimera_core::params::{
    DriveParams, EnvParams, FilterParams, FolderParams, OutParams, ParamSnapshot, PitchParams,
};
use chimera_core::preset::Performance;

#[test]
fn op_rejects_out_of_range() {
    for (i, op) in Op::ALL.iter().enumerate() {
        assert_eq!(Op::try_from(i as u8), Ok(*op));
        assert_eq!(op.index(), i);
    }
    assert_eq!(Op::try_from(6), Err(OpOutOfRange(6)));
    assert_eq!(Op::try_from(255), Err(OpOutOfRange(255)));
}

#[test]
fn op_nudge_clamps() {
    assert_eq!(Op::A.nudged(-1), Op::A);
    assert_eq!(Op::A.nudged(2), Op::C);
    assert_eq!(Op::C.nudged(127), Op::F);
    assert_eq!(Op::F.nudged(-128), Op::A);
}

#[test]
fn block_ref_all_has_no_duplicates() {
    for (i, b) in BlockRef::ALL.iter().enumerate() {
        assert!(!BlockRef::ALL[..i].contains(b), "{b:?} listed twice");
    }
}

/// `block()` hands out the instance whose spec table `BlockRef::specs`
/// names; a Part view resolves every address but THEME and the channel
/// overview (the UI holds them), a Sound all but the FX and the Part's own
/// mix settings.
#[test]
fn block_and_specs_agree() {
    let mut perf = Performance::new();
    let part = perf.edit(0);
    for b in BlockRef::ALL {
        if matches!(b, BlockRef::Theme | BlockRef::Channels) {
            assert!(part.block(b).is_none());
            assert!(ParamSnapshot::default().block(b).is_none());
            continue;
        }
        let blk = part.block(b).expect("a Part resolves every block");
        assert!(core::ptr::eq(blk.specs(), b.specs()), "{b:?}");
        let not_in_sound = matches!(
            b,
            BlockRef::Chorus
                | BlockRef::Delay
                | BlockRef::Reverb
                | BlockRef::Tape
                | BlockRef::Comp
                | BlockRef::Part
        );
        assert_eq!(
            ParamSnapshot::default().block(b).is_some(),
            !not_in_sound,
            "{b:?}"
        );
    }
}

#[test]
fn block_mut_reaches_the_named_instance() {
    let mut p = ParamSnapshot::default();
    p.block_mut(BlockRef::AlgoOp(Op::C))
        .unwrap()
        .set(AlgoOpParams::LEVEL, 42.0);
    assert_eq!(p.algo.ops[2].level, 42);
    p.block_mut(BlockRef::Env(EnvSlot::Env2))
        .unwrap()
        .set(EnvParams::ATTACK, 0.5);
    assert_eq!(p.envelopes[1].attack, 0.5);
    assert_eq!(p.envelopes[0].attack, 0.189);
    p.block_mut(BlockRef::Out)
        .unwrap()
        .set(OutParams::VOLUME, 0.25);
    assert_eq!(p.out.volume, 0.25);
    assert_eq!(p.block(BlockRef::Out).unwrap().get(OutParams::VOLUME), 0.25);
}

/// Spec §4: exactly these are modulatable (read by `Voice` per block).
#[test]
fn modulatable_addresses_are_exactly_the_spec_list() {
    let mut want = vec![
        ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE),
        ParamAddr::new(BlockRef::Drive, DriveParams::TONE),
        ParamAddr::new(BlockRef::Drive, DriveParams::MIX),
        ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF),
        ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE),
        ParamAddr::new(BlockRef::Filter, FilterParams::DRIVE),
        ParamAddr::new(BlockRef::Folder, FolderParams::FOLD),
        ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY),
        ParamAddr::new(BlockRef::Folder, FolderParams::MIX),
        ParamAddr::new(BlockRef::Out, OutParams::VOLUME),
        ParamAddr::new(BlockRef::Out, OutParams::VCA),
        ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH),
        ParamAddr::new(BlockRef::Pitch, PitchParams::FINE),
    ];
    want.push(ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH));
    for op in Op::ALL {
        want.push(ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::LEVEL));
    }
    for s in EnvSlot::ALL {
        for id in [
            EnvParams::LEVEL,
            EnvParams::TIME,
            EnvParams::RISE,
            EnvParams::FALL,
            EnvParams::SHAPE,
        ] {
            want.push(ParamAddr::new(BlockRef::Env(s), id));
        }
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
    let a = ParamAddr::new(BlockRef::Algo, chimera_core::block::ParamId(99));
    assert!(a.spec().is_none());
    assert!(!a.modulatable());
}
