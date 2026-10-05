#![no_main]

// Any bytes: an error or a finite source at a real rate, never a panic; the
// rest of the conversion runs on what reads.
use chimera_desktop::import::tape::convert::{Channel, clean, quantise_tpdf, read_wav, yin};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    for ch in [Channel::Mix, Channel::L, Channel::R] {
        let Ok(mut m) = read_wav(data, ch) else {
            return;
        };
        assert!(m.rate > 0);
        assert!(m.samples.iter().all(|s| s.is_finite()));
        if let Some(s) = m.smpl {
            assert!(s.unity <= 127);
        }
        clean(&mut m);
        assert!(m.samples.iter().all(|s| s.abs() <= 1.0));
        let _ = yin(&m.samples, m.rate);
        assert_eq!(quantise_tpdf(&m.samples, 0).len() % 2, 0);
    }
});
