//! A send's taper (ADR 0278, #1004): 0 off, the top today's unity, the low
//! steps fading in from far below hearing instead of jumping to −42 dB.

use chimera_core::dsp::{Gain, SendLevel};

fn db(g: Gain) -> f32 {
    20.0 * g.get().log10()
}

#[test]
fn zero_is_off_and_the_top_is_unity() {
    assert_eq!(SendLevel::OFF.gain(), Gain::ZERO);
    assert_eq!(SendLevel::FULL.gain(), Gain::UNITY);
    assert_eq!(SendLevel::step::<128>(), SendLevel::FULL);
}

#[test]
fn the_first_step_fades_in_below_minus_60_db() {
    let g = SendLevel::step::<1>().gain();
    assert!(g > Gain::ZERO);
    assert!(db(g) < -60.0, "step 1 at {} dB", db(g));
}

#[test]
fn half_way_sits_at_minus_12_db() {
    assert!((db(SendLevel::step::<64>().gain()) + 12.04).abs() < 0.01);
}

#[test]
fn the_taper_rises_every_step() {
    let gains: Vec<Gain> = (0..=128)
        .map(|n| SendLevel::new(n as f32 / 128.0).gain())
        .collect();
    assert!(gains.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn a_level_out_of_range_is_held_to_the_knob() {
    assert_eq!(SendLevel::new(1.5), SendLevel::FULL);
    assert_eq!(SendLevel::new(-0.2), SendLevel::OFF);
    assert_eq!(SendLevel::new(f32::NAN), SendLevel::OFF);
}

/// Re-voicing a send set on the linear knob: the grid step whose gain is
/// nearest, within 0.25 dB of before (the factory's own, ADR 0278).
#[test]
fn from_gain_keeps_a_linear_sends_loudness() {
    let factory = [
        (0.35, 76),
        (0.3, 70),
        (0.25, 64),
        (0.15, 50),
        (0.1, 40),
        (0.5, 91),
        (0.0703125, 34),
        (0.1796875, 54),
    ];
    for (old, step) in factory {
        let l = SendLevel::from_gain(Gain::new(old));
        assert_eq!(l.grid_step(), Some(step), "{old}");
        let off = db(l.gain()) - db(Gain::new(old));
        assert!(off.abs() < 0.25, "{old}: {off} dB");
    }
}

#[test]
fn from_gain_undoes_gain_on_the_grid() {
    for n in 0..=128u8 {
        let l = SendLevel::new(f32::from(n) / 128.0);
        assert_eq!(SendLevel::from_gain(l.gain()), l, "step {n}");
        assert_eq!(l.grid_step(), Some(n));
    }
    assert_eq!(SendLevel::new(0.3 / 128.0).grid_step(), None);
}

// The mixer's use of it: `instrument_test`'s
// `fx_send_puts_no_dry_signal_on_pair_1` feeds the FX bus `SEND.gain()`;
// the delay's REV SEND: `fx_bus_test`'s `rev_send_half_sits_12_db_under_full`.
