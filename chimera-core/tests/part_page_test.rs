//! Part-chain pages driven by slot bindings (spec §5): encoders, shift-snap
//! and display go through the bound param's spec. Parity tests pin today's
//! step sizes (plan § Encoder step audit).

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::FilterMode;
use chimera_core::dsp::modal::{ModalParams, ResonatorMode};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::{BlockDef, ParamSlot, VizType, slot_addr};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::page::PageLayout;
use chimera_core::ui::part_page;
use chimera_core::ui::view::{SlotCtx, View, view};
use chimera_hal::{ButtonId, EncoderId};

mod screen;

/// One encoder turn with operator A selected.
fn turn(def: &BlockDef, slot: usize, delta: i8, p: &mut ParamSnapshot) {
    let mut op = Op::A;
    part_page::apply_encoder(def, slot, delta, p, &mut op);
}

fn snap(def: &BlockDef, slot: usize, delta: i8, p: &mut ParamSnapshot) {
    part_page::snap_encoder(def, slot, delta, p, Op::A);
}

fn read(def: &BlockDef, p: &ParamSnapshot) -> [f32; 6] {
    part_page::read_values(def, p, Op::A)
}

#[test]
fn modal_pages() {
    let mut p = ParamSnapshot::default();
    p.modal.mode = ResonatorMode::Sympathetic;
    let m = p.modal;
    assert_eq!(
        read(&reg::MODAL_2, &p),
        [m.couple, m.halo, m.body, m.ens_depth, m.ens_mix, 0.0]
    );
    turn(&reg::MODAL_2, 0, 1, &mut p);
    assert_eq!(p.modal.couple, m.couple + 1.0 / 128.0);

    // BANK: MODES, then nothing.
    p.modal.mode = ResonatorMode::Modal;
    let ctx = SlotCtx::read(&p, Op::A);
    assert_eq!(
        slot_addr(&reg::MODAL_2, 0, &ctx),
        Some(ParamAddr::new(BlockRef::Modal, ModalParams::MODES))
    );
    assert_eq!(view(&reg::MODAL_2, 1, &ctx), View::Empty);
    let before = p.modal;
    turn(&reg::MODAL_2, 1, 5, &mut p);
    assert_eq!(format!("{:?}", p.modal), format!("{before:?}"));

    // BOWED reads nothing on MDL2.
    p.modal.mode = ResonatorMode::Bowed;
    let ctx = SlotCtx::read(&p, Op::A);
    for k in 0..6 {
        assert_eq!(view(&reg::MODAL_2, k, &ctx), View::Empty, "slot {k}");
    }

    // Plan D3: MODE reaches Sympathetic; Review Focus 3: snap lands on a choice.
    assert_eq!(read(&reg::MODAL_1, &p)[0], 2.0 / 3.0);
    turn(&reg::MODAL_1, 0, 1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::Sympathetic);
    // Top clamp: Sympathetic is the last choice, +1 stays put.
    turn(&reg::MODAL_1, 0, 1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::Sympathetic);
    snap(&reg::MODAL_1, 0, -1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::String);
    // Shift-snap on an Enum jumps straight to the far end.
    snap(&reg::MODAL_1, 0, 1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::Sympathetic);
}

/// EXC's cells follow MODEL: the pluck's, the strike's and the bow's.
#[test]
fn exciter_page_follows_the_model() {
    let mut p = ParamSnapshot::default();
    p.modal.mode = ResonatorMode::String;
    let m = p.modal;
    assert_eq!(
        read(&reg::MODAL_EXC, &p),
        [m.excite, m.color, 0.0, 0.0, 0.0, 0.0]
    );

    p.modal.mode = ResonatorMode::Modal;
    let ctx = SlotCtx::read(&p, Op::A);
    assert_eq!(
        slot_addr(&reg::MODAL_EXC, 1, &ctx),
        Some(ParamAddr::new(BlockRef::Modal, ModalParams::BURST))
    );

    p.modal.mode = ResonatorMode::Bowed;
    let ctx = SlotCtx::read(&p, Op::A);
    let at = |k| slot_addr(&reg::MODAL_EXC, k, &ctx);
    assert_eq!(
        at(0),
        Some(ParamAddr::new(BlockRef::Modal, ModalParams::FORCE))
    );
    assert_eq!(
        at(1),
        Some(ParamAddr::new(BlockRef::Modal, ModalParams::SPEED))
    );
    assert_eq!(view(&reg::MODAL_EXC, 2, &ctx), View::Empty);
    let force = p.modal.force;
    turn(&reg::MODAL_EXC, 0, 1, &mut p);
    assert_eq!(p.modal.force, force + 1.0 / 128.0);
}

/// SPACE on MDL is the Part's REV send, the value SENDS edits.
#[test]
fn space_is_the_parts_reverb_send() {
    let mut ui = UiState::new();
    screen::load_init(&mut ui, EngineType::Modal);
    assert_eq!(ui.nav.active_block_def().id, reg::MODAL_EXC.id);
    screen::feed(&mut ui, screen::Input::press(ButtonId::Plus));
    assert_eq!(ui.nav.active_block_def().id, reg::MODAL_1.id);
    let was = ui.performance.parts[0].mix.sends[2];
    screen::feed(&mut ui, screen::Input::turn(EncoderId::F, 10));
    let now = ui.performance.parts[0].mix.sends[2];
    assert!((now - was - 10.0 / 128.0).abs() < 1e-6, "{was} → {now}");
    let edit = ui.performance.edit(0);
    let space = part_page::read_values(&reg::MODAL_1, &edit, Op::A)[5];
    assert_eq!(part_page::read_values(&reg::SENDS, &edit, Op::A)[2], space);
}

#[test]
fn drive_filter_folder_pages() {
    let mut p = ParamSnapshot::default();
    assert_eq!(read(&reg::DRIVE, &p), [0.0, 0.5, 1.0, 0.0, 0.0, 0.0]);
    turn(&reg::DRIVE, 0, 5, &mut p);
    assert_eq!(p.drive.drive, 5.0 * ((1.0 - 0.0) / 128.0));
    snap(&reg::DRIVE, 1, 1, &mut p);
    assert_eq!(p.drive.tone, 107.0 / 127.0);

    assert_eq!(read(&reg::FILTER, &p), [0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    turn(&reg::FILTER, 1, -1, &mut p);
    assert_eq!(p.filter.cutoff, 20000.0 - (20000.0 - 20.0) / 128.0);
    turn(&reg::FILTER, 3, 1, &mut p);
    assert_eq!(
        p.filter.mode(),
        FilterMode::Lp6,
        "the next in the SVF's list"
    );
    turn(&reg::FILTER, 3, 20, &mut p);
    assert_eq!(p.filter.mode(), FilterMode::Phaser, "clamps at the last");

    assert_eq!(read(&reg::FOLDER, &p), [0.0, 0.5, 0.5, 1.0, 0.0, 0.0]);
    turn(&reg::FOLDER, 0, 4, &mut p);
    assert_eq!(p.folder.fold, 4.0 / 128.0);
}

#[test]
fn envelope_and_lfo_pages() {
    let mut p = ParamSnapshot::default();
    assert_eq!(
        read(&reg::ENVELOPE, &p),
        [0.189, 0.559, 0.7, 0.559, 0.0, 0.0]
    );
    turn(&reg::ENVELOPE, 0, 1, &mut p);
    assert_eq!(p.envelopes[0].attack, 0.189 + 1.0 / 128.0);
    turn(&reg::ENVELOPE, 2, -1, &mut p);
    assert_eq!(p.envelopes[0].sustain, 0.7 - 1.0 / 128.0);

    assert_eq!(read(&reg::LFO, &p)[0], (1.0 - 0.01) / (20.0 - 0.01));
    turn(&reg::LFO, 0, 2, &mut p);
    assert_eq!(p.lfos[0].rate, 1.0 + 2.0 * 0.15);
    turn(&reg::LFO, 1, 9, &mut p);
    assert_eq!(p.lfos[0].shape, 4);
    turn(&reg::LFO, 2, 1, &mut p); // SYNC: free-running -> retrigger
    assert_eq!(p.lfos[0].sync, 1);
    snap(&reg::LFO, 1, -1, &mut p); // shift-snap works on LFO (spec)
    assert_eq!(p.lfos[0].shape, 0);
}

/// The selector machinery sub-project 4's per-operator pages will use.
static OP_PAGE: BlockDef = BlockDef {
    id: 63,
    name: "Op",
    short: "OP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::select_op(),
        ParamSlot::selected_op(AlgoOpParams::LEVEL),
        ParamSlot::selected_op(AlgoOpParams::DETUNE),
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
    ],
};

#[test]
fn select_op_page_follows_the_selection() {
    let mut p = ParamSnapshot::default();
    p.algo = AlgoParams::single(WaveId::W1); // operators 2–6 at LEVEL 0
    let mut op = Op::A;
    part_page::apply_encoder(&OP_PAGE, 0, 1, &mut p, &mut op);
    assert_eq!(op, Op::B);
    part_page::apply_encoder(&OP_PAGE, 0, 9, &mut p, &mut op);
    assert_eq!(op, Op::F);
    part_page::apply_encoder(&OP_PAGE, 0, -4, &mut p, &mut op);
    assert_eq!(op, Op::B);
    part_page::apply_encoder(&OP_PAGE, 1, 5, &mut p, &mut op);
    assert_eq!(p.algo.ops[1].level, 5);
    part_page::apply_encoder(&OP_PAGE, 2, -9, &mut p, &mut op);
    assert_eq!(p.algo.ops[1].detune, -3);
    assert_eq!(part_page::read_values(&OP_PAGE, &p, op)[0], 1.0 / 5.0);
    part_page::snap_encoder(&OP_PAGE, 1, 1, &mut p, op); // Int(99): the snap is the top
    assert_eq!(p.algo.ops[1].level, 99);
    part_page::snap_encoder(&OP_PAGE, 0, 1, &mut p, op); // selector: no snap
    assert_eq!(op, Op::B);
}

/// Review Focus 5 (spec §5): a route primed on a `SelectedOp` slot names
/// the operator selected at that moment.
#[test]
fn a_selected_op_slot_resolves_to_the_operator_selected_now() {
    let level = |op| Some(ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::LEVEL));
    let on = |op| SlotCtx::read(&ParamSnapshot::default(), op);
    assert_eq!(slot_addr(&OP_PAGE, 1, &on(Op::B)), level(Op::B));
    assert_eq!(slot_addr(&OP_PAGE, 1, &on(Op::F)), level(Op::F));
    assert_eq!(slot_addr(&OP_PAGE, 0, &on(Op::B)), None);
}

#[test]
fn algo_pages_edit_every_operator_and_the_algorithm() {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = AlgoParams::single(WaveId::W1); // LEVELs 99, 0…; ALG A = B = T1
    for slot in 0..6 {
        turn(&reg::ALGO_WAVE, slot, 1 + slot as i8, &mut p);
    }
    assert_eq!(p.algo.ops.map(|o| o.wave), [1, 2, 3, 4, 5, 6]);
    turn(&reg::ALGO_LEVEL, 5, 40, &mut p);
    assert_eq!(p.algo.ops[5].level, 40);
    turn(&reg::ALGO_LEVEL, 0, 5, &mut p);
    assert_eq!(p.algo.ops[0].level, 99, "clamped");
    turn(&reg::ALGO_ALG, 0, 21, &mut p);
    turn(&reg::ALGO_ALG, 1, 29, &mut p);
    turn(&reg::ALGO_ALG, 2, 64, &mut p);
    turn(&reg::ALGO_ALG, 3, -30, &mut p);
    assert_eq!(
        (p.algo.alg_a, p.algo.alg_b, p.algo.morph, p.algo.transpose),
        (21, 29, 64, -24)
    );
    snap(&reg::ALGO_ALG, 2, 1, &mut p); // MIX + turn snaps MORPH like any Uni value
    assert_eq!(p.algo.morph, 100);
}

#[test]
fn the_osc_node_has_every_operator_parameter() {
    use chimera_core::dsp::algo::params::ALGO_OP_SPECS;
    use chimera_core::ui::block_def::SlotBinding;
    let block = reg::ALGO_CHAIN
        .blocks
        .iter()
        .find(|b| b.def.id == reg::ALGO_WAVE.id)
        .unwrap();
    let mut edited = Vec::new();
    for sub in 0..block.sub_page_count() {
        let def = block.active_def(sub);
        for (i, slot) in def.params.iter().enumerate() {
            let SlotBinding::Param(a) = slot.binding else {
                panic!("{} slot {i}", def.name)
            };
            assert_eq!(
                a.block,
                BlockRef::AlgoOp(Op::ALL[i]),
                "{} slot {i}",
                def.name
            );
            edited.push(a.param);
        }
    }
    for s in &ALGO_OP_SPECS {
        assert!(edited.contains(&s.id), "{} has no page", s.label);
    }
    let names: Vec<&str> = (0..block.sub_page_count())
        .map(|s| block.active_def(s).short)
        .collect();
    assert_eq!(
        names,
        [
            "OSC", "CRS", "FIN", "DET", "LVL", "VEL", "AR", "D1R", "D1L", "D2R", "RR", "RS", "FBK"
        ]
    );
}
