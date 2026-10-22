//! The tape delay (FX diet spec § Delay).

use chimera_core::dsp::delay::{DelayParams, TapeDelay};
use chimera_core::dsp::sin_turns;
use chimera_hal::BLOCK_SIZE;

#[test]
fn sin_turns_is_within_a_thousandth_of_sinf() {
    for i in 0..4096 {
        let p = i as f32 / 4096.0;
        let e = (sin_turns(p) - libm::sinf(2.0 * core::f32::consts::PI * p)).abs();
        assert!(e < 1e-3, "phase {p}: {e}");
    }
}

/// With no wow the read is exact, before and after the line wraps
/// (24,064 samples): each click comes back 480 samples later at MIX.
#[test]
fn with_no_wow_each_click_repeats_time_later() {
    let p = DelayParams {
        time_ms: 10.0,
        feedback: 0.0,
        wow_flutter: 0.0,
        saturation: 0.0,
        tone: 1.0,
        mix: 0.5,
        rev_send: 0.0,
        ..DelayParams::DEFAULT
    };
    let mut d = Box::new(TapeDelay::new());
    let mut out = Vec::new();
    for b in 0..500 {
        let mut block: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
            if (b * BLOCK_SIZE + i).is_multiple_of(1000) {
                1.0
            } else {
                0.0
            }
        });
        d.process_wet(&mut block, &p, 48_000);
        out.extend_from_slice(&block);
    }
    for (n, &s) in out.iter().enumerate() {
        let want = if n >= 480 && (n - 480).is_multiple_of(1000) {
            0.5
        } else {
            0.0
        };
        assert_eq!(s, want, "sample {n}");
    }
}

/// FX diet spec § REV SEND: a new id, 6 (0–5 are in use; ADR 0009), off by
/// default.
#[test]
fn rev_send_is_param_6_and_off_by_default() {
    use chimera_core::block::{Block, ParamId};
    use chimera_core::dsp::delay::DELAY_SPECS;
    let mut p = DelayParams::default();
    assert_eq!(DelayParams::REV_SEND, ParamId(6));
    assert_eq!(p.rev_send, 0.0);
    p.write(DelayParams::REV_SEND, 0.4);
    assert_eq!(p.get(DelayParams::REV_SEND), 0.4);
    let s = DELAY_SPECS
        .iter()
        .find(|s| s.id == DelayParams::REV_SEND)
        .unwrap();
    assert_eq!(
        (s.label, s.min, s.max, s.default, s.step),
        ("REV", 0.0, 1.0, 0.0, 1.0 / 128.0)
    );
}

/// The loop saturates even at SAT 0: held DC at FDBK 1 stays within
/// |input| + FDBK instead of adding a repeat every 10 ms.
#[test]
fn full_feedback_with_no_saturation_stays_bounded() {
    let p = DelayParams {
        time_ms: 10.0,
        feedback: 1.0,
        wow_flutter: 0.0,
        saturation: 0.0,
        tone: 1.0,
        mix: 1.0,
        rev_send: 0.0,
        ..DelayParams::DEFAULT
    };
    let mut d = Box::new(TapeDelay::new());
    let mut peak = 0.0f32;
    for _ in 0..(5 * 48_000 / BLOCK_SIZE) {
        let mut block = [0.5; BLOCK_SIZE];
        d.process_wet(&mut block, &p, 48_000);
        peak = block.iter().fold(peak, |m, s| m.max(s.abs()));
    }
    assert!(peak <= 1.5, "peak {peak}");
}

/// ADR 0090: truncation takes a tail to exact zero, where an `f32` line
/// decayed toward denormals.
#[test]
fn tail_reaches_exact_zero() {
    let p = DelayParams {
        feedback: 0.9,
        saturation: 0.2,
        ..DelayParams::default()
    };
    let mut d = Box::new(TapeDelay::new());
    let mut block = [0.0; BLOCK_SIZE];
    block[0] = 0.5;
    d.process_wet(&mut block, &p, 48_000);
    assert!(!d.is_silent());
    let blocks = 30 * 48_000 / BLOCK_SIZE;
    let ended = (0..blocks).position(|_| {
        d.process_wet(&mut [0.0; BLOCK_SIZE], &p, 48_000);
        d.is_silent()
    });
    let ended = ended.expect("the tail never reached zero");
    let ended_s = (ended + 2) as f32 * BLOCK_SIZE as f32 / 48_000.0;
    assert!(ended_s <= 17.0, "tail silent after {ended_s:.2} s");
}

/// The `f32` loop the line replaced, for one constant setting with no
/// MECHANICS and a whole-sample TIME: `TapeDelay::process_wet`'s order of
/// operations, so the two differ only by the line's quantisation.
struct F32Line {
    line: Vec<f32>,
    pos: usize,
    lp: f32,
}

impl F32Line {
    fn new(delay: usize) -> Self {
        Self {
            line: vec![0.0; delay],
            pos: 0,
            lp: 0.0,
        }
    }

    fn next(&mut self, dry: f32, p: &DelayParams) -> f32 {
        let delayed = self.line[self.pos];
        self.lp += (0.2 + p.tone * 0.75) * (delayed - self.lp);
        let g = 1.0 + p.saturation * 3.0;
        let saturated = libm::tanhf(self.lp * g) * (1.0 / g);
        self.line[self.pos] = dry + saturated * p.feedback;
        self.pos = (self.pos + 1) % self.line.len();
        delayed * p.mix
    }
}

/// The grain on a decaying tail, dBFS RMS against 1.0: a 10 ms loop
/// filled with a 0.25-peak 1 kHz tone, then silence. Each pass of the
/// `i16` loop is fitted to the `f32` loop's by one gain, which takes out
/// the repeat and the level truncation costs it; what is left over
/// `passes` is the grain.
fn grain_db(feedback: f32, tone: f32, passes: core::ops::Range<usize>) -> f64 {
    const PASS: usize = 480;
    let p = DelayParams {
        time_ms: 10.0,
        feedback,
        tone,
        wow_flutter: 0.0,
        mix: 1.0,
        ..DelayParams::default()
    };
    let mut d = Box::new(TapeDelay::new());
    let mut f = F32Line::new(PASS);
    let mut grain = 0.0f64;
    for pass in 0..passes.end {
        let (mut got, mut want) = (Vec::new(), Vec::new());
        for b in 0..PASS / BLOCK_SIZE {
            let mut block: [f32; BLOCK_SIZE] = core::array::from_fn(|i| {
                let t = (pass * PASS / BLOCK_SIZE + b) * BLOCK_SIZE + i;
                if t < PASS {
                    0.25 * sin_turns((t % 48) as f32 / 48.0)
                } else {
                    0.0
                }
            });
            want.extend(block.iter().map(|&x| f64::from(f.next(x, &p))));
            d.process_wet(&mut block, &p, 48_000);
            got.extend(block.iter().map(|&y| f64::from(y)));
        }
        // A pass the `f32` loop leaves silent has no repeat to fit.
        if passes.contains(&pass) && want.iter().any(|&w| w != 0.0) {
            let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
            let a = dot(&got, &want) / dot(&want, &want);
            grain += got
                .iter()
                .zip(&want)
                .map(|(y, w)| (y - a * w).powi(2))
                .sum::<f64>();
        }
    }
    10.0 * (grain / (passes.len() * PASS) as f64).log10()
}

/// The grain once feedback has summed it, with the tail still far above
/// it (ADR 0090): −83 dBFS at FDBK 0.9; at 0.99 −77 at the default TONE,
/// whose low-pass keeps the grain's highs from summing, and −65 to −69 at
/// TONE 1, the worst case.
#[test]
fn noise_floor_tracks_feedback() {
    let at_09 = grain_db(0.9, 0.6, 10..40);
    let at_099 = grain_db(0.99, 0.6, 50..200);
    let bright = grain_db(0.99, 1.0, 50..200);
    eprintln!("grain: FDBK 0.9 {at_09:.1}, 0.99 {at_099:.1}, 0.99 TONE 1 {bright:.1} dBFS");
    assert!((at_09 + 83.0).abs() <= 3.0, "FDBK 0.9: {at_09:.1} dBFS");
    assert!((at_099 + 77.0).abs() <= 3.0, "FDBK 0.99: {at_099:.1} dBFS");
    assert!(
        (bright + 66.0).abs() <= 3.0,
        "FDBK 0.99 TONE 1: {bright:.1} dBFS"
    );
}

/// ADR 0090: a +6 dB send hard-clips at ±2.0 on the write; the cast
/// saturates, so a held positive send never reads back negative.
#[test]
fn hot_send_clips_never_wraps() {
    let p = DelayParams {
        time_ms: 10.0,
        feedback: 1.0,
        saturation: 0.0,
        mix: 1.0,
        ..DelayParams::default()
    };
    let mut d = Box::new(TapeDelay::new());
    let mut peak = 0.0f32;
    for _ in 0..(48_000 / BLOCK_SIZE) {
        let mut block = [2.0; BLOCK_SIZE];
        d.process_wet(&mut block, &p, 48_000);
        for y in block {
            assert!(y.is_finite() && y >= 0.0, "{y}");
            peak = peak.max(y.abs());
        }
    }
    assert!(peak <= 2.0 + 1e-3, "peak {peak}");
}
