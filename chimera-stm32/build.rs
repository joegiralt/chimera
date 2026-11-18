use std::env;
use std::fs;
use std::path::PathBuf;

// ITCM's geometry (ADR 0266, 0286), `hw`'s own constants: one source.
include!("../chimera-engine/src/itcm_geometry.rs");

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let memory = fs::read_to_string("memory.x").unwrap();
    assert!(
        memory.contains("    /* ITCM REGION */\n"),
        "memory.x: the ITCM region's place"
    );
    fs::write(
        out_dir.join("memory.x"),
        memory.replace(
            "    /* ITCM REGION */\n",
            &format!("    ITCM (rwx) : ORIGIN = 0x{ITCM_ORIGIN:08X}, LENGTH = {ITCM_STATE}\n"),
        ),
    )
    .unwrap();

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
        out_dir.join("itcm.x"),
        format!(
            r#"
/* ITCM (ADR 0266, 0286): CPU-only state, never DMA's, built in place at
   boot (NOLOAD: `shared::zero_itcm` word-fills it first, then the builds):
   the kits (ADR 0265), `ed`'s state and the audio's snapshots (`AUDIO`).
   tools/itcm-residents.txt is the list `just check-itcm` holds. */
SECTIONS {{
    .itcm (NOLOAD) : ALIGN(8) {{
        __sitcm = .;
        *(.itcm .itcm.*);
        . = ALIGN(8);
        __eitcm = .;
    }} > ITCM
}}
INSERT AFTER .uninit;
ASSERT(__sitcm == 0x{ITCM_ORIGIN:08X}, "ITCM's state starts at ITCM_ORIGIN, clear of address 0");
ASSERT(__eitcm <= ORIGIN(ITCM) + LENGTH(ITCM), "ITCM overflows");
/* What a later move or a hot kernel may still take (ADR 0286). */
ASSERT(ORIGIN(ITCM) + LENGTH(ITCM) - __eitcm >= {ITCM_MARGIN}, "ITCM: under the margin (ADR 0286)");
"#
        ),
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
       calls too. `IrqState::work` is placed by its own `link_section`
       (`.bank2.text.card_work`): a glob can't reach an outlined helper,
       and LLVM outlines nothing from a function with an explicit section,
       so `Rings::wants`' fold closures stay here with it (#884). */
    *(.text.*13chimera_board8card_dma*8IrqState* .text.*13chimera_board8card_dma*4Held3end*
      .text.*13chimera_board8card_dma*4Held3fly* .text.*13chimera_board8card_dma*4Held4xfer*
      .text.*13chimera_board8card_dma*4Held5ready* .text.*13chimera_board8card_dma*4Held6polled*
      .text.*13chimera_board8card_dma*4Held4stop* .text.*13chimera_board8card_dma*4Held7command*
      .text.*13chimera_board8card_dma*6Flying4land* .text.*13chimera_board8card_dma6failed*
      .text.*13chimera_board8card_dma6wanted* .text.*13chimera_board8card_dma7serving*
      .text.*13chimera_board8card_dma8watch_in* .text.*6stream5serve*
      .text.*5Rings5wants* .text.*6stream3edf*);
    *(.rodata.*13chimera_board8card_dma*8IrqState* .rodata.*6stream5serve*);
    /* USB MIDI (`usb_midi::UsbDoor`): `usb_midi::door`, the class's
       allocation, descriptors (`chimera_midi::usb_class`) and the
       device's poll with it, and the packets, IN queue and Configured
       edge it runs (`chimera_midi::usb`). Bank 1 keeps
       the slot, the class's shims (its vtable is bank 1's) and the closed
       path. */
    *(.text.*13chimera_board8usb_midi4door* .rodata.*13chimera_board8usb_midi4door*
      .text.*12chimera_midi9usb_class* .rodata.*12chimera_midi9usb_class*
      .text.*12chimera_midi3usb* .rodata.*12chimera_midi3usb*);
    /* USB audio (ADR 0248, through USB MIDI's door): the stream's
       allocation, interrupt events and pump (`usb_audio::door`), and the
       pacer, packing and parity they run (`chimera_midi::uac`). Bank 1
       keeps the ring the audio interrupt pushes, the alt-setting shims
       (`chimera_midi::uac_link`) and `usb_audio::closed`. */
    *(.text.*13chimera_board9usb_audio4door* .rodata.*13chimera_board9usb_audio4door*
      .text.*12chimera_midi3uac* .rodata.*12chimera_midi3uac*);
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
      .text.*12chimera_core2ui4walk12tape_held_in .text.*12chimera_core2ui4walk11tape_fmt_in
      .text.*12chimera_core2ui4walk11law_text_in .text.*12chimera_core2ui4walk13delay_note_in
      .text.*12chimera_core2ui4walk15readout_word_in .text.*12chimera_core2ui4walk14tape_writes_in
      .text.*12chimera_core2ui4walk12band_note_in);
    *(.rodata.*12chimera_core2ui10tape_strip6raster* .rodata.*12chimera_core2ui11source_cell4card*
      .rodata.*12chimera_core2ui5onset6detect* .rodata.*12chimera_core2ui3jog*
      .rodata.*12chimera_core2ui9pit_tuner4calc* .rodata.*12chimera_core2ui4walk*);
    /* RANDOMIZE's roll (ADR 0232, `UiDoor::roll`): `randomize::kern`,
       text and rodata. The page's `Block` impl stays: its vtable is bank
       1's, which may not point into bank 2. */
    *(.text.*14chimera_engine9randomize4kern* .rodata.*14chimera_engine9randomize4kern*);
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
    // MODAL's block (ADR 0215): `modal::kern`, the render the door calls
    // with `ModalEngine::block` inlined, and the loops only it reaches:
    // BOWED's, and STRING's and SYMP's main string with its colour. Text
    // and rodata.
    let modal = "    *(.text.*5modal4kern* .rodata.*5modal4kern* .text.*5modal12render_bowed* .rodata.*5modal12render_bowed*
      .text.*11StringVoice6render* .rodata.*11StringVoice6render* .text.*11StringVoice6colour* .rodata.*11StringVoice6colour*);
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
    // `dsp::grain`, text and rodata; TAPE's loop (`delay::tape`) and the
    // chorus's (`chorus::kern`) too (ADR 0213).
    let grain = "    *(.text.*5delay5grain* .rodata.*5delay5grain* .text.*3dsp5grain* .rodata.*3dsp5grain*);
    *(.text.*5delay4tape* .rodata.*5delay4tape* .text.*6chorus4kern* .rodata.*6chorus4kern*);
";
    // The filter probes (`filter-probes`, a measuring build never shipped):
    // `filter::probe` and the gate's `lpg_probe` in bank 2, so their ~40 KB
    // leaves bank 1 to the code they count (ADR 0198). `loop_count.py`
    // reads them by name, wherever they sit.
    let probes = if env::var_os("CARGO_FEATURE_FILTER_PROBES").is_some() {
        "    *(.text.*6filter5probe* .rodata.*6filter5probe* .text.*3lpg9lpg_probe* .rodata.*3lpg9lpg_probe*);
"
    } else {
        ""
    };
    // The NRPN address space (ADR 0216): `chimera_engine::nrpn`, text and
    // rodata, linked by the MIDI UI's canonical NRPNs.
    // And the MIDI pipe (Phase 1, `MidiDoor`): `DinPipe`, its counting sink
    // (`Counted`), the sinks it runs on (`PipeSinks`), USB's slot and held keys (`UsbSlot`, `Held`), the SysEx
    // bodies' ring (`SxRing`, #846), MPE's zone reads (`chimera_engine::mpe`, ADR 0252), the audio's program-change hold
    // (`NoteHold`, ADR 0224) and its `Ready` rings' push, and the `chimera_midi` stream, controller, sensing and
    // SysEx layers it runs, and SysEx bindings' matching (`sxmap`, #846). And the map's codec (`midi_map::codec`), the
    // controller maps beside it (`midi_map::ctl`, ADR 0226), and
    // the binding arithmetic the MIDI UI runs (`chimera_midi`'s take, law
    // and rel), learn's classifier (`chimera_midi::learn`) and the 14-bit
    // MSB check it shares with the codec (`chimera_midi::out`). And KEEP
    // MAP's mark (`Project::set_map_kept`, ADR 0253), only the MIDI UI's.
    // And a program change's load (`Project::program_load`), only the MIDI UI's.
    let nrpn = "    *(.text.*14chimera_engine4nrpn* .rodata.*14chimera_engine4nrpn*);
    *(.text.*8midi_map5codec* .rodata.*8midi_map5codec*);
    *(.text.*8midi_map3ctl* .rodata.*8midi_map3ctl*);
    *(.text.*14chimera_engine7midi_in*7DinPipe* .rodata.*14chimera_engine7midi_in*7DinPipe*);
    *(.text.*14chimera_engine7midi_in*7Counted* .rodata.*14chimera_engine7midi_in*7Counted*);
    *(.text.*14chimera_engine7midi_in*9PipeSinks* .rodata.*14chimera_engine7midi_in*9PipeSinks*);
    *(.text.*14chimera_engine7midi_in*7UsbSlot* .rodata.*14chimera_engine7midi_in*7UsbSlot*);
    *(.text.*14chimera_engine7midi_in*4Held* .rodata.*14chimera_engine7midi_in*4Held*);
    *(.text.*14chimera_engine3mpe* .rodata.*14chimera_engine3mpe*);
    *(.text.*14chimera_engine7midi_in*6SxRing* .rodata.*14chimera_engine7midi_in*6SxRing*);
    *(.text.*14chimera_engine7midi_in*8NoteHold* .rodata.*14chimera_engine7midi_in*8NoteHold*);
    *(.text.*10note_queue*5Ready* .rodata.*10note_queue*5Ready*);
    *(.text.*7project*7Project12program_load* .rodata.*7project*7Project12program_load*);
    *(.text.*12chimera_midi6stream* .rodata.*12chimera_midi6stream*);
    *(.text.*12chimera_midi3ctl* .rodata.*12chimera_midi3ctl*);
    *(.text.*12chimera_midi5sense* .rodata.*12chimera_midi5sense*);
    *(.text.*12chimera_midi5sysex* .rodata.*12chimera_midi5sysex*);
    *(.text.*12chimera_midi5sxmap* .rodata.*12chimera_midi5sxmap*);
    *(.text.*12chimera_midi4take* .rodata.*12chimera_midi4take* .text.*12chimera_midi3law* .rodata.*12chimera_midi3law*);
    *(.text.*12chimera_midi3rel* .rodata.*12chimera_midi3rel*);
    *(.text.*12chimera_midi5learn* .rodata.*12chimera_midi5learn*);
    *(.text.*12chimera_midi3out* .rodata.*12chimera_midi3out*);
    *(.text.*7project*7Project12set_map_kept* .rodata.*7project*7Project12set_map_kept*);
";
    // Tuning (ADR 0240): the Scala parser and table (`chimera_theory::
    // tuning`), the tables' writers (`chimera_engine::tuning`; the audio's
    // read is inlined into bank 1) and the TUNING cell's apply
    // (`ui::tuning`).
    let tuning = "    *(.text.*14chimera_theory6tuning* .rodata.*14chimera_theory6tuning*);
    *(.text.*14chimera_engine6tuning* .rodata.*14chimera_engine6tuning*);
    *(.text.*12chimera_core2ui6tuning* .rodata.*12chimera_core2ui6tuning*);
";
    // The VCA's gate (ADR 0195): `voice::gate::kern`, as SEM's.
    // LFO DEPTH's routes (ADR 0250): `voice::depth::kern`, as the gate's.
    let gate = "    *(.text.*5voice4gate4kern* .rodata.*5voice4gate4kern*);
    *(.text.*5voice5depth4kern* .rodata.*5voice5depth4kern*);
";
    // ALGO's block (ADR 0212): `algo::engine::kern`, the render the door
    // calls with `AlgoEngine::block` inlined, and the kernel it runs,
    // `algo::kernel`, text and rodata.
    let algo = "    *(.text.*4algo6engine4kern* .rodata.*4algo6engine4kern* .text.*4algo6kernel* .rodata.*4algo6kernel*);
";
    // ORBIT (ADR 0208, #741). The runner: `orbit::door` in the engine, the
    // door's entries, and what only they reach: the block's steps
    // (`orbit::engine`), MUTATE, the walk, the runner's own block and load,
    // the MIDI clock's tick; the pools and the register note stay with the
    // scenes they edit. The pages: `ui::orbit::draw` and `ui::orbit::raster`,
    // their table at its slot after the bench's (`ORBIT_UI_DOOR_ADDR`);
    // `Ink` and the helpers built for it stay in bank 1 with their callers.
    // Text and rodata. Only with `orbit`: without it ORBIT isn't linked.
    let (orbit, orbit_slot, orbit_assert) = if env::var_os("CARGO_FEATURE_ORBIT").is_some() {
        (
            "    *(.text.*14chimera_engine5orbit4door* .rodata.*14chimera_engine5orbit4door*
      .text.*5orbit6engine* .rodata.*5orbit6engine* .text.*6mutate6mutate* .text.*6mutate5nudge*
      .text.*4walk9walk_next* .text.*11OrbitRunner7advance* .rodata.*11OrbitRunner7advance*
      .text.*11OrbitRunner4load* .text.*9MidiClock4tick*);
    *(.text.*2ui5orbit4draw* .rodata.*2ui5orbit4draw* .text.*2ui5orbit6raster* .rodata.*2ui5orbit6raster*);
",
            "    . = __bank2_door + 0x280; __bank2_orbit_ui_door = .;
    KEEP(*(.bank2.door.orbit_ui));
",
            "ASSERT(__bank2_orbit_ui_door == 0x08100380, \"ORBIT's pages' table at its slot\");",
        )
    } else {
        ("", "", "")
    };
    // The MIDI UI (Phase 1, ADR 0217): `ui::midi`, bindings resolving, the
    // ramp, bin and budget, text and rodata, its table at its slot after
    // ORBIT's (`MIDI_UI_DOOR_ADDR`). Bank 1 keeps `ui::cc`: the state's
    // home and the stubs.
    let (midi_ui, midi_ui_slot, midi_ui_assert) = (
        "    *(.text.*12chimera_core2ui4midi* .rodata.*12chimera_core2ui4midi*);\n",
        "    . = __bank2_door + 0x300; __bank2_midi_ui_door = .;\n    KEEP(*(.bank2.door.midi_ui));\n",
        "ASSERT(__bank2_midi_ui_door == 0x08100400, \"the MIDI UI's table at its slot\");",
    );
    // The console's `ed` (ADR 0251): `console::ed`, the browser editor's
    // parse and answers, text and rodata, its table at its slot after the
    // MIDI UI's (`ED_DOOR_ADDR`). Bank 1 keeps `EdLine` and the stub. The
    // first S4 group (ADR 0272: new 0.4 work goes high), after MIXER's
    // reserve; its table stays in S0 with the other doors.
    // VA's solos (ADR 0283): `va::osc::solo`, the saw's runs on its
    // anchor, the next S4 group. Its table heads the S4 groups
    // (`VA_SOLO_DOOR_ADDR`): the door's own is full, and a slot in S0
    // would take S0-S3 under ADR 0251's 8 KB.
    // The Parts' EQ (ADR 0284): `part_eq::dsp`, its block, the next S4
    // group; its table 16 B after the solos' (`PART_EQ_DOOR_ADDR`).
    let (ed, ed_slot, ed_assert) = (
        "    __bank2_va_solo_door = .;\n    KEEP(*(.bank2.door.va_solo));\n    . = __bank2_va_solo_door + 0x10;\n    __bank2_part_eq_door = .;\n    KEEP(*(.bank2.door.part_eq));\n    *(.text.*12chimera_core7console2ed* .rodata.*12chimera_core7console2ed*);\n    *(.text.*2va3osc4solo* .rodata.*2va3osc4solo*);\n    *(.text.*7part_eq3dsp* .rodata.*7part_eq3dsp*);\n",
        "    . = __bank2_door + 0x380; __bank2_ed_door = .;\n    KEEP(*(.bank2.door.ed));\n",
        "ASSERT(__bank2_ed_door == 0x08100480, \"ed's table at its slot\");\nASSERT(__bank2_va_solo_door == 0x08180400, \"VA's solos' table heads the S4 groups\");\nASSERT(__bank2_part_eq_door == 0x08180410, \"the Parts' EQ's table 16 B after the solos'\");",
    );
    // DRUM's voice groups (ADR 0280, `UiDoor::groups`, its one entry):
    // `ui::kit_groups` and the count it reads, `drum::groups`;
    // an S4 group after `ed`'s (ADR 0272: new 0.4 work goes high).
    let groups =
        "    *(.text.*12chimera_core2ui10kit_groups* .rodata.*12chimera_core2ui10kit_groups*
      .text.*14chimera_engine3dsp4drum6groups* .rodata.*14chimera_engine3dsp4drum6groups*);\n";
    // A Part's MAX VOICES and RESERVE (ADR 0281, `UiDoor::voices`, its
    // one entry): `ui::part_voices`, beside the voice groups.
    // `ToastTimer::shows`' instance (`Option<Toast>::is_some_and` with
    // its closure): only kit_groups and part_voices call it.
    // `Shares::refusal` is only ever called from part_voices: placed with
    // it if it gets a symbol of its own.
    let voices =
        "    *(.text.*12chimera_core2ui11part_voices* .rodata.*12chimera_core2ui11part_voices*
      .text.*10ToastTimer5shows* .text.*11voice_limit*6Shares7refusal*);\n";
    let s4 = format!("{ed}{groups}{voices}");
    fs::write(
        out_dir.join("bank2.x"),
        r#"
/* Bank 2's image (ADR 0137): the 256-byte header, the door table after it,
   then what the door reaches. Inserted before .text, and read before
   link.x so its statements match first: cortex-m-rt's *(.text .text.*)
   never takes a section named here. Every firmware build has
   it (ADR 0194). One section, exactly 1 MB (ADR 0272): the low groups in
   S0-S3, then fixed points for bootloader 1.09's slots, each a 1 KB
   reserve holding its stub at 0x08180000 (MIXER) and 0x081C0000 (EDIT)
   and an end marker on the last flash word of S5 and of S7, with room for
   S4's and S6's groups between. Every gap is filled 0xFF, as erased flash
   reads (lld's fill is a 32-bit pattern: all four bytes). */
SECTIONS {
  .bank2 : ALIGN(32) {
    __bank2_base = .;
    KEEP(*(.bank2.header));
    . = ALIGN(32); __bank2_door = .;
    KEEP(*(.bank2.door));
    /* SLOTS */
    /* BENCH SLOT */
    /* ORBIT SLOT */
    /* MIDI UI SLOT */
    /* ED SLOT */
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
       (ADR 0175, below). The pool's streamed commit and re-head are a
       load's alone: here, whatever the inliner leaves of them. */
    *(.text.*10instrument9admission* .text.*7project8streamed4load* .text.*6stream5stage5place*
      .text.*11source_file16read_source_head* .text.*11source_file16read_packed_into*
      .text.*7project4pool*source_commit_streamed* .text.*7project4pool*source_rehead*
      .text.*7project4pool*rehead_beside* .text.*7project4pool*rehead_in_place*);
    *(.rodata.*10instrument9admission* .rodata.*7project8streamed4load* .rodata.*6stream5stage5place*
      .rodata.*11source_file16read_source_head* .rodata.*11source_file16read_packed_into*
      .rodata.*7project4pool*source_commit_streamed* .rodata.*7project4pool*source_rehead*
      .rodata.*7project4pool*rehead_beside* .rodata.*7project4pool*rehead_in_place*);
    /* REVERB */
    /* VA */
    /* LFO */
    /* LOOK */
    /* FUZZ */
    /* MODAL */
    /* FED */
    /* SEM */
    /* BENCH */
    /* GRAIN */
    /* GATE */
    /* PROBES */
    /* ALGO */
    /* NRPN */
    /* ORBIT */
    /* MIDI UI */
    /* TUNING */
    . = ALIGN(32); __bank2_lo_end = .;
    . = __bank2_base + 0x80000; __slot_mixer = .;
    KEEP(*(.bank2.slot.mixer));
    . = __bank2_base + 0x80400;
    /* S4 GROUPS */
    . = ALIGN(32); __bank2_s4_end = .;
    . = __bank2_base + 0xBFFE0;
    KEEP(*(.bank2.slot.mixer_end));
    . = __bank2_base + 0xC0000; __slot_edit = .;
    KEEP(*(.bank2.slot.edit));
    . = __bank2_base + 0xC0400;
    /* S6 GROUPS */
    . = ALIGN(32); __bank2_s6_end = .;
    . = __bank2_base + 0xFFFE0;
    KEEP(*(.bank2.slot.edit_end));
    __bank2_end = .;
  } > BANK2 =0xFFFFFFFF
} INSERT BEFORE .text;
ASSERT(ADDR(.bank2) == 0x08100000, "bank 2 starts at 0x08100000");
ASSERT(__bank2_lo_end <= 0x08180000, "bank 2's low groups fit S0-S3, clear of MIXER's slot at 0x08180000");
ASSERT(__bank2_s4_end <= 0x081BFFE0, "the S4 groups fit S4-S5, clear of S5's end marker at 0x081BFFE0");
ASSERT(__bank2_s6_end <= 0x081FFFE0, "the S6 groups fit S6-S7, clear of S7's end marker at 0x081FFFE0");
ASSERT(__slot_mixer == 0x08180000, "MIXER's stub at 0x08180000, where bootloader 1.09 jumps");
ASSERT(__slot_edit == 0x081C0000, "EDIT's stub at 0x081C0000, where bootloader 1.09 jumps");
ASSERT(__bank2_end == 0x08200000, "bank 2 is exactly 1 MB, S7's end marker last");
ASSERT(__bank2_door == 0x08100100, "the door follows the 256-byte header");
/* ASSERTS */
/* BENCH ASSERT */
/* ORBIT ASSERT */
/* MIDI UI ASSERT */
/* ED ASSERT */
"#
        .replace("    /* REVERB */\n", reverb)
        .replace("    /* SLOTS */\n", slots)
        .replace("/* ASSERTS */", asserts)
        .replace("    /* VA */\n", va)
        .replace("    /* LFO */\n", lfo)
        .replace("    /* LOOK */\n", look)
        .replace("    /* FUZZ */\n", fuzz)
        .replace("    /* MODAL */\n", modal)
        .replace("    /* FED */\n", fed)
        .replace("    /* SEM */\n", sem)
        .replace("    /* BENCH */\n", bench)
        .replace("    /* BENCH SLOT */\n", bench_slot)
        .replace("/* BENCH ASSERT */", bench_assert)
        .replace("    /* GRAIN */\n", grain)
        .replace("    /* GATE */\n", gate)
        .replace("    /* PROBES */\n", probes)
        .replace("    /* ALGO */\n", algo)
        .replace("    /* NRPN */\n", nrpn)
        .replace("    /* ORBIT */\n", orbit)
        .replace("    /* ORBIT SLOT */\n", orbit_slot)
        .replace("/* ORBIT ASSERT */", orbit_assert)
        .replace("    /* MIDI UI */\n", midi_ui)
        .replace("    /* TUNING */\n", tuning)
        .replace("    /* MIDI UI SLOT */\n", midi_ui_slot)
        .replace("/* MIDI UI ASSERT */", midi_ui_assert)
        .replace("    /* S4 GROUPS */\n", &s4)
        .replace("    /* ED SLOT */\n", ed_slot)
        .replace("/* ED ASSERT */", ed_assert),
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
    println!("cargo:rustc-link-arg=-Titcm.x");
    println!("cargo:rustc-link-arg=-Taxi_margin.x");
    // The relocations stay in the ELF: the door check reads them.
    println!("cargo:rustc-link-arg=--emit-relocs");
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=../chimera-engine/src/itcm_geometry.rs");
    println!("cargo:rerun-if-changed=build.rs");
}
