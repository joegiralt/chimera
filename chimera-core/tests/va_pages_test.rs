//! VA's pages (spec § Pages, ADR 0112 point 8): VA, OSC 1, OSC 2.

mod common;
mod screen;

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::va::{MODE_NAMES, VaParams};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry::{PITCH, VA_CHAIN, VA_HOME, VA_OSC1, VA_OSC2};
use chimera_core::ui::view::{self, SlotCtx};
use chimera_core::ui::{draw, nav, theme};
use chimera_hal::ButtonId;
use screen::{load_init, page_def_id, tap};

type P = VaParams;

fn ctx() -> SlotCtx {
    SlotCtx::read(&Sound::init(EngineType::Va).params, Op::A)
}

fn labels(def: &BlockDef) -> Vec<&'static str> {
    let c = ctx();
    (0..6).map(|i| view::view(def, i, &c).label()).collect()
}

fn addrs(def: &BlockDef) -> Vec<Option<ParamAddr>> {
    let c = ctx();
    (0..6).map(|i| view::view(def, i, &c).addr()).collect()
}

fn va(ids: [Option<chimera_core::block::ParamId>; 6]) -> Vec<Option<ParamAddr>> {
    ids.map(|i| i.map(|i| ParamAddr::new(BlockRef::Va, i)))
        .to_vec()
}

#[test]
fn chain_is_va_then_the_voice_chain() {
    let map: Vec<_> = VA_CHAIN
        .blocks
        .iter()
        .map(|b| b.map.unwrap_or(b.def.short))
        .collect();
    assert_eq!(map, ["VA", "FLT", "AMP", "MOD"]);
    assert_eq!((VA_CHAIN.home().node(), VA_CHAIN.home().sub()), (0, 0));
    let subs: Vec<u16> = VA_CHAIN.blocks[0].sub_pages.iter().map(|d| d.id).collect();
    assert_eq!(subs, [88, 89, PITCH.id]);
    assert_eq!((VA_HOME.id, VA_OSC1.id, VA_OSC2.id), (87, 88, 89));
}

#[test]
fn va_page_cells() {
    assert_eq!(
        labels(&VA_HOME),
        ["WAVE1", "WAVE2", "MODE", "BLEND", "SUB", "STABILITY"]
    );
    assert_eq!(
        addrs(&VA_HOME),
        va([P::WAVE1, P::WAVE2, P::MODE, P::AMOUNT, P::SUB, P::STAB].map(Some))
    );
}

#[test]
fn osc_page_cells() {
    for (def, ids) in [
        (&VA_OSC1, [P::CRSE1, P::FINE1, P::SHAPE1]),
        (&VA_OSC2, [P::CRSE2, P::FINE2, P::SHAPE2]),
    ] {
        assert_eq!(labels(def), ["CRSE", "FINE", "SHAPE", "--", "--", "--"]);
        let [a, b, c] = ids;
        assert_eq!(
            addrs(def),
            va([Some(a), Some(b), Some(c), None, None, None])
        );
    }
}

#[test]
fn bars_follow_motion() {
    let c = ctx();
    for def in [&VA_HOME, &VA_OSC1, &VA_OSC2] {
        for i in 0..6 {
            let Some(a) = view::view(def, i, &c).addr() else {
                continue;
            };
            let want = !matches!(a.param, P::MODE | P::CRSE1 | P::CRSE2);
            assert_eq!(
                a.spec().unwrap().motion().is_continuous(),
                want,
                "{}",
                a.spec().unwrap().label
            );
        }
    }
}

#[test]
fn va_pages_have_no_gestures() {
    let c = ctx();
    for def in [&VA_HOME, &VA_OSC1, &VA_OSC2] {
        for i in 0..6 {
            // MIX + SEQ's learn, every mappable cell's, and no other.
            let h = view::view(def, i, &c).hint(&c);
            assert!(h.iter().all(|g| *g == view::LEARN));
        }
    }
}

#[test]
fn mode_names_fit_the_cell() {
    let c = ctx();
    for name in MODE_NAMES {
        let w = draw::text_width(&theme::FONT_VALUE, name, 0);
        // the cell's value slot (a column less its gutter); a MODE has no bar
        assert!(w <= theme::CELL_COL_W - 4, "{name}: {w} px");
    }
    for def in [&VA_HOME, &VA_OSC1, &VA_OSC2] {
        for i in 0..6 {
            // The cell draws its label in FONT_LABEL, tracked.
            let l = view::view(def, i, &c).label();
            let w = draw::text_width(&theme::FONT_LABEL, l, theme::LABEL_TRACKING);
            assert!(w <= theme::CELL_BAR_W, "{l}: {w} px");
        }
    }
}

#[test]
fn a_va_load_lands_on_va() {
    let mut ui = crate::screen::stack_ui();
    load_init(&mut ui, EngineType::Va);
    assert_eq!(page_def_id(&ui), VA_HOME.id);
    assert!(core::ptr::eq(nav::chain_def_for(EngineType::Va), &VA_CHAIN));
}

#[test]
fn edit_walks_and_wraps() {
    let mut ui = crate::screen::stack_ui();
    load_init(&mut ui, EngineType::Va);
    let mut walk = Vec::new();
    for _ in 0..4 {
        tap(&mut ui, ButtonId::Edit);
        walk.push(page_def_id(&ui));
    }
    assert_eq!(walk, [VA_OSC1.id, VA_OSC2.id, PITCH.id, VA_HOME.id]);
}
