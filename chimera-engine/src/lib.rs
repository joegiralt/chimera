//! The synth's model and everything the audio runs, in the fast profile
//! (ADR 0108). `chimera-core` re-exports every module here under its own
//! name and adds the UI and the console, which build for size.
#![no_std]

#[cfg(all(feature = "test-support", target_os = "none"))]
compile_error!("test-support is for host tests: it forges confirmations and fresh files");
#[cfg(all(feature = "host-project", target_os = "none"))]
compile_error!("host-project is for host tools: it leaks a heap arena per project");

// `SymPool::boxed`, for tests; `Project::boxed`, for host tools too.
#[cfg(any(test, feature = "test-support", feature = "host-project"))]
extern crate alloc;

pub use chimera_hal::{MidiChannel, MidiNote, Velocity};

pub mod addr;
pub mod audio_out;
pub mod block;
pub mod boot;
pub mod budget;
pub mod clock_plan;
pub mod confirm;
pub mod dsp;
pub mod factory;
pub mod fmt_buf;
pub mod glyph;
pub mod hw;
pub mod import;
/// The in-place constructors' helpers, also `chimera-core`'s `UiState`'s.
pub mod in_place;
pub mod instrument;
pub mod meter;
pub mod midi_in;
pub mod midi_map;
pub mod midi_settings;
pub mod mod_path;
pub mod modulation;
pub mod name;
pub mod note_queue;
pub mod orbit;
pub mod params;
pub mod part;
pub mod perf;
pub mod played;
pub mod preset;
#[cfg(feature = "probe")]
pub mod probe;
pub mod project;
pub mod reset;
pub mod scope;
pub mod source_table;
pub mod storage;
pub mod sym_alloc;
pub mod taps;
pub mod theme_settings;
pub mod tracked;
pub mod triple;
pub mod voice_alloc;
