//! The shared FX bus (instrument-core spec § Audio path): each effect runs
//! once on the sum of the parts' sends; the return is the sum of the effects
//! that are on.

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

/// With only the reverb on, the return is exactly the reverb of its send.
#[test]
fn return_is_the_processed_send() {
    let mut p = FxParams::default();
    p.reverb.mix = 0.5;
    p.reverb.time = 0.7;
    let mut bus = Box::new(FxBus::new());
    let mut alone = Box::new(Reverb::new());
    for b in 0..20 {
        let input = if b < 4 { burst() } else { [0.0; BLOCK_SIZE] };
        let mut sends = [[0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE], input];
        let mut ret = [0.0f32; BLOCK_SIZE];
        bus.process(&mut sends, &p, SR, &mut ret);
        let mut want = input;
        alone.process(&mut want, &p.reverb);
        assert_eq!(ret, want, "block {b}");
    }
}
