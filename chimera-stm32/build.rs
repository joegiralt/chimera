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

    fs::write(
        out_dir.join("dtcm.x"),
        r#"
/* The zero-wait working copy of the wave tables, filled from flash at boot
   (the flash tables stay the source), at the top of DTCM (ADR 0025). The
   stack is the rest, growing down toward ORIGIN(DTCM), so an overflow
   faults below it instead of overwriting the tables. */
SECTIONS {
    .dtcm_waves (ORIGIN(DTCM) + LENGTH(DTCM) - SIZEOF(.dtcm_waves)) (NOLOAD) : ALIGN(8) {
        __sdtcm_waves = .;
        *(.dtcm_waves .dtcm_waves.*);
        . = ALIGN(8);
        __edtcm_waves = .;
    } > DTCM
}
INSERT AFTER .uninit;
ASSERT(__edtcm_waves == ORIGIN(DTCM) + LENGTH(DTCM), "the wave copy must end DTCM, above the stack");
ASSERT(_stack_start == __sdtcm_waves && _stack_end == ORIGIN(DTCM), "the stack must be the rest of DTCM, below the wave copy");
ASSERT(_stack_start - _stack_end >= 32K, "DTCM must keep at least 32 KB of stack beside the wave copy");
"#,
    )
    .unwrap();

    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rustc-link-arg=-Tram_d2.x");
    println!("cargo:rustc-link-arg=-Tdtcm.x");
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
}
