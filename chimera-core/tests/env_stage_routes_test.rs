//! #149: on an ENV A page, ATTACK, DECAY, RELEASE and HOLD prime the
//! slot's TIME and SUS its LEVEL (MIX + PLUS); a route there moves those
//! cells' bars as the envelope moves: every stage × 2^(−5·Σ), the peak
//! (so the sustain) at the LEVEL sum.

#![cfg(feature = "tape")]

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::modulator::EnvSlot;
use chimera_core::dsp::modulator::law::speed_ranges;
use chimera_core::modulation::{MAX_MOD_SOURCES, ModSource, ModState};
use chimera_core::params::EnvParams as E;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use screen::{settle, ui_for};

const ENV1: BlockRef = BlockRef::Env(EnvSlot::Env1);

/// ENV 1 / A with VEL (the UI's stand-in 1) routed into `to` at `amount`.
fn routed(to: chimera_core::block::ParamId, amount: i8) -> UiState {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut ui = ui_for("env_a");
    let s = ui.project_mut().edit_part(PartId::ALL[0]).sound;
    s.dest_registry
        .add(ParamAddr::new(ENV1, to), *b"ENV TIME")
        .unwrap();
    s.mod_state = ModState::from_registry(&s.dest_registry, MAX_MOD_SOURCES);
    s.mod_state.set_amount(ModSource::Vel.index(), 0, amount);
    settle(&mut ui);
    ui
}

fn env(ui: &UiState) -> chimera_core::params::EnvParams {
    ui.project().part(PartId::ALL[0]).sound.params.envelopes[0]
}

/// The route's sum with VEL at the UI's stand-in, 1.
fn sum(ui: &UiState, to: chimera_core::block::ParamId) -> f32 {
    let mut src = [0.0; MAX_MOD_SOURCES];
    src[ModSource::Vel.index()] = 1.0;
    let s = &ui.project().part(PartId::ALL[0]).sound;
    s.mod_state.offset_for(ParamAddr::new(ENV1, to), &src)
}

#[test]
fn a_time_route_moves_every_stage_bar() {
    for amount in [24, -24] {
        let ui = routed(E::TIME, amount);
        stages_follow(&ui, sum(&ui, E::TIME));
    }
}

fn stages_follow(ui: &UiState, sum: f32) {
    assert!(sum != 0.0);
    let e = env(ui);
    let r = speed_ranges(e.speed);
    // ATTACK, DECAY, RELEASE, HOLD in slots 0, 1, 3, 4.
    for (slot, set, oct) in [
        (0, e.attack, r.attack.oct),
        (1, e.decay, r.dec_rel.oct),
        (3, e.release, r.dec_rel.oct),
        (4, e.hold, r.hold.oct),
    ] {
        let want = chimera_core::dsp::clampf(set - 5.0 * sum / oct, 0.0, 1.0);
        let got = ui.renderer.anim[slot].target();
        assert!((got - want).abs() < 1e-3, "slot {slot}: {got} want {want}");
    }
}

#[test]
fn a_level_route_moves_the_sustain_bar() {
    let ui = routed(E::LEVEL, 64);
    let e = env(&ui);
    let got = ui.renderer.anim[2].target();
    let want = e.sustain * sum(&ui, E::LEVEL);
    assert!((got - want).abs() < 0.02, "SUS: {got} want {want}");
}
