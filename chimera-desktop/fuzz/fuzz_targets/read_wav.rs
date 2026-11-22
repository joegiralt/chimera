#![no_main]

// Any bytes: an error or a finite source at a real rate, never a panic; the
// rest of the conversion runs on what reads.
use chimera_desktop::import::tape::Rate;
use chimera_desktop::import::tape::convert::{
    Channel, WAV_RATES, clean, quantise_tpdf, read_wav, resample, yin,
};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    for ch in [Channel::Mix, Channel::L, Channel::R] {
        let Ok(mut m) = read_wav(data, ch) else {
            return;
        };
        assert!(WAV_RATES.contains(&m.rate));
        assert!(m.samples.iter().all(|s| s.is_finite()));
        if let Some(s) = m.smpl {
            assert!(s.unity.is_none_or(|u| u <= 127));
            assert!(
                s.loop_frames
                    .is_none_or(|(a, b)| a <= b && (b as usize) < m.samples.len())
            );
        }
        clean(&mut m);
        assert!(m.samples.iter().all(|s| s.abs() <= 1.0));
        let _ = yin(&m.samples, m.rate);
        let to = Rate::ALL[usize::from(data[0]) % Rate::ALL.len()];
        let y = resample(&m.samples, m.rate, to.hz().into()).expect("both rates real");
        assert!(y.iter().all(|s| s.is_finite()));
        assert_eq!(quantise_tpdf(&y, 0).len() % 2, 0);
    }
});
