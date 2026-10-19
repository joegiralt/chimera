//! OP1–OP6 are OSC sub-pages after the twelve group pages (ADR 0130).

mod common;
mod screen;

use chimera_core::addr::{BlockRead, BlockRef, Op, ParamAddr};
use chimera_core::dsp::algo::formant::Skirt;
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::factory::{Demo, demo_sound};
use chimera_core::modulation::ModSource;
use chimera_core::params::EngineType;
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::ui::block_registry::{ALGO_CHAIN, ALGO_FEEDBACK, ALGO_OP_PAGES, MODAL_1};
use chimera_core::ui::op_page::{band_curve, grain_curve, shown};
use chimera_core::ui::view::{SlotCtx, view};
use chimera_core::ui::{UiState, draw, part_page, theme};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use common::sweep::{Bench, Patch};
use screen::*;

const P0: PartId = PartId::ALL[0];

#[test]
fn osc_sub_row_ends_with_the_six_operator_pages() {
    let osc = ALGO_CHAIN.block_at(1).unwrap();
    let shorts: Vec<_> = osc.sub_pages.iter().map(|d| d.short).collect();
    assert_eq!(
        &shorts[shorts.len() - 6..],
        ["OP1", "OP2", "OP3", "OP4", "OP5", "OP6"]
    );
    assert_eq!(
        &shorts[..12],
        [
            "CRS", "FIN", "DET", "LVL", "VEL", "AR", "D1R", "D1L", "D2R", "RR", "RS", "FBK"
        ]
    );
}

#[test]
fn op_page_cells_edit_their_operator() {
    let ids = [
        AlgoOpParams::WAVE,
        AlgoOpParams::COARSE,
        AlgoOpParams::FINE,
        AlgoOpParams::LEVEL,
        AlgoOpParams::FEEDBACK,
        AlgoOpParams::VELOCITY,
    ];
    for (n, op) in Op::ALL.into_iter().enumerate() {
        for (k, id) in ids.into_iter().enumerate() {
            // A cell at its top only moves down.
            let moved = |m: Op, d: i8| {
                let mut s = Sound::init(EngineType::Algo);
                let before = s.params.clone();
                part_page::apply_encoder(&ALGO_OP_PAGES[n], k, d, &mut s.params, &mut Op::A);
                s.params.block(BlockRef::AlgoOp(m)).unwrap().get(id)
                    != before.block(BlockRef::AlgoOp(m)).unwrap().get(id)
            };
            for m in Op::ALL {
                assert_eq!(moved(m, 1) || moved(m, -1), m == op, "OP{} cell {k}", n + 1);
            }
        }
    }
}

#[test]
fn edit_and_seq_walk_into_and_out_of_the_op_pages() {
    let mut ui = crate::screen::stack_ui();
    init_part(&mut ui, PartId::ALL[0]);
    to_osc(&mut ui);
    let osc = page_def_id(&ui);
    for _ in 0..13 {
        feed(&mut ui, Input::press(ButtonId::Edit));
    }
    assert_eq!(page_def_id(&ui), ALGO_OP_PAGES[0].id, "13 EDITs: OP1");
    for (n, def) in ALGO_OP_PAGES.iter().enumerate().skip(1) {
        feed(&mut ui, Input::press(ButtonId::Edit));
        assert_eq!(page_def_id(&ui), def.id, "OP{}", n + 1);
    }
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(page_def_id(&ui), osc, "EDIT wraps from OP6 to OSC");
    for _ in 0..18 {
        feed(&mut ui, Input::press(ButtonId::Edit));
    }
    assert_eq!(page_def_id(&ui), ALGO_OP_PAGES[5].id);
    for _ in 0..6 {
        tap(&mut ui, ButtonId::Seq);
    }
    assert_eq!(page_def_id(&ui), ALGO_FEEDBACK.id, "SEQ 6 from OP6: FBK");
}

#[test]
fn op_pages_show_each_kinds_cells() {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo.try_set_kind(1, OpKind::Formant).unwrap();
    s.params.algo.try_set_kind(2, OpKind::Noise).unwrap();
    let ctx = SlotCtx::read(&s.params, Op::A);
    let labels = |n: usize| {
        (0..6)
            .map(|i| view(&ALGO_OP_PAGES[n], i, &ctx).label())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        labels(0),
        ["WAVE", "COARSE", "FINE", "LEVEL", "FDBK", "VELOCITY"]
    );
    assert_eq!(labels(1), ["WAVE", "FORMANT", "BW", "SKIRT", "KEY", "SLOT"]);
    assert_eq!(
        labels(2),
        ["WAVE", "CENTRE", "WIDTH", "RES", "SLOT", "LEVEL"]
    );
    for n in 0..3 {
        for l in labels(n) {
            let w = draw::text_width(&theme::FONT_LABEL, l, theme::LABEL_TRACKING);
            assert!(w <= theme::CELL_BAR_W, "{l}: {w} px");
        }
    }
    // Each cell edits its own operator's param.
    let at = |n: usize, i: usize| view(&ALGO_OP_PAGES[n], i, &ctx).addr().unwrap();
    assert_eq!(at(1, 1).block, BlockRef::AlgoOp(Op::B));
    assert_eq!(at(1, 1).param, AlgoOpParams::FRMT);
    assert_eq!(at(2, 2).param, AlgoOpParams::BW);
    assert_eq!(at(2, 5).param, AlgoOpParams::LEVEL);
}

/// Part 1 on a plain ALGO INIT, at OSC's sub-page `def`.
fn on_page(rig: &mut Rig, def: &chimera_core::ui::block_def::BlockDef) {
    to_osc(rig.ui);
    to_osc_sub(rig.ui, def);
}

fn cell_label(ui: &UiState, i: usize) -> &'static str {
    let ctx = SlotCtx::read(ui.params(), ui.selected_op());
    view(ui.page_def(), i, &ctx).label()
}

#[test]
fn a_kind_change_on_an_op_page_keeps_it() {
    let mut store = MemStore::new(1);
    let plain = put_with(&mut store, "PLAIN", |p| {
        **p.edit_part(P0).sound = Sound::init(EngineType::Algo);
    });
    let modal = put_with(&mut store, "MODAL", |p| {
        **p.edit_part(P0).sound = Sound::init(EngineType::Modal);
    });
    let mut ui = Box::new(crate::screen::stack_ui());
    let mut rig = Rig::new(&mut ui, store);
    rig.load(plain);
    on_page(&mut rig, &ALGO_OP_PAGES[1]);
    // The page's own WAVE cell, W1 to FRMT: the list gains FORMANT.
    feed(rig.ui, Input::turn(EncoderId::A, 16));
    let kind = |ui: &UiState| ui.params().algo.kinds().get(1);
    assert_eq!(kind(rig.ui), OpKind::Formant);
    assert_eq!(page_def_id(rig.ui), ALGO_OP_PAGES[1].id);
    assert_eq!(cell_label(rig.ui, 1), "FORMANT");
    feed(rig.ui, Input::turn(EncoderId::A, -16));
    assert_eq!(kind(rig.ui), OpKind::Table(WaveId::W1));
    assert_eq!(
        (page_def_id(rig.ui), cell_label(rig.ui, 1)),
        (ALGO_OP_PAGES[1].id, "COARSE")
    );
    // A project load that changes the engine: MODAL's home (ADR 0066).
    rig.load(modal);
    assert_eq!(page_def_id(rig.ui), MODAL_1.id);
}

#[test]
fn grain_curve_is_one_odd_grain() {
    let g = grain_curve(Skirt::S1, 4.0);
    assert_eq!((g[0], g[63]), (0, 0));
    for i in 0..32 {
        assert!((g[i] as i16 + g[63 - i] as i16).abs() <= 1, "{i}");
    }
}

#[test]
fn band_curve_is_a_bell_at_its_centre() {
    let b = band_curve(1000.0, 200.0);
    // 20 Hz to 20 kHz across 64 points: 1 kHz sits at 63·log(50)/log(1000).
    let top = (0..64).max_by_key(|&i| b[i]).unwrap();
    assert!((35..=36).contains(&top), "{top}");
    assert!(b[top] >= 230, "{}", b[top]);
    assert!(b[..top].windows(2).all(|w| w[0] <= w[1]));
    assert!(b[top..].windows(2).all(|w| w[0] >= w[1]));
    assert!(b[0] < 10 && b[63] < 10);
    // Wider is wider.
    let wide = band_curve(1000.0, 2000.0);
    assert!(wide[20] > b[20] && wide[50] > b[50]);
}

/// The UI beside the audio: each block publishes the voices' views, as
/// the shells do, and the UI takes Part 1's.
struct Live {
    ui: Box<UiState>,
    bench: Bench,
    held: Option<u8>,
}

/// 50 ms of blocks.
const SETTLE: usize = 75;

impl Live {
    fn new(s: Sound) -> Live {
        let mut bench = Bench::new(Patch::AlgoInit, PartParams::default(), FxParams::default());
        bench.edit_sound(|b| *b = s);
        Live {
            ui: Box::new(crate::screen::stack_ui()),
            bench,
            held: None,
        }
    }

    fn run(&mut self, blocks: usize) {
        for _ in 0..blocks {
            self.bench.block();
            self.ui.set_formant_views(&self.bench.formant_views());
        }
    }

    fn hold_note(&mut self, n: u8) {
        self.release();
        self.bench.note(n, Some(100));
        self.held = Some(n);
        self.run(SETTLE);
    }

    fn release(&mut self) {
        if let Some(n) = self.held.take() {
            self.bench.note(n, None);
        }
    }

    fn set_param(&mut self, op: Op, id: chimera_core::block::ParamId, v: f32) {
        self.bench.edit_sound(|s| {
            use chimera_core::addr::Blocks;
            s.params.block_mut(BlockRef::AlgoOp(op)).unwrap().set(id, v);
        });
    }

    /// The FC the OP page's band shows for OP2.
    fn viz_fc(&self) -> f32 {
        shown(
            &self.bench.sound().params.algo,
            self.ui.formant_view(),
            Op::B,
        )
        .0
    }
}

#[test]
fn the_viz_follows_the_live_voice() {
    // TALK BASS with KEY 0 on OP2 and its sample and hold muted: only
    // ENV 2 moves OP2's FC.
    let mut s = demo_sound(Demo::TalkBass);
    s.params.algo.ops[1].key = 0;
    let vowel = ParamAddr::new(BlockRef::Algo, AlgoParams::VOWEL);
    let col = s.mod_state.find(vowel).unwrap();
    s.mod_state.set_amount(ModSource::Lfo1.index(), col, 0);
    let mut w = Live::new(s);
    let resting = w.viz_fc();
    assert!(!w.ui.formant_view().sounding);
    w.hold_note(36);
    assert!(w.ui.formant_view().sounding);
    let low = w.viz_fc();
    w.hold_note(60);
    let high = w.viz_fc();
    assert_eq!(low, high, "KEY 0 in TALK BASS");
    assert_ne!(low, resting, "ENV 2 moves VOWEL under the held note");
    w.set_param(Op::B, AlgoOpParams::KEY, 100.0);
    w.hold_note(72);
    assert!(w.viz_fc() > high * 1.9, "{} vs {high}", w.viz_fc());
    // Released and rung out: the stored params at C4 again.
    w.release();
    w.run(48_000 / 64 * 4);
    assert!(!w.ui.formant_view().sounding);
    assert_eq!(w.viz_fc(), resting);
}
