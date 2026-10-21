//! The grain engine (ADR 0199), one for every user: the delay's GRAIN
//! and TAPE's STRETCH read their own audio through `core::GrainSource`.
//! With `bank2` it links into bank 2 whole (`bank2.x`, `*3dsp5grain*`).

pub mod core;
