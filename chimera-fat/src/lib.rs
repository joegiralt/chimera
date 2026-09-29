//! FAT16/FAT32 behind `chimera_hal::store`: the volume parser, the FAT
//! core (`blocks`, `fat`, `dir`, `fsinfo`, `fs`), the card deadline, and the
//! `Store` over them.
#![no_std]

pub mod blocks;
pub mod deadline;
pub mod dir;
pub mod fat;
pub mod fs;
pub mod fsinfo;
mod store;
pub mod volume;

pub use store::{BusPhase, FatStore, Medium, SdBus};
