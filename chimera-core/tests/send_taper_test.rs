//! A Part send's taper (ADR 0278, #1004): 0 off, the top today's unity, the
//! low steps fading in from far below hearing instead of jumping to −42 dB.

use chimera_core::params::send_gain;

const STEP: f32 = 1.0 / 128.0;

fn db(g: f32) -> f32 {
    20.0 * g.log10()
}

#[test]
fn zero_is_off_and_the_top_is_unity() {
    assert_eq!(send_gain(0.0), 0.0);
    assert_eq!(send_gain(1.0), 1.0);
}

#[test]
fn the_first_step_fades_in_below_minus_60_db() {
    let g = send_gain(STEP);
    assert!(g > 0.0);
    assert!(db(g) < -60.0, "step 1 at {} dB", db(g));
}

#[test]
fn half_way_sits_at_minus_12_db() {
    assert!((db(send_gain(0.5)) + 12.04).abs() < 0.01);
}

#[test]
fn the_taper_rises_every_step() {
    let gains: Vec<f32> = (0..=128).map(|n| send_gain(n as f32 * STEP)).collect();
    assert!(gains.windows(2).all(|w| w[0] < w[1]));
}

// The mixer's use of it: `instrument_test`'s
// `fx_send_puts_no_dry_signal_on_pair_1` feeds the FX bus `send_gain(SEND)`.
