//! FLD / VCA (filter-routing spec § 5, § UI "AMP").

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::params::{EngineType, OutParams};
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::ui::block_registry::FOLDER;
use chimera_core::ui::nav::chain_def_for;
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
    r.mod_state
        .set_fixed(ModSource::Env2.index(), FixedCol::Vca, 127);
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
    feed(&mut ui, Input::turn(EncoderId::E, -20));
    assert_eq!(ui.params().out.vca_vel, 1.0);
}

/// MIX+PLUS on AMP's VEL primes the VCA, a permanent column: already
/// there, and the registry stays empty.
#[test]
fn mix_plus_on_vel_primes_the_vca() {
    let mut ui = on_amp();
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    assert_eq!(ui.prime_status(), Some(PrimeStatus::AlreadyRouted));
    assert!(
        ui.project()
            .part(PartId::ALL[0])
            .sound
            .dest_registry
            .is_empty()
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
        looks >> 8 & 3
    };
    assert_eq!(vel_look(&mut ui), Look::Dimmed as u32);
    let m = &mut ui.project_mut().edit_part(PartId::ALL[0]).sound.mod_state;
    m.set_fixed(ModSource::Env2.index(), FixedCol::Vca, 127);
    assert_eq!(vel_look(&mut ui), Look::Live as u32);
}

#[test]
fn amp_rows() {
    let got: Vec<_> = (0..6).map(|i| FOLDER.params[i].label()).collect();
    assert_eq!(got, ["CHORUS", "DELAY", "REVERB", "FOLD", "VEL", "MIX"]);
}

/// AMP's cell 2 and the mixer's SENDS cell 2 are one address.
#[test]
fn amp_and_sends_are_one_value() {
    use chimera_core::addr::Op;
    use chimera_core::params::ParamSnapshot;
    use chimera_core::ui::block_registry::SENDS;
    use chimera_core::ui::part_page;
    let mut p = ParamSnapshot::default();
    part_page::apply_encoder(&FOLDER, 2, 3, &mut p, &mut Op::A);
    assert_eq!(part_page::read_values(&SENDS, &p, Op::A)[2], 3.0 / 128.0);
}

#[test]
fn sym_is_one_edit_down() {
    use chimera_core::addr::Op;
    use chimera_core::params::FolderParams;
    use chimera_core::params::ParamSnapshot;
    use chimera_core::ui::block_def::slot_addr;
    use chimera_core::ui::view::SlotCtx;
    let ctx = SlotCtx::read(&ParamSnapshot::default(), Op::A);
    let mut seen = 0;
    for ct in EngineType::ALL {
        let c = chain_def_for(ct);
        for b in c.blocks.iter().filter(|b| b.def.id == FOLDER.id) {
            seen += 1;
            assert_eq!(b.sub_page_count(), 2, "{ct:?}");
            assert_eq!(b.active_def(1).id, 98, "{ct:?}");
            let sym = ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY);
            assert_eq!(slot_addr(b.active_def(1), 0, &ctx), Some(sym), "{ct:?}");
        }
    }
    assert!(seen >= 4);
}

#[test]
fn labels_fit() {
    use chimera_core::ui::{draw, theme};
    for s in ["CHORUS", "DELAY", "REVERB", "SYM"] {
        let w = draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING);
        assert!(w <= theme::CELL_BAR_W, "{s} {w}");
    }
}
