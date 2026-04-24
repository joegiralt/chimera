use chimera_core::mod_path::{ModDestRegistry, ParamPath};

#[test]
fn registry_starts_empty() {
    let reg = ModDestRegistry::new();
    assert_eq!(reg.count, 0);
}

#[test]
fn registry_add_and_find() {
    let mut reg = ModDestRegistry::new();
    let path = ParamPath::Block { block: 1, param: 0 };
    reg.add(path, *b"DrvDrv\0\0");
    assert_eq!(reg.count, 1);
    assert!(reg.is_primed(path));
    assert_eq!(reg.find(path), Some(0));
}

#[test]
fn registry_no_duplicates() {
    let mut reg = ModDestRegistry::new();
    let path = ParamPath::FmOp { op: 0, param: 2 };
    reg.add(path, *b"O1 Lvl\0\0");
    reg.add(path, *b"O1 Lvl\0\0");
    assert_eq!(reg.count, 1);
}

#[test]
fn registry_remove() {
    let mut reg = ModDestRegistry::new();
    let p1 = ParamPath::FmOp { op: 0, param: 2 };
    let p2 = ParamPath::FmOp { op: 1, param: 2 };
    reg.add(p1, *b"O1 Lvl\0\0");
    reg.add(p2, *b"O2 Lvl\0\0");
    assert_eq!(reg.count, 2);
    reg.remove(p1);
    assert_eq!(reg.count, 1);
    assert!(!reg.is_primed(p1));
    assert!(reg.is_primed(p2));
}

#[test]
fn registry_fm_op_paths_are_distinct() {
    let p0 = ParamPath::FmOp { op: 0, param: 2 };
    let p1 = ParamPath::FmOp { op: 1, param: 2 };
    assert_ne!(p0, p1);
    let mut reg = ModDestRegistry::new();
    reg.add(p0, *b"O1 Lvl\0\0");
    assert!(reg.is_primed(p0));
    assert!(!reg.is_primed(p1));
}

#[test]
fn registry_max_capacity() {
    let mut reg = ModDestRegistry::new();
    for i in 0..32u8 {
        reg.add(ParamPath::Block { block: i, param: 0 }, *b"Test\0\0\0\0");
    }
    assert_eq!(reg.count, 32);
    // 33rd should be ignored
    reg.add(ParamPath::Block { block: 32, param: 0 }, *b"Over\0\0\0\0");
    assert_eq!(reg.count, 32);
}
