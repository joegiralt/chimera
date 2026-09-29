#![no_main]

// The assertions the codec tests run on mutated files, on the fuzzer's bytes.
#[path = "../../tests/common/codec_util.rs"]
mod codec_util;

libfuzzer_sys::fuzz_target!(|data: &[u8]| codec_util::check_decode(data));
