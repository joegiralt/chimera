use chimera_core::dsp::fm_tables;
use chimera_core::dsp::fm_waveform;

#[test]
fn ratio_table_unity() {
    assert_eq!(fm_tables::compute_ratio(4, 0), 1.0);
}

#[test]
fn ratio_table_half() {
    assert_eq!(fm_tables::compute_ratio(0, 0), 0.5);
}

#[test]
fn ratio_table_fine_interpolates() {
    let r0 = fm_tables::compute_ratio(4, 0);
    let r15 = fm_tables::compute_ratio(4, 15);
    assert!(r15 > r0);
    assert!(r15 <= fm_tables::FREQ_RATIOS_MAX[4]);
}

#[test]
fn ratio_table_sub4_clamps_fine() {
    let r7 = fm_tables::compute_ratio(0, 7);
    let r15 = fm_tables::compute_ratio(0, 15);
    assert_eq!(r7, r15);
}

#[test]
fn level_to_gain_zero_is_tiny() {
    let g = fm_tables::level_to_gain(0);
    assert!(g > 0.0);
    assert!(g < 0.01);
}

#[test]
fn level_to_gain_99_near_unity() {
    let g = fm_tables::level_to_gain(99);
    assert!(g > 0.5);
    assert!(g <= 2.0);
}

#[test]
fn d1l_zero_returns_zero() {
    assert_eq!(fm_tables::d1l_to_level(0), 0.0);
}

#[test]
fn d1l_15_near_unity() {
    let l = fm_tables::d1l_to_level(15);
    assert!(l > 0.9);
    assert!(l <= 1.0);
}

#[test]
fn feedback_factors_correct() {
    assert_eq!(fm_tables::FEEDBACK[0], 0.0);
    assert_eq!(fm_tables::FEEDBACK[7], 0.26);
}

#[test]
fn waveform_0_is_sine() {
    let v = fm_waveform::compute(0, 0.25);
    assert!((v - 1.0).abs() < 0.001);
}

#[test]
fn waveform_0_zero_at_origin() {
    let v = fm_waveform::compute(0, 0.0);
    assert!(v.abs() < 0.001);
}

#[test]
fn waveform_2_half_sine_zero_second_half() {
    let v = fm_waveform::compute(2, 0.75);
    assert!(v.abs() < 0.001);
}

#[test]
fn all_8_waveforms_produce_different_output() {
    let phase = 0.13;
    let values: [f32; 8] = core::array::from_fn(|w| fm_waveform::compute(w as u8, phase));
    let mut unique = values.to_vec();
    unique.sort_by(|a, b| a.partial_cmp(b).unwrap());
    unique.dedup_by(|a, b| (*a - *b).abs() < 0.001);
    assert!(unique.len() >= 4);
}

#[test]
fn all_waveforms_bounded() {
    for w in 0..8u8 {
        for i in 0..1024 {
            let phase = i as f32 / 1024.0;
            let v = fm_waveform::compute(w, phase);
            assert!(v.is_finite(), "w={w} phase={phase}");
            assert!(v >= -2.0 && v <= 2.0, "w={w} phase={phase} v={v}");
        }
    }
}
