//! Spec § Principles: `f32` only in the render path. The chip has no
//! double-precision FPU, so `f64` becomes software float (#26), and libm's
//! `f32` trig calls soft-double internally.

#[test]
fn the_algo_module_uses_no_f64_and_no_libm() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/dsp/algo");
    let mut files = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let src = std::fs::read_to_string(&path).unwrap();
        for bad in ["f64", "libm"] {
            assert!(!src.contains(bad), "{} mentions {bad}", path.display());
        }
        files += 1;
    }
    assert!(files >= 4, "{files} files scanned");
}
