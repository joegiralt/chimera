use chimera_core::addr::Op;
use chimera_core::params::EngineType;
use chimera_core::preset::{POOL_SIZE, Performance, Sound};
use chimera_core::project::{
    Origin, PartFrom, PartId, PartSource, Project, ReplaceError, ReplaceGuard, SlotId, TemplateCrc,
};
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::nav::{Browse, NavCtx};
use chimera_core::ui::page::PageKey;
use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

/// A load into a Clean or Stale Part: the guard lets it through.
fn load(p: &mut Project, t: TemplateCrc, src: PartSource) -> Result<(), ReplaceError> {
    let c = ReplaceGuard::check(p, t, src).expect("a Clean or Stale Part");
    p.replace_part(c)
}

/// Mock controls for testing UI input handling.
struct MockControls {
    buttons: [(ButtonId, ButtonState); 16],
    button_count: usize,
    encoder_deltas: [(EncoderId, i8); 4],
    delta_count: usize,
}

impl MockControls {
    fn new() -> Self {
        Self {
            buttons: [(ButtonId::B1, ButtonState::Up); 16],
            button_count: 0,
            encoder_deltas: [(EncoderId::A, 0); 4],
            delta_count: 0,
        }
    }

    fn button(mut self, id: ButtonId, state: ButtonState) -> Self {
        self.buttons[self.button_count] = (id, state);
        self.button_count += 1;
        self
    }

    fn encoder(mut self, id: EncoderId, delta: i8) -> Self {
        self.encoder_deltas[self.delta_count] = (id, delta);
        self.delta_count += 1;
        self
    }
}

impl Controls for MockControls {
    fn button_state(&self, id: ButtonId) -> ButtonState {
        for &(button_id, state) in self.buttons.iter().take(self.button_count) {
            if button_id == id {
                return state;
            }
        }
        ButtonState::Up
    }

    fn encoder_delta(&self, id: EncoderId) -> i8 {
        for &(encoder_id, delta) in self.encoder_deltas.iter().take(self.delta_count) {
            if encoder_id == id {
                return delta;
            }
        }
        0
    }
}

#[test]
fn patch_init_has_musically_useful_defaults() {
    let p = Sound::init(EngineType::Algo);
    assert_eq!(p.engine(), EngineType::Algo);
    assert!(p.params.out.volume > 0.0);
    assert!(p.params.filter.cutoff > 1000.0);
    assert!(p.name.as_str() == "INIT");
}

/// Editing a loaded Part edits its copy: the slot keeps its Sound.
#[test]
fn part_edit_does_not_modify_pool() {
    let (mut p, t) = Project::boxed();
    let (part, slot) = (PartId::ALL[0], SlotId::ALL[0]);
    p.pool_store(slot, Sound::init(EngineType::Algo));
    load(
        &mut p,
        t,
        PartSource {
            part,
            from: PartFrom::Slot(slot),
        },
    )
    .unwrap();
    p.edit_part(part).sound.params.out.volume = 0.0; // mute
    assert_eq!(p.part(part).sound.params.out.volume, 0.0);
    assert!(p.pool().get(slot).unwrap().params.out.volume > 0.0);
}

/// Spec § Vocabulary: a Performance holds MAX_PARTS Parts, each playing a Sound.
#[test]
fn performance_has_six_parts_playing_sounds() {
    let perf = Performance::new();
    assert_eq!(perf.parts().len(), chimera_core::hw::MAX_PARTS);
    let sound: &Sound = &perf.parts()[0].sound;
    assert_eq!(sound.engine(), EngineType::Algo);
    assert_eq!(Project::boxed().0.meta().name().as_str(), "NEW PROJECT");
}

// ── Navigation tests ─────────────────────────────────────────────

/// The node of the page shown.
fn node(ui: &UiState) -> usize {
    let cx = NavCtx {
        engines: [EngineType::Algo; 6],
        dyn_rows: 0,
    };
    ui.location().page(&cx).unwrap().1.node() as usize
}

#[test]
fn plus_moves_one_block_per_press() {
    let mut ui = UiState::new();
    let start_node = node(&ui);

    // Single Pressed event → one step
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Pressed));
    assert_eq!(node(&ui), start_node + 1);

    // Held does NOT advance further
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Held));
    assert_eq!(node(&ui), start_node + 1);

    // Released does NOT advance
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Released));
    assert_eq!(node(&ui), start_node + 1);

    // Another Pressed → one more step
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Pressed));
    assert_eq!(node(&ui), start_node + 2);
}

#[test]
fn minus_moves_one_block_per_press() {
    let mut ui = UiState::new();

    // Move forward first so we have room to go back
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Pressed));
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Pressed));
    assert_eq!(node(&ui), 2);

    // Minus → one step back
    ui.handle_input(&MockControls::new().button(ButtonId::Minus, ButtonState::Pressed));
    assert_eq!(node(&ui), 1);

    // Held does NOT go further
    ui.handle_input(&MockControls::new().button(ButtonId::Minus, ButtonState::Held));
    assert_eq!(node(&ui), 1);
}

#[test]
fn edit_held_with_b_press_does_not_navigate() {
    let mut ui = UiState::new();
    let start_node = node(&ui);

    // Edit+B1 should open browser, NOT navigate
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Edit, ButtonState::Held)
            .button(ButtonId::B1, ButtonState::Pressed),
    );

    // On the Sound rung, not a page step
    assert!(ui.location().browse().is_some());
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
    assert_eq!(node(&ui), start_node);
}

// ── Sound browser integration tests ─────────────────────────────

/// Helper: open sound browser for a part via Edit + B-button
fn open_browser(ui: &mut UiState, btn: ButtonId) {
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Edit, ButtonState::Held)
            .button(btn, ButtonState::Pressed),
    );
}

#[test]
fn edit_b1_opens_patch_browser() {
    let mut ui = UiState::new();
    assert!(ui.location().browse().is_none());

    open_browser(&mut ui, ButtonId::B1);
    assert!(
        ui.location()
            .browse()
            .is_some_and(|(p, _)| p == PartId::ALL[0])
    );
}

#[test]
fn edit_b3_opens_browser_for_track_2() {
    let mut ui = UiState::new();

    open_browser(&mut ui, ButtonId::B3);
    assert!(
        ui.location()
            .browse()
            .is_some_and(|(p, _)| p == PartId::ALL[2])
    );
}

#[test]
fn b1_without_edit_does_not_open_browser() {
    let mut ui = UiState::new();

    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
    assert!(ui.location().browse().is_none());
}

#[test]
fn browser_load_copies_patch_to_part() {
    let mut ui = UiState::new();

    // Store a named sound in pool slot 2
    let mut sound = Sound::init(EngineType::Algo);
    sound.name = chimera_core::name::Name::new("Test Sound").unwrap();
    ui.project_mut().pool_store(SlotId::ALL[2], sound);

    // Open browser for B1 (part 0)
    open_browser(&mut ui, ButtonId::B1);
    assert_eq!(
        ui.location().browse(),
        Some((PartId::ALL[0], Browse::default()))
    );

    // Scroll down to slot 2
    ui.handle_input(&MockControls::new().encoder(EncoderId::A, 2));

    // Confirm selection
    ui.handle_input(&MockControls::new().button(ButtonId::Edit, ButtonState::Pressed));

    // Should exit browser and load sound into part 0
    assert!(ui.location().browse().is_none());
    assert_eq!(
        ui.project().part(PartId::ALL[0]).sound.name.as_str(),
        "Test Sound"
    );
    assert!(matches!(
        ui.project().part(PartId::ALL[0]).origin(),
        Origin::Slot { slot, .. } if slot == SlotId::ALL[2]
    ));
}

#[test]
fn browser_cancel_does_not_load() {
    let mut ui = UiState::new();
    let original_name = ui.project().part(PartId::ALL[0]).sound.name;

    // Store sound and open browser
    ui.project_mut()
        .pool_store(SlotId::ALL[0], Sound::init(EngineType::Modal));
    open_browser(&mut ui, ButtonId::B1);
    assert!(ui.location().browse().is_some());

    // Cancel by pressing a B-button
    ui.handle_input(&MockControls::new().button(ButtonId::B2, ButtonState::Pressed));

    assert!(ui.location().browse().is_none());
    assert_eq!(ui.project().part(PartId::ALL[0]).sound.name, original_name);
}

#[test]
fn browser_init_entries_set_the_engine() {
    let mut ui = UiState::new();

    // Open browser, scroll to "INIT Modal" (POOL_SIZE + 1)
    open_browser(&mut ui, ButtonId::B1);
    ui.handle_input(&MockControls::new().encoder(EncoderId::A, (POOL_SIZE + 1) as i8));
    ui.handle_input(&MockControls::new().button(ButtonId::Edit, ButtonState::Pressed));

    assert!(ui.location().browse().is_none());
    assert_eq!(
        ui.project().part(PartId::ALL[0]).sound.engine(),
        EngineType::Modal
    );
    assert!(ui.project().part(PartId::ALL[0]).sound.name.as_str() == "INIT");

    // Open browser again, scroll to "INIT Algo" (POOL_SIZE)
    open_browser(&mut ui, ButtonId::B1);
    ui.handle_input(&MockControls::new().encoder(EncoderId::A, POOL_SIZE as i8));
    ui.handle_input(&MockControls::new().button(ButtonId::Edit, ButtonState::Pressed));

    assert!(ui.location().browse().is_none());
    assert_eq!(
        ui.project().part(PartId::ALL[0]).sound.engine(),
        EngineType::Algo
    );
}

/// Loading from the browser replaces the Sound only: the Part keeps its
/// channel and mix (Review Focus: a load must not re-route MIDI).
#[test]
fn browser_load_keeps_part_mix() {
    let mut ui = UiState::new();
    ui.project_mut().edit_part(PartId::ALL[2]).mix.channel =
        chimera_core::MidiChannel::new(9).unwrap();
    ui.project_mut().edit_part(PartId::ALL[2]).mix.level = 0.3;
    open_browser(&mut ui, ButtonId::B3);
    ui.handle_input(&MockControls::new().encoder(EncoderId::A, (POOL_SIZE + 1) as i8));
    ui.handle_input(&MockControls::new().button(ButtonId::Edit, ButtonState::Pressed));
    assert_eq!(
        ui.project().part(PartId::ALL[2]).sound.engine(),
        EngineType::Modal
    );
    assert_eq!(ui.project().part(PartId::ALL[2]).mix.channel.get(), 9);
    assert_eq!(ui.project().part(PartId::ALL[2]).mix.level, 0.3);
}

// ── Priming by slot address ──────────────────────────────────────

fn press(ui: &mut UiState, id: ButtonId) {
    ui.handle_input(&MockControls::new().button(id, ButtonState::Pressed));
}

/// MIX + Plus on the focused slot.
fn prime(ui: &mut UiState) {
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Pressed)
            .button(ButtonId::Plus, ButtonState::Pressed),
    );
}

fn primed(ui: &UiState) -> Vec<chimera_core::addr::ParamAddr> {
    let reg = &ui.project().part(ui.active_part).sound.dest_registry;
    (0..reg.len()).map(|i| reg.get(i).unwrap().addr).collect()
}

/// Positive control: the filter page primes cutoff.
#[test]
fn priming_on_main_page_registers_focused_param() {
    let mut ui = UiState::new();
    press(&mut ui, ButtonId::Plus);
    press(&mut ui, ButtonId::Plus);
    press(&mut ui, ButtonId::Plus); // node 3: Filter
    ui.handle_input(&MockControls::new().encoder(EncoderId::B, 1));
    prime(&mut ui); // slot 1: cutoff
    assert_eq!(
        primed(&ui),
        [chimera_core::addr::ParamAddr::new(
            chimera_core::addr::BlockRef::Filter,
            chimera_core::params::FilterParams::CUTOFF
        )]
    );
}

/// L1 (node 5, sub-page 5): slot 0 is LFO rate, which is not
/// modulatable, so the registry refuses it.
#[test]
fn priming_on_the_lfo_sub_page_registers_nothing() {
    let mut ui = UiState::new();
    for _ in 0..5 {
        press(&mut ui, ButtonId::Plus);
    }
    for _ in 0..5 {
        press(&mut ui, ButtonId::Edit); // E1, E2, E3, SPD, L1
    }
    assert_eq!(
        ui.page(),
        PageKey::Part {
            def: reg::LFO.id,
            op: Op::A
        }
    );
    prime(&mut ui);
    assert_eq!(primed(&ui), [chimera_core::modulation::CUTOFF]);
}

/// The WAVE page's slots are Enums, never modulatable, so the registry
/// refuses them and nothing is registered.
#[test]
fn priming_a_wave_registers_nothing() {
    let mut ui = UiState::new();
    press(&mut ui, ButtonId::Plus); // WAVE
    assert_eq!(
        ui.page(),
        PageKey::Part {
            def: reg::ALGO_WAVE.id,
            op: Op::A
        }
    );
    ui.handle_input(&MockControls::new().encoder(EncoderId::C, 1)); // focus slot 2
    prime(&mut ui);
    assert_eq!(primed(&ui), [chimera_core::modulation::CUTOFF]);
}

/// `Performance::default()` is `Performance::new()` (clippy new_without_default).
#[test]
fn performance_default_is_new() {
    use chimera_core::part::PartParams;
    let p = chimera_core::preset::Performance::default();
    for (i, part) in p.parts().iter().enumerate() {
        assert_eq!(part.mix, PartParams::for_part(i));
    }
}
