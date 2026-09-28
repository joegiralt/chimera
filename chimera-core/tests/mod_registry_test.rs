use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::ParamId;
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::modal::ModalParams;
use chimera_core::mod_path::{ModDestRegistry, RegistryError};
use chimera_core::params::{DriveParams, EnvParams};

fn op_level(op: Op) -> ParamAddr {
    ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::LEVEL)
}

#[test]
fn registry_starts_empty() {
    let reg = ModDestRegistry::new();
    assert_eq!(reg.len(), 0);
    assert!(reg.is_empty());
}

#[test]
fn registry_add_and_find() {
    let mut reg = ModDestRegistry::new();
    let addr = ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE);
    assert_eq!(reg.add(addr, *b"DrvDrv\0\0"), Ok(()));
    assert_eq!(reg.len(), 1);
    assert!(reg.is_primed(addr));
    assert_eq!(reg.find(addr), Some(0));
}

#[test]
fn registry_no_duplicates() {
    let mut reg = ModDestRegistry::new();
    assert_eq!(reg.add(op_level(Op::A), *b"O1 Lvl\0\0"), Ok(()));
    assert_eq!(reg.add(op_level(Op::A), *b"O1 Lvl\0\0"), Ok(()));
    assert_eq!(reg.len(), 1);
}

#[test]
fn registry_remove() {
    let mut reg = ModDestRegistry::new();
    reg.add(op_level(Op::A), *b"O1 Lvl\0\0").unwrap();
    reg.add(op_level(Op::B), *b"O2 Lvl\0\0").unwrap();
    assert_eq!(reg.len(), 2);
    reg.remove(op_level(Op::A));
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_primed(op_level(Op::A)));
    assert!(reg.is_primed(op_level(Op::B)));
}

#[test]
fn registry_fm_ops_are_distinct() {
    assert_ne!(op_level(Op::A), op_level(Op::B));
    let mut reg = ModDestRegistry::new();
    reg.add(op_level(Op::A), *b"O1 Lvl\0\0").unwrap();
    assert!(reg.is_primed(op_level(Op::A)));
    assert!(!reg.is_primed(op_level(Op::B)));
}

/// Spec § Testing "Registry": adding a non-modulatable address is refused.
#[test]
fn registry_refuses_non_modulatable() {
    let mut reg = ModDestRegistry::new();
    let refused = [
        ParamAddr::new(BlockRef::Modal, ModalParams::EXCITE), // note-on only
        ParamAddr::new(BlockRef::Algo, AlgoParams::ALG_A),    // Enum
        ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::WAVE), // Enum
        ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::AR), // note-on only
        ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::FEEDBACK), // not a destination yet
        ParamAddr::new(BlockRef::AmpEnv, EnvParams::ATTACK),  // off the VCA
        ParamAddr::new(BlockRef::Filter, ParamId(3)),         // retired (FM)
        ParamAddr::new(BlockRef::FilterEnv, EnvParams::ATTACK), // never read (plan D7)
        ParamAddr::new(BlockRef::Algo, ParamId(99)),          // no such param
    ];
    for addr in refused {
        assert_eq!(
            reg.add(addr, *b"X\0\0\0\0\0\0\0"),
            Err(RegistryError::NotModulatable),
            "{addr:?}"
        );
    }
    assert!(reg.is_empty());
}

/// Every address is accepted exactly when it is modulatable.
#[test]
fn registry_accepts_exactly_the_modulatable_addresses() {
    let mut accepted = 0;
    for b in BlockRef::ALL {
        for s in b.specs() {
            let addr = ParamAddr::new(b, s.id);
            // A fresh registry each time: there are more modulatable
            // addresses than the matrix capacity (`MAX_REGISTRY_DESTS`).
            let mut reg = ModDestRegistry::new();
            assert_eq!(
                reg.add(addr, *b"X\0\0\0\0\0\0\0").is_ok(),
                addr.modulatable(),
                "{addr:?}"
            );
            accepted += addr.modulatable() as usize;
        }
    }
    assert_eq!(accepted, 17);
}
