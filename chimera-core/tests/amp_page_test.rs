// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! AMP: the VCA's VEL and its low-pass gate, LPG · DECAY · STRIKE (ADR
//! 0195); the sends on AMP › SENDS (filter-routing spec § 5; the fold left
//! for the FX slot, ADR 0155).

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::params::{EngineType, OutParams};
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::ui::block_registry::AMP;
use chimera_core::ui::nav::chain_def_for;
use chimera_core::ui::{PrimeStatus, UiState, view};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

const VEL: ParamAddr = ParamAddr::new(BlockRef::Out, OutParams::VCA_VEL);

/// DRUM's AMP is its pad's (`DRUM_AMP`), in the same place.
fn amp_of(ct: EngineType) -> u16 {
    match ct {
        EngineType::Drum => chimera_core::ui::block_registry::DRUM_AMP.id,
        _ => AMP.id,
    }
}

#[test]
fn every_part_chain_has_amp_last_before_mod() {
    for ct in EngineType::ALL {
        let blocks = chain_def_for(ct).blocks;
        assert_eq!(blocks[blocks.len() - 2].def.id, amp_of(ct), "{ct:?}");
    }
    assert_eq!((AMP.name, AMP.short), ("Amp", "AMP"));
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
    for _ in 0..3 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    ui
}

#[test]
fn a_dimmed_vel_ignores_its_encoder() {
    let mut ui = on_amp();
    feed(&mut ui, Input::turn(EncoderId::A, -20));
    assert_eq!(ui.params().out.vca_vel, 1.0);
}

/// MIX+PLUS on AMP's VEL primes the VCA, a permanent column: already
/// there, and the registry stays empty.
#[test]
fn mix_plus_on_vel_primes_the_vca() {
    let mut ui = on_amp();
    feed(&mut ui, Input::turn(EncoderId::A, 1));
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
        looks & 3
    };
    assert_eq!(vel_look(&mut ui), Look::Dimmed as u32);
    let m = &mut ui.project_mut().edit_part(PartId::ALL[0]).sound.mod_state;
    m.set_fixed(ModSource::Env2.index(), FixedCol::Vca, 127);
    assert_eq!(vel_look(&mut ui), Look::Live as u32);
}

#[test]
fn amp_rows() {
    use chimera_core::ui::block_registry::SENDS;
    let got: Vec<_> = (0..6).map(|i| AMP.params[i].label()).collect();
    assert_eq!(got, ["VEL", "LPG", "DECAY", "STRIKE", "BASE", "--"]);
    let got: Vec<_> = (0..6).map(|i| SENDS.params[i].label()).collect();
    assert_eq!(got, ["CHORUS", "DELAY", "REVERB", "--", "--", "--"]);
}

/// AMP's one sub-page is SENDS, the mixer's own: one value, one focus.
#[test]
fn amp_sends_is_the_mixers_sends() {
    use chimera_core::ui::block_registry::SENDS;
    for ct in EngineType::ALL {
        let c = chain_def_for(ct);
        let b = c.blocks.iter().find(|b| b.def.id == amp_of(ct)).unwrap();
        // DRUM's: the pad's GATE, then SENDS.
        let last = b.sub_pages.len() - 1;
        assert_eq!(b.sub_page_count(), 2 + last, "{ct:?}: AMP, SENDS");
        assert_eq!(b.sub_pages[last].id, SENDS.id, "{ct:?}");
    }
}

#[test]
fn labels_fit() {
    use chimera_core::ui::{draw, theme};
    for s in ["CHORUS", "DELAY", "REVERB", "VEL", "LPG", "DECAY", "STRIKE"] {
        let w = draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING);
        assert!(w <= theme::CELL_BAR_W, "{s} {w}");
    }
}
