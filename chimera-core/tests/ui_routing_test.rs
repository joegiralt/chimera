//! Priming mod destinations from pages (spec §5, Review Focus 1 and 5).

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::pizza::PizzaParams;
use chimera_core::ui::UiState;
use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

/// Mock controls for driving `UiState::handle_input`.
struct MockControls {
    buttons: Vec<(ButtonId, ButtonState)>,
    encoders: Vec<(EncoderId, i8)>,
}

impl MockControls {
    fn new() -> Self {
        Self { buttons: Vec::new(), encoders: Vec::new() }
    }
    fn button(mut self, id: ButtonId, state: ButtonState) -> Self {
        self.buttons.push((id, state));
        self
    }
    fn encoder(mut self, id: EncoderId, delta: i8) -> Self {
        self.encoders.push((id, delta));
        self
    }
}

impl Controls for MockControls {
    fn button_state(&self, id: ButtonId) -> ButtonState {
        self.buttons.iter().find(|b| b.0 == id).map_or(ButtonState::Up, |b| b.1)
    }
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.encoders.iter().find(|e| e.0 == id).map_or(0, |e| e.1)
    }
}

fn press(ui: &mut UiState, id: ButtonId) {
    ui.handle_input(&MockControls::new().button(id, ButtonState::Pressed));
}

/// Touch encoder A (focus slot 0), then MIX + Plus.
fn prime_slot_0(ui: &mut UiState) {
    ui.handle_input(&MockControls::new().encoder(EncoderId::A, 1));
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(ButtonId::Plus, ButtonState::Pressed),
    );
}

fn primed(ui: &UiState) -> Vec<ParamAddr> {
    let reg = &ui.project.tracks[0].patch.dest_registry;
    (0..reg.len()).filter_map(|i| reg.get(i)).map(|e| e.addr).collect()
}

#[test]
fn priming_on_a_part_page_registers_its_address() {
    let mut ui = UiState::new(); // Part 1, Pizza page
    prime_slot_0(&mut ui);
    assert_eq!(primed(&ui), [ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE)]);
    let reg = &ui.project.tracks[0].patch.dest_registry;
    assert_eq!(reg.get(0).unwrap().label_str(), "PIZSHAPE");
    assert_eq!(ui.mod_state().num_dests(), 1);
}

/// Review Focus 1: Mixer/System/Demo slots are Legacy — priming there must
/// not register anything (it used to register `Block{node,i}`, which the
/// voice read as a Pizza/Drive/Filter/Folder param).
#[test]
fn priming_on_legacy_page_registers_nothing() {
    let mut ui = UiState::new();
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(ButtonId::B1, ButtonState::Pressed),
    ); // Mixer chain
    prime_slot_0(&mut ui);
    assert!(primed(&ui).is_empty());
    press(&mut ui, ButtonId::Menu); // System chain
    prime_slot_0(&mut ui);
    assert!(primed(&ui).is_empty());
}

/// Spec §4: the registry refuses non-modulatable params (LFO RATE).
#[test]
fn priming_a_non_modulatable_param_is_refused() {
    let mut ui = UiState::new();
    for _ in 0..4 {
        press(&mut ui, ButtonId::Plus); // → MOD node
    }
    press(&mut ui, ButtonId::Edit); // Envelope sub-page
    press(&mut ui, ButtonId::Edit); // LFO sub-page
    prime_slot_0(&mut ui);
    assert!(primed(&ui).is_empty());
}

/// Review Focus 5 / spec §5: a route primed on a `SelectedOp` slot names the
/// operator selected at that moment; changing the selection later does not
/// retarget it.
#[test]
fn selected_op_route_is_concrete() {
    use chimera_core::addr::Op;
    use chimera_core::params::FmOpParams;
    use chimera_core::preset::POOL_SIZE;

    let mut ui = UiState::new();
    // Load "(init) FM" into track 1 via the patch browser.
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Edit, ButtonState::Held)
            .button(ButtonId::B1, ButtonState::Pressed),
    );
    ui.handle_input(&MockControls::new().encoder(EncoderId::Main, (POOL_SIZE + 2) as i8));
    press(&mut ui, ButtonId::Edit);
    press(&mut ui, ButtonId::Edit); // FM node → Operator sub-page
    ui.handle_input(&MockControls::new().encoder(EncoderId::A, 1)); // select op B
    assert_eq!(ui.selected_op(), Op::B);
    ui.handle_input(&MockControls::new().encoder(EncoderId::D, 1)); // FDBK slot
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(ButtonId::Plus, ButtonState::Pressed),
    );
    let fdbk_b = ParamAddr::new(BlockRef::FmOp(Op::B), FmOpParams::FEEDBACK);
    assert!(primed(&ui).contains(&fdbk_b));

    ui.handle_input(&MockControls::new().encoder(EncoderId::A, 1)); // select op C
    assert_eq!(ui.selected_op(), Op::C);
    assert!(primed(&ui).contains(&fdbk_b));
    assert!(!primed(&ui).contains(&ParamAddr::new(BlockRef::FmOp(Op::C), FmOpParams::FEEDBACK)));
    assert_eq!(ui.project.tracks[0].patch.params.fm.operators[1].feedback, 1.0);
}

/// Spec §5: the FM operator selection is part of the page identity, so
/// turning the selector on the FM_OP page must refresh `ui.page()` too
/// (pins the redraw key update at ui/mod.rs ~343).
#[test]
fn fm_operator_selector_updates_the_page_key() {
    use chimera_core::addr::Op;
    use chimera_core::preset::{ChainType, Track};
    use chimera_core::ui::block_registry as reg;
    use chimera_core::ui::page::PageKey;

    let mut ui = UiState::new();
    ui.project.tracks[0] = Track::new(ChainType::Fm);
    ui.nav.chain_type = ChainType::Fm;
    press(&mut ui, ButtonId::Edit); // sub-page 1: FM_OP
    assert_eq!(ui.page(), PageKey::Part { def: reg::FM_OP.id, op: Op::A });
    ui.handle_input(&MockControls::new().encoder(EncoderId::A, 1)); // selector: A -> B
    assert_eq!(ui.page(), PageKey::Part { def: reg::FM_OP.id, op: Op::B });
}

/// Spec §4: after loading the FM init patch the matrix rows are ENV and LFO
/// (they used to be "Op1 Env".."Op4 Env", of which only two produced values).
#[test]
fn fm_matrix_rows_are_env_and_lfo() {
    use chimera_core::preset::POOL_SIZE;

    let mut ui = UiState::new();
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Edit, ButtonState::Held)
            .button(ButtonId::B1, ButtonState::Pressed),
    );
    ui.handle_input(&MockControls::new().encoder(EncoderId::Main, (POOL_SIZE + 2) as i8));
    press(&mut ui, ButtonId::Edit); // load "(init) FM"
    let rows: Vec<&str> = (0..ui.matrix_state.num_sources)
        .map(|i| ui.matrix_state.sources[i].unwrap().name)
        .collect();
    assert_eq!(rows, ["ENV", "LFO"]);
    assert_eq!(ui.matrix_state.num_dests, 0);
}
