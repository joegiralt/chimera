use chimera_core::addr::ParamAddr;
use chimera_core::mod_path::{legacy_to_addr, ModDestRegistry, ParamPath, RegistryError};
use chimera_core::preset::ChainType;

const PIZZA: ChainType = ChainType::PizzaPoly;

#[test]
fn registry_starts_empty() {
    let reg = ModDestRegistry::new();
    assert_eq!(reg.len(), 0);
    assert!(reg.is_empty());
}

#[test]
fn registry_add_and_find() {
    let mut reg = ModDestRegistry::new();
    let path = ParamPath::Block { block: 1, param: 0 };
    assert_eq!(reg.add(PIZZA, path, *b"DrvDrv\0\0"), Ok(()));
    assert_eq!(reg.len(), 1);
    assert!(reg.is_primed(path));
    assert_eq!(reg.find(path), Some(0));
}

#[test]
fn registry_no_duplicates() {
    let mut reg = ModDestRegistry::new();
    let path = ParamPath::FmOp { op: 0, param: 2 };
    assert_eq!(reg.add(ChainType::Fm, path, *b"O1 Lvl\0\0"), Ok(()));
    assert_eq!(reg.add(ChainType::Fm, path, *b"O1 Lvl\0\0"), Ok(()));
    assert_eq!(reg.len(), 1);
}

#[test]
fn registry_remove() {
    let mut reg = ModDestRegistry::new();
    let p1 = ParamPath::FmOp { op: 0, param: 2 };
    let p2 = ParamPath::FmOp { op: 1, param: 2 };
    reg.add(ChainType::Fm, p1, *b"O1 Lvl\0\0").unwrap();
    reg.add(ChainType::Fm, p2, *b"O2 Lvl\0\0").unwrap();
    assert_eq!(reg.len(), 2);
    reg.remove(p1);
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_primed(p1));
    assert!(reg.is_primed(p2));
}

#[test]
fn registry_fm_op_paths_are_distinct() {
    let p0 = ParamPath::FmOp { op: 0, param: 2 };
    let p1 = ParamPath::FmOp { op: 1, param: 2 };
    assert_ne!(p0, p1);
    let mut reg = ModDestRegistry::new();
    reg.add(ChainType::Fm, p0, *b"O1 Lvl\0\0").unwrap();
    assert!(reg.is_primed(p0));
    assert!(!reg.is_primed(p1));
}

/// Spec § Testing "Registry": adding a non-modulatable address is refused.
#[test]
fn registry_refuses_non_modulatable() {
    let mut reg = ModDestRegistry::new();
    let refused = [
        (ChainType::Modal, ParamPath::Block { block: 0, param: 1 }), // Modal EXCITE (note-on only)
        (ChainType::Fm, ParamPath::Block { block: 0, param: 0 }),    // FM ALG (Enum)
        (ChainType::Fm, ParamPath::FmOp { op: 0, param: 1 }),        // op waveform (Enum)
        (ChainType::Fm, ParamPath::FmEnv { op: 0, param: 0 }),       // op AR (note-on only)
        (PIZZA, ParamPath::Block { block: 2, param: 3 }),            // filter FM amount (never read)
        (PIZZA, ParamPath::Block { block: 9, param: 0 }),            // no such node
        (PIZZA, ParamPath::FmOp { op: 7, param: 2 }),                // no such operator
    ];
    for (chain, path) in refused {
        assert_eq!(reg.add(chain, path, *b"X\0\0\0\0\0\0\0"), Err(RegistryError::NotModulatable), "{path:?}");
    }
    assert!(reg.is_empty());
}

/// Every path the UI can emit is accepted exactly when its address is modulatable.
#[test]
fn registry_accepts_exactly_the_modulatable_paths() {
    for chain in ChainType::ALL {
        let mut reg = ModDestRegistry::new();
        let mut accepted = 0;
        let blocks = (0..6u8).flat_map(|block| (0..6u8).map(move |param| ParamPath::Block { block, param }));
        let ops = (0..4u8).flat_map(|op| (0..6u8).map(move |param| ParamPath::FmOp { op, param }));
        for path in blocks.chain(ops) {
            let ok = legacy_to_addr(chain, path).is_some_and(ParamAddr::modulatable);
            assert_eq!(reg.add(chain, path, *b"X\0\0\0\0\0\0\0").is_ok(), ok, "{chain:?} {path:?}");
            accepted += ok as usize;
        }
        assert_eq!(reg.len(), accepted);
    }
}
