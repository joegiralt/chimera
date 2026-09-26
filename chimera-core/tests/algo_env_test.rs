//! Spec § Rendering: TX81Z-style rates stepped per sample in `f32`; the
//! fastest attack is at most 16 samples; stage times follow the rate.

use chimera_core::MidiNote;
use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv, Stage, key_scale};
use chimera_core::dsp::algo::tx::d1l_level;

const SR: f32 = 48_000.0;
const HOLD: EnvRates = EnvRates {
    ar: 31,
    d1r: 0,
    d1l: 15,
    d2r: 0,
    rr: 8,
    rs: 0,
};

fn started(r: EnvRates, note: MidiNote) -> OpEnv {
    let mut e = OpEnv::IDLE;
    e.note_on(EnvCoefs::new(r, note, SR));
    e
}

/// Samples spent in `stage` from now, stepping until it changes.
fn samples_in(e: &mut OpEnv, stage: Stage) -> usize {
    let mut n = 0;
    while e.stage() == stage {
        e.step();
        n += 1;
        assert!(n < 50_000_000, "{stage:?} never ends");
    }
    n
}

fn attack(ar: u8) -> usize {
    samples_in(
        &mut started(EnvRates { ar, ..HOLD }, MidiNote::A4),
        Stage::Attack,
    )
}

#[test]
fn the_fastest_attack_is_at_most_16_samples() {
    assert!(attack(31) <= 16, "{}", attack(31));
}

#[test]
fn attack_time_doubles_every_two_rate_steps() {
    for ar in [5u8, 11, 17, 23] {
        let r = attack(ar) as f32 / attack(ar + 2) as f32;
        assert!((r - 2.0).abs() < 0.1, "AR {ar}: {r}");
    }
}

#[test]
fn decay_time_doubles_every_two_rate_steps() {
    let decay = |d1r: u8| {
        let mut e = started(
            EnvRates {
                d1r,
                d1l: 0,
                ..HOLD
            },
            MidiNote::A4,
        );
        samples_in(&mut e, Stage::Attack);
        samples_in(&mut e, Stage::Decay1)
    };
    for d1r in [11u8, 13, 15, 17] {
        let r = decay(d1r) as f32 / decay(d1r + 2) as f32;
        assert!((r - 2.0).abs() < 0.1, "D1R {d1r}: {r}");
    }
}

#[test]
fn decay1_stops_at_d1l_and_d2r_zero_holds() {
    let mut e = started(
        EnvRates {
            d1r: 20,
            d1l: 10,
            ..HOLD
        },
        MidiNote::A4,
    );
    samples_in(&mut e, Stage::Attack);
    samples_in(&mut e, Stage::Decay1);
    assert_eq!(e.level(), d1l_level(10));
    for _ in 0..48_000 {
        e.step();
    }
    assert_eq!((e.stage(), e.level()), (Stage::Decay2, d1l_level(10)));
}

#[test]
fn d1l_15_goes_straight_to_the_second_decay() {
    let mut e = started(
        EnvRates {
            d1r: 20,
            d2r: 20,
            ..HOLD
        },
        MidiNote::A4,
    );
    samples_in(&mut e, Stage::Attack);
    assert_eq!(e.stage(), Stage::Decay2);
    for _ in 0..48_000 {
        e.step();
    }
    assert!(e.level() < 1.0);
}

#[test]
fn release_ends_idle_and_silent() {
    let mut e = started(HOLD, MidiNote::A4);
    samples_in(&mut e, Stage::Attack);
    e.note_off();
    samples_in(&mut e, Stage::Release);
    assert!(e.is_idle());
    assert_eq!(e.step(), 0.0);
}

#[test]
fn attack_rate_zero_never_sounds() {
    let mut e = started(EnvRates { ar: 0, ..HOLD }, MidiNote::A4);
    for _ in 0..10_000 {
        assert_eq!(e.step(), 0.0);
    }
    assert_eq!(e.stage(), Stage::Attack);
}

#[test]
fn a_retrigger_attacks_from_the_current_level() {
    let mut e = started(HOLD, MidiNote::A4);
    samples_in(&mut e, Stage::Attack);
    e.note_off();
    for _ in 0..2_000 {
        e.step();
    }
    let before = e.level();
    assert!(before > 0.0);
    e.note_on(EnvCoefs::new(HOLD, MidiNote::A4, SR));
    assert!(e.step() >= before);
}

#[test]
fn rate_scaling_speeds_up_high_notes() {
    let top = MidiNote::new(108).unwrap();
    assert_eq!(key_scale(3, MidiNote::new(21).unwrap()), 0);
    assert!(key_scale(3, top) > key_scale(0, top));
    let slow = EnvRates { ar: 10, ..HOLD };
    let fast = EnvRates { rs: 3, ..slow };
    let t = |r| samples_in(&mut started(r, top), Stage::Attack);
    assert!(t(fast) < t(slow));
}

#[test]
fn new_rates_apply_to_the_running_stage() {
    let slow = EnvRates {
        d1r: 4,
        d1l: 0,
        ..HOLD
    };
    let mut e = started(slow, MidiNote::A4);
    samples_in(&mut e, Stage::Attack);
    for _ in 0..100 {
        e.step();
    }
    e.set_coefs(EnvCoefs::new(
        EnvRates { d1r: 30, ..slow },
        MidiNote::A4,
        SR,
    ));
    assert!(samples_in(&mut e, Stage::Decay1) < 2_000);
}
