// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! FB through the door (ADR 0190): open, each TYPE's FB block crosses to
//! bank 2's loop and is heard; closed, the block runs the plain moving path,
//! bit for bit FB 0's. The host builds its own door (`open_host`); the door
//! is a process-wide static, so the tests take turns.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use std::sync::Mutex;

use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::dsp::filter::{Fed, Filter, FilterKind, FilterMods};
use chimera_engine::params::FilterParams;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

static DOOR: Mutex<()> = Mutex::new(());

fn render(kind: FilterKind, fb: f32) -> Vec<u32> {
    let mut p = FilterParams::default();
    p.set_kind(kind);
    (p.cutoff, p.resonance) = (500.0, 0.7);
    let mut f = Filter::new();
    let mut out = Vec::new();
    for b in 0..24 {
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
            let n = (b * BLOCK_SIZE + i) as f32;
            0.6 * (n * 0.0291).sin()
        });
        let mods = FilterMods {
            fb: Fed::new(fb, fb),
            ..FilterMods::NONE
        };
        f.process(&mut buf, &p, SAMPLE_RATE, &mods);
        out.extend(buf.iter().map(|v| v.to_bits()));
    }
    out
}

#[test]
fn fb_through_the_door_is_heard_on_every_type() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    DOOR_CELL.open_host();
    for k in FilterKind::BUILT {
        assert_ne!(render(k, 0.8), render(k, 0.0), "{k:?}");
    }
    DOOR_CELL.close();
}

#[test]
fn fb_with_the_door_closed_is_fb_zero() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    assert!(!DOOR_CELL.is_open());
    for k in FilterKind::BUILT {
        assert_eq!(render(k, 0.8), render(k, 0.0), "{k:?}");
    }
}
