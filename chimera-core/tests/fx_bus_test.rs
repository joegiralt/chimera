//! The shared FX bus (instrument-core spec § Audio path, FX diet spec
//! § Bus): each effect runs once on the sum of the parts' sends; the return
//! is the sum of the wet outputs of the effects that are on.

use chimera_core::dsp::Stereo;
use chimera_core::dsp::chorus::{ChorusParams, JunoChorus};
use chimera_core::dsp::delay::{DelayParams, TapeDelay};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::reverb::Reverb;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

fn burst() -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| if i % 16 == 0 { 0.5 } else { -0.1 })
}

fn full() -> Stereo {
    Stereo {
        l: [1.0; BLOCK_SIZE],
        r: [1.0; BLOCK_SIZE],
    }
}

/// A mono effect's wet block, on both sides.
fn mono(mut x: [f32; BLOCK_SIZE], f: impl FnOnce(&mut [f32; BLOCK_SIZE])) -> Stereo {
    f(&mut x);
    Stereo { l: x, r: x }
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
    let mut ret = full();
    bus.process(&mut sends, &FxParams::default(), SR, &mut ret);
    assert_eq!(ret, Stereo::SILENT);
}

/// Send/return: the bus's return is each effect's own wet output (its wet
/// signal × MIX, the return level) and carries none of the dry send.
#[test]
fn return_is_each_effects_wet_output() {
    let mut p = FxParams {
        chorus: ChorusParams {
            mode: 3,
            rate: 0.5,
            depth: 0.5,
            mix: 0.5,
        },
        delay: DelayParams {
            time_ms: 1.0,
            feedback: 0.6,
            mix: 0.5,
            ..DelayParams::default()
        },
        ..FxParams::default()
    };
    p.reverb.mix = 0.5;
    p.reverb.time = 0.7;
    for slot in 0..FX_SENDS {
        let mut params = FxParams::default();
        match slot {
            0 => params.chorus = p.chorus,
            1 => params.delay = p.delay,
            _ => params.reverb = p.reverb,
        }
        let mut bus = Box::new(FxBus::new());
        let (mut c, mut d, mut r) = (
            Box::new(JunoChorus::new()),
            Box::new(TapeDelay::new()),
            Box::new(Reverb::new()),
        );
        let mut heard = 0.0f32;
        for b in 0..80 {
            let input = if b < 4 { burst() } else { [0.0; BLOCK_SIZE] };
            let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
            sends[slot] = input;
            let mut ret = Stereo::SILENT;
            bus.process(&mut sends, &params, SR, &mut ret);
            let want = match slot {
                0 => {
                    let mut w = Stereo::SILENT;
                    c.process_wet(&input, &params.chorus, SR, &mut w);
                    w
                }
                1 => mono(input, |w| d.process_wet(w, &params.delay, SR)),
                _ => mono(input, |w| r.process_wet(w, &params.reverb)),
            };
            assert_eq!(ret, want, "effect {slot} block {b}");
            heard = ret
                .l
                .iter()
                .chain(&ret.r)
                .fold(heard, |m, s| m.max(s.abs()));
        }
        assert!(heard > 1e-3, "effect {slot} returns something");
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
        let mut ret = full();
        bus.process(&mut sends, &p, SR, &mut ret);
        assert_eq!(ret, Stereo::SILENT, "block {b}");
    }
}
