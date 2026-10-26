#![no_main]

use chimera_midi::ctl::Controllers;
use chimera_midi::stream::{Event, Parsed, Parser};

// Arbitrary bytes must never panic; two buffer sizes cover the overflow edge,
// and layer 2 hears every voice event.
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let mut small = Parser::<4>::new();
    let mut big = Parser::<32>::new();
    let mut ctl = Controllers::new();
    for &b in data {
        let _ = small.push(b);
        if let Some(Parsed::Event(Event::Voice(ch, v))) = big.push(b) {
            let _ = ctl.hear(ch, v);
        }
    }
});
