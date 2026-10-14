#![no_main]

// Any bytes: refused, or 32 voices that each map to a Source that builds,
// never a panic.
use chimera_core::import::tx81z::parse;
use chimera_desktop::factory_sounds::source;
use chimera_desktop::import::tx81z::to_source;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let Ok(bank) = parse(data) else {
        return;
    };
    for (i, v) in bank.voices.iter().enumerate() {
        let (s, _) = to_source(v, &format!("tx81z 00000000 #{}", i + 1));
        source::patch(&s).expect("a mapped voice builds");
    }
});
