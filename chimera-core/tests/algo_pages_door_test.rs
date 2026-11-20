//! ALGO PAGES behind bank 2's door (ADR 0300), under `bank2` on a host,
//! where the table answers only while the door cell is open. The
//! operator-first chains' tables are bank 2's: closed, FIRST at OPS lays
//! OSC out as PARAMS does, and a place is always on the chain it was made
//! for. One test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

mod common;
mod screen;

use chimera_core::bank2::DOOR_CELL;
use chimera_core::ui::block_registry::{
    ALGO_CHAIN, ALGO_D1R, ALGO_FORMANT_CHAIN, ALGO_OP_PAGES, ALGO_WAVE,
};
use chimera_core::ui::nav::PartChain;
use chimera_core::ui::notes::{ALGO_OPS_CHAIN, ALGO_OPS_FORMANT_CHAIN};
use chimera_core::ui::theme_settings::{AlgoFirst, ThemeSettings};
use chimera_hal::ButtonId;
use screen::*;

fn ops_ui() -> chimera_core::ui::UiState {
    let mut ui = stack_ui();
    let t = ui.theme();
    ui.set_theme(ThemeSettings {
        algo_first: AlgoFirst::Operator,
        ..t
    });
    ui
}

#[test]
fn closed_params_open_ops() {
    DOOR_CELL.close();
    for c in [
        PartChain::Algo,
        PartChain::AlgoFormant,
        PartChain::AlgoOps,
        PartChain::AlgoOpsFormant,
    ] {
        let now = c.ordered(AlgoFirst::Operator);
        assert!(
            matches!(now, PartChain::Algo | PartChain::AlgoFormant),
            "{c:?} closed: {now:?}"
        );
        assert_eq!(now.has_formant_page(), c.has_formant_page());
    }
    // A twin made before the door closed still reads a chain: PARAMS'.
    assert!(std::ptr::eq(PartChain::AlgoOps.def(), &ALGO_CHAIN));
    assert!(std::ptr::eq(
        PartChain::AlgoOpsFormant.def(),
        &ALGO_FORMANT_CHAIN
    ));
    // The keys walk OSC as PARAMS lays it out: WAVE, then down to D1R.
    let mut ui = ops_ui();
    plus(&mut ui, osc_node());
    assert_eq!(ui.page_def().id, ALGO_WAVE.id);
    to_osc_sub(&mut ui, &ALGO_D1R);
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(
        ui.page_def().id,
        chimera_core::ui::block_registry::ALGO_AR.id
    );

    DOOR_CELL.open_host();
    assert_eq!(
        PartChain::Algo.ordered(AlgoFirst::Operator),
        PartChain::AlgoOps
    );
    assert!(std::ptr::eq(PartChain::AlgoOps.def(), &ALGO_OPS_CHAIN));
    assert!(std::ptr::eq(
        PartChain::AlgoOpsFormant.def(),
        &ALGO_OPS_FORMANT_CHAIN
    ));
    let mut ui = ops_ui();
    plus(&mut ui, osc_node());
    assert_eq!(ui.page_def().id, ALGO_OP_PAGES[0].id);
}
