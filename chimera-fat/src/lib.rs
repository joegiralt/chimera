//! FAT16/FAT32 behind `chimera_hal::store`: the volume parser, the card
//! deadline, and later the
//! `embedded-sdmmc` store.
#![no_std]

pub mod deadline;
pub mod volume;
