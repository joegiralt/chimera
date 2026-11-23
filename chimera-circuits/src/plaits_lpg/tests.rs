// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The lpg-circuit lab's Plaits rows as assertions: `docs/lpg/gate.md`
//! (the gate checks and the "over time" trace on a 220 Hz saw ping).

use super::*;
use crate::buchla292b::stim::{Wave, checks, perc, tone};

#[test]
fn gate_checks_meet_the_ledger() {
    // gate.md, the Plaits rows (DECAY ½, COLOUR ½): saw onset 2442 Hz,
    // 50 ms 1553, 200 ms 1665; −20 dB at 493 ms, −40 at 1143, −35.5 dB at
    // 1 s. Fold 677 / 660 / 585 Hz; 439 / 1091 ms; −37.3 dB.
    let n = (1.6 * FS) as usize;
    for (w, on, c50, c200, t20, t40, floor) in [
        (Wave::Saw, 2442.0, 1553.0, 1665.0, 0.493, 1.143, -35.5),
        (Wave::Fold, 677.0, 660.0, 585.0, 0.439, 1.091, -37.3),
    ] {
        let y = plaits(&tone(w, 220.0, n), &perc(n, 1.0), DECAY, COLOUR);
        let c = checks(&y);
        let near = |g: f64, w: f64| (g / w - 1.0).abs() < 0.02;
        assert!(near(c.centroid_on, on), "{w:?} onset {}", c.centroid_on);
        assert!(
            near(c.c50, c50) && near(c.c200, c200),
            "{w:?} {} {}",
            c.c50,
            c.c200
        );
        let (a, b) = (c.t20.unwrap(), c.t40.unwrap());
        assert!(
            (a - t20).abs() < 0.004 && (b - t40).abs() < 0.004,
            "{w:?} {a} {b}"
        );
        assert!((c.floor_1s - floor).abs() < 0.3, "{w:?} 1 s {}", c.floor_1s);
    }
}

#[test]
fn state_and_cutoff_follow_the_ledger() {
    // gate.md, "Over time": Plaits' state (= gain) and cutoff after the
    // ping: 0.971 / 13927 Hz at 2 ms, 0.923 / 11655 at 10 ms, 0.692 / 4422
    // at 50 ms, 0.367 / 1366 at 200 ms, 0.021 / 1104 at 1 s.
    let n = (1.6 * FS) as usize;
    let tr = trace(&perc(n, 1.0), DECAY, COLOUR);
    for (ms, st, fc) in [
        (2.0, 0.971, 13927.0),
        (10.0, 0.923, 11655.0),
        (50.0, 0.692, 4422.0),
        (200.0, 0.367, 1366.0),
        (1000.0, 0.021, 1104.0),
    ] {
        let k = (ms * 1e-3 * FS) as usize / P_BLOCK;
        let s = tr[k].0;
        assert!((s - st).abs() < 0.0015, "{ms} ms: state {s}");
        // The lab's trace computed the cutoff from the state before the
        // block's update (its audio path, like ours, uses the state after):
        // the ledger's column is the previous block's cutoff.
        let f = tr[k - 1].1;
        assert!((f / fc - 1.0).abs() < 0.002, "{ms} ms: cutoff {f}");
    }
}

#[test]
fn rest_level_settles_at_the_cutoff() {
    for hz in [2000.0, 6000.0, 12000.0] {
        let l = rest_level(hz, COLOUR);
        let tr = trace(&vec![l; 4800], DECAY, COLOUR);
        let f = tr[tr.len() - 1].1;
        assert!((f / hz - 1.0).abs() < 0.01, "{hz}: {f}");
    }
    assert_eq!(rest_level(500.0, COLOUR), 0.0);
}
