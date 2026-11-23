// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

#![no_main]

use chimera_midi::ctl::Controllers;
use chimera_midi::stream::{Event, Parsed, Parser, RealTime};

// Arbitrary bytes must never panic; two buffer sizes cover the overflow edge,
// and layer 2 hears every voice event. FF resets layer 2, as the pipe does.
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let mut small = Parser::<4>::new();
    let mut big = Parser::<32>::new();
    let mut ctl = Controllers::new();
    for &b in data {
        let _ = small.push(b);
        match big.push(b) {
            Some(Parsed::Event(Event::Voice(ch, v))) => {
                let _ = ctl.hear(ch, v);
            }
            Some(Parsed::RealTime(RealTime::Reset)) => ctl = Controllers::new(),
            _ => {}
        }
    }
});
