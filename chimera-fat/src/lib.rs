//! FAT16/FAT32 behind `chimera_hal::store`: the volume parser, the card
//! deadline, and the `embedded-sdmmc` store.
#![no_std]

pub mod deadline;
mod store;
pub mod volume;

pub use store::{BusPhase, FatStore, FixedTime, Medium, SdBus};
