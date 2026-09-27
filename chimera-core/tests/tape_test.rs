//! Tape on DAC pair 1 (FX diet spec § Tape, § Testing).

use chimera_core::dsp::tape::{DRY_TAP, ENGAGE, Tape, TapeParams};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

type Pair = [f32; 2 * BLOCK_SIZE];

/// xorshift noise in ±`amp`, the same on every run.
fn noise(n: usize, amp: f32) -> Vec<f32> {
    let mut x = 0x2545_f491u32;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
        })
        .collect()
}

fn sine(n: usize, hz: f32, amp: f32) -> Vec<f32> {
    (0..n)
        .map(|i| amp * libm::sinf(2.0 * core::f32::consts::PI * hz * i as f32 / SR as f32))
        .collect()
}

/// `x` on both sides through `tape`, `p(block)` each block; the left side.
fn run(tape: &mut Tape, x: &[f32], p: impl Fn(usize) -> TapeParams) -> Vec<f32> {
    let mut out = Vec::with_capacity(x.len());
    for (b, chunk) in x.as_chunks::<BLOCK_SIZE>().0.iter().enumerate() {
        let mut pair: Pair = core::array::from_fn(|i| chunk[i / 2]);
        tape.process(&mut pair, &p(b), SR);
        out.extend((0..BLOCK_SIZE).map(|i| pair[2 * i]));
    }
    out
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

fn at(drive: f32, tone: f32, mix: f32) -> TapeParams {
    TapeParams {
        drive,
        tone,
        wow: 0.0,
        mix,
    }
}

/// Spec § Testing: MIX 0 (DRIVE 0 or not) is an exact bypass.
#[test]
fn mix_0_is_an_exact_bypass() {
    let x = noise(64 * BLOCK_SIZE, 0.9);
    for p in [TapeParams::default(), at(1.0, 0.0, 0.0)] {
        let mut tape = Box::new(Tape::new());
        assert_eq!(run(&mut tape, &x, |_| p), x, "{p:?}");
    }
}

/// Back to MIX 0, the tape fades out over `ENGAGE` samples and then passes
/// every block untouched.
#[test]
fn it_releases_back_to_an_exact_bypass() {
    let x = noise(64 * BLOCK_SIZE, 0.5);
    let mut tape = Box::new(Tape::new());
    let out = run(&mut tape, &x, |b| {
        if b < 16 {
            at(1.0, 0.5, 1.0)
        } else {
            at(1.0, 0.5, 0.0)
        }
    });
    assert_ne!(out[..16 * BLOCK_SIZE], x[..16 * BLOCK_SIZE]);
    let done = 16 * BLOCK_SIZE + ENGAGE as usize;
    assert_eq!(out[done..], x[done..]);
}

/// The dry tap lines up with the wet: at MIX 1 an impulse comes out
/// `DRY_TAP` samples later, and at MIX ½ there is one peak, not two.
#[test]
fn the_wet_lines_up_with_the_dry_tap() {
    for mix in [1.0, 0.5] {
        let mut tape = Box::new(Tape::new());
        let mut x = vec![0.0; 32 * BLOCK_SIZE];
        let at_n = 16 * BLOCK_SIZE;
        x[at_n] = 0.01;
        let out = run(&mut tape, &x, |_| at(0.0, 1.0, mix));
        let peak = (at_n..at_n + 64)
            .max_by(|&a, &b| out[a].abs().total_cmp(&out[b].abs()))
            .unwrap();
        assert_eq!(peak - at_n, DRY_TAP, "MIX {mix}");
    }
}

/// Small-signal gain of a sine at `hz`, dB.
fn gain_db(p: TapeParams, hz: f32) -> f32 {
    let n = SR as usize / BLOCK_SIZE * BLOCK_SIZE;
    let x = sine(n, hz, 0.01);
    let mut tape = Box::new(Tape::new());
    let out = run(&mut tape, &x, |_| p);
    db(rms(&out[n / 2..]) / rms(&x[n / 2..]))
}

/// Spec § Testing: the head bump shows in the response, about +2 dB at
/// 80 Hz over 1 kHz.
#[test]
fn the_head_bump_shows_at_80_hz() {
    let p = at(0.0, 1.0, 1.0);
    let bump = gain_db(p, 80.0) - gain_db(p, 1_000.0);
    assert!((1.5..=2.5).contains(&bump), "{bump} dB");
}

/// Pre- and de-emphasis cancel: at DRIVE 0 and TONE 1 the small-signal
/// response is flat within 1 dB from 300 Hz to 5 kHz.
#[test]
fn the_emphasis_pair_is_flat_at_small_signal() {
    let p = at(0.0, 1.0, 1.0);
    let ref_db = gain_db(p, 1_000.0);
    for hz in [300.0, 2_000.0, 3_000.0, 5_000.0] {
        let d = gain_db(p, hz) - ref_db;
        assert!(d.abs() < 1.0, "{hz} Hz: {d} dB");
    }
}

/// TONE darkens and brightens: 8 kHz sits lower at TONE 0 than at 1.
#[test]
fn tone_moves_the_top() {
    let (dark, bright) = (
        gain_db(at(0.0, 0.0, 1.0), 8_000.0),
        gain_db(at(0.0, 1.0, 1.0), 8_000.0),
    );
    assert!(bright - dark > 4.0, "{dark} vs {bright}");
}

fn crest_db(x: &[f32]) -> f32 {
    db(x.iter().fold(0.0f32, |m, s| m.max(s.abs())) / rms(x))
}

/// Decaying 220 Hz plucks, one every 100 ms, peaking at `amp`.
fn plucks(n: usize, amp: f32) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let t = (i % 4_800) as f32 / SR as f32;
            amp * libm::expf(-30.0 * t) * libm::sinf(2.0 * core::f32::consts::PI * 220.0 * t)
        })
        .collect()
}

/// Spec § Testing: DRIVE lowers the crest factor of peaky material, and
/// the level compensation holds its RMS within 3 dB.
#[test]
fn drive_lowers_the_crest_factor_at_a_held_level() {
    let n = 256 * BLOCK_SIZE;
    let x = plucks(n, 0.7);
    let render = |d: f32| {
        let mut tape = Box::new(Tape::new());
        run(&mut tape, &x, |_| at(d, 0.5, 1.0))[n / 4..].to_vec()
    };
    let clean = render(0.0);
    let (c0, mut last) = (crest_db(&clean), crest_db(&clean));
    for d in [0.5, 1.0] {
        let driven = render(d);
        let c = crest_db(&driven);
        assert!(last - c >= 2.0, "DRIVE {d}: crest {last} → {c} dB");
        last = c;
        let level = db(rms(&driven) / rms(&clean));
        assert!(level.abs() <= 3.0, "DRIVE {d}: {level} dB");
    }
    assert!(c0 - last >= 6.0, "{c0} → {last}");
}

/// Power of `x` at `hz` (an exact bin: `x` is a whole second).
fn power_at(x: &[f32], hz: f32) -> f32 {
    let w = 2.0 * core::f64::consts::PI * hz as f64 / SR as f64;
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, &s) in x.iter().enumerate() {
        re += s as f64 * (w * i as f64).cos();
        im += s as f64 * (w * i as f64).sin();
    }
    ((re * re + im * im) / (x.len() as f64).powi(2)) as f32
}

/// Where the base-rate output carries harmonic `k` of `f0`: folded at the
/// 2× rate, then at the base rate.
fn folded(k: u32, f0: u32) -> u32 {
    let fold = |f: u32, fs: u32| {
        let f = f % fs;
        if f > fs / 2 { fs - f } else { f }
    };
    fold(fold(k * f0, 2 * SR), SR)
}

/// Spec § Testing: the tape's aliasing is below the 15-tap half-band's
/// 49 dB stopband. A 1,003 Hz sine at −12 dBFS, full DRIVE, TONE 1: every
/// alias of harmonics 1..=200 landing in 20 Hz–20 kHz sums to at least
/// 49 dB under the fundamental (measured −84 dB).
#[test]
fn aliasing_stays_below_the_stopband() {
    const F0: u32 = 1_003;
    let n = 2 * SR as usize;
    let x = sine(n, F0 as f32, 0.25);
    let mut tape = Box::new(Tape::new());
    let out = run(&mut tape, &x, |_| at(1.0, 1.0, 1.0));
    let sec = &out[SR as usize..];
    let fund = power_at(sec, F0 as f32);
    let mut aliases: Vec<u32> = (1..=200)
        .filter(|&k| k * F0 > SR / 2)
        .map(|k| folded(k, F0))
        .filter(|&f| (20..=20_000).contains(&f) && f % F0 != 0)
        .collect();
    aliases.sort();
    aliases.dedup();
    let alias: f32 = aliases.iter().map(|&f| power_at(sec, f as f32)).sum();
    let rel = 10.0 * (alias / fund).log10();
    assert!(rel <= -49.0, "{rel} dB");
}

/// WOW moves the pitch: WOW 1 differs from WOW 0.
#[test]
fn wow_reaches_the_wet() {
    let x = sine(64 * BLOCK_SIZE, 440.0, 0.25);
    let render = |wow: f32| {
        let mut tape = Box::new(Tape::new());
        run(&mut tape, &x, |_| TapeParams {
            wow,
            ..at(0.3, 0.5, 1.0)
        })
    };
    assert_ne!(render(0.0), render(1.0));
}

/// Largest second difference in `x`.
fn kink(x: &[f32]) -> f32 {
    x.windows(3)
        .fold(0.0f32, |m, w| m.max((w[2] - 2.0 * w[1] + w[0]).abs()))
}

/// Spec § Testing: no clicks when a control sweeps. Over the 50 ms after a
/// jump the largest second difference is at most 1.5× the larger of two
/// renders held at the old and the new value.
#[test]
fn a_control_jump_does_not_click() {
    let n = 96 * BLOCK_SIZE;
    let x = sine(n, 220.0, 0.5);
    let (at_b, win) = (24, 2_400);
    let cases: [(&str, TapeParams, TapeParams); 6] = [
        ("DRIVE", at(0.0, 0.5, 1.0), at(1.0, 0.5, 1.0)),
        ("TONE", at(0.5, 0.0, 1.0), at(0.5, 1.0, 1.0)),
        (
            "WOW",
            at(0.5, 0.5, 1.0),
            TapeParams {
                wow: 1.0,
                ..at(0.5, 0.5, 1.0)
            },
        ),
        ("MIX on", at(0.5, 0.5, 0.0), at(0.5, 0.5, 1.0)),
        ("MIX half", at(1.0, 0.5, 1.0), at(1.0, 0.5, 0.5)),
        ("MIX off", at(0.5, 0.5, 1.0), at(0.5, 0.5, 0.0)),
    ];
    for (name, a, b) in cases {
        let render = |f: &dyn Fn(usize) -> TapeParams| {
            let mut tape = Box::new(Tape::new());
            run(&mut tape, &x, f)
        };
        let moved = render(&|blk| if blk < at_b { a } else { b });
        let (held_a, held_b) = (render(&|_| a), render(&|_| b));
        let w = at_b * BLOCK_SIZE..at_b * BLOCK_SIZE + win;
        let bound = kink(&held_a[w.clone()]).max(kink(&held_b[w.clone()]));
        let k = kink(&moved[w]);
        assert!(k <= 1.5 * bound, "{name}: {k} vs {bound}");
    }
}

/// A NaN or out-of-range control renders finite, and the tape comes back.
#[test]
fn nan_and_out_of_range_controls_render_finite() {
    let x = noise(32 * BLOCK_SIZE, 0.5);
    let mut tape = Box::new(Tape::new());
    let bad = TapeParams {
        drive: f32::NAN,
        tone: 7.0,
        wow: -3.0,
        mix: f32::NAN,
    };
    let out = run(
        &mut tape,
        &x,
        |b| if b < 16 { bad } else { at(0.5, 0.5, 1.0) },
    );
    assert!(out.iter().all(|s| s.is_finite()));
    assert_ne!(out[16 * BLOCK_SIZE..], x[16 * BLOCK_SIZE..]);
}

/// Built in place, the tape is `new()`'s.
#[test]
fn built_in_place_it_processes_like_new() {
    let x = noise(16 * BLOCK_SIZE, 0.5);
    let mut slot = Box::new(core::mem::MaybeUninit::<Tape>::uninit());
    let placed = Tape::init_in_place(&mut slot);
    let p = |_| at(0.7, 0.3, 0.8);
    assert_eq!(run(placed, &x, p), run(&mut Box::new(Tape::new()), &x, p));
}

/// The divide-free soft clip: slope 1 at 0, monotonic, ±1 from |v| = 1.5.
/// Its compare-free clamp rounds to within 2^-22.
#[test]
fn the_soft_clip_is_unity_then_flat() {
    use chimera_core::dsp::tape::soft_clip;
    for v in [1e-6, 1e-3, 0.01] {
        assert!((soft_clip(v) - v).abs() < 1e-6 + 0.2 * v * v, "{v}");
    }
    assert_eq!((soft_clip(1.5), soft_clip(-9.0)), (1.0, -1.0));
    let mut last = -1.0;
    for i in -300..=300 {
        let y = soft_clip(i as f32 / 100.0);
        assert!(y >= last - 1e-6 && y.abs() <= 1.0 + 1e-6, "{i}");
        last = y;
    }
}

/// Spec § Tape: the 15-tap half-band is ±0.05 dB to 16 kHz and 49 dB down
/// from 32 kHz, at 96 kHz.
#[test]
fn the_half_band_meets_its_bands() {
    use chimera_core::dsp::tape::HB;
    let h = |hz: f64| {
        let w = 2.0 * core::f64::consts::PI * hz / 96_000.0;
        let s: f64 = HB
            .iter()
            .enumerate()
            .map(|(j, &c)| 2.0 * c as f64 * ((2 * j + 1) as f64 * w).cos())
            .sum();
        20.0 * (0.5 + s).abs().log10()
    };
    for k in 0..=160 {
        let (pass, stop) = (k as f64 * 100.0, 32_000.0 + k as f64 * 100.0);
        assert!(h(pass).abs() < 0.05, "{pass} Hz: {} dB", h(pass));
        assert!(h(stop) < -49.0, "{stop} Hz: {} dB", h(stop));
    }
}
