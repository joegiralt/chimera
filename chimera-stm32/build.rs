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
    // switch tables follow their names). TAPE's machine glyphs'
    // raster too (ADR 0173: `tape_glyph::draw`, the door's `render`).
    let reverb = "    *(.text.*6Reverb7process* .text.*3rev4ramp .text.*3rev4room* .text.*3rev5bloom* .text.*3rev5burst*
      .text.*3rev11loop_engine* .text.*3rev8run_loop*);
    *(.rodata.*6Reverb7process* .rodata.*3rev4ramp .rodata.*3rev4room* .rodata.*3rev5bloom* .rodata.*3rev5burst*
      .rodata.*3rev11loop_engine* .rodata.*3rev8run_loop*);
    *(.text.*14chimera_engine10tape_glyph4draw*);
    *(.rodata.*14chimera_engine10tape_glyph4draw*);
    /* The card interrupt's streaming work (ADR 0175, `card_dma::CardDoor`):
       `IrqState`'s steps, the reads' bursts, the stops and the drains, the
       serve's pick. Bank 1 keeps the handler's shim, `closed` and what it
       needs (`Held::op`/`run`, `Flying::ground`, `give`,
       `CardSide::requeue`), and `fill_done`, which the bench's TAPE rig
       calls too. */
    *(.text.*13chimera_board8card_dma*8IrqState* .text.*13chimera_board8card_dma*4Held3end*
      .text.*13chimera_board8card_dma*4Held3fly* .text.*13chimera_board8card_dma*4Held4xfer*
      .text.*13chimera_board8card_dma*4Held5ready* .text.*13chimera_board8card_dma*4Held6polled*
      .text.*13chimera_board8card_dma*4Held4stop* .text.*13chimera_board8card_dma*4Held7command*
      .text.*13chimera_board8card_dma*6Flying4land* .text.*13chimera_board8card_dma6failed*
      .text.*13chimera_board8card_dma6wanted* .text.*13chimera_board8card_dma7serving*
      .text.*13chimera_board8card_dma8watch_in* .text.*6stream5serve*
      .text.*5Rings5wants* .text.*6stream3edf*);
    *(.rodata.*13chimera_board8card_dma*8IrqState* .rodata.*6stream5serve*);
    /* TAPE's UI (ADR 0175, `chimera_core::bank2::UiDoor`): the strip's
       raster and the SOURCE cell's card work and turns, SCRUB's jog
       (`Jog::pass`, ADR 0205) and PIT's tuner (ADR 0201,
       `pit_tuner::calc`). Bank 1 keeps the overview's cache, the cell's
       and the tuner's state and the wrappers.
       Whole functions only, not their closures: the machine outliner
       shares their small calls with bank 1's code. */
    *(.text.*12chimera_core2ui10tape_strip6raster8strip_in .text.*12chimera_core2ui10tape_strip6raster3tri
      .text.*12chimera_core2ui10tape_strip6raster6key_in .text.*12chimera_core2ui10tape_strip6raster*8Overview5of_in
      .text.*12chimera_core2ui11source_cell4card5stops .text.*12chimera_core2ui11source_cell4card9heads_key
      .text.*12chimera_core2ui11source_cell4card11moved_heads* .text.*12chimera_core2ui11source_cell4card*10SourceCell4size
      .text.*12chimera_core2ui11source_cell4card*10SourceCell7turn_in .text.*12chimera_core2ui11source_cell4card*10SourceCell4pick
      .text.*12chimera_core2ui11source_cell4card*10SourceCell7load_in .text.*12chimera_core2ui11source_cell4card*10SourceCell7kind_in
      .text.*12chimera_core2ui11source_cell4card*10SourceCell9rehead_in .text.*12chimera_core2ui11source_cell4card*10SourceCell6run_in
      .text.*7project8streamed*10StreamLoad5start .text.*7project8streamed*10StreamLoad5found
      .text.*7project8streamed*10StreamLoad6rehead .text.*7project8streamed9load_kind
      .text.*12chimera_core2ui5onset6detect* .text.*12chimera_core2ui3jog*3Jog4pass*
      .text.*12chimera_core2ui3jog*3Jog4rate* .text.*12chimera_core2ui9pit_tuner4calc*
      .text.*12chimera_core2ui4walk15machine_edit_in .text.*12chimera_core2ui4walk12tape_note_in
      .text.*12chimera_core2ui4walk12tape_held_in);
    *(.rodata.*12chimera_core2ui10tape_strip6raster* .rodata.*12chimera_core2ui11source_cell4card*
      .rodata.*12chimera_core2ui5onset6detect* .rodata.*12chimera_core2ui3jog*
      .rodata.*12chimera_core2ui9pit_tuner4calc* .rodata.*12chimera_core2ui4walk*);
";
    // The crates the engine can't name keep their own tables, each at a
    // fixed slot after the door (`chimera_engine::bank2`'s `*_DOOR_ADDR`):
    // the board's (the card interrupt), then the UI's.
    let (slots, asserts) = (
        "    . = __bank2_door + 0x100; __bank2_board_door = .;
    KEEP(*(.bank2.door.board));
    . = __bank2_door + 0x180; __bank2_ui_door = .;
    KEEP(*(.bank2.door.ui));
",
        "ASSERT(__bank2_board_door == 0x08100200, \"the board's table at its slot\");
ASSERT(__bank2_ui_door == 0x08100280, \"the UI's table at its slot\");",
    );
    // The VA's MOD oscillators (ADR 0163): `va::osc::door`, every MOD
    // render's entry and its runs' instances, LOOPFM's render, and NOISE's
    // pass (ADR 0197, `va::noise::kern`).
    let va = "    *(.text.*3osc4door* .rodata.*3osc4door* .text.*4lbfm11render_loop* .rodata.*4lbfm11render_loop*
      .text.*8VaEngine13render_loopfm* .rodata.*8VaEngine13render_loopfm*
      .text.*2va5noise4kern* .rodata.*2va5noise4kern*);
";
    // The LFO page's glyphs and scope curve (ADR 0177): `lfo_glyph::draw`,
    // the door's `render` and `curve`.
    let lfo =
        "    *(.text.*14chimera_engine9lfo_glyph4draw* .rodata.*14chimera_engine9lfo_glyph4draw*);
";
    // FLT › FX's curve rows and the curves they trace: `voice_fx::look`,
    // `soft::curve` and `fold::curve`. Text is `look::render` alone: `trace`
    // stays in bank 1 with its caller.
    let look = "    *(.text.*8voice_fx4look6render* .rodata.*8voice_fx4look* .text.*8voice_fx4soft5curve* .text.*8voice_fx4fold5curve*);
";
    // FUZZ's kernel (ADR 0157): `voice_fx::fuzz`, text and rodata (its law
    // table).
    let fuzz = "    *(.text.*8voice_fx4fuzz* .rodata.*8voice_fx4fuzz*);
";
    // The filters' FB loops (ADR 0190): each type's `fed` module, text only
    // (`filter::fed`).
    let fed = "    *(.text.*6filter*3fed*);
";
    // SEM's kernel (ADR 0192): `filter::sem::kern`, every loop of it, text
    // and rodata.
    let sem = "    *(.text.*6filter3sem4kern* .rodata.*6filter3sem4kern*);
";
    // The bench (ADR 0194): `chimera_stm32::bench`, text and rodata, and
    // its table at the slot after the UI's. A bench build only.
    let (bench, bench_slot, bench_assert) = if env::var_os("CARGO_FEATURE_BENCH").is_some() {
        (
            "    *(.text.*13chimera_stm325bench* .rodata.*13chimera_stm325bench*);\n",
            "    . = __bank2_door + 0x200; __bank2_bench_door = .;\n    KEEP(*(.bank2.door.bench));\n",
            "ASSERT(__bank2_bench_door == 0x08100300, \"the bench's table at its slot\");",
        )
    } else {
        ("", "", "")
    };
    // The delay's GRAIN (ADR 0202): `delay::grain` and the grain core
    // `dsp::grain`, text and rodata.
    let grain = "    *(.text.*5delay5grain* .rodata.*5delay5grain* .text.*3dsp5grain* .rodata.*3dsp5grain*);
";
    // The VCA's gate (ADR 0195): `voice::gate::kern`, as SEM's.
    let gate = "    *(.text.*5voice4gate4kern* .rodata.*5voice4gate4kern*);
";
    fs::write(
        out_dir.join("bank2.x"),
        r#"
/* Bank 2's image (ADR 0137): the 256-byte header, the door table after it,
   then what the door reaches. Inserted before .text, and read before
   link.x so its statements match first: cortex-m-rt's *(.text .text.*)
   never takes a section named here. Every firmware build has
   it (ADR 0194). */
SECTIONS {
  .bank2 : ALIGN(32) {
    KEEP(*(.bank2.header));
    . = ALIGN(32); __bank2_door = .;
    KEEP(*(.bank2.door));
    /* SLOTS */
    /* BENCH SLOT */
    *(.bank2.text .bank2.text.*);
    *(.bank2.rodata .bank2.rodata.*);
    /* TAPE's deck (`tape_engine::proxy`), by v0-mangled path: its own
       modules (`scrub`'s units among them, ADR 0205), and every item of
       `TapeDeck`; its spans and the stream's
       cursor, which only the deck calls, and `ratio_max`, which only
       admission does. `params` and `machine` stay in bank 1, as do `Heads`
       and the cost table. */
    *(.text.*11tape_engine5scrub* .text.*11tape_engine5heads* .text.*11tape_engine7stretch* .text.*11tape_engine4wave* .text.*11tape_engine4read*
      .text.*11tape_engine3age* .text.*11tape_engine6warble* .text.*11tape_engine7oneshot* .text.*11tape_engine4tron*
      .text.*11tape_engine5drift* .text.*11tape_engine4span* .text.*6stream6cursor* .text.*11tape_engine*8TapeDeck*
      .text.*11tape_engine9ratio_max*);
    *(.rodata.*11tape_engine5scrub* .rodata.*11tape_engine5heads* .rodata.*11tape_engine7stretch* .rodata.*11tape_engine4wave* .rodata.*11tape_engine4read*
      .rodata.*11tape_engine3age* .rodata.*11tape_engine6warble* .rodata.*11tape_engine7oneshot* .rodata.*11tape_engine4tron*
      .rodata.*11tape_engine5drift* .rodata.*11tape_engine4span* .rodata.*6stream6cursor* .rodata.*11tape_engine*8TapeDeck*);
    /* Streamed admission and loads (`AdmitDoor`, `LoadDoor`): the bill,
       the stealing pick and the block's re-bill; the heads' placement, a
       load's passes and their head reads. Bank 1 keeps the wrappers and
       the plain pick; a load's start and `load_kind` follow the UI's table
       (ADR 0175, below). */
    *(.text.*10instrument9admission* .text.*7project8streamed4load* .text.*6stream5stage5place*
      .text.*11source_file16read_source_head* .text.*11source_file16read_packed_into*);
    *(.rodata.*10instrument9admission* .rodata.*7project8streamed4load* .rodata.*6stream5stage5place*
      .rodata.*11source_file16read_source_head* .rodata.*11source_file16read_packed_into*);
    /* REVERB */
    /* VA */
    /* LFO */
    /* LOOK */
    /* FUZZ */
    /* FED */
    /* SEM */
    /* BENCH */
    /* GRAIN */
    /* GATE */
    . = ALIGN(32); __bank2_end = .;
  } > BANK2
} INSERT BEFORE .text;
ASSERT(ADDR(.bank2) == 0x08100000, "bank 2 starts at 0x08100000");
ASSERT(__bank2_end <= 0x08180000, "bank 2 stays clear of bootloader 1.09's slots at 0x08180000");
ASSERT(__bank2_end == ADDR(.bank2) || __bank2_door == 0x08100100, "the door follows the 256-byte header");
/* ASSERTS */
/* BENCH ASSERT */
"#
        .replace("    /* REVERB */\n", reverb)
        .replace("    /* SLOTS */\n", slots)
        .replace("/* ASSERTS */", asserts)
        .replace("    /* VA */\n", va)
        .replace("    /* LFO */\n", lfo)
        .replace("    /* LOOK */\n", look)
        .replace("    /* FUZZ */\n", fuzz)
        .replace("    /* FED */\n", fed)
        .replace("    /* SEM */\n", sem)
        .replace("    /* BENCH */\n", bench)
        .replace("    /* BENCH SLOT */\n", bench_slot)
        .replace("/* BENCH ASSERT */", bench_assert)
        .replace("    /* GRAIN */\n", grain)
        .replace("    /* GATE */\n", gate),
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
