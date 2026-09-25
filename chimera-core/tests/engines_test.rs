//! `Engines` dispatch and the VCA/activity table (spec §3), one test per row:
//!
//! | Engine | Amp env on VCA | Voice active while     |
//! |--------|----------------|------------------------|
//! | Pizza  | yes            | `amp_env.is_active()`  |
//! | FM     | yes            | `!fm.is_idle()`        |
//! | Modal  | no             | `modal.is_active()`    |
//! | Va     | yes (silent)   | never                  |

mod common;

use chimera_core::dsp::engines::Engines;
use chimera_core::dsp::envelope::Envelope;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::{SR, expects_sound};

fn params(kind: EngineType) -> ParamSnapshot {
    ParamSnapshot::for_engine(kind)
}

fn render(e: &mut Engines, kind: EngineType, p: &ParamSnapshot, blocks: usize) -> f32 {
    let mut out = [0.0f32; BLOCK_SIZE];
    let mut peak = 0.0f32;
    for _ in 0..blocks {
        e.render(kind, &mut out, p);
        peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
    }
    peak
}

#[test]
fn all_lists_every_engine_once_in_order() {
    assert_eq!(EngineType::ALL.len(), EngineType::Va as usize + 1);
    for (i, &e) in EngineType::ALL.iter().enumerate() {
        assert_eq!(e as usize, i);
        let _ = expects_sound(e); // exhaustive match: new variants fail to compile
    }
}

#[test]
fn every_engine_renders_per_its_expectation() {
    for kind in EngineType::ALL {
        let mut e = Engines::new(SR);
        e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &params(kind));
        let peak = render(&mut e, kind, &params(kind), 8);
        assert!(peak.is_finite());
        assert_eq!(peak > 1e-3, expects_sound(kind), "{kind:?}: peak {peak}");
    }
}

#[test]
fn pizza_row_amp_env_on_vca_and_lives_with_it() {
    let kind = EngineType::Pizza;
    assert!(Engines::uses_amp_env(kind));
    let mut e = Engines::new(SR);
    let mut env = Envelope::new();
    assert!(!e.is_active(kind, &env));
    e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &params(kind));
    env.note_on(Velocity::DEFAULT.unit());
    assert!(e.is_active(kind, &env));
    // The oscillator keeps running after note-off; only the envelope ends the voice.
    e.note_off(kind);
    render(&mut e, kind, &params(kind), 10);
    assert!(e.is_active(kind, &env));
    assert!(!e.is_active(kind, &Envelope::new()));
}

#[test]
fn fm_row_amp_env_on_vca_and_lives_until_operators_idle() {
    let kind = EngineType::Fm;
    assert!(Engines::uses_amp_env(kind));
    let mut e = Engines::new(SR);
    let idle_env = Envelope::new(); // FM activity ignores the amp envelope
    assert!(!e.is_active(kind, &idle_env));
    e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &params(kind));
    render(&mut e, kind, &params(kind), 1);
    assert!(e.is_active(kind, &idle_env));
    e.note_off(kind);
    render(&mut e, kind, &params(kind), 400);
    assert!(!e.is_active(kind, &idle_env));
}

#[test]
fn modal_row_no_amp_env_and_lives_until_modes_decay() {
    let kind = EngineType::Modal;
    assert!(!Engines::uses_amp_env(kind));
    let mut p = params(kind);
    p.modal.mode = ResonatorMode::Modal;
    p.modal.decay = 0.0;
    let mut e = Engines::new(SR);
    let mut running_env = Envelope::new();
    running_env.note_on(1.0); // Modal activity ignores the amp envelope
    assert!(!e.is_active(kind, &running_env));
    e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &p);
    assert!(e.is_active(kind, &running_env));
    e.note_off(kind);
    render(&mut e, kind, &p, 400);
    assert!(!e.is_active(kind, &running_env));
}

#[test]
fn va_row_amp_env_on_vca_never_active_silent() {
    let kind = EngineType::Va;
    assert!(Engines::uses_amp_env(kind));
    let mut e = Engines::new(SR);
    let mut env = Envelope::new();
    env.note_on(1.0);
    e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &params(kind));
    assert!(!e.is_active(kind, &env));
    assert_eq!(render(&mut e, kind, &params(kind), 4), 0.0);
}
