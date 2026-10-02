//! ADR 0013 memory budgets. The `const` assertions next to each type fail the
//! build on both targets; these repeat them at run time so a failure prints
//! the numbers.

use core::mem::size_of;

use chimera_core::dsp::algo::engine::AlgoEngine;
use chimera_core::dsp::engines::EngineSlot;
use chimera_core::dsp::modal::{MAX_STRING_DELAY, ModalEngine};
use chimera_core::dsp::note_to_freq;
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
    let (algo, modal, slot) = (
        size_of::<AlgoEngine>(),
        size_of::<ModalEngine>(),
        size_of::<EngineSlot>(),
    );
    let chain = size_of::<Voice>() - slot;
    for (name, size) in [
        ("AlgoEngine", algo),
        ("ModalEngine", modal),
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
        slot <= algo.max(modal).next_multiple_of(align) + align,
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

/// Spec § Hardware parity: the Project (Performance and pool), the
/// framebuffer, the UI reserve, the AudioShared, scope and AudioStats triple
/// buffers, the scope's writer, the FX bus and the card's store (ADR 0014,
/// ADR 0021) fit AXI, with 64 KB to spare.
#[test]
fn axi_residents_fit() {
    use chimera_core::dsp::fx_bus::FxBus;
    use chimera_core::instrument::{AXI_RESIDENT, AudioShared};
    use chimera_core::perf::load::AudioStats;
    use chimera_core::project::Project;
    use chimera_core::scope::{ScopeFrame, ScopeWriter};
    use chimera_core::triple::TripleBuffer;
    let parts = [
        ("framebuffer", hw::FB_BYTES),
        ("UI reserve", hw::UI_RESERVE),
        ("Project", size_of::<Project>()),
        ("AudioShared x3", size_of::<TripleBuffer<AudioShared>>()),
        ("scope x3", size_of::<TripleBuffer<ScopeFrame>>()),
        ("scope writer", size_of::<ScopeWriter>()),
        ("AudioStats x3", size_of::<TripleBuffer<AudioStats>>()),
        ("FxBus", size_of::<FxBus>()),
        ("store reserve", hw::STORE_RESERVE),
    ];
    for (name, size) in parts {
        eprintln!("{name:>15} {size:>7} B");
    }
    let total: usize = parts.iter().map(|p| p.1).sum();
    eprintln!("{:>15} {total:>7} B of {} B", "AXI", hw::AXI_SRAM);
    assert_eq!(total, AXI_RESIDENT);
    assert!(
        hw::AXI_SRAM - total >= 64 * 1024,
        "{} B of AXI left",
        hw::AXI_SRAM - total
    );
}

/// The whole pool with its bookkeeping (allocator, part buses, sends) fits D2.
#[test]
fn instrument_fits_d2() {
    let size = size_of::<chimera_core::instrument::Instrument>();
    eprintln!("Instrument = {size} B, budget {} B", hw::VOICE_RAM_BUDGET);
    assert!(size <= hw::VOICE_RAM_BUDGET, "Instrument = {size} B");
}

/// The Project is the Performance and the pool's slots, plus no more than
/// its meta and the slots' generations.
#[test]
fn axi_counts_the_project() {
    use chimera_core::preset::{POOL_SIZE, Performance, Sound};
    use chimera_core::project::Project;
    let (project, perf, slots) = (
        size_of::<Project>(),
        size_of::<Performance>(),
        POOL_SIZE * size_of::<Option<Sound>>(),
    );
    eprintln!("Project = {project} B: Performance {perf} B, slots {slots} B");
    assert!(
        project - (perf + slots) <= 128,
        "Project = {project} B, {} B over its parts",
        project - (perf + slots)
    );
}

/// UiState is one AXI static (`chimera-stm32/src/shared.rs`). Its Project
/// is counted on its own in `axi_residents_fit`; the rest (navigation,
/// renderer, regions, focus) comes out of the UI reserve.
#[test]
fn ui_state_fits_the_ui_reserve() {
    use chimera_core::project::Project;
    let rest = size_of::<chimera_core::ui::UiState>() - size_of::<Project>();
    eprintln!(
        "UiState without its Project = {rest} B, reserve {} B",
        hw::UI_RESERVE
    );
    // 4 KB, not 2: SETTINGS' project listing (`MAX_LISTED` entries).
    assert!(
        rest <= 4 * 1024,
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
