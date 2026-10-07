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
/// the Project (Performance and pool), the framebuffer, the UI,
/// the AudioShared, scope, AudioStats and
/// ORBIT view triple buffers, the scope's writer, ORBIT's runner, the FX bus
/// and the card's store (ADR 0014, ADR 0021).
/// ORBIT's settings ride inside `AudioShared` and its song inside `Project`,
/// so neither is counted again.
#[test]
fn axi_report() {
    use chimera_core::dsp::fx_bus::FxBus;
    use chimera_core::instrument::AudioShared;
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
        ("AudioShared x3", size_of::<TripleBuffer<AudioShared>>()),
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

/// The Project is the Performance and ORBIT's song, plus no more than its
/// meta and the pool's index: the Sounds live in the arena (ADR 0087).
#[test]
fn axi_counts_the_project() {
    use chimera_core::orbit::scene::OrbitSong;
    use chimera_core::preset::Performance;
    use chimera_core::project::{Pool, Project};
    let (project, perf, pool, orbit) = (
        size_of::<Project>(),
        size_of::<Performance>(),
        size_of::<Pool>(),
        size_of::<OrbitSong>(),
    );
    eprintln!(
        "Project = {project} B: Performance {perf} B, pool index and scratch {pool} B, ORBIT {orbit} B"
    );
    // The index (128 Sound entries, 32 source entries, their
    // generations), the arena's writer and a load's scratch Sound.
    let scratch = size_of::<chimera_core::preset::Sound>();
    assert!(pool - scratch <= 3072, "the pool's index is {pool} B");
    let parts = perf + pool + orbit;
    assert!(
        project - parts <= 128,
        "Project = {project} B, {} B over its parts",
        project - parts
    );
}

/// UiState is one AXI static (`chimera-board/src/shared.rs`). Its Project
/// is counted on its own in `axi_report`; the rest (navigation, renderer,
/// regions, focus) is held to a growth tripwire.
#[test]
fn ui_state_rest_under_4k() {
    use chimera_core::project::Project;
    let rest = size_of::<chimera_core::ui::UiState>() - size_of::<Project>();
    eprintln!("UiState without its Project = {rest} B");
    // A growth tripwire, no physical limit: `UiState` is an AXI static
    // built in place, never moved or cloned. 512 B of it is TAPE's strip
    // (the overview's 432 B cache and its key, spec § RAM); 2.75 KB its
    // SOURCE cell (a load's `SourceCheck` 1 KB, the 16 heads, the
    // per-source memory's 512 B, the card's 64 rows; TAPE Task 17); 128 B
    // the filter types' (the LEVEL LEDs' holds and the matrix's taps: 88 B
    // on their own, 128 beside TAPE's, padded).
    assert!(
        rest <= 4096 + 512 + 2816 + 128,
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
#[test]
fn delay_line_frees_48128() {
    use chimera_core::dsp::delay::TapeDelay;
    assert_eq!(TAPE_DELAY_F32 - size_of::<TapeDelay>(), 48_128);
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
