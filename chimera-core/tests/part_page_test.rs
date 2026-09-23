//! Part-chain pages driven by slot bindings (spec §5): encoders, shift-snap
//! and display go through the bound param's spec. Parity tests pin today's
//! step sizes (plan § Encoder step audit).

use chimera_core::addr::Op;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::params::ParamSnapshot;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::part_page;

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
fn pizza_page() {
    let mut p = ParamSnapshot::default();
    assert_eq!(read(&reg::PIZZA, &p), [0.5, 0.0, 0.8, 0.0, 0.0, 0.0]);
    turn(&reg::PIZZA, 0, 3, &mut p);
    assert_eq!(p.pizza.shape, 0.5 + 3.0 * (1.0 / 128.0));
    turn(&reg::PIZZA, 2, 127, &mut p);
    assert_eq!(p.pizza.level, 1.0);
    snap(&reg::PIZZA, 1, 1, &mut p); // shift-snap works on Pizza (spec)
    assert_eq!(p.pizza.crush, 100.0 / 127.0);
    turn(&reg::PIZZA, 4, 1, &mut p); // empty slot: nothing happens
}

#[test]
fn modal_pages() {
    let mut p = ParamSnapshot::default();
    turn(&reg::MODAL_1, 1, 2, &mut p);
    assert_eq!(p.modal.excite, 0.8 + 2.0 * (1.0 / 128.0));
    turn(&reg::MODAL_2, 5, -1, &mut p);
    assert_eq!(p.modal.ks_ens_mix, 0.0);
    assert_eq!(read(&reg::MODAL_2, &p), [0.3, 0.0, 0.2, 0.0, 0.3, 0.0]);
    // Plan D3: MODE reaches Sympathetic; Review Focus 3: snap lands on a choice.
    p.modal.mode = ResonatorMode::Bowed;
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

#[test]
fn drive_filter_folder_pages() {
    let mut p = ParamSnapshot::default();
    assert_eq!(read(&reg::DRIVE, &p), [0.0, 0.5, 1.0, 0.0, 0.0, 0.0]);
    turn(&reg::DRIVE, 0, 5, &mut p);
    assert_eq!(p.drive.drive, 5.0 * ((1.0 - 0.0) / 128.0));
    snap(&reg::DRIVE, 1, 1, &mut p);
    assert_eq!(p.drive.tone, 107.0 / 127.0);

    assert_eq!(read(&reg::FILTER, &p), [1.0, 0.0, 0.0, 0.0, 0.5, 0.0]);
    turn(&reg::FILTER, 0, -1, &mut p);
    assert_eq!(p.filter.cutoff, 20000.0 - (20000.0 - 20.0) / 128.0);
    turn(&reg::FILTER, 4, 1, &mut p);
    assert_eq!(p.filter.env_amount, (1.0 - -1.0) / 128.0);

    assert_eq!(read(&reg::FOLDER, &p), [0.0, 0.5, 0.5, 0.0, 0.0, 0.0]);
    turn(&reg::FOLDER, 0, 4, &mut p);
    assert_eq!(p.folder.fold, 4.0 / 128.0);
}

#[test]
fn envelope_and_lfo_pages() {
    let mut p = ParamSnapshot::default();
    assert_eq!(
        read(&reg::ENVELOPE, &p),
        [(0.01 - 0.001) / (10.0 - 0.001), (0.3 - 0.001) / (10.0 - 0.001), 0.7, (0.3 - 0.001) / (10.0 - 0.001), 1.0, 0.5]
    );
    turn(&reg::ENVELOPE, 0, 1, &mut p);
    assert_eq!(p.envelopes[0].attack, 0.01 + (10.0 - 0.001) / 128.0);
    turn(&reg::ENVELOPE, 2, -1, &mut p);
    assert_eq!(p.envelopes[0].sustain, 0.7 - 1.0 / 128.0);

    assert_eq!(read(&reg::LFO, &p)[0], (1.0 - 0.01) / (20.0 - 0.01));
    turn(&reg::LFO, 0, 2, &mut p);
    assert_eq!(p.lfo.rate, 1.0 + 2.0 * 0.15);
    turn(&reg::LFO, 1, 9, &mut p);
    assert_eq!(p.lfo.shape, 4);
    turn(&reg::LFO, 2, 1, &mut p); // SYNC: free-running -> retrigger
    assert_eq!(p.lfo.sync, 1);
    turn(&reg::LFO, 5, 3, &mut p);
    assert_eq!(p.lfo.offset, 3.0 * (1.0 / 128.0) * 2.0);
    snap(&reg::LFO, 1, -1, &mut p); // shift-snap works on LFO (spec)
    assert_eq!(p.lfo.shape, 0);
}

#[test]
fn fm_operator_page_follows_the_selection() {
    let mut p = ParamSnapshot::default();
    let mut op = Op::A;
    part_page::apply_encoder(&reg::FM_OP, 0, 1, &mut p, &mut op); // selector
    assert_eq!(op, Op::B);
    part_page::apply_encoder(&reg::FM_OP, 0, 9, &mut p, &mut op);
    assert_eq!(op, Op::D);
    part_page::apply_encoder(&reg::FM_OP, 0, -2, &mut p, &mut op);
    assert_eq!(op, Op::B);
    part_page::apply_encoder(&reg::FM_OP, 2, 5, &mut p, &mut op);
    assert_eq!(p.fm.operators[1].level, 5.0);
    part_page::apply_encoder(&reg::FM_OP, 4, -9, &mut p, &mut op);
    assert_eq!(p.fm.operators[1].detune, -7);
    part_page::apply_encoder(&reg::FM_RATIO, 4, 1, &mut p, &mut op); // FINE of B
    assert_eq!(p.fm.operators[1].fine, 1);
    assert_eq!(part_page::read_values(&reg::FM_OP, &p, op)[0], 1.0 / 3.0);
    // Review Focus 3: snapping a Stepped level lands on an integer.
    part_page::snap_encoder(&reg::FM_OP, 2, 1, &mut p, op);
    assert_eq!(p.fm.operators[1].level, 78.0); // 99 * 100/127 = 77.95 → 78
    part_page::snap_encoder(&reg::FM_OP, 0, 1, &mut p, op); // selector: no snap
    assert_eq!(op, Op::B);
}

#[test]
fn fm_fixed_pages() {
    let mut p = ParamSnapshot::default();
    turn(&reg::FM_ALG, 0, 9, &mut p);
    assert_eq!(p.fm.algorithm, 7);
    turn(&reg::FM_ALG, 2, 1, &mut p); // LEVEL = voice output volume
    assert_eq!(p.out.volume, 0.8 + 1.0 / 128.0);
    turn(&reg::FM_RATIO, 2, 1, &mut p);
    assert_eq!(p.fm.operators[2].coarse, 5);
    snap(&reg::FM_RATIO, 0, 1, &mut p);
    assert_eq!(p.fm.operators[0].coarse, 63);
    turn(&reg::FM_ENV3, 2, -1, &mut p);
    assert_eq!(p.fm.operators[2].decay1_level, 14);
    turn(&reg::FM_ENV2, 4, -20, &mut p); // plan D4: RR reaches 0
    assert_eq!(p.fm.operators[1].release_rate, 0);
    snap(&reg::FM_ENV1, 0, -1, &mut p);
    assert_eq!(p.fm.operators[0].attack_rate, 0);
}
