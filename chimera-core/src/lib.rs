#![no_std]

pub use chimera_hal::{MidiChannel, MidiNote, Velocity};

pub mod addr;
pub mod audio_out;
pub mod block;
pub mod clock_plan;
pub mod dsp;
pub mod hw;
mod in_place;
pub mod instrument;
pub mod mod_path;
pub mod modulation;
pub mod note_queue;
pub mod params;
pub mod part;
pub mod perf;
pub mod preset;
pub mod scope;
pub mod triple;
pub mod ui;
pub mod voice_alloc;
