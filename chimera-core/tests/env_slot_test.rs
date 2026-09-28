//! The ENV slot: A or B, the matrix's inputs, TYPE/MODE/FORM changes
//! (filter-routing spec § 1, § Tests "TYPE, MODE and FORM changes").

use chimera_core::dsp::envelope::{EnvMods, Envelope};
use chimera_core::dsp::modulator::func::Slides;
use chimera_core::dsp::modulator::{EnvForm, EnvType, Func, FuncMode, LfoForm};
use chimera_core::params::{EnvParams, ParamSnapshot};

const SR: u32 = 48_000;

fn a() -> EnvParams {
    EnvParams::default()
}

fn b(f: Func) -> EnvParams {
    let mut p = EnvParams {
        env_type: EnvType::B,
        ..EnvParams::default()
    };
    p.func.set_func(f);
    p
}

fn kinds() -> Vec<EnvParams> {
    vec![
        a(),
        b(Func::Env(EnvForm::Ad)),
        b(Func::Env(EnvForm::Ahr)),
        b(Func::Env(EnvForm::Cycle)),
        b(Func::Lfo(LfoForm::Free)),
        b(Func::Lfo(LfoForm::Lfv)),
        b(Func::Burst(EnvForm::Ad)),
        b(Func::Burst(EnvForm::Cycle)),
    ]
}

#[test]
fn env3_defaults_to_b_env_ad() {
    let p = ParamSnapshot::default();
    assert_eq!(p.envelopes[0].env_type, EnvType::A);
    assert_eq!(p.envelopes[1].env_type, EnvType::A);
    let e3 = &p.envelopes[2];
    assert_eq!(
        (e3.env_type, e3.func.func()),
        (EnvType::B, Func::Env(EnvForm::Ad))
    );
    // Each MODE keeps its own FORM: LFO's default is FREE, not ENV's AD.
    assert_eq!(e3.func.lfo_form, LfoForm::Free);
}

/// Every change between the kinds, mid-note, key down and up: the first
/// output after it is the last before it; into A or B ENV nothing glides.
#[test]
fn type_mode_and_form_changes_never_step() {
    for from in kinds() {
        for to in kinds() {
            for key_down in [true, false] {
                let mut e = Envelope::new();
                e.note_on();
                for blk in 0..12 {
                    e.run_block(&from, &EnvMods::NONE, key_down || blk < 6, SR, None);
                }
                let before = e.output();
                let start = e.run_block(&to, &EnvMods::NONE, key_down, SR, None);
                assert!((start - before).abs() < 1e-6, "{from:?} → {to:?}");
                let into_env = to.env_type == EnvType::A || to.func.mode == FuncMode::Env;
                if into_env && (0.0..=1.0).contains(&before) {
                    assert!(!e.gliding(), "{from:?} → {to:?} glides");
                }
                let mut last = [0.0f32; 4];
                for l in last.iter_mut() {
                    *l = e.run_block(&to, &EnvMods::NONE, key_down, SR, None);
                }
                assert!(!e.gliding(), "the glide lasts 256 samples");
                // A held CYCLE never parks, whatever level it was entered at.
                if key_down
                    && to.env_type == EnvType::B
                    && to.func.func() == Func::Env(EnvForm::Cycle)
                {
                    assert_ne!(last[2], last[3], "{from:?} → CYCLE is moving");
                }
            }
        }
    }
}

/// Review Focus 3: TYPE, MODE and FORM spun one step every block.
#[test]
fn spinning_type_every_block_stays_bounded() {
    let k = kinds();
    let mut e = Envelope::new();
    e.note_on();
    for blk in 0..1000 {
        let p = &k[blk % k.len()];
        let before = e.output();
        let v = e.run_block(p, &EnvMods::NONE, blk % 97 < 60, SR, None);
        assert!(
            v.is_finite() && (-1.0 - 1e-6..=1.0 + 1e-6).contains(&v),
            "block {blk}: {v}"
        );
        assert!((v - before).abs() < 1e-6, "block {blk}: {before} → {v}");
    }
}

/// LEVEL and TIME are inert on B; RISE, FALL and SHAPE on A.
#[test]
fn a_and_b_destinations_are_inert_on_the_other_type() {
    let render = |p: &EnvParams, m: &EnvMods| {
        let mut e = Envelope::new();
        e.note_on();
        (0..50)
            .map(|_| e.run_block(p, m, true, SR, None))
            .collect::<Vec<_>>()
    };
    let slides = EnvMods {
        slides: Slides {
            rise: 0.3,
            fall: -0.2,
            shape: 0.4,
        },
        ..EnvMods::NONE
    };
    let ctrl = EnvMods {
        level: Some(0.3),
        time: 0.5,
        ..EnvMods::NONE
    };
    let pa = a();
    let pb = b(Func::Env(EnvForm::Ad));
    assert_eq!(render(&pa, &slides), render(&pa, &EnvMods::NONE));
    assert_eq!(render(&pb, &ctrl), render(&pb, &EnvMods::NONE));
    assert_ne!(render(&pb, &slides), render(&pb, &EnvMods::NONE));
    assert_ne!(render(&pa, &ctrl), render(&pa, &EnvMods::NONE));
}

/// A FORM change first seen at a note-on (no block ran in between) runs
/// the new FORM's note-on: an LFO switched to SYNC restarts at its PHASE.
#[test]
fn a_change_seen_at_a_note_on_runs_the_new_note_on() {
    let (free, sync) = (b(Func::Lfo(LfoForm::Free)), b(Func::Lfo(LfoForm::Sync)));
    let mut e = Envelope::new();
    e.note_on();
    for _ in 0..20 {
        e.run_block(&free, &EnvMods::NONE, true, SR, None);
    }
    e.note_on();
    let mut fresh = Envelope::new();
    fresh.note_on();
    let (mut x, mut y) = (0.0, 0.0);
    for _ in 0..6 {
        x = e.run_block(&sync, &EnvMods::NONE, true, SR, None);
        y = fresh.run_block(&sync, &EnvMods::NONE, true, SR, None);
    }
    assert!(!e.gliding(), "the glide is over");
    assert!(
        (x - y).abs() < 1e-6,
        "{x} vs {y}: SYNC restarted at the note-on"
    );
}

#[test]
fn holds_follow_the_lifetime_rule() {
    // A and B AD/AHR hold until idle; B CYCLE and B LFO only while held.
    for (p, holds_after_key_up) in [
        (a(), true),
        (b(Func::Env(EnvForm::Ad)), true),
        (b(Func::Env(EnvForm::Cycle)), false),
        (b(Func::Lfo(LfoForm::Free)), false),
    ] {
        let mut e = Envelope::new();
        e.note_on();
        e.run_block(&p, &EnvMods::NONE, true, SR, None);
        e.run_block(&p, &EnvMods::NONE, false, SR, None);
        assert_eq!(e.holds(false), holds_after_key_up, "{p:?}");
    }
}

/// A note-on that changes TYPE on a sounding A takes over from where it
/// is, with the new kind's SHAPE and TILT: no step, and within ±1.
fn note_on_takes_over_a_sounding_a(to: EnvParams) {
    let from = a();
    let mut e = Envelope::new();
    e.note_on();
    for _ in 0..20 {
        e.run_block(&from, &EnvMods::NONE, true, SR, None);
    }
    let before = e.output();
    assert!(before > 0.5, "A is sounding: {before}");
    e.note_on();
    let start = e.run_block(&to, &EnvMods::NONE, true, SR, None);
    assert!((start - before).abs() < 1e-6, "{before} → {start}");
    for blk in 0..8 {
        let v = e.run_block(&to, &EnvMods::NONE, true, SR, None);
        assert!((-1.0 - 1e-6..=1.0 + 1e-6).contains(&v), "block {blk}: {v}");
    }
}

#[test]
fn a_note_on_into_b_lfo_glides_within_bounds() {
    let mut lfo = b(Func::Lfo(LfoForm::Free));
    (lfo.func.shape, lfo.func.fall) = (1.0, 0.75); // TILT top (a ramp), PHASE 0.75
    note_on_takes_over_a_sounding_a(lfo);
}

#[test]
fn a_note_on_into_b_env_enters_at_the_level_with_its_shape() {
    let mut ahr = b(Func::Env(EnvForm::Ahr));
    ahr.func.shape = 0.9;
    note_on_takes_over_a_sounding_a(ahr);
}

/// A B LFO near −1 switched to A glides up from there: no step to A's 0.
#[test]
fn a_negative_lfo_into_a_glides_from_where_it_was() {
    let mut lfo = b(Func::Lfo(LfoForm::Free));
    lfo.func.fall = 0.0; // PHASE 0: the ramp's bottom
    let mut e = Envelope::new();
    e.note_on();
    e.run_block(&lfo, &EnvMods::NONE, true, SR, None);
    let before = e.output();
    assert!(before < -0.9, "the LFO is near −1: {before}");
    let start = e.run_block(&a(), &EnvMods::NONE, true, SR, None);
    assert!((start - before).abs() < 1e-6, "{before} → {start}");
}

/// A second change inside the glide from a negative LFO: A still below 0
/// hands over to B ENV without a step.
#[test]
fn a_change_mid_glide_continues_below_zero() {
    let mut lfo = b(Func::Lfo(LfoForm::Free));
    lfo.func.fall = 0.0; // PHASE 0: the ramp's bottom
    let mut e = Envelope::new();
    e.note_on();
    e.run_block(&lfo, &EnvMods::NONE, true, SR, None);
    e.run_block(&a(), &EnvMods::NONE, true, SR, None);
    assert!(e.gliding());
    let before = e.output();
    assert!(
        before < -0.1,
        "A is still gliding up from the LFO: {before}"
    );
    let start = e.run_block(&b(Func::Env(EnvForm::Ad)), &EnvMods::NONE, true, SR, None);
    assert!((start - before).abs() < 1e-6, "{before} → {start}");
}

/// A fast B LFO switched in at +1: the leftover plus the LFO stays within
/// ±1 through the glide, block starts and on the VCA path.
#[test]
fn a_fast_lfo_glide_stays_within_bounds() {
    let mut lfo = b(Func::Lfo(LfoForm::Free));
    lfo.func.rise = 1.0; // the top rate
    for vca in [false, true] {
        let mut e = Envelope::new();
        e.note_on();
        for _ in 0..20 {
            e.run_block(&a(), &EnvMods::NONE, true, SR, None);
        }
        assert!(e.output() > 0.9, "A is near +1: {}", e.output());
        let mut worst = 0.0f32;
        for _ in 0..((256 / chimera_hal::BLOCK_SIZE) + 1) {
            let mut g = [0.0f32; chimera_hal::BLOCK_SIZE];
            let v = if vca {
                e.run_block(&lfo, &EnvMods::NONE, true, SR, Some((&mut g, 1.0)))
            } else {
                e.run_block(&lfo, &EnvMods::NONE, true, SR, None)
            };
            worst = g.iter().fold(worst.max(v.abs()), |w, x| w.max(x.abs()));
        }
        assert!(worst <= 1.0 + 1e-6, "vca {vca}: {worst}");
    }
}
