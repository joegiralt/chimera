#![no_main]

// The assertions the codec tests run on mutated files, on the fuzzer's bytes:
// raw (framing) and with the CRC repaired (records).
#[path = "../../tests/common/codec_util.rs"]
mod codec_util;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    codec_util::check_decode(data);
    let mut fixed = data.to_vec();
    codec_util::fix_crc(&mut fixed);
    codec_util::check_decode(&fixed);
});
