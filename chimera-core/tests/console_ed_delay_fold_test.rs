// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `ed since` says the delay's note again when its fold moves (ADR 0289,
//! 0302): the note's text reads the fold the audio thread publishes,
//! outside the note's own value. Its own binary: it publishes the global
//! clock (`meter::publish_rev_clock`).
mod common;
use common::console_fake::{Fake, ask};

use chimera_core::dsp::rev::sync::{FoldState, Tempo};
use chimera_core::meter::publish_rev_clock;

fn set(f: &mut Fake, addr: &str, v: &str) {
    assert_eq!(ask(f, &format!("ed set {addr} {v}\n")), "OK queued\n");
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let q = f.ed.as_mut().unwrap();
    chimera_core::bank2::ed_apply(q, &mut f.ui);
}

/// `ed since <n>`'s lines but for its `OK`, and its number.
fn since(f: &mut Fake, n: u32) -> (Vec<String>, u32) {
    let got = ask(f, &format!("ed since {n}\n"));
    let mut lines: Vec<String> = got.lines().map(str::to_string).collect();
    let last = lines.pop().unwrap();
    let m = last.strip_prefix("OK ").unwrap_or_else(|| panic!("{got}"));
    (lines, m.parse().unwrap_or_else(|_| panic!("{got}")))
}

fn get(f: &mut Fake, addr: &str) -> String {
    ask(f, &format!("ed get {addr}\n"))
        .lines()
        .next()
        .unwrap()
        .to_string()
}

/// SYNC on a 1/4: at 60 BPM the engine halves it (fold 1) and the cell
/// reads `▾1/8` under bits that never moved. `since` says the note's
/// line then, `ed get`'s own, and again when the fold comes back; the
/// reverb's fold moving alone says nothing of the delay's.
#[test]
fn since_says_the_note_again_when_the_delays_fold_moves() {
    publish_rev_clock(Tempo(120.0), FoldState(0), FoldState(0));
    let mut f = Fake::new();
    set(&mut f, "DELAY.SYNC", "SYNC");
    set(&mut f, "DELAY.NOTE", "1");
    let (_, m) = since(&mut f, 0);
    let whole = get(&mut f, "DELAY.NOTE");
    assert!(whole.ends_with(" \"1/4\""), "{whole}");

    // The delay's fold alone: the delay's values, with the note's new text.
    publish_rev_clock(Tempo(120.0), FoldState(0), FoldState(1));
    let folded = get(&mut f, "DELAY.NOTE");
    assert!(folded.ends_with("1/8\""), "{folded}");
    assert_ne!(folded, whole);
    let (lines, m) = since(&mut f, m);
    assert!(lines.contains(&folded), "{lines:?}");
    assert!(
        lines.iter().all(|l| l.starts_with("val DELAY.")),
        "the delay's block alone: {lines:?}"
    );

    // Nothing moved: nothing said.
    let (lines, m) = since(&mut f, m);
    assert!(lines.is_empty(), "{lines:?}");

    // The fold back: the note whole again.
    publish_rev_clock(Tempo(120.0), FoldState(0), FoldState(0));
    let (lines, m) = since(&mut f, m);
    assert!(lines.contains(&whole), "{lines:?}");

    // The reverb's fold alone: the reverb's values, none of the delay's.
    publish_rev_clock(Tempo(120.0), FoldState(1), FoldState(0));
    let (lines, _) = since(&mut f, m);
    assert!(!lines.is_empty());
    assert!(
        lines.iter().all(|l| l.starts_with("val REVERB.")),
        "{lines:?}"
    );
    publish_rev_clock(Tempo(120.0), FoldState(0), FoldState(0));
}
