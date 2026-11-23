// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A voice's tuning offset (ADR 0240): a strike starts on its key's, a
//! glide eases to the new key's at the glide's own pace, never a step.

#![cfg(feature = "tuning")]

use chimera_engine::tuning::{VoiceTunes, keep_for};

const SR: u32 = 48_000;

#[test]
fn a_strike_starts_on_its_offset() {
    let mut t = VoiceTunes::NEW;
    assert_eq!(t.strike(2, 0.25), 0.25);
    assert_eq!(t.step(2), 0.25);
}

#[test]
fn a_glide_moves_little_in_one_block_then_lands_smoothly() {
    let mut t = VoiceTunes::NEW;
    t.strike(0, 0.0);
    // STEAL GLIDE's 300 ms: a time constant of 100 ms.
    assert_eq!(t.glide(0, 1.0, keep_for(0.1, SR)), 0.0, "where it was");
    let first = t.step(0);
    assert!(
        first > 0.0 && first < 0.05,
        "one block moves under 5 %: {first}"
    );
    let mut last = first;
    for _ in 0..(SR / 64) {
        let now = t.step(0);
        assert!(now >= last && now - last < 0.05, "smooth: {last} to {now}");
        last = now;
    }
    assert!((last - 1.0).abs() < 1e-3, "lands: {last}");
}

#[test]
fn twelve_tet_never_moves() {
    let mut t = VoiceTunes::NEW;
    t.strike(1, 0.0);
    t.glide(1, 0.0, keep_for(0.01, SR));
    assert_eq!(t.step(1).to_bits(), 0.0f32.to_bits());
}
