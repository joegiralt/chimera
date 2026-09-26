use chimera_core::audio_out::{
    DacSample, Half, HalfPlan, desynced, interleave, plan_halves, to_dac,
};
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS};
use chimera_core::instrument::DacOut;
use chimera_core::part::DacPair;

#[test]
fn full_scale_is_the_24_bit_extremes_left_justified() {
    assert_eq!(to_dac(1.0).get(), 0x7FFF_FF00);
    assert_eq!(to_dac(-1.0).get(), -0x7FFF_FF00);
    assert_eq!(to_dac(0.0), DacSample::ZERO);
    assert_eq!(DacSample::ZERO.get(), 0);
}

#[test]
fn out_of_range_clamps_to_full_scale() {
    assert_eq!(to_dac(1.5), to_dac(1.0));
    assert_eq!(to_dac(-7.0), to_dac(-1.0));
}

#[test]
fn rounds_to_the_nearest_24_bit_step() {
    assert_eq!(to_dac(0.5).get(), 4_194_304 << 8);
    assert_eq!(to_dac(-0.5).get(), -(4_194_304 << 8));
    assert_eq!(to_dac(1.0 / 8_388_607.0).get(), 1 << 8);
}

#[test]
fn the_low_byte_is_always_zero() {
    for i in -1000..=1000 {
        let x = i as f32 / 997.0;
        assert_eq!(to_dac(x).get() & 0xFF, 0, "{x}");
    }
}

#[test]
fn non_finite_input_is_silent_or_clamped() {
    assert_eq!(to_dac(f32::NAN), DacSample::ZERO);
    assert_eq!(to_dac(f32::INFINITY), to_dac(1.0));
    assert_eq!(to_dac(f32::NEG_INFINITY), to_dac(-1.0));
}

#[test]
fn interleave_converts_one_pair_in_slot_order() {
    let mut dac: DacOut = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
    for (p, pair) in dac.iter_mut().enumerate() {
        for (i, s) in pair.iter_mut().enumerate() {
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            *s = sign * (p as f32 + 1.0) * 0.1 + i as f32 * 1e-4;
        }
    }
    for pair in DacPair::ALL {
        let mut out = [DacSample::ZERO; BLOCK_SIZE * 2];
        interleave(&dac, pair, &mut out);
        for i in 0..BLOCK_SIZE * 2 {
            assert_eq!(out[i], to_dac(dac[pair.index()][i]), "{pair:?} word {i}");
        }
    }
}

#[test]
fn dac_pairs_in_order() {
    assert_eq!(DacPair::ALL, [DacPair::P1, DacPair::P2, DacPair::P3]);
    assert_eq!((Half::First.index(), Half::Second.index()), (0, 1));
}

#[test]
fn half_transfer_renders_the_first_half() {
    assert_eq!(
        plan_halves(true, false),
        HalfPlan {
            halves: [Some(Half::First), None],
            overrun: false
        }
    );
}

#[test]
fn transfer_complete_renders_the_second_half() {
    assert_eq!(
        plan_halves(false, true),
        HalfPlan {
            halves: [Some(Half::Second), None],
            overrun: false
        }
    );
}

#[test]
fn both_flags_render_both_halves_oldest_first_and_count_one_overrun() {
    assert_eq!(
        plan_halves(true, true),
        HalfPlan {
            halves: [Some(Half::First), Some(Half::Second)],
            overrun: true
        }
    );
}

#[test]
fn no_flag_renders_nothing() {
    assert_eq!(
        plan_halves(false, false),
        HalfPlan {
            halves: [None, None],
            overrun: false
        }
    );
}

#[test]
fn desync_tolerates_16_words_either_way_across_the_wrap() {
    assert!(!desynced(128, 128, 256, 16));
    assert!(!desynced(128, 144, 256, 16));
    assert!(!desynced(144, 128, 256, 16));
    assert!(!desynced(4, 244, 256, 16));
    assert!(desynced(128, 145, 256, 16));
    assert!(desynced(0, 128, 256, 16));
}
