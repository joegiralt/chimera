fn main() {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(out.join("waves.rs"), chimera_waves::emit_rust()).expect("write waves.rs");
    println!("cargo:rerun-if-changed=build.rs");
}
