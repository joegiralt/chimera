//! FB (#393, ADR 0190): the filter's own output, a sample late, through a
//! one-pole into its cutoff, on every TYPE. FB 0 is today's filter bit for
//! bit; full FB at full RES stays bounded; FB moves without a click; the
//! one-pole keeps the top end down.

mod common;

use chimera_core::dsp::filter::{Fed, Filter, FilterKind, FilterMods};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot};
use chimera_core::voice_alloc::VoiceIdx;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use common::rig::{Rig, Src};

const KINDS: [FilterKind; 5] = FilterKind::BUILT;

/// A saw at `hz`, peak `amp`, sample `n`.
fn saw(n: usize, hz: f32, amp: f32) -> f32 {
    let p = (n as f32 * hz / SAMPLE_RATE as f32).fract();
    amp * (2.0 * p - 1.0)
}

fn params(kind: FilterKind, cutoff: f32, res: f32) -> FilterParams {
    let mut p = FilterParams::default();
    p.set_kind(kind);
    (p.cutoff, p.resonance) = (cutoff, res);
    p
}

/// `blocks` of a saw through `kind` at FB `fb` (held).
fn run(kind: FilterKind, cutoff: f32, res: f32, fb: f32, amp: f32, blocks: usize) -> Vec<f32> {
    run_with(kind, cutoff, res, Fed::new(fb, fb), amp, blocks)
}

fn run_with(
    kind: FilterKind,
    cutoff: f32,
    res: f32,
    fed: Fed,
    amp: f32,
    blocks: usize,
) -> Vec<f32> {
    let p = params(kind, cutoff, res);
    let mut f = Filter::new();
    let mut out = Vec::new();
    for b in 0..blocks {
        let mut buf: [f32; BLOCK_SIZE] =
            core::array::from_fn(|i| saw(b * BLOCK_SIZE + i, 110.0, amp));
        let mods = FilterMods {
            fb: fed,
            ..FilterMods::NONE
        };
        f.process(&mut buf, &p, SAMPLE_RATE, &mods);
        out.extend_from_slice(&buf);
    }
    out
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

#[test]
fn fb_zero_is_off() {
    assert_eq!(Fed::new(0.0, 0.0), Fed::OFF);
    assert!(!Fed::OFF.on());
    assert!(Fed::new(0.0, 0.01).on());
    assert_eq!(FilterMods::NONE.fb, Fed::OFF);
    for k in KINDS {
        let a = run(k, 400.0, 0.7, 0.0, 0.5, 40);
        let mut p = params(k, 400.0, 0.7);
        p.fb = 0.0;
        let mut f = Filter::new();
        let mut b = Vec::new();
        for n in 0..40 {
            let mut buf: [f32; BLOCK_SIZE] =
                core::array::from_fn(|i| saw(n * BLOCK_SIZE + i, 110.0, 0.5));
            f.process(&mut buf, &p, SAMPLE_RATE, &FilterMods::NONE);
            b.extend_from_slice(&buf);
        }
        let bits = |x: &[f32]| x.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&a), bits(&b), "{k:?}");
    }
}

/// FB moves the sound on every TYPE, both ways.
#[test]
fn fb_is_heard_on_every_type() {
    for k in KINDS {
        let dry = run(k, 300.0, 0.5, 0.0, 0.8, 60);
        for fb in [-1.0, -0.5, 0.5, 1.0] {
            let wet = run(k, 300.0, 0.5, fb, 0.8, 60);
            let d = dry
                .iter()
                .zip(&wet)
                .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
            assert!(d > 0.01, "{k:?} FB {fb}: {d}");
        }
    }
}

/// Full FB at full RES and a hot input stays finite and bounded on every
/// TYPE and cutoff; the 2040's guard holds it within full scale, SEM's
/// rails within its bound.
#[test]
fn full_fb_at_full_res_is_bounded() {
    for k in KINDS {
        for cutoff in [40.0, 400.0, 4_000.0, 18_000.0] {
            for fb in [-1.0, 1.0] {
                let out = run(k, cutoff, 1.0, fb, 2.0, 300);
                let pk = peak(&out);
                // SEM keeps the circuit's headroom: its rails, `SEM_BOUND`.
                let top = match k {
                    FilterKind::Sem => chimera_core::dsp::filter::sem::SEM_BOUND,
                    _ => 4.0,
                };
                assert!(
                    pk.is_finite() && pk < top,
                    "{k:?} {cutoff} Hz FB {fb}: {pk}"
                );
                if k == FilterKind::K2040 {
                    assert!(pk <= 1.0 + 1e-6, "{cutoff} Hz FB {fb}: the guard, {pk}");
                }
            }
        }
    }
}

/// A held VA note, FB stepped 0 → full in one block (a route's step, the
/// amount ramped inside it): the change is no louder a step than FB full
/// held.
#[test]
fn stepping_fb_never_clicks() {
    for k in KINDS {
        let mut p = ParamSnapshot::for_engine(EngineType::Va);
        p.filter.set_kind(k);
        (p.filter.cutoff, p.filter.resonance) = (500.0, 0.8);
        let mut rig = Rig::at(SAMPLE_RATE, VoiceIdx::ALL[0], Src::None);
        rig.note_on(MidiNote::new(45).unwrap(), Velocity::DEFAULT, &p);
        let mut block = [0.0f32; BLOCK_SIZE];
        let (mut change, mut held) = (Vec::new(), Vec::new());
        for b in 0..200 {
            p.filter.fb = if b < 100 { 0.0 } else { 1.0 };
            rig.render(&mut block, &p, &ModState::new());
            if (99..102).contains(&b) {
                change.extend_from_slice(&block);
            } else if (150..190).contains(&b) {
                held.extend_from_slice(&block);
            }
        }
        let (c, h) = (common::step(&change), common::step(&held));
        assert!(c <= 1.25 * h + 0.01, "{k:?}: {c} against {h}");
    }
}

/// Energy in the top octaves against the whole, dB (three differences,
/// ≈ +18 dB an octave).
fn hf_db(x: &[f32]) -> f32 {
    let mut y = x.to_vec();
    for _ in 0..3 {
        y = y.windows(2).map(|w| w[1] - w[0]).collect();
    }
    let e = |v: &[f32]| v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32;
    10.0 * (e(&y) / e(x).max(1e-20)).log10()
}

/// At full FB and RES 0.8 the one-pole on the FB signal keeps each TYPE's
/// top end at or under the unfiltered loop's (the demo's audible fizz:
/// CLEAN, DIRTY, the 2040), or quiet anyway, under −45 dB (LADDER, whose
/// loop it reshapes).
#[test]
fn the_one_pole_holds_the_fizz() {
    for k in KINDS {
        for fb in [-1.0f32, 1.0] {
            let tail = |fed| hf_db(&run_with(k, 400.0, 0.8, fed, 0.5, 200)[BLOCK_SIZE * 50..]);
            let (lp, raw) = (tail(Fed::new(fb, fb)), tail(Fed::with_lp(fb, fb, 1.0)));
            eprintln!("{k:?} FB {fb}: {lp:.1} dB with the one-pole, {raw:.1} without");
            assert!(
                lp <= (raw + 0.5).max(-45.0),
                "{k:?} FB {fb}: {lp} against {raw}"
            );
        }
    }
}

/// EXP's make-up gain (ADR 0191) rides FB's loop as the plain one: a hair
/// of FB on the 2040 at RES 0.8 reads within 0.5 dB of FB 0, on each voice.
#[test]
fn the_2040s_make_up_holds_under_fb() {
    use chimera_core::dsp::filter::K2040Voice;
    let rms = |x: &[f32]| {
        let t = &x[x.len() / 2..];
        (t.iter().map(|v| v * v).sum::<f32>() / t.len() as f32).sqrt()
    };
    for v in [K2040Voice::Rev2, K2040Voice::Exp] {
        let mut p = params(FilterKind::K2040, 2000.0, 0.8);
        p.voice = v;
        let go = |fed: Fed| {
            let mut f = Filter::new();
            let mut out = Vec::new();
            for b in 0..40 {
                let mut buf: [f32; BLOCK_SIZE] =
                    core::array::from_fn(|i| saw(b * BLOCK_SIZE + i, 110.0, 0.25));
                let mods = FilterMods {
                    fb: fed,
                    ..FilterMods::NONE
                };
                f.process(&mut buf, &p, SAMPLE_RATE, &mods);
                out.extend_from_slice(&buf);
            }
            rms(&out)
        };
        let d = 20.0 * (go(Fed::with_lp(1e-4, 1e-4, 1.0)) / go(Fed::OFF)).log10();
        assert!(d.abs() < 0.5, "{v:?}: FB a hair off FB 0 by {d:.2} dB");
    }
}
