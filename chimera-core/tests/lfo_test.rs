use chimera_core::dsp::lfo::{Lfo, LfoParams};

const SAMPLE_RATE: u32 = chimera_hal::SAMPLE_RATE;

#[test]
fn lfo_default_is_zero() {
    let lfo = Lfo::new();
    assert!((lfo.current() - 0.0).abs() < 1e-6, "new LFO should output 0.0");
}

#[test]
fn lfo_sine_output_range() {
    let mut lfo = Lfo::new();
    let mut params = LfoParams::default();
    params.shape = 0; // sine
    params.rate = 5.0;

    for _ in 0..1000 {
        let out = lfo.process(&params, SAMPLE_RATE);
        assert!(
            out >= -1.0 && out <= 1.0,
            "sine LFO output out of range: {}",
            out
        );
    }
}

#[test]
fn lfo_triangle_output_range() {
    let mut lfo = Lfo::new();
    let mut params = LfoParams::default();
    params.shape = 1; // triangle
    params.rate = 5.0;

    for _ in 0..1000 {
        let out = lfo.process(&params, SAMPLE_RATE);
        assert!(
            out >= -1.0 && out <= 1.0,
            "triangle LFO output out of range: {}",
            out
        );
    }
}

#[test]
fn lfo_saw_output_range() {
    let mut lfo = Lfo::new();
    let mut params = LfoParams::default();
    params.shape = 2; // saw
    params.rate = 5.0;

    for _ in 0..1000 {
        let out = lfo.process(&params, SAMPLE_RATE);
        assert!(
            out >= -1.0 && out <= 1.0,
            "saw LFO output out of range: {}",
            out
        );
    }
}

#[test]
fn lfo_square_output_bipolar() {
    let mut lfo = Lfo::new();
    let mut params = LfoParams::default();
    params.shape = 3; // square
    params.rate = 2.0;

    for _ in 0..1000 {
        let out = lfo.process(&params, SAMPLE_RATE);
        assert!(
            (out - 1.0).abs() < 1e-6 || (out - (-1.0)).abs() < 1e-6,
            "square LFO should output exactly +1.0 or -1.0, got {}",
            out
        );
    }
}

#[test]
fn lfo_rate_affects_speed() {
    // Process two LFOs: one slow, one fast. The fast one should complete more cycles.
    let mut lfo_slow = Lfo::new();
    let mut lfo_fast = Lfo::new();

    let mut params_slow = LfoParams::default();
    params_slow.rate = 1.0;
    params_slow.shape = 2; // saw (monotonic ramp -> easy to count wraps)

    let mut params_fast = LfoParams::default();
    params_fast.rate = 10.0;
    params_fast.shape = 2;

    // Count zero crossings (negative to positive) as proxy for cycles
    let mut prev_slow = 0.0f32;
    let mut prev_fast = 0.0f32;
    let mut wraps_slow = 0u32;
    let mut wraps_fast = 0u32;

    for _ in 0..2000 {
        let s = lfo_slow.process(&params_slow, SAMPLE_RATE);
        let f = lfo_fast.process(&params_fast, SAMPLE_RATE);

        // Saw wraps from +1 back to -1
        if s < prev_slow - 0.5 {
            wraps_slow += 1;
        }
        if f < prev_fast - 0.5 {
            wraps_fast += 1;
        }
        prev_slow = s;
        prev_fast = f;
    }

    assert!(
        wraps_fast > wraps_slow,
        "faster rate should cycle more: slow={} fast={}",
        wraps_slow,
        wraps_fast
    );
}

#[test]
fn lfo_depth_scales_output() {
    let mut lfo_full = Lfo::new();
    let mut lfo_half = Lfo::new();

    let mut params_full = LfoParams::default();
    params_full.shape = 0; // sine
    params_full.rate = 3.0;
    params_full.depth = 1.0;

    let mut params_half = LfoParams::default();
    params_half.shape = 0;
    params_half.rate = 3.0;
    params_half.depth = 0.5;

    let mut max_full = 0.0f32;
    let mut max_half = 0.0f32;

    for _ in 0..1000 {
        let f = lfo_full.process(&params_full, SAMPLE_RATE);
        let h = lfo_half.process(&params_half, SAMPLE_RATE);
        max_full = max_full.max(f.abs());
        max_half = max_half.max(h.abs());
    }

    assert!(
        max_half < max_full * 0.7,
        "depth=0.5 should produce roughly half amplitude: full_max={} half_max={}",
        max_full,
        max_half
    );
}

#[test]
fn lfo_offset_shifts_output() {
    let mut lfo = Lfo::new();
    let mut params = LfoParams::default();
    params.shape = 0; // sine
    params.rate = 3.0;
    params.depth = 0.5;
    params.offset = 0.5;

    let mut min_out = f32::MAX;
    let mut max_out = f32::MIN;

    for _ in 0..1000 {
        let out = lfo.process(&params, SAMPLE_RATE);
        min_out = min_out.min(out);
        max_out = max_out.max(out);
    }

    // depth=0.5 raw range is -0.5..0.5, offset=0.5 shifts to 0.0..1.0
    assert!(
        min_out >= -1.0 && max_out <= 1.0,
        "output should be clamped to -1..1: min={} max={}",
        min_out,
        max_out
    );
    // With offset=0.5 and depth=0.5, minimum should be around 0.0
    assert!(
        min_out > -0.1,
        "offset should shift minimum up: min={}",
        min_out
    );
    // Maximum should be around 1.0
    assert!(
        max_out > 0.8,
        "offset should shift maximum up: max={}",
        max_out
    );
}

#[test]
fn lfo_retrigger_resets_phase() {
    let mut lfo = Lfo::new();
    let mut params = LfoParams::default();
    params.shape = 2; // saw: output = phase*2 - 1, so phase=0 -> output=-1
    params.rate = 5.0;

    // Advance a few blocks
    for _ in 0..100 {
        lfo.process(&params, SAMPLE_RATE);
    }

    // Retrigger to phase 0
    lfo.retrigger(0.0);

    // Next process should start from phase 0
    let out = lfo.process(&params, SAMPLE_RATE);
    // At phase=0, saw outputs 0*2 - 1 = -1.0
    assert!(
        (out - (-1.0)).abs() < 0.05,
        "after retrigger(0.0), saw should start near -1.0, got {}",
        out
    );
}
