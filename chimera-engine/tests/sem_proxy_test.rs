// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SEM through the door (ADR 0192): open, the block crosses to bank 2's
//! kernel and is heard, the same as a build without bank 2 would play it;
//! closed, the block stays dry. The host builds its own door (`open_host`);
//! the door is a process-wide static, so the tests take turns.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use std::sync::Mutex;

use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::dsp::filter::{Fed, Filter, FilterKind, FilterMode, FilterMods};
use chimera_engine::params::FilterParams;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

static DOOR: Mutex<()> = Mutex::new(());

fn input(b: usize) -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| {
        let n = (b * BLOCK_SIZE + i) as f32;
        0.6 * (n * 0.0291).sin()
    })
}

/// 24 blocks of SEM, `bp` or MODE ½, FB `fb`: the output's bits.
fn render(bp: bool, fb: f32) -> (Vec<u32>, Vec<u32>) {
    let mut p = FilterParams::default();
    p.set_kind(FilterKind::Sem);
    p.set_mode(if bp {
        FilterMode::Bp12
    } else {
        FilterMode::Lp12
    });
    (p.cutoff, p.resonance, p.morph) = (500.0, 0.9, 0.5);
    let mut f = Filter::new();
    let (mut out, mut dry) = (Vec::new(), Vec::new());
    for b in 0..24 {
        let mut buf = input(b);
        dry.extend(buf.iter().map(|v| v.to_bits()));
        let mods = FilterMods {
            fb: Fed::new(fb, fb),
            ..FilterMods::NONE
        };
        f.process(&mut buf, &p, SAMPLE_RATE, &mods);
        out.extend(buf.iter().map(|v| v.to_bits()));
    }
    (out, dry)
}

#[test]
fn sem_through_the_door_is_heard() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    DOOR_CELL.open_host();
    for bp in [false, true] {
        let (y, dry) = render(bp, 0.0);
        assert_ne!(y, dry, "bp {bp}");
        assert_ne!(render(bp, 0.8).0, y, "FB, bp {bp}");
    }
    DOOR_CELL.close();
}

#[test]
fn sem_with_the_door_closed_is_dry() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    assert!(!DOOR_CELL.is_open());
    for (bp, fb) in [(false, 0.0), (true, 0.0), (false, 0.8)] {
        let (y, dry) = render(bp, fb);
        assert_eq!(y, dry, "bp {bp} fb {fb}");
    }
}
