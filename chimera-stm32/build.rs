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

    fs::write(
        out_dir.join("bank2.x"),
        r#"
/* Bank 2's image (ADR 0137): the 256-byte header, the door table after it,
   then what the door reaches. Inserted before .text so cortex-m-rt's
   *(.text .text.*) never takes a .bank2.* section. Without `bank2` it is
   empty and the Justfile makes no .bk2. */
SECTIONS {
  .bank2 : ALIGN(32) {
    KEEP(*(.bank2.header));
    . = ALIGN(32); __bank2_door = .;
    KEEP(*(.bank2.door));
    *(.bank2.text .bank2.text.*);
    *(.bank2.rodata .bank2.rodata.*);
    . = ALIGN(32); __bank2_end = .;
  } > BANK2
} INSERT BEFORE .text;
ASSERT(ADDR(.bank2) == 0x08100000, "bank 2 starts at 0x08100000");
ASSERT(__bank2_end <= 0x08180000, "bank 2 stays clear of bootloader 1.09's slots at 0x08180000");
ASSERT(__bank2_end == ADDR(.bank2) || __bank2_door == 0x08100100, "the door follows the 256-byte header");
"#,
    )
    .unwrap();

    // The AXI margin: what the UI's stack-free state and the card may still
    // grow into. Shipping keeps 32 KiB; a bench build carries the probe's
    // extra state and keeps 8 KiB (ADR 0087).
    let margin = if env::var_os("CARGO_FEATURE_BENCH").is_some() {
        "8K"
    } else {
        "32K"
    };
    fs::write(
        out_dir.join("axi_margin.x"),
        format!(
            "AXI_MARGIN = {margin};\nAXI_END = ORIGIN(RAM) + LENGTH(RAM);\nASSERT(AXI_END - __sheap >= AXI_MARGIN, \"AXI: under the margin (ADR 0087)\");\n"
        ),
    )
    .unwrap();

    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rustc-link-arg=-Tram_d2.x");
    println!("cargo:rustc-link-arg=-Tdtcm.x");
    println!("cargo:rustc-link-arg=-Taxi_margin.x");
    println!("cargo:rustc-link-arg=-Tbank2.x");
    // The relocations stay in the ELF: the door check reads them.
    println!("cargo:rustc-link-arg=--emit-relocs");
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
}
