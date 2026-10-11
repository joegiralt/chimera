#![no_main]

// The library list's header peek must never panic, whatever the bytes.
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = chimera_core::storage::peek_library(data, data.len() as u32);
});
