use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    fs::copy("memory.x", out_dir.join("memory.x")).unwrap();

    // Linker section for RAM_D2: DMA buffers (`.ram_d2`) first so they keep
    // their address at 0x3000_0000, then the voice pool (`.ram_d2.voices`).
    fs::write(
        out_dir.join("ram_d2.x"),
        r#"
SECTIONS {
    .ram_d2 (NOLOAD) : ALIGN(4) {
        *(.ram_d2);
        *(.ram_d2.*);
        . = ALIGN(4);
    } > RAM_D2
}
INSERT AFTER .uninit;
"#,
    )
    .unwrap();

    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rustc-link-arg=-Tram_d2.x");
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
}
