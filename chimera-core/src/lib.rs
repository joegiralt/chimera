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
    addr, audio_out, block, board, boot, budget, clock_plan, dsp, factory, hw, import, in_place,
    instrument, lfo_glyph, meter, midi_in, midi_map, mod_path, modulation, mpe, name, note_queue,
    nrpn, orbit, out_ring, params, part, perf, played, preset, project, randomize, reset,
    scala_name, scope, source_table, storage, stream, sym_alloc, taps, tracked, triple, tuning,
    voice_alloc,
};

// The engine's TAPE (`bank2` brings it) with the UI that loads its sources,
// or neither: the engine alone plays nothing but SOURCE MISSING.
const _: () = assert!(
    cfg!(feature = "tape") == chimera_engine::stream::STREAMS,
    "chimera-engine/tape needs chimera-core/tape"
);

pub mod bank2;
pub mod console;
pub mod errand;
pub mod ui;
