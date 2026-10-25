//! MIDI vocabulary and pure decisions: no hardware, no allocation.
#![no_std]
#![forbid(unsafe_code)]

pub mod ctl;
pub mod law;
pub mod learn;
pub mod rel;
pub mod stream;
pub mod take;
pub mod value;
