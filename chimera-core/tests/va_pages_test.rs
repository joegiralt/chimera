//! VA's pages (spec § Pages, ADR 0112 point 8): VA, OSC 1, OSC 2.

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::va::{MODE_NAMES, VaParams};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry::{PITCH, VA_CHAIN, VA_HOME, VA_OSC1, VA_OSC2};
use chimera_core::ui::view::{self, SlotCtx};
use chimera_core::ui::{draw, hint, theme};

type P = VaParams;

fn ctx() -> SlotCtx {
    SlotCtx::read(&Sound::init(EngineType::Algo).params, Op::A)
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
    assert_eq!(map, ["VA", "DRV", "FLT", "AMP", "MOD"]);
    assert_eq!((VA_CHAIN.home().node(), VA_CHAIN.home().sub()), (0, 0));
    let subs: Vec<u16> = VA_CHAIN.blocks[0].sub_pages.iter().map(|d| d.id).collect();
    assert_eq!(subs, [88, 89, PITCH.id]);
    assert_eq!((VA_HOME.id, VA_OSC1.id, VA_OSC2.id), (87, 88, 89));
}

#[test]
fn va_page_cells() {
    assert_eq!(
        labels(&VA_HOME),
        ["WAVE1", "WAVE2", "MODE", "AMNT", "SUB", "STAB"]
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
            assert!(view::view(def, i, &c).hint() == hint::NONE);
        }
    }
}

#[test]
fn mode_names_fit_the_cell() {
    let c = ctx();
    for name in MODE_NAMES {
        let w = draw::text_width(&theme::FONT_VALUE, name, 0);
        assert!(w <= theme::CELL_BAR_W, "{name}: {w} px");
    }
    for def in [&VA_HOME, &VA_OSC1, &VA_OSC2] {
        for i in 0..6 {
            let l = view::view(def, i, &c).label();
            assert!(
                draw::text_width(&theme::FONT_VALUE, l, 0) <= theme::CELL_COL_W - 4,
                "{l}"
            );
        }
    }
}
