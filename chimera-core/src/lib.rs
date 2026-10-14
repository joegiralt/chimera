//! The UI and the console, built for size (ADR 0108), over
//! `chimera-engine`, whose modules are re-exported here under their own
//! names: `chimera_core::dsp` is `chimera_engine::dsp`.
#![no_std]

#[cfg(all(feature = "test-support", target_os = "none"))]
compile_error!("test-support is for host tests: it forges confirmations and fresh files");

pub use chimera_engine::{MidiChannel, MidiNote, Velocity};

#[cfg(feature = "probe")]
pub use chimera_engine::probe;
pub use chimera_engine::{
    addr, audio_out, block, boot, budget, clock_plan, dsp, factory, hw, import, in_place,
    instrument, meter, midi_in, midi_map, mod_path, modulation, name, note_queue, orbit, params,
    part, perf, played, preset, project, reset, scope, source_table, storage, sym_alloc, taps,
    tracked, triple, voice_alloc,
};

pub mod bank2;
pub mod console;
pub mod ui;
