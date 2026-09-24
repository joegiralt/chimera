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
    eprintln!("[Voice; {}] = {size} B, budget {} B", hw::MAX_VOICES, hw::VOICE_RAM_BUDGET);
    assert!(size <= hw::VOICE_RAM_BUDGET, "[Voice; {}] = {size} B", hw::MAX_VOICES);
}

/// ADR 0014: Modal's string buffers hold the period of E1 (MIDI 28, 41.2 Hz)
/// at 48 kHz exactly; lower notes clamp to the buffer.
#[test]
fn modal_strings_cover_e1_and_no_lower() {
    let period = |n: u8| (hw::SAMPLE_RATE as f32 / note_to_freq(n)) as usize;
    assert_eq!(period(28), 1164);
    assert!(period(28) <= MAX_STRING_DELAY - 1, "E1 must not clamp");
    assert!(period(27) > MAX_STRING_DELAY - 1, "buffer is larger than E1 needs");
}
