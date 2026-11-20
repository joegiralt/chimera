//! ALGO's 0.4 UI tickets (ADR 0298): the envelope pages' order, names and
//! wiring (#987, #956), RATE SCALE's hint (#988), the ratio readout
//! (#986), FDBK in tenths of the TX81Z's steps (#990, ADR 0299) and the
//! chart's feedback loops (#989). Each ticket's tests follow its imports.

mod common;
mod screen;

use chimera_core::addr::{BlockRead, BlockRef, Op};
use chimera_core::block::{ParamId, ValFmt};
use chimera_core::dsp::algo::env::EnvRates;
use chimera_core::dsp::algo::params::{ALGO_OP_SPECS, AlgoOpParams};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::ui::block_def::{BlockDef, VizType};
use chimera_core::ui::block_registry::{
    ALGO_AR, ALGO_CHAIN, ALGO_D1L, ALGO_D1R, ALGO_D2R, ALGO_RR,
};
use chimera_core::ui::glyph::{FocusGlyph, Gauge, gauge};
use chimera_core::ui::{UiState, components, draw, dungeon_map, part_page, theme};
use chimera_hal::EncoderId;
use screen::*;

/// An envelope page: its def, the TX81Z's name on its tab, the stored
/// param, the words the header, the row and the focus band spell out, and
/// the engine's rate it must move.
struct Stage {
    def: &'static BlockDef,
    tx: &'static str,
    id: ParamId,
    words: &'static str,
    rate: fn(&EnvRates) -> u8,
}

/// The TX81Z's order: AR, D1R, D1L, D2R, RR.
const STAGES: [Stage; 5] = [
    Stage {
        def: &ALGO_AR,
        tx: "AR",
        id: AlgoOpParams::AR,
        words: "ATTACK RATE",
        rate: |r| r.ar,
    },
    Stage {
        def: &ALGO_D1R,
        tx: "D1R",
        id: AlgoOpParams::D1R,
        words: "DECAY 1 RATE",
        rate: |r| r.d1r,
    },
    Stage {
        def: &ALGO_D1L,
        tx: "D1L",
        id: AlgoOpParams::D1L,
        words: "DECAY 1 LEVEL",
        rate: |r| r.d1l,
    },
    Stage {
        def: &ALGO_D2R,
        tx: "D2R",
        id: AlgoOpParams::D2R,
        words: "DECAY 2 RATE",
        rate: |r| r.d2r,
    },
    Stage {
        def: &ALGO_RR,
        tx: "RR",
        id: AlgoOpParams::RR,
        words: "RELEASE RATE",
        rate: |r| r.rr,
    },
];

/// Part 1 on ALGO's INIT, at OSC, under STACK's keys.
fn ui() -> UiState {
    let mut ui = stack_ui();
    init_part(&mut ui, PartId::ALL[0]);
    to_osc(&mut ui);
    ui
}

#[test]
fn the_envelope_pages_follow_the_tx81z_order() {
    let subs = ALGO_CHAIN.blocks[osc_node()].sub_pages;
    let at = |d: &BlockDef| subs.iter().position(|s| s.id == d.id).unwrap();
    let first = at(STAGES[0].def);
    for (i, s) in STAGES.iter().enumerate() {
        assert_eq!(at(s.def), first + i, "{}", s.tx);
        assert_eq!(s.def.short, s.tx);
        assert_eq!(s.def.viz, VizType::OpEnv, "{}", s.tx);
    }
}

/// #987: each envelope cell edits the parameter its label names, for
/// every operator, and that parameter is the engine's rate of that name.
#[test]
fn each_envelope_cell_edits_the_parameter_its_label_names() {
    for s in &STAGES {
        let spec = ALGO_OP_SPECS.iter().find(|p| p.id == s.id).unwrap();
        assert_eq!((spec.label, spec.ident), (s.tx, s.tx));
        for (k, op) in Op::ALL.into_iter().enumerate() {
            // A cell at its top only moves down.
            for d in [1i8, -1] {
                let mut sound = Sound::init(EngineType::Algo);
                let before = sound.params.clone();
                part_page::apply_encoder(s.def, k, d, &mut sound.params, &mut Op::A);
                let moved: Vec<(Op, ParamId)> = Op::ALL
                    .into_iter()
                    .flat_map(|m| ALGO_OP_SPECS.iter().map(move |p| (m, p.id)))
                    .filter(|&(m, id)| {
                        let b = BlockRef::AlgoOp(m);
                        sound.params.block(b).unwrap().get(id) != before.block(b).unwrap().get(id)
                    })
                    .collect();
                let (was, now) = (
                    before.algo.ops[op.index()].rates(),
                    sound.params.algo.ops[op.index()].rates(),
                );
                if moved.is_empty() {
                    assert_eq!(was, now);
                    continue;
                }
                assert_eq!(moved, [(op, s.id)], "{} OP{}", s.tx, k + 1);
                // The engine's rate of that name moved, and no other.
                assert_ne!((s.rate)(&was), (s.rate)(&now), "{}", s.tx);
                let others = |r: &EnvRates| {
                    STAGES
                        .iter()
                        .filter(|o| o.id != s.id)
                        .map(|o| (o.rate)(r))
                        .collect::<Vec<_>>()
                };
                assert_eq!(others(&was), others(&now), "{}", s.tx);
            }
        }
    }
}

/// #987, #956: the header, the row and the focus band spell the stage out.
#[test]
fn the_envelope_pages_spell_their_stage_out() {
    for s in &STAGES {
        // The header has 90 px beside `PART 6 · SOUND`: LEVEL is LVL there.
        let header = s.words.replace("1 LEVEL", "1 LVL");
        assert_eq!(s.def.name.to_uppercase(), header);
        let row = dungeon_map::ROW_WORDS.iter().find(|w| w.0.id == s.def.id);
        assert_eq!(row.unwrap().1, s.words);
        let mut ui = ui();
        to_osc_sub(&mut ui, s.def);
        for (k, e) in [EncoderId::A, EncoderId::F].into_iter().enumerate() {
            feed(&mut ui, Input::turn(e, -1));
            let op = [1usize, 6][k];
            // The lit cell names the operator; the band, the stage.
            assert_eq!(ui.focused_slot(), op - 1);
            assert_eq!(ui.focus_label_for_test().as_str(), s.words);
        }
    }
}

/// The op page's FDBK cell is short for its 62 px; the band has room.
#[test]
fn the_op_pages_feedback_reads_in_full_in_the_band() {
    use chimera_core::ui::block_registry::ALGO_OP_PAGES;
    let mut ui = ui();
    to_osc_sub(&mut ui, &ALGO_OP_PAGES[0]);
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    assert_eq!(ui.focus_label_for_test().as_str(), "FEEDBACK");
    feed(&mut ui, Input::turn(EncoderId::D, -1));
    assert_eq!(ui.focus_label_for_test().as_str(), "LEVEL");
}

/// Each label clears the arc gauge at NORMAL; with the operator before
/// it (`OP6 ATTACK RATE`) none would, so the lit cell names the operator.
#[test]
fn the_spelled_out_labels_fit_the_band() {
    let arc = gauge(FocusGlyph::Arc, 0.0, ValFmt::Int(31), |_| Gauge::None);
    let slot = components::focus_label_slot(&arc);
    let w = |t: &str| draw::text_width(&theme::FONT_VALUE, t, theme::LABEL_TRACKING);
    for s in &STAGES {
        assert!(
            slot.left + w(s.words) <= slot.right,
            "{}: {}",
            s.words,
            w(s.words)
        );
        let with_op = format!("OP6 {}", s.words);
        assert!(
            slot.left + w(&with_op) > slot.right,
            "{with_op} fits after all"
        );
    }
    let bar = gauge(FocusGlyph::LevelBar, 0.0, ValFmt::Uni, |_| Gauge::None);
    assert!(slot.left + w("FEEDBACK") <= components::focus_label_slot(&bar).right);
}

// #988: RATE SCALE's words, and DECAY 1 LEVEL's under a held decay.

use chimera_core::addr::ParamAddr;
use chimera_core::ui::block_registry::ALGO_RATE_SCALE;
use chimera_core::ui::notes;
use chimera_core::ui::view::{SlotCtx, view};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::RgbColor;

fn op_addr(op: Op, id: ParamId) -> ParamAddr {
    ParamAddr::new(BlockRef::AlgoOp(op), id)
}

/// #988: RATE SCALE says what it does, on every operator, in the hint's
/// place; its cells carry the operators' names, not the tab's RS.
#[test]
fn rate_scale_says_what_it_does() {
    let s = Sound::init(EngineType::Algo);
    for op in Op::ALL {
        assert_eq!(
            notes::cell(op_addr(op, AlgoOpParams::RATE_SCALE), &s),
            Some("HIGH NOTES, FASTER ENVELOPES")
        );
    }
    let w = draw::text_width(&theme::FONT_LABEL, notes::RATE_SCALE, theme::LABEL_TRACKING);
    assert!(w <= theme::CONTENT_W, "{w}");
    let ctx = SlotCtx::read(&s.params, Op::A);
    let labels: Vec<_> = (0..6)
        .map(|k| view(&ALGO_RATE_SCALE, k, &ctx).label())
        .collect();
    assert_eq!(labels, ["OP1", "OP2", "OP3", "OP4", "OP5", "OP6"]);
    assert_eq!(
        (ALGO_RATE_SCALE.name, ALGO_RATE_SCALE.short),
        ("Rate Scale", "RS")
    );
}

/// The words reach the screen: the band draws them where the hint was.
#[test]
fn rate_scales_words_are_drawn_in_the_band() {
    let mut ui = ui();
    to_osc_sub(&mut ui, &ALGO_RATE_SCALE);
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    settle(&mut ui);
    let shot = render_ui(&ui);
    let mut want = Fb::new();
    draw::text_tracked(
        &mut want,
        &theme::FONT_LABEL,
        notes::RATE_SCALE,
        theme::MARGIN_X,
        chimera_core::ui::hint::FOCUS_Y,
        theme::MID,
        chimera_core::ui::source_cell::tracking(notes::RATE_SCALE),
    );
    let lit: Vec<_> = (0..H as i32)
        .flat_map(|y| (0..W as i32).map(move |x| (x, y)))
        .filter(|&(x, y)| want.at(x, y) != Rgb565::BLACK)
        .collect();
    assert!(!lit.is_empty());
    assert!(lit.iter().all(|&(x, y)| shot.at(x, y) == want.at(x, y)));
}

/// #987's likely cause: with DECAY 1 RATE 0 the envelope holds at full
/// and DECAY 1 LEVEL does nothing, so its band says why.
#[test]
fn decay_1_level_says_when_it_is_never_reached() {
    let d1l = op_addr(Op::B, AlgoOpParams::D1L);
    let mut s = Sound::init(EngineType::Algo);
    // INIT: D1R 0, D1L 15, where D2 runs at once.
    assert_eq!(notes::cell(d1l, &s), None);
    s.params.algo.ops[1].d1l = 9;
    assert_eq!(notes::cell(d1l, &s), Some(notes::D1_HOLDS));
    assert_eq!(notes::cell(op_addr(Op::A, AlgoOpParams::D1L), &s), None);
    s.params.algo.ops[1].d1r = 1;
    assert_eq!(notes::cell(d1l, &s), None);
    let w = draw::text_width(&theme::FONT_LABEL, notes::D1_HOLDS, theme::LABEL_TRACKING);
    assert!(w <= theme::CONTENT_W, "{w}");
}
