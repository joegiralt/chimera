use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    fs::copy("memory.x", out_dir.join("memory.x")).unwrap();

    fs::write(
        out_dir.join("ram_d2.x"),
        r#"
/* The DMA rings and the card's staging alone in the first 16 KB of D2: the
   one MPU region that is non-cacheable (ADR 0020). The voice pool follows. */
SECTIONS {
    .ram_d2_dma (NOLOAD) : ALIGN(16384) {
        __sram_d2_dma = .;
        *(.ram_d2.dma .ram_d2.dma.*);
        . = __sram_d2_dma + 16384;
        __eram_d2_dma = .;
    } > RAM_D2
    .ram_d2 (NOLOAD) : ALIGN(32) {
        *(.ram_d2.voices .ram_d2.voices.*);
        *(.ram_d2 .ram_d2.*);
        . = ALIGN(4);
    } > RAM_D2
}
INSERT AFTER .uninit;
ASSERT(__sram_d2_dma == ORIGIN(RAM_D2), "the DMA region must start D2, where the MPU region is");
ASSERT(__eram_d2_dma - __sram_d2_dma == 16384, "the DMA region must fill exactly its 16 KB MPU region");
"#,
    )
    .unwrap();

    fs::write(
        out_dir.join("sram4.x"),
        r#"
/* D3 SRAM4: the stream rings, beside the card's DMA (not in the D2 region). */
SECTIONS {
    .sram4 (NOLOAD) : ALIGN(32) {
        *(.sram4 .sram4.*);
        . = ALIGN(4);
    } > RAM_D3
}
INSERT AFTER .uninit;
ASSERT(SIZEOF(.sram4) <= LENGTH(RAM_D3), "SRAM4 overflows");
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

    // The reverb's engines (ADR 0142): `Reverb::process`, the door's entry,
    // its TYPEs' modules and `run_loop`'s monomorphs, code and rodata (their
    // switch tables follow their names). Only with `bank2`: without it
    // `Reverb::play` calls `process` in bank 1, where the bench's A/B
    // (`bench-stress` against `bench-stress,bank2`) keeps it. TAPE's
    // machine glyphs' raster too (ADR 0173: `tape_glyph::draw`, the door's
    // `render`), which `paint` calls in bank 1 without it.
    let reverb = if env::var_os("CARGO_FEATURE_BANK2").is_some() {
        "    *(.text.*6Reverb7process* .text.*3rev4ramp .text.*3rev4room* .text.*3rev5bloom* .text.*3rev5burst*
      .text.*3rev11loop_engine* .text.*3rev8run_loop*);
    *(.rodata.*6Reverb7process* .rodata.*3rev4ramp .rodata.*3rev4room* .rodata.*3rev5bloom* .rodata.*3rev5burst*
      .rodata.*3rev11loop_engine* .rodata.*3rev8run_loop*);
    *(.text.*14chimera_engine10tape_glyph4draw*);
    *(.rodata.*14chimera_engine10tape_glyph4draw*);
"
    } else {
        ""
    };
    fs::write(
        out_dir.join("bank2.x"),
        r#"
/* Bank 2's image (ADR 0137): the 256-byte header, the door table after it,
   then what the door reaches. Inserted before .text, and read before
   link.x so its statements match first: cortex-m-rt's *(.text .text.*)
   never takes a section named here. Without `bank2` it is empty and the
   Justfile makes no .bk2. */
SECTIONS {
  .bank2 : ALIGN(32) {
    KEEP(*(.bank2.header));
    . = ALIGN(32); __bank2_door = .;
    KEEP(*(.bank2.door));
    *(.bank2.text .bank2.text.*);
    *(.bank2.rodata .bank2.rodata.*);
    /* TAPE's deck (`tape_engine::proxy`), by v0-mangled path: its own
       modules, and every item of `TapeDeck`; its spans and the stream's
       cursor, which only the deck calls. `params` and `machine` stay in
       bank 1, as do `Heads` and the cost table. */
    *(.text.*11tape_engine5heads* .text.*11tape_engine7stretch* .text.*11tape_engine4wave* .text.*11tape_engine4read*
      .text.*11tape_engine3age* .text.*11tape_engine6warble* .text.*11tape_engine7oneshot* .text.*11tape_engine4tron*
      .text.*11tape_engine4span* .text.*6stream6cursor* .text.*11tape_engine*8TapeDeck*);
    *(.rodata.*11tape_engine5heads* .rodata.*11tape_engine7stretch* .rodata.*11tape_engine4wave* .rodata.*11tape_engine4read*
      .rodata.*11tape_engine3age* .rodata.*11tape_engine6warble* .rodata.*11tape_engine7oneshot* .rodata.*11tape_engine4tron*
      .rodata.*11tape_engine4span* .rodata.*6stream6cursor* .rodata.*11tape_engine*8TapeDeck*);
    /* Streamed admission and loads (`AdmitDoor`, `LoadDoor`): the bill,
       the stealing pick and the block's re-bill; the heads' placement, a
       load's passes and their head reads. Bank 1 keeps the wrappers, the
       plain pick and `load_kind`'s whole check. */
    *(.text.*10instrument9admission* .text.*7project8streamed4load* .text.*6stream5stage5place*
      .text.*11source_file16read_source_head* .text.*11source_file16read_packed_into*);
    *(.rodata.*10instrument9admission* .rodata.*7project8streamed4load* .rodata.*6stream5stage5place*
      .rodata.*11source_file16read_source_head* .rodata.*11source_file16read_packed_into*);
    /* REVERB */
    . = ALIGN(32); __bank2_end = .;
  } > BANK2
} INSERT BEFORE .text;
ASSERT(ADDR(.bank2) == 0x08100000, "bank 2 starts at 0x08100000");
ASSERT(__bank2_end <= 0x08180000, "bank 2 stays clear of bootloader 1.09's slots at 0x08180000");
ASSERT(__bank2_end == ADDR(.bank2) || __bank2_door == 0x08100100, "the door follows the 256-byte header");
"#
        .replace("    /* REVERB */\n", reverb),
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
    // bank2.x before cortex-m-rt's link.x: an input section goes to the
    // first output statement that matches it in the order the scripts are
    // read, INSERT or not, so after link.x its *(.text.*) would take TAPE's.
    println!("cargo:rustc-link-arg=-Tbank2.x");
    println!("cargo:rustc-link-arg=-Tlink.x");
    println!("cargo:rustc-link-arg=-Tram_d2.x");
    println!("cargo:rustc-link-arg=-Tsram4.x");
    println!("cargo:rustc-link-arg=-Tdtcm.x");
    println!("cargo:rustc-link-arg=-Taxi_margin.x");
    // The relocations stay in the ELF: the door check reads them.
    println!("cargo:rustc-link-arg=--emit-relocs");
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
}
