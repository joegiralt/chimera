//! RANDOMIZE's roll (#809, ADR 0232): one Sound per seed and amounts,
//! musical by construction. Bounds and determinism over many seeds, and
//! every roll played through the whole engine: never silent, never far
//! louder than its engine's INIT.

mod common;

use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::tx::ratio;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::params::EngineType;
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::randomize::kern::roll;
use chimera_core::randomize::{Amount, Randomize, Seed};
use common::sweep::{BPS, Bench, Patch, Tape};

const ENGINES: [EngineType; 4] = [
    EngineType::Algo,
    EngineType::Va,
    EngineType::Modal,
    EngineType::Tape,
];

/// Case `k`: every amount 1..=3 in turn (81 combinations), its own seed.
fn case(k: usize) -> Randomize {
    let a = |d: usize| Amount::new((k / 3usize.pow(d as u32) % 3 + 1) as u8);
    Randomize {
        osc: a(0),
        env: a(1),
        tone: a(2),
        mods: a(3),
        seed: Seed::new((k * 37 % 256) as u8),
    }
}

/// Setting `k` of all 4^4: every amount OFF..=3, its own seed.
fn every(k: usize) -> Randomize {
    let a = |d: u32| Amount::new((k / 4usize.pow(d) % 4) as u8);
    Randomize {
        osc: a(0),
        env: a(1),
        tone: a(2),
        mods: a(3),
        seed: Seed::new((k * 53 % 256) as u8),
    }
}

fn rolled(e: EngineType, r: &Randomize) -> Sound {
    let mut s = Sound::init(e);
    roll(&mut s, r);
    s
}

#[test]
fn a_seed_rolls_the_same_sound_every_time() {
    for e in ENGINES {
        for k in 0..81 {
            assert!(
                rolled(e, &case(k)).bits_eq(&rolled(e, &case(k))),
                "{e:?} case {k}"
            );
        }
    }
}

#[test]
fn another_seed_rolls_another_sound() {
    for e in ENGINES {
        let r = Randomize::DEFAULT;
        let next = Randomize {
            seed: Seed::new(1),
            ..r
        };
        assert!(!rolled(e, &r).bits_eq(&rolled(e, &next)), "{e:?}");
    }
}

#[test]
fn off_leaves_the_sound_as_it_is() {
    let off = Randomize {
        osc: Amount::OFF,
        env: Amount::OFF,
        tone: Amount::OFF,
        mods: Amount::OFF,
        seed: Seed::new(9),
    };
    for e in ENGINES {
        assert!(rolled(e, &off).bits_eq(&Sound::init(e)), "{e:?}");
    }
}

/// Each dimension draws from its own stream: MOD's amount never moves the
/// oscillators, OSC's never the envelope's family on VA.
#[test]
fn one_amount_never_moves_anothers_values() {
    for k in 0..81 {
        let r = case(k);
        let wilder = Randomize {
            mods: Amount::new(3),
            tone: Amount::new(1),
            ..r
        };
        let (a, b) = (rolled(EngineType::Va, &r), rolled(EngineType::Va, &wilder));
        assert_eq!(a.params.va, b.params.va, "case {k}");
        let calmer = Randomize {
            osc: Amount::new(1),
            ..r
        };
        let c = rolled(EngineType::Va, &calmer);
        let env = |s: &Sound| {
            let e = s.params.envelopes[0];
            [e.attack, e.decay, e.sustain, e.release]
        };
        assert_eq!(env(&a), env(&c), "case {k}");
    }
}

/// ALGO: the other amounts never move what OSC rolled; ENV draws as many
/// values whatever OSC rolled, so an operator whose role stays keeps its
/// envelope; TONE's filter is OSC's to keep too.
#[test]
fn algo_amounts_draw_apart() {
    use chimera_core::dsp::algo::params::AlgoParams;
    let osc = |a: &AlgoParams| {
        let ops: Vec<_> = a.ops.iter().map(|o| (o.coarse, o.fine, o.detune)).collect();
        (a.algs(), a.morph, a.kinds(), ops)
    };
    let roles = |a: &AlgoParams| {
        let (x, y) = a.algs();
        let b = if a.morph > 0 {
            y.algorithm().carriers
        } else {
            0
        };
        x.algorithm().carriers | b
    };
    for k in 0..81 {
        let r = case(k);
        let a = rolled(EngineType::Algo, &r);
        let other = Randomize {
            env: Amount::new(r.env.get() % 3 + 1),
            tone: Amount::new(r.tone.get() % 3 + 1),
            mods: Amount::new(r.mods.get() % 3 + 1),
            ..r
        };
        let b = rolled(EngineType::Algo, &other);
        assert_eq!(osc(&a.params.algo), osc(&b.params.algo), "case {k}");
        let calmer = Randomize {
            osc: Amount::new(r.osc.get() % 3 + 1),
            ..r
        };
        let c = rolled(EngineType::Algo, &calmer);
        let (ra, rc) = (roles(&a.params.algo), roles(&c.params.algo));
        for i in 0..6 {
            let (x, y) = (&a.params.algo.ops[i], &c.params.algo.ops[i]);
            assert_eq!(
                (x.ar, x.d1l, x.rr, x.rate_scale),
                (y.ar, y.d1l, y.rr, y.rate_scale),
                "case {k} op {i}"
            );
            if (ra ^ rc) & 1 << i == 0 {
                assert_eq!((x.d1r, x.d2r), (y.d1r, y.d2r), "case {k} op {i}: same role");
            }
        }
        let f = |s: &Sound| (s.params.filter.cutoff, s.params.filter.resonance);
        assert_eq!(f(&a), f(&c), "case {k}");
    }
}

/// The note keeps its pitch: carriers sound in octaves (0.5, 1, 2), loud; VA's
/// OSC 2 at an octave, fifth or fourth; the operators' and ENV 1's times
/// inside their families; the filter never shut; OUT, PITCH and the sends
/// untouched; ENV 1 into VCA kept on VA and TAPE.
#[test]
fn every_roll_stays_inside_its_musical_ranges() {
    for e in ENGINES {
        let init = Sound::init(e);
        for k in 0..81 {
            for seed in [0u8, 77, 255] {
                let r = Randomize {
                    seed: Seed::new(seed.wrapping_add(k as u8)),
                    ..case(k)
                };
                let s = rolled(e, &r);
                let p = &s.params;
                let at = format!("{e:?} case {k} seed {}", r.seed.get());
                assert_eq!(p.out.volume, init.params.out.volume, "{at}");
                assert_eq!(p.out.sends, init.params.out.sends, "{at}");
                assert_eq!(p.pitch.pitch, init.params.pitch.pitch, "{at}");
                assert_eq!(
                    p.lfos[0].depth, init.params.lfos[0].depth,
                    "{at}: the wheel"
                );
                assert!(p.filter.cutoff >= 500.0, "{at}: cutoff {}", p.filter.cutoff);
                assert!(p.filter.resonance <= 0.65, "{at}");
                let vca = s
                    .mod_state
                    .fixed_amount(ModSource::Env1.index(), FixedCol::Vca);
                assert_eq!(
                    vca,
                    init.mod_state
                        .fixed_amount(ModSource::Env1.index(), FixedCol::Vca),
                    "{at}"
                );
                match e {
                    EngineType::Algo => algo_bounds(&s, &at),
                    EngineType::Va => {
                        let v = &p.va;
                        assert!(
                            [0, 12, -12, 7, 19, 24, 5, -5].contains(&v.coarse[1]),
                            "{at}"
                        );
                        assert!([0, 12, -12].contains(&v.coarse[0]), "{at}");
                        assert!(v.fine[1].abs() <= 25.0 && v.noise <= 0.25, "{at}");
                    }
                    EngineType::Modal => {
                        let m = &p.modal;
                        assert!(m.excite >= 0.6, "{at}: the exciter always strikes");
                        assert!(m.damp >= 0.25, "{at}");
                    }
                    EngineType::Tape => {
                        assert_eq!(p.tape.machine, init.params.tape.machine, "{at}");
                    }
                }
                if matches!(e, EngineType::Va | EngineType::Tape) {
                    let env = &p.envelopes[0];
                    assert!(env.attack <= 0.7 && env.release <= 0.8, "{at}");
                }
            }
        }
    }
}

fn algo_bounds(s: &Sound, at: &str) {
    let a = &s.params.algo;
    let (x, y) = a.algs();
    let mut carriers = x.algorithm().carriers;
    if a.morph > 0 {
        carriers |= y.algorithm().carriers;
    }
    assert!(carriers != 0, "{at}");
    for (i, op) in a.ops.iter().enumerate() {
        if carriers & 1 << i != 0 {
            let r = ratio(op.coarse, op.fine);
            assert!([0.5, 1.0, 2.0].contains(&r), "{at}: carrier {i} at {r}");
            assert!(op.level >= 80, "{at}: carrier {i} level {}", op.level);
            assert_eq!(op.feedback, 0, "{at}: carrier {i}");
        }
        assert!(op.ar >= 8, "{at}: op {i} AR {}", op.ar);
        assert!((1..=15).contains(&op.rr), "{at}");
        assert!(op.feedback <= 4, "{at}");
    }
    assert!(AlgoId::from_index(x.get()).is_some());
}

/// Part 1's output, pair 1, over a C4 at velocity 100 held one second:
/// its loudest tenth of a second, so a pluck reads at its strike.
fn held_rms(s: Sound) -> (f32, bool) {
    let mut b = Bench::new(Patch::AlgoInit, PartParams::default(), FxParams::OFF);
    b.edit_sound(|x| *x = s);
    let mut t = Tape::default();
    b.note(60, Some(100));
    for _ in 0..BPS {
        b.render(&mut t);
    }
    let tenth = BPS / 10;
    let loudest = (0..10)
        .map(|i| t.rms(0, i * tenth, (i + 1) * tenth))
        .fold(0.0, f32::max);
    (loudest, t.finite())
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// Every roll plays: through the whole engine its loudest tenth of a
/// second sits within −24..+12 dB of its engine's INIT, at every one of the
/// 256 amount settings, from INIT and over an already-rolled Sound (a
/// carrier left at 99 by the last roll must not modulate at 99). Measured
/// −22.3..+10.0: MODAL's top is BOWED, which sustains where INIT's STRING
/// decays, at −6 dBFS.
#[test]
fn every_roll_plays_never_silent_never_deafening() {
    for e in [EngineType::Algo, EngineType::Va, EngineType::Modal] {
        let (init, _) = held_rms(Sound::init(e));
        let mut worst = (f32::MAX, f32::MIN);
        for k in 0..256 {
            let r = every(k);
            let wild = Randomize {
                osc: Amount::new(3),
                env: Amount::new(1),
                tone: Amount::new(3),
                mods: Amount::new(3),
                seed: Seed::new((k * 101 % 256) as u8),
            };
            let mut over = rolled(e, &wild);
            roll(&mut over, &r);
            for (from, s) in [("INIT", rolled(e, &r)), ("a roll", over)] {
                let (rms, finite) = held_rms(s);
                let rel = db(rms) - db(init);
                assert!(finite, "{e:?} {r:?} over {from}");
                assert!(
                    rel > -24.0,
                    "{e:?} {r:?} over {from}: {rel:.1} dB under INIT"
                );
                assert!(rel < 12.0, "{e:?} {r:?} over {from}: {rel:.1} dB over INIT");
                worst = (worst.0.min(rel), worst.1.max(rel));
            }
        }
        println!(
            "{e:?}: INIT {:.1} dBFS, rolls {:.1}..{:.1} dB",
            db(init),
            worst.0,
            worst.1
        );
    }
}

/// A seed's roll is the same over another seed's: SEED turned back lands
/// where it was.
#[test]
fn a_seed_rolls_the_same_over_any_earlier_seed() {
    for e in ENGINES {
        for k in 0..81 {
            let r = case(k);
            let mut twice = rolled(
                e,
                &Randomize {
                    seed: Seed::new(r.seed.get().wrapping_add(1)),
                    ..r
                },
            );
            roll(&mut twice, &r);
            assert!(twice.bits_eq(&rolled(e, &r)), "{e:?} case {k}");
        }
    }
}
