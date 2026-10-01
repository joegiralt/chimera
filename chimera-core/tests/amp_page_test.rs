//! FLD / VCA (filter-routing spec § 5, § UI "AMP").

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::modulation::{ModSource, VCA};
use chimera_core::params::{EngineType, OutParams};
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::ui::block_registry::FOLDER;
use chimera_core::ui::chain::chain_def_for;
use chimera_core::ui::{PrimeStatus, UiState, view};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

const VEL: ParamAddr = ParamAddr::new(BlockRef::Out, OutParams::VCA_VEL);

#[test]
fn every_part_chain_has_amp_last_before_mod() {
    for ct in EngineType::ALL {
        let blocks = chain_def_for(ct).blocks;
        assert_eq!(blocks[blocks.len() - 2].def.id, FOLDER.id, "{ct:?}");
    }
    assert_eq!((FOLDER.name, FOLDER.short), ("Fold / VCA", "AMP"));
}

#[test]
fn vel_is_dimmed_until_a_vca_route_exists() {
    let s = Sound::init(EngineType::Algo);
    assert!(view::dimmed(VEL, &s));
    let mut r = s.clone();
    let d = r.mod_state.push(VCA).unwrap();
    r.mod_state.set_route(ModSource::Env2.index(), d, 127);
    assert!(!view::dimmed(VEL, &r));
}

fn on_amp() -> UiState {
    let mut ui = UiState::new();
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    ui
}

#[test]
fn a_dimmed_vel_ignores_its_encoder() {
    let mut ui = on_amp();
    feed(&mut ui, Input::turn(EncoderId::D, -20));
    assert_eq!(ui.params().out.vca_vel, 1.0);
}

/// MIX+PLUS on AMP's VEL primes the hidden VCA (the plan's Decisions table).
#[test]
fn mix_plus_on_vel_primes_the_vca() {
    let mut ui = on_amp();
    feed(&mut ui, Input::turn(EncoderId::D, 1));
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Added));
    assert!(
        ui.project()
            .part(PartId::ALL[0])
            .sound
            .dest_registry
            .is_primed(VCA)
    );
}

/// The Cells key carries VEL's look, so a route into VCA redraws the cells
/// even without a matrix revision.
#[test]
fn the_cells_key_carries_vels_dimming() {
    use chimera_core::ui::components::Look;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::region::{RegionData, RegionKind};
    let mut ui = on_amp();
    let vel_look = |ui: &mut UiState| {
        let mut fb = Fb::new();
        ui.render_dirty_with_audio(&mut fb, &PerfStats::zero(), None, &scope_fixture());
        let Some(RegionData::Cells { looks, .. }) = ui.drawn_key(RegionKind::Cells) else {
            panic!("AMP has cells");
        };
        looks >> 6 & 3
    };
    assert_eq!(vel_look(&mut ui), Look::Dimmed as u16);
    let m = &mut ui
        .project_mut()
        .edit_part(PartId::ALL[0])
        .part
        .sound
        .mod_state;
    let d = m.push(VCA).unwrap();
    m.set_route(ModSource::Env2.index(), d, 127);
    assert_eq!(vel_look(&mut ui), Look::Live as u16);
}
