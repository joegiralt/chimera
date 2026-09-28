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
