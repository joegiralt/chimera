//! SETTINGS › PERSONALIZE › ALGO PAGES (#985, ADR 0300): FIRST picks which
//! of ALGO's views OSC opens on. Every page stays, only the order changes,
//! and PARAMETER, the default, is the chain as it was.

mod common;
mod screen;

use chimera_core::addr::{BlockRef, Op};
use chimera_core::block::Block;
use chimera_core::block::DiskCode;
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::{BlockDef, ChainDef2};
use chimera_core::ui::block_registry::{
    ALGO_CHAIN, ALGO_D1R, ALGO_FORMANT, ALGO_FORMANT_CHAIN, ALGO_OP_PAGES, ALGO_OPS_CHAIN,
    ALGO_OPS_FORMANT_CHAIN, ALGO_WAVE, SYS_ALGO_PAGES,
};
use chimera_core::ui::nav::PartChain;
use chimera_core::ui::theme_settings::{AlgoFirst, THEME_SPECS, ThemeSettings};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

const P0: PartId = PartId::ALL[0];

/// Every page of `chain`, in the order its keys walk it.
fn pages(chain: &ChainDef2) -> Vec<&'static BlockDef> {
    chain
        .blocks
        .iter()
        .flat_map(|b| std::iter::once(b.def).chain(b.sub_pages.iter().copied()))
        .collect()
}

fn ids(chain: &ChainDef2) -> Vec<u16> {
    let mut v: Vec<u16> = pages(chain).iter().map(|d| d.id).collect();
    v.sort_unstable();
    v
}

fn shorts(chain: &ChainDef2, node: usize) -> Vec<&'static str> {
    let b = &chain.blocks[node];
    std::iter::once(b.def)
        .chain(b.sub_pages.iter().copied())
        .map(|d| d.short)
        .collect()
}

/// ALGO PAGES' FIRST at `first`, the rest of the theme as it is.
fn set_first(ui: &mut UiState, first: AlgoFirst) {
    let t = ui.theme();
    ui.set_theme(ThemeSettings {
        algo_first: first,
        ..t
    });
}

#[test]
fn parameter_is_the_default_and_the_chain_as_it_was() {
    assert_eq!(ThemeSettings::DEFAULT.algo_first, AlgoFirst::Parameter);
    let s = THEME_SPECS
        .iter()
        .find(|s| s.id == ThemeSettings::ALGO_FIRST)
        .unwrap();
    assert_eq!(
        (s.ident, s.label, s.default, s.max),
        ("ALGO_FIRST", "FIRST", 0.0, 1.0)
    );
    let init = Sound::init(EngineType::Algo);
    assert_eq!(PartChain::of(&init), PartChain::Algo);
    assert_eq!(
        PartChain::Algo.ordered(AlgoFirst::Parameter),
        PartChain::Algo
    );
    assert!(std::ptr::eq(PartChain::Algo.def(), &ALGO_CHAIN));
    assert_eq!(
        shorts(&ALGO_CHAIN, osc_node()),
        [
            "OSC", "CRS", "FIN", "DET", "LVL", "VEL", "AR", "D1R", "D1L", "D2R", "RR", "RS", "FBK",
            "OP1", "OP2", "OP3", "OP4", "OP5", "OP6"
        ]
    );
    let ui = UiState::new();
    assert_eq!(ui.theme().algo_first, AlgoFirst::Parameter);
}

/// Both orders reach every ALGO page, each once, on the same nodes.
#[test]
fn both_orders_hold_every_page() {
    for (param, ops) in [
        (&ALGO_CHAIN, &ALGO_OPS_CHAIN),
        (&ALGO_FORMANT_CHAIN, &ALGO_OPS_FORMANT_CHAIN),
    ] {
        assert_eq!(ids(param), ids(ops));
        let mut once = ids(ops);
        once.dedup();
        assert_eq!(once.len(), pages(ops).len(), "a page twice");
        assert_eq!(param.len(), ops.len());
        for (n, (a, b)) in param.blocks.iter().zip(ops.blocks).enumerate() {
            let label = |c: &chimera_core::ui::block_def::ChainBlock| c.map.unwrap_or(c.def.short);
            assert_eq!(label(a), label(b), "node {n} on the map");
            if n != osc_node() {
                assert_eq!(shorts(param, n), shorts(ops, n), "node {n}");
            }
        }
        assert_eq!(param.home(), ops.home());
    }
}

/// OPERATOR: OP1 is OSC's home, then OP2 to OP6, then WAVE and the
/// parameter pages in their own order; FORMANT follows WAVE as it does.
#[test]
fn operator_first_lays_osc_out_by_operator() {
    assert_eq!(
        shorts(&ALGO_OPS_CHAIN, osc_node()),
        [
            "OP1", "OP2", "OP3", "OP4", "OP5", "OP6", "OSC", "CRS", "FIN", "DET", "LVL", "VEL",
            "AR", "D1R", "D1L", "D2R", "RR", "RS", "FBK"
        ]
    );
    assert_eq!(
        shorts(&ALGO_OPS_FORMANT_CHAIN, osc_node())[..9],
        [
            "OP1", "OP2", "OP3", "OP4", "OP5", "OP6", "OSC", "FRM", "CRS"
        ]
    );
    assert_eq!(ALGO_OPS_CHAIN.blocks[osc_node()].map, Some("OSC"));
    for (was, first, now) in [
        (PartChain::Algo, AlgoFirst::Operator, PartChain::AlgoOps),
        (
            PartChain::AlgoFormant,
            AlgoFirst::Operator,
            PartChain::AlgoOpsFormant,
        ),
        (PartChain::AlgoOps, AlgoFirst::Parameter, PartChain::Algo),
        (
            PartChain::AlgoOpsFormant,
            AlgoFirst::Parameter,
            PartChain::AlgoFormant,
        ),
        (PartChain::AlgoOps, AlgoFirst::Operator, PartChain::AlgoOps),
        (PartChain::Modal, AlgoFirst::Operator, PartChain::Modal),
        (PartChain::Tape, AlgoFirst::Operator, PartChain::Tape),
        (PartChain::Va, AlgoFirst::Operator, PartChain::Va),
        (PartChain::Drum, AlgoFirst::Operator, PartChain::Drum),
    ] {
        assert_eq!(was.ordered(first), now);
        assert_eq!(now.engine(), was.engine());
    }
    assert!(std::ptr::eq(PartChain::AlgoOps.def(), &ALGO_OPS_CHAIN));
    assert!(std::ptr::eq(
        PartChain::AlgoOpsFormant.def(),
        &ALGO_OPS_FORMANT_CHAIN
    ));
    assert!(PartChain::AlgoOpsFormant.has_formant_page());
    assert!(!PartChain::AlgoOps.has_formant_page());
}

/// The keys walk it: PLUS to OSC lands on OP1, EDIT steps OP2 to OP6,
/// then WAVE and on down to FBK.
#[test]
fn the_keys_walk_osc_operator_first() {
    let mut ui = stack_ui();
    set_first(&mut ui, AlgoFirst::Operator);
    plus(&mut ui, osc_node());
    let want = pages(&ALGO_OPS_CHAIN);
    let osc: Vec<_> = std::iter::once(ALGO_OPS_CHAIN.blocks[osc_node()].def)
        .chain(ALGO_OPS_CHAIN.blocks[osc_node()].sub_pages.iter().copied())
        .collect();
    assert!(want.iter().any(|d| d.id == osc[0].id));
    assert_eq!(ui.page_def().id, ALGO_OP_PAGES[0].id);
    for d in &osc[1..] {
        feed(&mut ui, Input::press(ButtonId::Edit));
        assert_eq!(ui.page_def().id, d.id, "{}", d.short);
    }
    assert_eq!(osc[6].id, ALGO_WAVE.id);
}

/// The cell: PERSONALIZE › ALGO PAGES › FIRST. Turned while a Part sits on
/// a page, MENU comes back to that page, now in the other order.
#[test]
fn turning_first_keeps_the_page_and_reorders_the_chain() {
    let mut ui = stack_ui();
    to_osc(&mut ui);
    to_osc_sub(&mut ui, &ALGO_D1R);
    to_leaf(&mut ui, &["PERSONALIZE", "ALGO PAGES"]);
    assert_eq!(ui.page_def().id, SYS_ALGO_PAGES.id);
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_eq!(ui.theme().algo_first, AlgoFirst::Operator);
    for _ in 0..8 {
        if !ui.in_settings() {
            break;
        }
        tap(&mut ui, ButtonId::Menu);
    }
    assert_eq!(ui.page_def().id, ALGO_D1R.id, "the page is kept by its id");
    // SEQ steps up the operator-first order: AR, VEL … CRS, WAVE, then
    // OP6, where PARAMETER's order has WAVE for its home.
    let up = ALGO_OPS_CHAIN.blocks[osc_node()].sub_pages;
    let at = up.iter().position(|d| d.id == ALGO_D1R.id).unwrap();
    for k in 1..=at - 4 {
        tap(&mut ui, ButtonId::Seq);
        assert_eq!(ui.page_def().id, up[at - k].id, "{k} up");
    }
    assert_eq!(ui.page_def().id, ALGO_OP_PAGES[5].id);
    // And back to PARAMETER: the same page, the old order.
    to_leaf(&mut ui, &["PERSONALIZE", "ALGO PAGES"]);
    feed(&mut ui, Input::turn(EncoderId::A, -1));
    assert_eq!(ui.theme().algo_first, AlgoFirst::Parameter);
    for _ in 0..8 {
        if !ui.in_settings() {
            break;
        }
        tap(&mut ui, ButtonId::Menu);
    }
    assert_eq!(ui.page_def().id, ALGO_OP_PAGES[5].id);
}

/// A SYSTEM file's order lands while a Part's page is up: the page stays.
#[test]
fn a_loaded_order_keeps_the_page_on_show() {
    let mut ui = stack_ui();
    to_osc(&mut ui);
    to_osc_sub(&mut ui, &ALGO_OP_PAGES[2]);
    set_first(&mut ui, AlgoFirst::Operator);
    assert_eq!(ui.page_def().id, ALGO_OP_PAGES[2].id);
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.page_def().id, ALGO_OP_PAGES[3].id);
    set_first(&mut ui, AlgoFirst::Parameter);
    assert_eq!(ui.page_def().id, ALGO_OP_PAGES[3].id);
}

/// A formant operator brings FORMANT's page in either order.
#[test]
fn the_formant_page_joins_either_order() {
    let mut ui = stack_ui();
    set_first(&mut ui, AlgoFirst::Operator);
    plus(&mut ui, osc_node());
    let was = PartChain::of(&ui.project().part(P0).sound).ordered(AlgoFirst::Operator);
    let mut s = Sound::clone(&ui.project().part(P0).sound);
    s.params.algo.try_set_kind(1, OpKind::Formant).unwrap();
    **ui.project_mut().edit_part(P0).sound = s;
    ui.replaced_for_test(was);
    assert_eq!(ui.page_def().id, ALGO_OP_PAGES[0].id);
    let mut seen = vec![ui.page_def().id];
    for _ in 0..19 {
        feed(&mut ui, Input::press(ButtonId::Edit));
        seen.push(ui.page_def().id);
    }
    let want: Vec<u16> = std::iter::once(ALGO_OPS_FORMANT_CHAIN.blocks[osc_node()].def)
        .chain(
            ALGO_OPS_FORMANT_CHAIN.blocks[osc_node()]
                .sub_pages
                .iter()
                .copied(),
        )
        .map(|d| d.id)
        .collect();
    assert_eq!(seen, want);
    assert!(seen.contains(&ALGO_FORMANT.id));
}

/// FIRST is SYSTEM's, a frozen code; a SYSTEM from before has none and
/// reads PARAMETER.
#[test]
fn first_is_stored_by_frozen_code() {
    assert_eq!(
        AlgoFirst::ALL.map(|a| (a.disk_code(), a.disk_ident())),
        [(0, "PARAMETER"), (1, "OPERATOR")]
    );
    for a in AlgoFirst::ALL {
        assert_eq!(AlgoFirst::from_disk_code(a.disk_code()), Some(a));
        let mut t = ThemeSettings::DEFAULT;
        assert!(t.set_enum_code(ThemeSettings::ALGO_FIRST, a.disk_code()));
        assert_eq!(t.algo_first, a);
        assert_eq!(t.enum_code(ThemeSettings::ALGO_FIRST), Some(a.disk_code()));
        assert_eq!(
            t.enum_ident(ThemeSettings::ALGO_FIRST),
            Some(a.disk_ident())
        );
    }
    assert_eq!(AlgoFirst::from_disk_code(2), None);
    let mut t = ThemeSettings::DEFAULT;
    assert!(!t.set_enum_code(ThemeSettings::ALGO_FIRST, 2));
    assert_eq!(t.algo_first, AlgoFirst::Parameter);
    let _ = (BlockRef::Theme, Op::A);
}
