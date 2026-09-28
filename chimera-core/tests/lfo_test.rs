use chimera_core::dsp::lfo::{Lfo, LfoParams};

const SAMPLE_RATE: u32 = chimera_hal::SAMPLE_RATE;

#[test]
fn lfo_default_is_zero() {
    let lfo = Lfo::new();
    assert!(
        (lfo.current() - 0.0).abs() < 1e-6,
        "new LFO should output 0.0"
    );
}

#[test]
fn lfo_sine_output_range() {
    let mut lfo = Lfo::new();
    let params = LfoParams {
        shape: 0, // sine
        rate: 5.0,
        ..Default::default()
    };

    for _ in 0..1000 {
        let out = lfo.process(&params, SAMPLE_RATE);
        assert!(
            (-1.0..=1.0).contains(&out),
            "sine LFO output out of range: {}",
            out
        );
    }
}

#[test]
fn lfo_triangle_output_range() {
    let mut lfo = Lfo::new();
    let params = LfoParams {
        shape: 1, // triangle
        rate: 5.0,
        ..Default::default()
    };

    for _ in 0..1000 {
        let out = lfo.process(&params, SAMPLE_RATE);
        assert!(
            (-1.0..=1.0).contains(&out),
            "triangle LFO output out of range: {}",
            out
        );
    }
}

#[test]
fn lfo_saw_output_range() {
    let mut lfo = Lfo::new();
    let params = LfoParams {
        shape: 2, // saw
        rate: 5.0,
        ..Default::default()
    };

    for _ in 0..1000 {
        let out = lfo.process(&params, SAMPLE_RATE);
        assert!(
            (-1.0..=1.0).contains(&out),
            "saw LFO output out of range: {}",
            out
        );
    }
}

#[test]
fn lfo_square_output_bipolar() {
    let mut lfo = Lfo::new();
    let params = LfoParams {
        shape: 3, // square
        rate: 2.0,
        ..Default::default()
    };

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

    let params_slow = LfoParams {
        rate: 1.0,
        shape: 2, // saw (monotonic ramp -> easy to count wraps)
        ..Default::default()
    };

    let params_fast = LfoParams {
        rate: 10.0,
        shape: 2,
        ..Default::default()
    };

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

    let params_full = LfoParams {
        shape: 0, // sine
        rate: 3.0,
        depth: 1.0,
        ..Default::default()
    };

    let params_half = LfoParams {
        shape: 0,
        rate: 3.0,
        depth: 0.5,
        ..Default::default()
    };

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

/// Spec § LFO slots: OFFSET is stored but no longer applied.
#[test]
fn lfo_offset_is_not_applied() {
    let (mut a, mut b) = (Lfo::new(), Lfo::new());
    let p = LfoParams {
        offset: 0.5,
        depth: 0.5,
        rate: 5.0,
        ..Default::default()
    };
    let q = LfoParams { offset: 0.0, ..p };
    for _ in 0..500 {
        assert_eq!(a.process(&p, SAMPLE_RATE), b.process(&q, SAMPLE_RATE));
    }
}

#[test]
fn lfo_retrigger_resets_phase() {
    let mut lfo = Lfo::new();
    let params = LfoParams {
        shape: 2, // saw: output = phase*2 - 1, so phase=0 -> output=-1
        rate: 5.0,
        ..Default::default()
    };

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
