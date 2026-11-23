// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The compressor's KEY through bank 2's door (ADR 0303): open, LOW keys
//! the compressor from the bass; closed, every KEY plays FULL, bit for
//! bit, and KEY's state is at rest.
#![cfg(feature = "bank2")]

mod common;

use chimera_core::bank2::DOOR_CELL;
use chimera_core::comp_key::door::state;
use chimera_core::comp_key::{CompKey, Door, KeyState};
use chimera_core::dsp::comp::{CompParams, Ratio};
use chimera_core::dsp::fx_bus::{FxBus, FxParams};
use chimera_core::hw::DAC_PAIRS;
use chimera_hal::BLOCK_SIZE;
use common::SR;

/// A 3 kHz tone through the master section under `key`, squashed hard:
/// the pairs, and the bus after.
fn play(key: CompKey) -> (Vec<[[f32; 2 * BLOCK_SIZE]; DAC_PAIRS]>, Box<FxBus>) {
    let p = FxParams {
        comp: CompParams {
            thresh: 0.0,
            ratio: Ratio::MAX,
            key,
            ..CompParams::DEFAULT
        },
        ..FxParams::OFF
    };
    let mut bus = Box::new(FxBus::new());
    let mut got = Vec::new();
    // Long enough for the tone's onset, which LOW does hear, to let go.
    for b in 0..600 {
        let mut out = [[0.0f32; 2 * BLOCK_SIZE]; DAC_PAIRS];
        for i in 0..BLOCK_SIZE {
            let t = (b * BLOCK_SIZE + i) as f32 / SR as f32;
            let x = 0.5 * (2.0 * core::f32::consts::PI * 3_000.0 * t).sin();
            (out[0][2 * i], out[0][2 * i + 1]) = (x, x);
        }
        bus.master(&mut out, &p, SR);
        got.push(out);
    }
    (got, bus)
}

/// One test: the door is process-wide.
#[test]
fn the_door_keys_open_and_plays_full_closed() {
    DOOR_CELL.open_host();
    let (full, _) = play(CompKey::Full);
    let (low, bus) = play(CompKey::Low);
    assert_ne!(low, full, "open: LOW does not hear 3 kHz");
    assert!(bus.master_gr_db() < 0.1, "{}", bus.master_gr_db());
    assert_ne!(*bus.comp_key(), KeyState::REST);
    assert_eq!(state(), Door::Open);
    assert_eq!(CompKey::Low.in_effect(state()), CompKey::Low);
    DOOR_CELL.close();
    assert_eq!(state(), Door::Closed);
    for key in CompKey::ALL.into_iter().filter(|k| *k != CompKey::Full) {
        let (closed, bus) = play(key);
        assert!(closed == full, "closed: {key:?} plays FULL");
        assert_eq!(*bus.comp_key(), KeyState::REST);
        // And the page says so under the curve.
        assert_eq!(key.in_effect(state()), CompKey::Full);
    }
    DOOR_CELL.open_host();
}
