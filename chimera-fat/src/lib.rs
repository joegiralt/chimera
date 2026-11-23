// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! FAT16/FAT32 behind `chimera_hal::store`: the volume parser, the FAT
//! core (`blocks`, `fat`, `dir`, `fsinfo`, `fs`), the card deadline and
//! presence check, the lost-cluster sweep, and the `Store` over them.
#![no_std]

pub mod blocks;
pub mod deadline;
pub mod dir;
pub mod fat;
pub mod fs;
pub mod fsinfo;
pub mod h7_spi;
pub mod mark;
pub mod sd;
pub mod sd_addr;
pub mod sd_proto;
pub mod shared_card;
mod store;
pub mod sweep;
pub mod volume;

pub use store::{BusPhase, FatStore, Medium, SdBus};
