//! The shared FX bus (instrument-core spec § Audio path): each effect runs
//! once on the sum of the parts' sends; the return is the sum of the wet
//! outputs of the effects that are on (send/return: no dry signal).

use chimera_core::dsp::fx_bus::{FxBus, FxParams, FX_SENDS};
use chimera_core::dsp::reverb::Reverb;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

fn burst() -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| if i % 16 == 0 { 0.5 } else { -0.1 })
}

/// Defaults match the old `ParamSnapshot` FX: everything off.
#[test]
fn defaults_are_all_off() {
    let p = FxParams::default();
    assert!(!p.chorus.is_on() && !p.delay.is_on() && !p.reverb.is_on());
    assert_eq!(p.reverb.mix, 0.0);
    assert_eq!(p.delay.time_ms, 375.0);
}

#[test]
fn effects_that_are_off_return_nothing() {
    let mut bus = Box::new(FxBus::new());
    let mut sends = [burst(); FX_SENDS];
    let mut ret = [1.0f32; BLOCK_SIZE];
    bus.process(&mut sends, &FxParams::default(), SR, &mut ret);
    assert!(ret.iter().all(|&s| s == 0.0));
}

/// Send/return: the return is each effect's wet signal × its MIX (the
/// return level) and carries none of the dry send. Checked against the
/// effects' standalone dry/wet `process`: return = standalone − dry × dry gain.
#[test]
fn return_is_wet_only() {
    use chimera_core::dsp::chorus::{ChorusParams, JunoChorus};
    use chimera_core::dsp::delay::{DelayParams, TapeDelay};
    let chorus = ChorusParams { mode: 3, rate: 0.5, depth: 0.5, mix: 0.5 };
    let delay = DelayParams { time_ms: 1.0, feedback: 0.6, mix: 0.5, ..DelayParams::default() };
    let mut p = FxParams::default();
    p.reverb.mix = 0.5;
    p.reverb.time = 0.7;
    for (slot, dry_gain) in [(0, 1.0 - 0.5 * 0.5), (1, 1.0 - 0.5), (2, 1.0 - 0.5)] {
        let mut params = FxParams::default();
        match slot {
            0 => params.chorus = chorus,
            1 => params.delay = delay,
            _ => params.reverb = p.reverb,
        }
        let mut bus = Box::new(FxBus::new());
        let (mut c, mut d, mut r) = (Box::new(JunoChorus::new()), Box::new(TapeDelay::new()), Box::new(Reverb::new()));
        let mut leaked = 0.0f32;
        for b in 0..80 {
            let input = if b < 4 { burst() } else { [0.0; BLOCK_SIZE] };
            let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
            sends[slot] = input;
            let mut ret = [0.0f32; BLOCK_SIZE];
            bus.process(&mut sends, &params, SR, &mut ret);
            let mut alone = input;
            match slot {
                0 => c.process(&mut alone, &params.chorus, SR),
                1 => d.process(&mut alone, &params.delay, SR),
                _ => r.process(&mut alone, &params.reverb),
            }
            for i in 0..BLOCK_SIZE {
                let wet = alone[i] - input[i] * dry_gain;
                assert!((ret[i] - wet).abs() < 1e-6, "effect {slot} block {b} sample {i}: {} vs {wet}", ret[i]);
                leaked = leaked.max(ret[i].abs());
            }
        }
        assert!(leaked > 1e-3, "effect {slot} returns something");
    }
}

/// Before the plate's first tank tap (3,411 samples) the reverb's wet
/// signal is silent: so is the return, however loud the send.
#[test]
fn reverb_return_is_silent_before_its_first_reflection() {
    let mut p = FxParams::default();
    p.reverb.mix = 0.5;
    let mut bus = Box::new(FxBus::new());
    for b in 0..3_411 / BLOCK_SIZE {
        let mut sends = [[0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE], burst()];
        let mut ret = [1.0f32; BLOCK_SIZE];
        bus.process(&mut sends, &p, SR, &mut ret);
        assert!(ret.iter().all(|&s| s == 0.0), "block {b}");
    }
}
