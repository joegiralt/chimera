// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The algorithmic engine (spec 2026-09-26-algo-engine-design): six
//! phase-modulation operators on 32 algorithms, morphing between two.

pub mod algorithms;
pub mod engine;
pub mod env;
pub mod formant;
pub mod kernel;
pub mod kinds;
pub mod math;
pub mod morph;
pub mod noise;
pub mod params;
pub mod plan;
pub mod tx;
pub mod vowel;
pub mod waves;
