/// Convert MIDI note number to frequency in Hz.
pub fn note_to_freq(note: u8) -> f32 {
    440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0)
}

pub mod drive;
pub mod midiverb;
pub mod envelope;
pub mod filter;
pub mod fm;
pub mod modal;
pub mod oscillator;
pub mod reverb;
pub mod voice;
pub mod wavefolder;
