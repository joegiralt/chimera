//! OP1–OP6 are OSC sub-pages after the twelve group pages (ADR 0130).

mod screen;

use chimera_core::addr::{BlockRead, BlockRef, Op};
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::ui::block_registry::{ALGO_CHAIN, ALGO_FEEDBACK, ALGO_OP_PAGES};
use chimera_core::ui::part_page;
use chimera_hal::ButtonId;
use screen::*;

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
    let mut ui = chimera_core::ui::UiState::new();
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
