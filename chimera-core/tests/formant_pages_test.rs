// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The FORMANT page and VOWEL's dimming: the Algo chain gains FRM while an
//! operator is a formant or a noise band, and every page is kept by id
//! through the change (formant-operators plan, Task 13).

mod screen;

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::factory::{Demo, demo_sound};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::{ALGO_DETUNE, ALGO_FORMANT, ALGO_OP_PAGES, ALGO_WAVE};
use chimera_core::ui::nav::PartChain;
use chimera_core::ui::view::dimmed;
use chimera_hal::EncoderId;
use chimera_hal::testkit::MemStore;
use screen::*;

const P0: PartId = PartId::ALL[0];

fn on_op1() -> Box<UiState> {
    let mut ui = Box::new(crate::screen::stack_ui());
    init_part(&mut ui, P0);
    to_osc(&mut ui);
    to_osc_sub(&mut ui, &ALGO_OP_PAGES[0]);
    ui
}

fn chain(ui: &UiState) -> PartChain {
    PartChain::of(&ui.project().part(P0).sound)
}

fn addr(op: Op, id: chimera_core::block::ParamId) -> ParamAddr {
    ParamAddr::new(BlockRef::AlgoOp(op), id)
}

#[test]
fn pages_adapt_to_formant_kinds() {
    let init = Sound::init(EngineType::Algo);
    assert_eq!(PartChain::of(&init), PartChain::Algo);
    assert!(PartChain::Algo.def().locate(ALGO_FORMANT.id).is_none());
    let vowel = ParamAddr::new(BlockRef::Algo, AlgoParams::VOWEL);
    assert!(dimmed(vowel, &init));

    // A WAVE edit through the encoder, on OP1's page: the page is kept.
    let mut ui = on_op1();
    feed(&mut ui, Input::turn(EncoderId::A, 16));
    assert_eq!(
        ui.project().part(P0).sound.params.algo.kinds().get(0),
        OpKind::Formant
    );
    assert_eq!(chain(&ui), PartChain::AlgoFormant);
    assert_eq!(page_def_id(&ui), ALGO_OP_PAGES[0].id);
    let at = PartChain::AlgoFormant
        .def()
        .locate(ALGO_FORMANT.id)
        .unwrap();
    assert_eq!((at.node(), at.sub()), (1, 1));
    let s = &ui.project().part(P0).sound;
    assert!(dimmed(addr(Op::B, AlgoOpParams::FRMT), s));
    assert!(!dimmed(addr(Op::A, AlgoOpParams::FRMT), s));
    assert!(!dimmed(vowel, s));

    // EDIT walks the new list (it wraps): FRM sits right after OSC.
    to_osc_sub(&mut ui, &ALGO_FORMANT);
    assert_eq!(page_def_id(&ui), ALGO_FORMANT.id);
}

#[test]
fn nois_dims_what_it_ignores() {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo.try_set_kind(0, OpKind::Noise).unwrap();
    for id in [
        AlgoOpParams::COARSE,
        AlgoOpParams::FINE,
        AlgoOpParams::DETUNE,
        AlgoOpParams::FEEDBACK,
        AlgoOpParams::SKIRT,
        AlgoOpParams::KEY,
    ] {
        assert!(dimmed(addr(Op::A, id), &s), "{id:?}");
    }
    for id in [
        AlgoOpParams::FRMT,
        AlgoOpParams::BW,
        AlgoOpParams::RES,
        AlgoOpParams::LEVEL,
    ] {
        assert!(!dimmed(addr(Op::A, id), &s), "{id:?}");
    }
    // A formant has no RES; a table has none of the formant cells.
    s.params.algo.try_set_kind(0, OpKind::Formant).unwrap();
    assert!(dimmed(addr(Op::A, AlgoOpParams::RES), &s));
    assert!(!dimmed(addr(Op::A, AlgoOpParams::SKIRT), &s));
    assert!(dimmed(addr(Op::B, AlgoOpParams::VSLOT), &s));
}

#[test]
fn the_page_walk_has_frm_exactly_when_a_formant_exists() {
    let has = |s: &Sound| PartChain::of(s).def().locate(ALGO_FORMANT.id).is_some();
    assert!(!has(&Sound::init(EngineType::Algo)));
    assert!(!has(&Sound::init(EngineType::Modal)));
    assert!(has(&demo_sound(Demo::Breath)));
}

#[test]
fn a_project_load_rechains_by_id() {
    let mut store = MemStore::new(1);
    let plain = put_with(&mut store, "PLAIN", |p| {
        **p.edit_part(P0).sound = Sound::init(EngineType::Algo);
    });
    let formant = put_with(&mut store, "FORMANT", |p| {
        **p.edit_part(P0).sound = demo_sound(Demo::Breath);
    });
    let mut ui = Box::new(crate::screen::stack_ui());
    let mut rig = Rig::new(&mut ui, store);
    rig.load(plain);
    to_osc(rig.ui);
    to_osc_sub(rig.ui, &ALGO_DETUNE);
    assert_eq!(chain(rig.ui), PartChain::Algo);

    // Into a formant project, from DET: kept by id.
    rig.load(formant);
    assert_eq!(chain(rig.ui), PartChain::AlgoFormant);
    assert_eq!(page_def_id(rig.ui), ALGO_DETUNE.id);

    // And out of it, from FRM: the node's first page.
    to_osc_sub(rig.ui, &ALGO_FORMANT);
    rig.load(plain);
    assert_eq!(chain(rig.ui), PartChain::Algo);
    assert_eq!(page_def_id(rig.ui), ALGO_WAVE.id);
}
