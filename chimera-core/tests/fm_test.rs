use chimera_core::dsp::fm_tables;

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
