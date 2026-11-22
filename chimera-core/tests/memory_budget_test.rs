//! ADR 0013 memory budgets. The `const` assertions next to each type fail the
//! build on both targets; these repeat them at run time so a failure prints
//! the numbers.

use core::mem::size_of;

use chimera_core::dsp::algo::engine::AlgoEngine;
use chimera_core::dsp::engines::EngineSlot;
use chimera_core::dsp::modal::{MAX_STRING_DELAY, ModalEngine};
use chimera_core::dsp::note_to_freq;
use chimera_core::dsp::tape_engine::TapeEngine;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw;

#[test]
fn voice_pool_fits_d2() {
    let size = size_of::<[Voice; hw::MAX_VOICES]>();
    eprintln!(
        "[Voice; {}] = {size} B, budget {} B",
        hw::MAX_VOICES,
        hw::VOICE_RAM_BUDGET
    );
    assert!(
        size <= hw::VOICE_RAM_BUDGET,
        "[Voice; {}] = {size} B",
        hw::MAX_VOICES
    );
}

/// ADR 0051: a voice holds its chain and one engine, never the sum of
/// engines; the chain can't grow unnoticed.
#[test]
fn voice_is_its_chain_plus_one_slot() {
    use core::mem::align_of;
    let (algo, modal, tape, slot) = (
        size_of::<AlgoEngine>(),
        size_of::<ModalEngine>(),
        size_of::<TapeEngine>(),
        size_of::<EngineSlot>(),
    );
    let chain = size_of::<Voice>() - slot;
    for (name, size) in [
        ("AlgoEngine", algo),
        ("ModalEngine", modal),
        ("TapeEngine", tape),
        ("EngineSlot", slot),
        ("chain", chain),
        ("Voice", size_of::<Voice>()),
    ] {
        eprintln!("{name:>12} {size:>7} B");
    }
    eprintln!(
        "{:>12} {:>7} B",
        format!("[Voice; {}]", hw::MAX_VOICES),
        size_of::<[Voice; hw::MAX_VOICES]>()
    );
    assert!(
        chain <= hw::VOICE_CHAIN_BYTES,
        "chain = {chain} B, budget {} B",
        hw::VOICE_CHAIN_BYTES
    );
    // `in_place_enum!`'s bound: the largest payload rounded up to the
    // slot's align, plus one align for the tag.
    let align = align_of::<EngineSlot>();
    assert!(
        slot <= algo.max(modal).max(tape).next_multiple_of(align) + align,
        "EngineSlot = {slot} B"
    );
}

/// A voice, re-read on formant-ops at Task 15 (6,304 B on main at
/// 4473f0a1): a tripwire, so a growth is seen and accounted. The played
/// Sound's FX block adds 16 B, and DRV's and the folder's blocks leave 24 B
/// (ADR 0155). FUZZ's state, one per place, adds 72 B (ADR 0156), and
/// PLAY's MODE beside it (ADR 0170) 8 B of padding. The filter's
/// STABILITY adds 40 B: the slot's chip, 36, and the played Sound's STAB
/// (ADR 0168). FB adds 24 B: the FB signal in CLEAN, DIRTY and the
/// four-pole slot, the last block's amount, and the played Sound's FB with
/// its padding (ADR 0190). SEM adds 32 B: its two states, FB signal and
/// last block's ends (28) and the played Sound's MODE knob (ADR 0192).
/// VA NOISE adds 8 B: the played Sound's KIND and LEVEL (ADR 0197). The
/// VCA's gate adds 72 B, its 68 B and padding (ADR 0195). STRETCH's four
/// params in the played Sound add 16 B (ADR 0204). Merged, 6,688 B.
/// WHEEL and PRESSURE add 8 B: two source values, the voice's own
/// pressure, the VCA routes' two rows (ADR 0236). SLIDE adds 16 B: its
/// value, the voice's own MPE bend and slide, padding (ADR 0252). The
/// played Sound's WIDTH adds 8 B, its 4 and padding; the last block's
/// OUT PAN and BASE (`LastPlayed`) 8 B more (ADR 0294). A fresh pluck's
/// level for the routed re-pluck adds 8 B, its 4 and padding (ADR 0310).
const VOICE_SIZE_BEFORE: usize = 6_736;

/// ADR 0131: the formant kinds live in the Algo slot, which still fits
/// inside Modal's, so they add nothing to `EngineSlot`.
#[test]
fn the_algo_engine_still_fits_its_slot() {
    assert!(size_of::<AlgoEngine>() <= size_of::<ModalEngine>());
    assert_eq!(size_of::<Voice>(), VOICE_SIZE_BEFORE);
}

/// ADR 0040, 0056: Modal's string lines hold G1 (MIDI 31, 49.0 Hz) at
/// 48 kHz, its period's whole samples and the low-pass's two taps, and no
/// more: F♯1 and lower clamp.
#[test]
fn modal_strings_cover_g1_and_no_lower() {
    let ring = |n: u8| (hw::SAMPLE_RATE as f32 / note_to_freq(n) - 0.5) as usize + 2;
    assert_eq!(ring(31), 981);
    assert_eq!(ring(31), MAX_STRING_DELAY, "G1 fits, exactly");
    assert!(ring(30) > MAX_STRING_DELAY, "the line fits F♯1");
    let inst = size_of::<chimera_core::instrument::Instrument>();
    eprintln!(
        "Instrument = {inst} B, {} B left in D2",
        hw::VOICE_RAM_BUDGET - inst
    );
}

#[test]
fn fx_bus_fits_its_axi_share() {
    use chimera_core::dsp::fx_bus::FxBus;
    let size = size_of::<FxBus>();
    eprintln!("FxBus = {size} B, budget {} B", hw::FX_BUS_BUDGET);
    assert!(size <= hw::FX_BUS_BUDGET, "FxBus = {size} B");
}

/// The AXI residents, reported (the linker enforces the margin, ADR 0087):
/// the Project (Performance and pool), the framebuffer, the UI, the scope,
/// AudioStats and
/// ORBIT view triple buffers, the scope's writer, ORBIT's runner, the FX bus
/// and the card's store (ADR 0014, ADR 0021). ORBIT's song rides inside
/// `Project`, so is not counted again; `AudioShared`'s snapshots are ITCM's
/// (ADR 0286, `itcm_residents_fit`).
#[test]
fn axi_report() {
    use chimera_core::dsp::fx_bus::FxBus;
    use chimera_core::orbit::runner::{OrbitRunner, OrbitView};
    use chimera_core::perf::load::AudioStats;
    use chimera_core::project::Project;
    use chimera_core::scope::{ScopeFrame, ScopeWriter};
    use chimera_core::triple::TripleBuffer;
    let parts = [
        ("framebuffer", hw::FB_BYTES),
        (
            "UI besides Project",
            size_of::<chimera_core::ui::UiState>() - size_of::<Project>(),
        ),
        ("Project", size_of::<Project>()),
        ("pool arena", chimera_core::project::arena::ARENA_BYTES),
        (
            "source table",
            size_of::<chimera_core::source_table::SourceTable>(),
        ),
        ("scope x3", size_of::<TripleBuffer<ScopeFrame>>()),
        ("scope writer", size_of::<ScopeWriter>()),
        ("AudioStats x3", size_of::<TripleBuffer<AudioStats>>()),
        ("OrbitView x3", size_of::<TripleBuffer<OrbitView>>()),
        ("OrbitRunner", size_of::<OrbitRunner>()),
        ("FxBus", size_of::<FxBus>()),
        ("store reserve", hw::STORE_RESERVE),
    ];
    for (name, size) in parts {
        eprintln!("{name:>15} {size:>7} B");
    }
    let total: usize = parts.iter().map(|p| p.1).sum();
    eprintln!("{:>15} {total:>7} B of {} B", "AXI", hw::AXI_SRAM);
}

/// The whole pool with its bookkeeping (allocator, part buses, sends) fits D2.
#[test]
fn instrument_fits_d2() {
    let size = size_of::<chimera_core::instrument::Instrument>();
    eprintln!("Instrument = {size} B, budget {} B", hw::VOICE_RAM_BUDGET);
    assert!(size <= hw::VOICE_RAM_BUDGET, "Instrument = {size} B");
}

/// The Project is the Performance, ORBIT's song, each Part's kept map
/// record (#815) and controller map's name (ADR 0226), plus no more than
/// its meta and the pool's index: the Sounds live in the arena (ADR 0087).
#[test]
fn axi_counts_the_project() {
    use chimera_core::midi_map::KeptMap;
    use chimera_core::orbit::scene::OrbitSong;
    use chimera_core::preset::Performance;
    use chimera_core::project::{Pool, Project};
    let (project, perf, pool, orbit, kept, ctl) = (
        size_of::<Project>(),
        size_of::<Performance>(),
        size_of::<Pool>(),
        size_of::<OrbitSong>(),
        size_of::<[KeptMap; hw::MAX_PARTS]>(),
        // None without `midi-map` (the bench builds).
        if cfg!(feature = "midi-map") {
            size_of::<[Option<chimera_core::name::SoundName>; hw::MAX_PARTS]>()
        } else {
            0
        },
    );
    eprintln!(
        "Project = {project} B: Performance {perf} B, pool index and scratch {pool} B, ORBIT {orbit} B, kept maps {kept} B, controller names {ctl} B"
    );
    // The index (128 Sound entries, 32 source entries, their
    // generations), the arena's writer and a load's scratch Sound with
    // its kept map record (#862).
    let scratch = size_of::<chimera_core::preset::Sound>() + size_of::<KeptMap>();
    assert!(pool - scratch <= 3072, "the pool's index is {pool} B");
    let parts = perf + pool + orbit + kept + ctl;
    assert!(
        project - parts <= 128,
        "Project = {project} B, {} B over its parts",
        project - parts
    );
}

/// UiState is one D2 static (`chimera-board/src/shared.rs`). Its Project
/// is counted on its own in `axi_report`; the rest (navigation, renderer,
/// regions, focus) is held to a growth tripwire.
#[test]
fn ui_state_rest_under_4k() {
    use chimera_core::project::Project;
    let rest = size_of::<chimera_core::ui::UiState>() - size_of::<Project>();
    eprintln!("UiState without its Project = {rest} B");
    // A growth tripwire, no physical limit: `UiState` is a D2 static
    // built in place, never moved or cloned. 512 B of it is TAPE's strip
    // (the overview's 432 B cache and its key, spec § RAM); 2.75 KB its
    // SOURCE cell (a load's `SourceCheck` 1 KB, the 16 heads, the
    // per-source memory's 512 B, the card's 64 rows; TAPE Task 17); 128 B
    // the filter types' (the LEVEL LEDs' holds and the matrix's taps: 88 B
    // on their own, 128 beside TAPE's, padded); 192 B the reverb's (the
    // TYPEs' knob memory, 108 B, and the cells' text keys); 3 KB the
    // library's page, 48 rows of 60 B, and a pending load (its source
    // streams in the SOURCE cell's slot); 80 B FACTORY's (showcase spec
    // § 1, Cost): the selected row's description slot, a `FactoryId` and a
    // `Note`, and the page's one set stamp; 16 B the screensaver's (its idle
    // timer and redraw count); 52 B the OP pages' formant view; 1,648 B the
    // MIDI map's CCs (6 Parts × 16 entries' 16 B ramp and bin), and 120 B more
    // for each entry's waiting bin, the hand's marks and each Part's room a
    // pass; 48 B bank 2's status (its version and seal, ADR 0137). 12 B the
    // six encoders' pushes (ADR 0140). 32 B the SOURCE cell's re-head
    // wait: the heads a streamed source wants, and since when; 128 B its
    // latch of each source's heads that can't be re-staged (slice 1
    // § 5.6). 16 B the LFO page's playhead and where it reads it (ADR
    // 0177). 152 B PIT's tuner: each Part's last note and the corner's two
    // lines (ADR 0201). 8 B more on the tree that merges them all
    // (integration-1021), past the sum of each feature's line. The focus
    // memory's 8 B (a slot per page id, the OP pages' ids up to 105, #484)
    // fit in what `LibId` gave back as a bare u32 instead of a tagged enum.
    assert!(
        rest <= 4096
            + 512
            + 2816
            + 128
            + 192
            + 3072
            + 80
            + 16
            + 52
            + 1648
            + 120
            + 48
            + 12
            + 32
            + 128
            + 16
            + 152
            + 8,
        "UiState grew to {rest} B besides its Project"
    );
}

/// Exclusive-state spec § Memory: Sympathetic's seven lines live in a pool
/// of four slots inside the `Instrument`, in D2. The voice holds only
/// SYMP's main string and lease: the largest model, within an align of
/// STRING's voice (ADR 0056).
#[test]
fn sympathetic_pool_fits_d2() {
    use chimera_core::dsp::modal::{SymPool, SympatheticSet, layout};
    use chimera_core::instrument::Instrument;
    let inst = size_of::<Instrument>();
    for (name, size) in [
        ("SympatheticVoice", layout::SYMPATHETIC_VOICE),
        ("StringVoice", layout::STRING),
        ("BowedString", layout::BOWED),
        ("ModelSlot", layout::MODEL_SLOT),
        ("SympatheticSet", size_of::<SympatheticSet>()),
        ("SymPool", size_of::<SymPool>()),
        ("Voice", size_of::<Voice>()),
        ("[Voice; 8]", size_of::<[Voice; hw::MAX_VOICES]>()),
        (
            "[NoteOrigin; 8]",
            size_of::<[chimera_core::instrument::NoteOrigin; hw::MAX_VOICES]>(),
        ),
        ("Instrument", inst),
        ("left in D2", hw::VOICE_RAM_BUDGET.saturating_sub(inst)),
    ] {
        eprintln!("{name:>16} {size:>7} B");
    }
    let align = layout::MODEL_SLOT_ALIGN;
    let largest = layout::SYMPATHETIC_VOICE.max(layout::BOWED);
    assert!(
        layout::MODEL_SLOT <= largest.next_multiple_of(align) + align,
        "ModelSlot = {} B: a model and the tag, not more",
        layout::MODEL_SLOT
    );
    assert!(
        inst <= hw::VOICE_RAM_BUDGET,
        "Instrument = {inst} B, budget {} B",
        hw::VOICE_RAM_BUDGET
    );
}

/// The runner is an AXI static beside the engine; its const assert is the
/// check, this prints the figure.
#[test]
fn orbit_runner_size() {
    use chimera_core::orbit::runner::{OrbitRunner, OrbitShared, OrbitView};
    eprintln!(
        "OrbitRunner = {} B, OrbitView = {} B, OrbitShared = {} B",
        size_of::<OrbitRunner>(),
        size_of::<OrbitView>(),
        size_of::<OrbitShared>()
    );
    assert!(
        size_of::<OrbitView>() <= 336,
        "OrbitView grew past 2dc9eefd's"
    );
    // § 5: the three copies in `AudioShared` grow by 12 B at most.
    assert!(
        size_of::<OrbitShared>() <= 880,
        "OrbitShared past 2dc9eefd's 876 + 4"
    );
}

/// `TapeDelay` as measured at cf50ee74, with the f32 line; the orbit
/// firmware's real AXI use there (`rust-size`: .data + .bss).
const TAPE_DELAY_F32: usize = 96_344;
const ORBIT_AXI_USED: usize = 384_264;

/// ADR 0090: the i16 delay line frees half its bytes, on every build.
/// GRAIN's grains and fades sit beside it (ADR 0202): 272 B, no line;
/// and SYNC's state (ADR 0302), 8 B: what it read, its fold, the glide's
/// speed.
#[test]
fn delay_line_frees_48128() {
    use chimera_core::dsp::delay::TapeDelay;
    use chimera_core::dsp::delay::grain::State;
    assert_eq!(size_of::<State>(), 272);
    assert_eq!(
        TAPE_DELAY_F32 + size_of::<State>() + 8 - size_of::<TapeDelay>(),
        48_128
    );
}

/// ADR 0087: the 160 KiB arena fits the 32 KiB margin only with the i16
/// line.
#[test]
fn arena_needs_the_delay() {
    use chimera_core::dsp::delay::TapeDelay;
    let freed = TAPE_DELAY_F32 - size_of::<TapeDelay>();
    let after = |freed: usize| ORBIT_AXI_USED - freed - 28_672 + 2_568 + 160 * 1024;
    assert!(
        after(freed) <= hw::AXI_SRAM - 32 * 1024,
        "160 KiB fits the 32 KiB margin with the i16 line"
    );
    assert!(
        after(0) > hw::AXI_SRAM - 32 * 1024,
        "and would not with the f32 line (ADR 0087)"
    );
}

/// D2 holds the voices' Instrument, the DMA rings' reserve, the UI's knob
/// publisher (ADR 0115), the formant views and the stream rings' third
/// slots, and keeps room: the figure ADR 0115 records.
#[test]
fn d2_holds_the_knob_publisher() {
    use chimera_core::dsp::knobs::KnobPublisher;
    let (inst, knobs) = (
        size_of::<chimera_core::instrument::Instrument>(),
        size_of::<KnobPublisher>(),
    );
    assert_eq!(knobs, hw::D2_KNOBS);
    let used = inst + hw::D2_DMA_RESERVE + knobs + hw::D2_FORMANT + hw::D2_THIRD_SLOTS;
    eprintln!(
        "D2: Instrument {inst} + DMA reserve {} + knobs {knobs} + formant views {} + third slots {} = {used} B, {} B free",
        hw::D2_DMA_RESERVE,
        hw::D2_FORMANT,
        hw::D2_THIRD_SLOTS,
        hw::D2_SRAM - used
    );
    // A slot a voice: 8 x 4,038 B, and the taken flag.
    assert_eq!(hw::D2_THIRD_SLOTS, 32_305);
    assert!(used <= hw::D2_SRAM);
}

/// Slice 1 § 10.3: every voice's ring fits SRAM4's 64 KiB.
#[test]
fn rings_fit_sram4() {
    use chimera_core::stream::ring::Rings;
    assert!(size_of::<Rings>() <= 64 * 1024, "{} B", size_of::<Rings>());
}

/// ADR 0265, 0269: a pad is 80 B and a kit 12 of them and TONAL (ADR 0270: an
/// enum over `PadId`, 2 B), held by the Project
/// through a pointer to an ITCM static (ADR 0266): the Parts, in D2, and
/// AXI don't grow.
#[test]
fn kits_are_compact_and_outside_the_parts() {
    use chimera_core::dsp::drum::types::{Kit, Pad};
    use chimera_core::project::Kits;
    eprintln!(
        "Pad = {} B, Kit = {} B, Kits = {} B",
        size_of::<Pad>(),
        size_of::<Kit>(),
        size_of::<Kits>()
    );
    // ADR 0269: 13 B of machine and mix, and a 64 B `PadSound` (57 B of
    // values, padded to its f32s' alignment); a kit 12 of them and TONAL,
    // padded to 4.
    assert_eq!(size_of::<Pad>(), 80);
    assert_eq!(size_of::<Kit>(), 12 * 80 + 4);
    // A kit, a selected pad, two `PadSet`s of lights, PAD FOLLOW's hold
    // (ADR 0268) and the kit's mark (ADR 0305, a tag and a CRC) a Part,
    // and a library load's scratch kit after the Parts' (ADR 0305), the
    // whole padded to 4.
    assert_eq!(
        size_of::<Kits>(),
        hw::MAX_PARTS * (12 * 80 + 4 + 1 + 4 + 2 + 8) + (12 * 80 + 4) + 2
    );
    // The audio's view (ITCM too): a Part's seqlock, TONAL, hit, two
    // `PadSet`s, seed and `KitBill` bits, and two slots of 12 pads at
    // 13 + 57 B each, padded to 4.
    use chimera_core::dsp::drum::view::KitView;
    assert_eq!(
        size_of::<KitView>(),
        hw::MAX_PARTS * (4 + 1 + 1 + 2 + 2 + 4 + 4 + 2 * 12 * 70usize).next_multiple_of(4)
    );
}

/// ADR 0266, 0286, 0284: ITCM's residents, the kits, the audio's kit view, `ed`'s
/// state, the audio's three `AudioShared` snapshots and the Parts' EQ, fit with the
/// margin to spare. The linker's assert (`itcm.x`) is the proof; this is a
/// conservative host estimate (each rounded up to 8, though `.itcm` packs
/// them at their own alignment, 4 here; the host's `usize` is 8 B), which
/// prints the sizes and fails first. `hw`'s geometry is the linker's own
/// (`itcm_geometry.rs`, which `chimera-stm32/build.rs` writes both from).
#[test]
fn itcm_residents_fit() {
    let parts = itcm_parts();
    for (name, count, size) in parts {
        eprintln!("{name:>12} x{count} {size:>7} B");
    }
    let total: usize = parts.iter().map(|p| p.2.next_multiple_of(8)).sum();
    eprintln!(
        "{:>15} {total:>7} B of {} B, margin {} B",
        "ITCM",
        hw::ITCM_STATE,
        hw::ITCM_MARGIN
    );
    assert!(
        total + hw::ITCM_MARGIN <= hw::ITCM_STATE,
        "ITCM: {total} B over its margin"
    );
}

/// ITCM's residents: the type and count `tools/itcm-residents.txt` gives
/// each, and the bytes of the static holding them.
fn itcm_parts() -> [(&'static str, usize, usize); 5] {
    use chimera_core::console::EdState;
    use chimera_core::dsp::drum::view::KitView;
    use chimera_core::instrument::AudioShared;
    use chimera_core::project::Kits;
    use chimera_core::triple::TripleBuffer;
    [
        ("Kits", 1, size_of::<Kits>()),
        ("KitView", 1, size_of::<KitView>()),
        ("EdState", 1, size_of::<EdState>()),
        ("AudioShared", 3, size_of::<TripleBuffer<AudioShared>>()),
        ("PartEqs", 1, size_of::<chimera_core::part_eq::PartEqs>()),
    ]
}

/// ADR 0286: one list of ITCM's residents. `check_itcm.py` holds the ELF to
/// `tools/itcm-residents.txt`; this sizes the same types, as many of each.
#[test]
fn itcm_residents_are_the_files() {
    const FILE: &str = include_str!("../../tools/itcm-residents.txt");
    let mut listed: Vec<(&str, usize)> = FILE
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('|').map(str::trim).collect();
            let count = f.get(2).and_then(|c| c.parse().ok());
            (f[1], count.expect("<symbol> | <type> | <count>"))
        })
        .collect();
    let mut sized: Vec<(&str, usize)> = itcm_parts().iter().map(|p| (p.0, p.1)).collect();
    listed.sort_unstable();
    sized.sort_unstable();
    assert_eq!(listed, sized);
}
