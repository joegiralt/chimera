use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    fs::copy("memory.x", out_dir.join("memory.x")).unwrap();

    fs::write(
        out_dir.join("ram_d2.x"),
        r#"
/* The DMA rings alone in the first 4 KB of D2: the one MPU region that is
   non-cacheable (ADR 0020). The voice pool follows. */
SECTIONS {
    .ram_d2_dma (NOLOAD) : ALIGN(4096) {
        __sram_d2_dma = .;
        *(.ram_d2.dma .ram_d2.dma.*);
        . = __sram_d2_dma + 4096;
        __eram_d2_dma = .;
    } > RAM_D2
    .ram_d2 (NOLOAD) : ALIGN(32) {
        *(.ram_d2.voices .ram_d2.voices.*);
        *(.ram_d2 .ram_d2.*);
        . = ALIGN(4);
    } > RAM_D2
}
INSERT AFTER .uninit;
ASSERT(__sram_d2_dma == ORIGIN(RAM_D2), "the DMA rings must start D2, where the MPU region is");
ASSERT(__eram_d2_dma - __sram_d2_dma == 4096, "the DMA rings must fill exactly their 4 KB MPU region");
"#,
    )
    .unwrap();

    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rustc-link-arg=-Tram_d2.x");
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
}
