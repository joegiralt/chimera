#![no_main]

// The assertions the project codec tests run on mutated files, on the
// fuzzer's bytes: raw (framing) and with the CRC repaired (records).
#[path = "../../tests/common/codec_util.rs"]
mod codec_util;

use chimera_core::project::Project;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    // No `test-support` here, so no `Project::boxed`.
    let mut raw = Box::<Project>::new_uninit();
    Project::init_in_place(&mut raw);
    // SAFETY: `init_in_place` built a valid project in the box.
    let mut p = unsafe { raw.assume_init() };
    codec_util::check_project_decode(data, &mut p);
    let mut fixed = data.to_vec();
    codec_util::fix_crc(&mut fixed);
    codec_util::check_project_decode(&fixed, &mut p);
});
