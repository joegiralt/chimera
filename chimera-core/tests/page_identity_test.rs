//! A page is kept by its `BlockDef.id`, not its position (ADR P).

use chimera_core::dsp::reverb::ReverbType;
use chimera_core::project::PartId;
use chimera_core::ui::block_def::{ChainBlock, ChainDef2, refocus};
use chimera_core::ui::block_registry::{
    ALGO_ALG, ALGO_COARSE, ALGO_DETUNE, ALGO_FINE, ALGO_LEVEL, ALGO_WAVE,
};
use chimera_core::ui::nav::{Location, NavCtx, NavKey, OrbitCx, PartChain, Recall, Step};

static OLD_SUBS: [&chimera_core::ui::block_def::BlockDef; 3] =
    [&ALGO_COARSE, &ALGO_FINE, &ALGO_DETUNE];
static NEW_SUBS: [&chimera_core::ui::block_def::BlockDef; 4] =
    [&ALGO_LEVEL, &ALGO_COARSE, &ALGO_FINE, &ALGO_DETUNE];
static OLD_BLOCKS: [ChainBlock; 2] = [
    ChainBlock::page(&ALGO_ALG),
    ChainBlock::with_subs(&ALGO_WAVE, &OLD_SUBS),
];
static NEW_BLOCKS: [ChainBlock; 2] = [
    ChainBlock::page(&ALGO_ALG),
    ChainBlock::with_subs(&ALGO_WAVE, &NEW_SUBS),
];
static SHORT_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&ALGO_ALG)];
static OLD: ChainDef2 = ChainDef2::new("old", &OLD_BLOCKS, &[]).with_home_def(&ALGO_WAVE);
static NEW: ChainDef2 = ChainDef2::new("new", &NEW_BLOCKS, &[]).with_home_def(&ALGO_WAVE);
static SHORT: ChainDef2 = ChainDef2::new("short", &SHORT_BLOCKS, &[]);

#[test]
fn refocus_keeps_the_page_by_identity() {
    let at = OLD.locate(ALGO_DETUNE.id).unwrap();
    assert_eq!(
        NEW.def_at(refocus(&OLD, at, &NEW)).unwrap().id,
        ALGO_DETUNE.id
    );
    let lvl = NEW.locate(ALGO_LEVEL.id).unwrap();
    assert_eq!(refocus(&NEW, lvl, &OLD), OLD.page(1, 0).unwrap());
    assert_eq!(refocus(&OLD, at, &SHORT), SHORT.home());
    assert_eq!(OLD.locate(ALGO_LEVEL.id), None);
}

fn cx() -> NavCtx {
    NavCtx {
        chains: [PartChain::Algo; 6],
        dyn_rows: 0,
        orbit: OrbitCx::NONE,
        rev: ReverbType::Quad,
        pages: chimera_core::ui::theme_settings::Pages::Stack,
        opens: Default::default(),
    }
}

fn go(l: &mut Location, k: NavKey, cx: &NavCtx, r: &mut Recall) {
    if let Step::Go(to) = l.step(k, cx, r) {
        *l = to;
    }
}

const P1: PartId = PartId::ALL[1];

#[test]
fn an_engine_change_still_goes_home() {
    let mut cx = cx();
    let detune = PartChain::Algo.def().locate(ALGO_DETUNE.id).unwrap();
    let mut r = Recall::new();
    let mut l = Location::pages(P1, detune);
    go(&mut l, NavKey::MixPart(P1), &cx, &mut r);
    cx.chains[1] = PartChain::Modal;
    go(&mut l, NavKey::Part(P1), &cx, &mut r);
    assert_eq!(l, Location::part_home(P1, PartChain::Modal.engine()));
}

#[test]
fn the_mixer_round_trip_returns_by_identity() {
    let cx = cx();
    let def = PartChain::Algo.def();
    let detune = def.locate(ALGO_DETUNE.id).unwrap();
    let mut r = Recall::new();
    let mut l = Location::pages(P1, detune);
    go(&mut l, NavKey::Part(P1), &cx, &mut r);
    assert_ne!(l, Location::pages(P1, detune), "on the mixer");
    go(&mut l, NavKey::Part(P1), &cx, &mut r);
    assert_eq!(l, Location::pages(P1, detune));
    let (c, at) = l.page(&cx).unwrap();
    assert_eq!(c.def_at(at).unwrap().id, ALGO_DETUNE.id);
}

/// The Part's pages kept by identity when its chain gains FRM meanwhile
/// (a different list: the position shifts, the page does not).
#[test]
fn the_mixer_round_trip_keeps_the_page_across_a_formant_edit() {
    let mut cx = cx();
    let detune = PartChain::Algo.def().locate(ALGO_DETUNE.id).unwrap();
    let mut r = Recall::new();
    let mut l = Location::pages(P1, detune);
    go(&mut l, NavKey::Part(P1), &cx, &mut r);
    cx.chains[1] = PartChain::AlgoFormant;
    go(&mut l, NavKey::Part(P1), &cx, &mut r);
    let (c, at) = l.page(&cx).unwrap();
    assert_eq!(c.def_at(at).unwrap().id, ALGO_DETUNE.id);
    assert_ne!(at, detune, "FRM shifted it");
    // And back, with FRM gone while away.
    go(&mut l, NavKey::Part(P1), &cx, &mut r);
    cx.chains[1] = PartChain::Algo;
    go(&mut l, NavKey::Part(P1), &cx, &mut r);
    let (c, at) = l.page(&cx).unwrap();
    assert_eq!(c.def_at(at).unwrap().id, ALGO_DETUNE.id);
}

#[test]
fn rechained_keeps_the_page_across_the_algo_chains() {
    let cx = NavCtx {
        chains: [PartChain::AlgoFormant; 6],
        ..cx()
    };
    let was = PartChain::Algo;
    let detune = was.def().locate(ALGO_DETUNE.id).unwrap();
    let l = Location::pages(P1, detune).rechained(P1, was, &cx);
    let (c, at) = l.page(&cx).unwrap();
    assert_eq!(c.def_at(at).unwrap().id, ALGO_DETUNE.id);
    // Off a page FRM-less chains lack, the node's first.
    let frm = PartChain::AlgoFormant
        .def()
        .locate(chimera_core::ui::block_registry::ALGO_FORMANT.id)
        .unwrap();
    let cx = NavCtx {
        chains: [PartChain::Algo; 6],
        ..cx
    };
    let l = Location::pages(P1, frm).rechained(P1, PartChain::AlgoFormant, &cx);
    let (c, at) = l.page(&cx).unwrap();
    assert_eq!(c.def_at(at).unwrap().id, ALGO_WAVE.id);
}
