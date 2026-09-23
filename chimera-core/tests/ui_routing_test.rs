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
