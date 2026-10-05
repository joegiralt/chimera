//! TAPE host conversion (spec § 6 steps 1–4 and 6): WAV in, 12-bit out.

use chimera_desktop::import::tape::convert::{
    Channel, Pitch, Smpl, clean, quantise_tpdf, read_wav, resample, yin,
};
use std::io::Cursor;

/// In f64: an f32 phase past a few thousand radians is a −40 dB noise floor.
fn sine(f: f32, rate: u32, secs: f32, amp: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    let w = std::f64::consts::TAU * f64::from(f) / f64::from(rate);
    (0..n).map(|i| amp * (w * i as f64).sin() as f32).collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

fn db(r: f32) -> f32 {
    20.0 * r.log10()
}

/// The middle 80 %, past the filter's edge transients.
fn middle(x: &[f32]) -> &[f32] {
    &x[x.len() / 10..x.len() * 9 / 10]
}

/// xorshift64*, white noise in −1..1.
fn noise(n: usize, mut s: u64) -> Vec<f32> {
    (0..n)
        .map(|_| {
            s ^= s >> 12;
            s ^= s << 25;
            s ^= s >> 27;
            let r = s.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40;
            r as f32 / (1u64 << 23) as f32 - 1.0
        })
        .collect()
}

fn wav<S: hound::Sample + Copy>(spec: hound::WavSpec, samples: &[S]) -> Vec<u8> {
    let mut c = Cursor::new(Vec::new());
    let mut w = hound::WavWriter::new(&mut c, spec).unwrap();
    for &s in samples {
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
    c.into_inner()
}

fn spec(channels: u16, bits: u16, format: hound::SampleFormat) -> hound::WavSpec {
    hound::WavSpec {
        channels,
        sample_rate: 48_000,
        bits_per_sample: bits,
        sample_format: format,
    }
}

#[test]
fn alias_band_is_80_db_down() {
    for (from, to) in [(48_000, 24_000), (44_100, 16_000)] {
        for k in [0.5, 0.6, 0.8] {
            let x = sine(k * to as f32, from, 1.0, 0.5);
            let y = resample(&x, from, to);
            let down = db(rms(middle(&y)) / rms(&x));
            println!("{from} → {to}, {k} × to: {down:.1} dB");
            assert!(down <= -80.0, "{from} → {to}, {k} × to: {down:.1} dB");
        }
        for k in [0.05, 0.1, 0.2, 0.3] {
            let x = sine(k * to as f32, from, 1.0, 0.5);
            let y = resample(&x, from, to);
            assert!((y.len() as f32 - x.len() as f32 * to as f32 / from as f32).abs() <= 2.0);
            let gain = db(rms(middle(&y)) / rms(&x));
            assert!(gain.abs() <= 1.0, "{from} → {to}, {k} × to: {gain:.2} dB");
        }
    }
}

/// A band-limited saw: every harmonic under Nyquist, 1/k.
fn saw(f: f32, rate: u32, secs: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    let top = ((rate as f32 / 2.0) / f) as usize;
    let w = std::f64::consts::TAU * f64::from(f) / f64::from(rate);
    (0..n)
        .map(|i| {
            let ph = w * i as f64;
            0.3 * (1..=top)
                .map(|k| (k as f64 * ph).sin() / k as f64)
                .sum::<f64>() as f32
        })
        .collect()
}

#[test]
fn yin_finds_the_period() {
    let x = saw(220.5, 24_000, 0.5);
    match yin(&x, 24_000) {
        Pitch::Pitched { period, root_cents } => {
            let want = 24_000.0 / 220.5;
            assert!((period - want).abs() < 0.01, "period {period}, want {want}");
            assert!(root_cents.abs_diff(5_704) <= 1, "root {root_cents}");
        }
        Pitch::Unpitched => panic!("saw read as unpitched"),
    }
}

#[test]
fn noise_is_unpitched() {
    assert_eq!(yin(&noise(24_000, 7), 24_000), Pitch::Unpitched);
    assert_eq!(yin(&[0.0; 4_800], 48_000), Pitch::Unpitched);
    assert_eq!(yin(&[], 48_000), Pitch::Unpitched);
}

/// Appends a `smpl` chunk (spec: 36 B header, 24 B a loop) and fixes the RIFF size.
fn with_smpl(mut b: Vec<u8>, unity: u32, fraction: u32, lp: (u32, u32)) -> Vec<u8> {
    b.extend_from_slice(b"smpl");
    b.extend_from_slice(&60u32.to_le_bytes());
    for v in [
        0, 0, 20_833, unity, fraction, 0, 0, 1, 0, 0, 0, lp.0, lp.1, 0, 0,
    ] {
        b.extend_from_slice(&u32::to_le_bytes(v));
    }
    let riff = (b.len() - 8) as u32;
    b[4..8].copy_from_slice(&riff.to_le_bytes());
    b
}

#[test]
fn smpl_root_is_read() {
    let plain = wav(spec(1, 16, hound::SampleFormat::Int), &[0i16; 1_000]);
    assert_eq!(read_wav(&plain, Channel::Mix).unwrap().smpl, None);
    let b = with_smpl(plain, 48, 0x8000_0000, (100, 900));
    let m = read_wav(&b, Channel::Mix).unwrap();
    assert_eq!(m.rate, 48_000);
    assert_eq!(m.samples.len(), 1_000);
    assert_eq!(
        m.smpl,
        Some(Smpl {
            unity: 48,
            fraction: 0x8000_0000,
            loop_frames: Some((100, 900))
        })
    );
}

#[test]
fn clean_trims_and_normalises() {
    // 1 kHz at 48 kHz: whole cycles, so every part has zero mean; the 0.1 DC is the only DC.
    let quiet = sine(1_000.0, 48_000, 0.2, 10f32.powf(-70.0 / 20.0));
    let loud = sine(1_000.0, 48_000, 1.0, 10f32.powf(-12.0 / 20.0));
    let x: Vec<f32> = [&quiet[..], &loud, &quiet]
        .concat()
        .iter()
        .map(|s| s + 0.1)
        .collect();
    let mut m = chimera_desktop::import::tape::convert::Mono {
        rate: 48_000,
        samples: x,
        smpl: None,
    };
    clean(&mut m);
    // The lead is cut to the onset; the tail keeps the high-pass's own ring
    // (−58 dBFS, 32 ms time constant) until it falls under −60 dBFS.
    let n = m.samples.len();
    assert!((48_000..48_000 + 1_200).contains(&n), "len {n}");
    let peak = m.samples.iter().fold(0f32, |a, s| a.max(s.abs()));
    assert!((db(peak) + 0.5).abs() <= 0.01, "peak {} dB", db(peak));
    let dc = m.samples.iter().map(|&s| s as f64).sum::<f64>() / m.samples.len() as f64;
    assert!(db(dc.abs() as f32) < -90.0, "DC {} dB", db(dc.abs() as f32));
}

#[test]
fn stereo_mixes_or_picks() {
    let b = wav(
        spec(2, 32, hound::SampleFormat::Float),
        &[1.0f32, -1.0, 1.0, -1.0],
    );
    for (ch, want) in [(Channel::Mix, 0.0), (Channel::L, 1.0), (Channel::R, -1.0)] {
        assert_eq!(read_wav(&b, ch).unwrap().samples, vec![want; 2]);
    }
}

#[test]
fn pcm_depths_scale_to_one() {
    let i = hound::SampleFormat::Int;
    let cases = [
        wav(spec(1, 8, i), &[i8::MIN, 64]),
        wav(spec(1, 16, i), &[i16::MIN, 1 << 14]),
        wav(spec(1, 24, i), &[-(1i32 << 23), 1 << 22]),
        wav(spec(1, 32, i), &[i32::MIN, 1 << 30]),
    ];
    for b in cases {
        assert_eq!(read_wav(&b, Channel::Mix).unwrap().samples, vec![-1.0, 0.5]);
    }
    assert!(read_wav(b"RIFF", Channel::Mix).is_err());
    let nan = wav(spec(1, 32, hound::SampleFormat::Float), &[0.0f32, f32::NAN]);
    assert!(read_wav(&nan, Channel::Mix).is_err());
}

#[test]
fn quantise_is_deterministic() {
    let x = noise(10_000, 3).iter().map(|s| s * 0.9).collect::<Vec<_>>();
    let a = quantise_tpdf(&x, 42);
    assert_eq!(a, quantise_tpdf(&x, 42));
    assert_ne!(a, quantise_tpdf(&x, 43));
    for (q, s) in a.iter().zip(&x) {
        assert!((-2048..=2047).contains(q));
        assert!((*q as f32 - s * 2048.0).abs() <= 1.5, "{q} for {s}");
    }
    let rail = quantise_tpdf(&[1.0, -1.0], 0);
    assert_eq!(rail, vec![2047, -2048]);
}

#[test]
fn frames_are_even() {
    for (n, want) in [(0, 0), (1, 2), (2, 2), (3, 4), (4_999, 5_000)] {
        let q = quantise_tpdf(&noise(n, 9), 1);
        assert_eq!(q.len(), want);
        // An odd source repeats its last sample, so a wrap never clicks through a 0.
        if n % 2 == 1 {
            assert_eq!(q[want - 1], q[want - 2]);
        }
    }
}
