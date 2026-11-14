//! DRUM (ADR 0265): a Part plays a 12-pad kit, each pad a machine with its
//! own knobs, level, pan, choke group and retrig.

pub mod engine;
pub mod kernel;
pub mod laws;
pub mod params;
pub mod sound;
pub mod types;
pub mod view;
