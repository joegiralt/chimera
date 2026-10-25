#![no_main]

use chimera_midi::stream::Parser;

// Arbitrary bytes must never panic; two buffer sizes cover the overflow edge.
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let mut small = Parser::<4>::new();
    let mut big = Parser::<32>::new();
    for &b in data {
        let _ = small.push(b);
        let _ = big.push(b);
    }
});
