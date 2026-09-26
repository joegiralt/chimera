//! Spec § Voice model: TX81Z ratios and FINE steps, the 0.75 dB LEVEL step
//! with level 0 silent, D1L, FEEDBACK and DETUNE; and the `f32` math.

use chimera_core::dsp::algo::math::{exp2, inv_sqrt, log2};
use chimera_core::dsp::algo::tx::{
    COARSE, COARSE_NAMES, FEEDBACK_CYCLES, FINE_TOP, LEVEL_GAIN, d1l_level, detune_factor,
    level_gain, ratio,
};

fn db(ratio: f32) -> f64 {
    20.0 * (ratio as f64).log10()
}

#[test]
fn the_math_is_close_to_std() {
    for i in 0..60_000 {
        let x = -30.0 + i as f32 * 0.001;
        let r = exp2(x) as f64 / 2f64.powf(x as f64);
        assert!((r - 1.0).abs() < 2e-6, "exp2({x})");
    }
    let mut x = 1e-3f32;
    while x < 1e5 {
        assert!(
            (log2(x) as f64 - (x as f64).log2()).abs() < 3e-5,
            "log2({x})"
        );
        x *= 1.01;
    }
    let mut x = 1.0f32;
    while x <= 6.0 {
        assert!(
            (inv_sqrt(x) as f64 * (x as f64).sqrt() - 1.0).abs() < 1e-5,
            "{x}"
        );
        x += 0.01;
    }
}

#[test]
fn exp2_is_exact_at_whole_octaves() {
    for i in -20..20 {
        assert_eq!(exp2(i as f32), 2f32.powi(i));
    }
}

#[test]
fn coarse_is_the_tx81z_ratio_table() {
    assert_eq!(COARSE[0], 0.50);
    assert_eq!(COARSE[4], 1.00);
    assert_eq!(COARSE[8], 2.00);
    assert_eq!(COARSE[13], 4.00);
    assert_eq!(COARSE[63], 25.95);
    assert!(COARSE.windows(2).all(|w| w[0] < w[1]));
    for (i, (c, top)) in COARSE.iter().zip(FINE_TOP).enumerate() {
        assert!(top > *c, "coarse {i}");
        assert_eq!(COARSE_NAMES[i].parse::<f32>().unwrap(), *c);
    }
}

#[test]
fn fine_steps_evenly_toward_the_top_and_eight_below_coarse_4() {
    assert_eq!(ratio(4, 0), 1.0);
    assert!((ratio(4, 15) - FINE_TOP[4]).abs() < 1e-6);
    assert!((ratio(4, 1) - ratio(4, 0) - (FINE_TOP[4] - 1.0) / 15.0).abs() < 1e-6);
    assert!((ratio(0, 7) - FINE_TOP[0]).abs() < 1e-6);
    assert_eq!(ratio(0, 15), ratio(0, 7));
    assert_eq!(ratio(200, 0), COARSE[63]);
}

#[test]
fn level_steps_are_0_75_db_99_is_unity_and_0_is_silent() {
    assert_eq!(LEVEL_GAIN[99], 1.0);
    assert_eq!(LEVEL_GAIN[0], 0.0);
    for l in 2..100 {
        let step = db(LEVEL_GAIN[l] / LEVEL_GAIN[l - 1]);
        assert!((step - 0.75).abs() < 1e-3, "level {l}: {step} dB");
    }
}

#[test]
fn a_fractional_level_lies_between_its_steps() {
    let g = level_gain(50.5);
    assert!(g > LEVEL_GAIN[50] && g < LEVEL_GAIN[51]);
    assert_eq!(level_gain(-3.0), 0.0);
    assert!((level_gain(120.0) - 1.0).abs() < 1e-6);
}

#[test]
fn d1l_steps_are_3_db_and_0_decays_to_silence() {
    assert_eq!(d1l_level(15), 1.0);
    assert_eq!(d1l_level(0), 0.0);
    for d in 2..16 {
        let step = db(d1l_level(d) / d1l_level(d - 1));
        assert!((step - 3.0).abs() < 1e-3, "d1l {d}: {step} dB");
    }
    assert_eq!(d1l_level(40), d1l_level(15));
}

#[test]
fn feedback_doubles_per_step() {
    assert_eq!(FEEDBACK_CYCLES[0], 0.0);
    for n in 2..8 {
        assert_eq!(FEEDBACK_CYCLES[n], 2.0 * FEEDBACK_CYCLES[n - 1]);
    }
}

#[test]
fn detune_is_small_and_symmetric() {
    let cents = |d: i8| 1200.0 * (detune_factor(d) as f64).log2();
    assert_eq!(detune_factor(0), 1.0);
    assert!((cents(3) - 4.5).abs() < 0.01);
    assert!((cents(-3) + 4.5).abs() < 0.01);
    assert_eq!(detune_factor(9), detune_factor(3));
}
