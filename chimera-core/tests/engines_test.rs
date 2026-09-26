//! `Engines` dispatch and the VCA/activity table (spec §3), one test per row:
//!
//! | Engine | Amp env on VCA | Voice active while     |
//! |--------|----------------|------------------------|
//! | Algo   | no             | a carrier's envelope   |
//! | Modal  | no             | `modal.is_active()`    |

mod common;

use chimera_core::dsp::algo::engine::AlgoLive;
use chimera_core::dsp::engines::Engines;
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
        e.render(kind, &mut out, p, &AlgoLive::from_params(&p.algo));
        peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
    }
    peak
}

#[test]
fn all_lists_every_engine_once_in_order() {
    assert_eq!(EngineType::ALL.len(), EngineType::Modal as usize + 1);
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
fn modal_row_no_amp_env_and_lives_until_modes_decay() {
    let kind = EngineType::Modal;
    assert!(!Engines::uses_amp_env(kind));
    let mut p = params(kind);
    p.modal.mode = ResonatorMode::Modal;
    p.modal.decay = 0.0;
    let mut e = Engines::new(SR);
    assert!(!e.is_active(kind));
    e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &p);
    assert!(e.is_active(kind));
    e.note_off(kind);
    render(&mut e, kind, &p, 400);
    assert!(!e.is_active(kind));
}

#[test]
fn algo_row_no_amp_env_and_lives_until_its_carriers_release() {
    let kind = EngineType::Algo;
    assert!(!Engines::uses_amp_env(kind));
    let mut e = Engines::new(SR);
    assert!(!e.is_active(kind));
    e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &params(kind));
    render(&mut e, kind, &params(kind), 1);
    assert!(e.is_active(kind));
    e.note_off(kind);
    render(&mut e, kind, &params(kind), 400);
    assert!(!e.is_active(kind));
}
