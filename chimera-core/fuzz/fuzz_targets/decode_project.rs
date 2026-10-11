#![no_main]

// The assertions the project codec tests run on mutated files, on the
// fuzzer's bytes: raw (framing) and with the CRC repaired (records).
#[path = "../../tests/common/codec_util.rs"]
mod codec_util;

use chimera_core::project::Project;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let (mut p, _) = Project::boxed();
    codec_util::check_project_decode(data, &mut p);
    let mut fixed = data.to_vec();
    codec_util::fix_crc(&mut fixed);
    codec_util::check_project_decode(&fixed, &mut p);
});
