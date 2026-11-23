// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host-side library: the importers the simulator's binaries share,
//! TAPE's listening rig, and the sim's card.

pub mod card_stream;
pub mod factory_sounds;
pub mod factory_tapes;
pub mod import;
pub mod listen;
pub mod privacy;
pub mod store;
