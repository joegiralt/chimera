//! ADR 0013 memory budgets. The `const` assertions next to each type fail the
//! build on both targets; these repeat them at run time so a failure prints
//! the numbers.

use core::mem::size_of;

use chimera_core::dsp::modal::MAX_STRING_DELAY;
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

/// ADR 0014: Modal's string buffers hold the period of E1 (MIDI 28, 41.2 Hz)
/// at 48 kHz exactly; lower notes clamp to the buffer.
#[test]
fn modal_strings_cover_e1_and_no_lower() {
    let period = |n: u8| (hw::SAMPLE_RATE as f32 / note_to_freq(n)) as usize;
    assert_eq!(period(28), 1164);
    assert!(period(28) < MAX_STRING_DELAY, "E1 must not clamp");
    assert!(
        period(27) > MAX_STRING_DELAY - 1,
        "buffer is larger than E1 needs"
    );
}

#[test]
fn fx_bus_fits_its_axi_share() {
    use chimera_core::dsp::fx_bus::FxBus;
    let size = size_of::<FxBus>();
    eprintln!("FxBus = {size} B, budget {} B", hw::FX_BUS_BUDGET);
    assert!(size <= hw::FX_BUS_BUDGET, "FxBus = {size} B");
}

/// Spec § Hardware parity: Performance + SoundPool + framebuffer + UI
/// reserve (+ both AudioShared copies + the FX bus, ADR 0014) fit AXI.
#[test]
fn axi_residents_fit() {
    use chimera_core::dsp::fx_bus::FxBus;
    use chimera_core::instrument::{AXI_RESIDENT, AudioShared};
    use chimera_core::preset::{Performance, SoundPool};
    let parts = [
        ("framebuffer", hw::FB_BYTES),
        ("UI reserve", hw::UI_RESERVE),
        ("Performance", size_of::<Performance>()),
        ("SoundPool", size_of::<SoundPool>()),
        ("AudioShared x2", 2 * size_of::<AudioShared>()),
        ("FxBus", size_of::<FxBus>()),
    ];
    for (name, size) in parts {
        eprintln!("{name:>15} {size:>7} B");
    }
    let total: usize = parts.iter().map(|p| p.1).sum();
    eprintln!("{:>15} {total:>7} B of {} B", "AXI", hw::AXI_SRAM);
    assert_eq!(total, AXI_RESIDENT);
    assert!(total <= hw::AXI_SRAM);
}

/// The whole pool with its bookkeeping (allocator, part buses, sends) fits D2.
#[test]
fn instrument_fits_d2() {
    let size = size_of::<chimera_core::instrument::Instrument>();
    eprintln!("Instrument = {size} B, budget {} B", hw::VOICE_RAM_BUDGET);
    assert!(size <= hw::VOICE_RAM_BUDGET, "Instrument = {size} B");
}

/// UiState lives on `main`'s stack in AXI. Its Performance and SoundPool
/// are counted on their own in `axi_residents_fit`; the rest (navigation,
/// renderer, regions, focus: 1 120 B after the UI refresh, +48 B for the
/// per-page focus) comes out of the UI reserve.
#[test]
fn ui_state_fits_the_ui_reserve() {
    use chimera_core::preset::{Performance, SoundPool};
    let rest =
        size_of::<chimera_core::ui::UiState>() - size_of::<Performance>() - size_of::<SoundPool>();
    eprintln!(
        "UiState without Performance and SoundPool = {rest} B, reserve {} B",
        hw::UI_RESERVE
    );
    assert!(
        rest <= 2 * 1024,
        "UiState grew to {rest} B besides its Performance and SoundPool"
    );
}
