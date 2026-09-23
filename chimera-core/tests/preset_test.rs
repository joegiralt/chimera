use chimera_core::preset::{ChainType, Patch, Project, SoundPool, Track, POOL_SIZE};
use chimera_core::ui::{UiMode, UiState};
use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

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
            encoder_deltas: [(EncoderId::Main, 0); 4],
            delta_count: 0,
        }
    }

    fn none() -> Self { Self::new() }

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
        for i in 0..self.button_count {
            if self.buttons[i].0 == id {
                return self.buttons[i].1;
            }
        }
        ButtonState::Up
    }

    fn encoder_delta(&self, id: EncoderId) -> i8 {
        for i in 0..self.delta_count {
            if self.encoder_deltas[i].0 == id {
                return self.encoder_deltas[i].1;
            }
        }
        0
    }
}

#[test]
fn patch_init_has_musically_useful_defaults() {
    let p = Patch::init(ChainType::PizzaPoly);
    assert_eq!(p.chain_type, ChainType::PizzaPoly);
    assert!(p.params.out.volume > 0.0);
    assert!(p.params.filter.cutoff > 1000.0);
    assert!(p.name_str().starts_with("(init)"));
}

#[test]
fn sound_pool_starts_empty() {
    let pool = SoundPool::new();
    assert!(pool.get(0).is_none());
    assert!(pool.get(31).is_none());
}

#[test]
fn sound_pool_store_and_retrieve() {
    let mut pool = SoundPool::new();
    let patch = Patch::init(ChainType::PizzaPoly);
    pool.store(0, patch);
    assert!(pool.get(0).is_some());
    assert_eq!(pool.get(0).unwrap().chain_type, ChainType::PizzaPoly);
}

#[test]
fn sound_pool_slot_count() {
    let pool = SoundPool::new();
    assert_eq!(pool.slot_count(), 32);
}

#[test]
fn track_starts_with_init_patch() {
    let track = Track::new(ChainType::PizzaPoly);
    assert_eq!(track.patch.chain_type, ChainType::PizzaPoly);
    assert!(track.loaded_from.is_none());
}

#[test]
fn track_load_from_pool_copies() {
    let mut pool = SoundPool::new();
    let mut patch = Patch::init(ChainType::PizzaPoly);
    patch.name = *b"Acid Bass\0\0\0\0\0\0\0";
    pool.store(3, patch);

    let mut track = Track::new(ChainType::PizzaPoly);
    track.load_from_pool(&pool, 3);

    assert_eq!(track.patch.name_str(), "Acid Bass");
    assert_eq!(track.loaded_from, Some(3));
}

#[test]
fn track_edit_does_not_modify_pool() {
    let mut pool = SoundPool::new();
    pool.store(0, Patch::init(ChainType::PizzaPoly));

    let mut track = Track::new(ChainType::PizzaPoly);
    track.load_from_pool(&pool, 0);
    track.patch.params.out.volume = 0.0; // mute

    // Pool slot unchanged
    assert!(pool.get(0).unwrap().params.out.volume > 0.0);
}

#[test]
fn track_save_to_pool_overwrites() {
    let mut pool = SoundPool::new();
    pool.store(5, Patch::init(ChainType::PizzaPoly));

    let mut track = Track::new(ChainType::Modal);
    track.patch.name = *b"My Sound\0\0\0\0\0\0\0\0";
    track.save_to_pool(&mut pool, 5);

    assert_eq!(pool.get(5).unwrap().name_str(), "My Sound");
    assert_eq!(pool.get(5).unwrap().chain_type, ChainType::Modal);
}

#[test]
fn project_has_six_tracks() {
    let project = Project::new();
    assert_eq!(project.tracks.len(), 6);
}

// ── Navigation tests ─────────────────────────────────────────────

#[test]
fn plus_moves_one_block_per_press() {
    let mut ui = UiState::new();
    let start_node = ui.nav.node;

    // Single Pressed event → one step
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Pressed));
    assert_eq!(ui.nav.node, start_node + 1);

    // Held does NOT advance further
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Held));
    assert_eq!(ui.nav.node, start_node + 1);

    // Released does NOT advance
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Released));
    assert_eq!(ui.nav.node, start_node + 1);

    // Another Pressed → one more step
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Pressed));
    assert_eq!(ui.nav.node, start_node + 2);
}

#[test]
fn minus_moves_one_block_per_press() {
    let mut ui = UiState::new();

    // Move forward first so we have room to go back
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Pressed));
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Pressed));
    assert_eq!(ui.nav.node, 2);

    // Minus → one step back
    ui.handle_input(&MockControls::new().button(ButtonId::Minus, ButtonState::Pressed));
    assert_eq!(ui.nav.node, 1);

    // Held does NOT go further
    ui.handle_input(&MockControls::new().button(ButtonId::Minus, ButtonState::Held));
    assert_eq!(ui.nav.node, 1);
}

#[test]
fn edit_held_with_b_press_does_not_navigate() {
    let mut ui = UiState::new();
    let start_node = ui.nav.node;

    // Edit+B1 should open browser, NOT navigate
    ui.handle_input(&MockControls::new()
        .button(ButtonId::Edit, ButtonState::Held)
        .button(ButtonId::B1, ButtonState::Pressed));

    // Should be in browser, not navigated
    assert!(matches!(ui.ui_mode, UiMode::PatchBrowser { .. }));
    assert_eq!(ui.nav.node, start_node);
}

// ── Patch browser integration tests ─────────────────────────────

/// Helper: open patch browser for a track via Edit + B-button
fn open_browser(ui: &mut UiState, btn: ButtonId) {
    ui.handle_input(&MockControls::new()
        .button(ButtonId::Edit, ButtonState::Held)
        .button(btn, ButtonState::Pressed));
}

#[test]
fn edit_b1_opens_patch_browser() {
    let mut ui = UiState::new();
    assert!(matches!(ui.ui_mode, UiMode::Normal));

    open_browser(&mut ui, ButtonId::B1);
    assert!(matches!(ui.ui_mode, UiMode::PatchBrowser { track: 0, .. }));
}

#[test]
fn edit_b3_opens_browser_for_track_2() {
    let mut ui = UiState::new();

    open_browser(&mut ui, ButtonId::B3);
    assert!(matches!(ui.ui_mode, UiMode::PatchBrowser { track: 2, .. }));
}

#[test]
fn b1_without_edit_does_not_open_browser() {
    let mut ui = UiState::new();

    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
    assert!(matches!(ui.ui_mode, UiMode::Normal));
}

#[test]
fn browser_load_copies_patch_to_track() {
    let mut ui = UiState::new();

    // Store a named patch in pool slot 2
    let mut patch = Patch::init(ChainType::PizzaPoly);
    patch.name = *b"Test Sound\0\0\0\0\0\0";
    ui.project.pool.store(2, patch);

    // Open browser for B1 (track 0)
    open_browser(&mut ui, ButtonId::B1);
    assert!(matches!(ui.ui_mode, UiMode::PatchBrowser { track: 0, cursor: 0, .. }));

    // Scroll down to slot 2
    ui.handle_input(&MockControls::new().encoder(EncoderId::Main, 2));

    // Confirm selection
    ui.handle_input(&MockControls::new().button(ButtonId::Edit, ButtonState::Pressed));

    // Should exit browser and load patch into track 0
    assert!(matches!(ui.ui_mode, UiMode::Normal));
    assert_eq!(ui.project.tracks[0].patch.name_str(), "Test Sound");
    assert_eq!(ui.project.tracks[0].loaded_from, Some(2));
}

#[test]
fn browser_cancel_does_not_load() {
    let mut ui = UiState::new();
    let original_name = ui.project.tracks[0].patch.name;

    // Store patch and open browser
    ui.project.pool.store(0, Patch::init(ChainType::Modal));
    open_browser(&mut ui, ButtonId::B1);
    assert!(matches!(ui.ui_mode, UiMode::PatchBrowser { .. }));

    // Cancel by pressing a B-button
    ui.handle_input(&MockControls::new().button(ButtonId::B2, ButtonState::Pressed));

    assert!(matches!(ui.ui_mode, UiMode::Normal));
    assert_eq!(ui.project.tracks[0].patch.name, original_name);
}

#[test]
fn browser_save_to_pool() {
    let mut ui = UiState::new();

    // Edit track 0's patch name
    ui.project.tracks[0].patch.name = *b"My Bass\0\0\0\0\0\0\0\0\0";

    // Open browser for B1
    open_browser(&mut ui, ButtonId::B1);
    assert!(matches!(ui.ui_mode, UiMode::PatchBrowser { .. }));

    // Scroll to slot 5 and save
    ui.handle_input(&MockControls::new().encoder(EncoderId::Main, 5));
    ui.handle_input(&MockControls::new().button(ButtonId::Seq, ButtonState::Pressed));

    // Should stay in browser, and pool slot 5 now has our patch
    assert!(matches!(ui.ui_mode, UiMode::PatchBrowser { .. }));
    assert_eq!(ui.project.pool.get(5).unwrap().name_str(), "My Bass");
}

#[test]
fn browser_init_entries_set_chain_type() {
    let mut ui = UiState::new();

    // Open browser, scroll to "(init) Modal" (POOL_SIZE + 1)
    open_browser(&mut ui, ButtonId::B1);
    ui.handle_input(&MockControls::new().encoder(EncoderId::Main, (POOL_SIZE + 1) as i8));
    ui.handle_input(&MockControls::new().button(ButtonId::Edit, ButtonState::Pressed));

    assert!(matches!(ui.ui_mode, UiMode::Normal));
    assert_eq!(ui.project.tracks[0].patch.chain_type, ChainType::Modal);
    assert!(ui.project.tracks[0].patch.name_str().starts_with("(init)"));

    // Open browser again, scroll to "(init) FM" (POOL_SIZE + 2)
    open_browser(&mut ui, ButtonId::B1);
    ui.handle_input(&MockControls::new().encoder(EncoderId::Main, (POOL_SIZE + 2) as i8));
    ui.handle_input(&MockControls::new().button(ButtonId::Edit, ButtonState::Pressed));

    assert!(matches!(ui.ui_mode, UiMode::Normal));
    assert_eq!(ui.project.tracks[0].patch.chain_type, ChainType::Fm);
}

// ── Priming guard (Task 16 bridge; deleted in Task 19) ───────────

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

fn primed(ui: &UiState) -> Vec<chimera_core::mod_path::ParamPath> {
    let reg = &ui.project.tracks[ui.active_track].patch.dest_registry;
    (0..reg.len()).map(|i| reg.get(i).unwrap().path).collect()
}

/// Positive control: the Pizza filter page primes cutoff.
#[test]
fn priming_on_main_page_registers_focused_param() {
    let mut ui = UiState::new();
    press(&mut ui, ButtonId::Plus);
    press(&mut ui, ButtonId::Plus); // node 2: Filter
    prime(&mut ui); // slot 0: cutoff
    assert_eq!(primed(&ui), [chimera_core::mod_path::ParamPath::Block { block: 2, param: 0 }]);
}

/// Pizza LFO sub-page (node 4, sub-page 2): `Block{4,0}` would resolve to the
/// AmpEnv attack on the Vca page, not LFO rate, so nothing is registered.
#[test]
fn priming_on_pizza_lfo_sub_page_registers_nothing() {
    let mut ui = UiState::new();
    for _ in 0..4 {
        press(&mut ui, ButtonId::Plus);
    }
    press(&mut ui, ButtonId::Edit);
    press(&mut ui, ButtonId::Edit); // sub-page 2: LFO
    assert_eq!(ui.page(), chimera_core::ui::page::PageId::Lfo);
    prime(&mut ui);
    assert!(primed(&ui).is_empty());
}

/// FmRatio slot 2 edits op C coarse; `Block{0,2}` on the FM chain would
/// resolve to FmAlg slot 2 (Out.VOLUME), so nothing new is registered.
#[test]
fn priming_on_fm_ratio_slot_2_registers_nothing() {
    let mut ui = UiState::new();
    ui.project.tracks[0] = Track::new(ChainType::Fm);
    ui.nav.chain_type = ChainType::Fm;
    press(&mut ui, ButtonId::Edit);
    press(&mut ui, ButtonId::Edit); // sub-page 2: FmRatio
    assert_eq!(ui.page(), chimera_core::ui::page::PageId::FmRatio);
    ui.handle_input(&MockControls::new().encoder(EncoderId::C, 1)); // focus slot 2
    let before = primed(&ui); // the FM init pre-wire
    prime(&mut ui);
    assert_eq!(primed(&ui), before);
}
