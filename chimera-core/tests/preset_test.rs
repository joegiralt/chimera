use chimera_core::preset::{ChainType, Patch, Project, SoundPool, Track, POOL_SIZE};
use chimera_core::params::Param;
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
    assert!(p.params.volume.value() > 0.0);
    assert!(p.params.filter.cutoff.value() > 1000.0);
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
    track.patch.params.volume = Param::new(0.0, 1.0, 0.0); // mute

    // Pool slot unchanged
    assert!(pool.get(0).unwrap().params.volume.value() > 0.0);
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

// ── UI integration tests ──────────────────────────────────────────

#[test]
fn double_tap_b1_opens_patch_browser() {
    let mut ui = UiState::new();
    assert!(matches!(ui.ui_mode, UiMode::Normal));

    // First press at tick 100
    ui.set_tick(100);
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
    assert!(matches!(ui.ui_mode, UiMode::Normal));

    // Second press 100ms later (50 ticks at 500Hz) — within 300ms window
    ui.set_tick(150);
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
    assert!(matches!(ui.ui_mode, UiMode::PatchBrowser { track: 0, .. }));
}

#[test]
fn double_tap_b3_opens_browser_for_track_2() {
    let mut ui = UiState::new();

    ui.set_tick(100);
    ui.handle_input(&MockControls::new().button(ButtonId::B3, ButtonState::Pressed));
    ui.set_tick(150);
    ui.handle_input(&MockControls::new().button(ButtonId::B3, ButtonState::Pressed));

    assert!(matches!(ui.ui_mode, UiMode::PatchBrowser { track: 2, .. }));
}

#[test]
fn slow_double_press_does_not_open_browser() {
    let mut ui = UiState::new();

    ui.set_tick(100);
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
    // 500ms later (250 ticks) — past the 300ms/150-tick window
    ui.set_tick(350);
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

    // Open browser for B1 (track 0) via double-tap
    ui.set_tick(100);
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
    ui.set_tick(150);
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
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

    // Store patch and open browser via double-tap
    ui.project.pool.store(0, Patch::init(ChainType::Modal));
    ui.set_tick(100);
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
    ui.set_tick(150);
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
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

    // Open browser for B1 via double-tap
    ui.set_tick(100);
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
    ui.set_tick(150);
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
    assert!(matches!(ui.ui_mode, UiMode::PatchBrowser { .. }));

    // Scroll to slot 5 and save
    ui.handle_input(&MockControls::new().encoder(EncoderId::Main, 5));
    ui.handle_input(&MockControls::new().button(ButtonId::Seq, ButtonState::Pressed));

    // Should stay in browser, and pool slot 5 now has our patch
    assert!(matches!(ui.ui_mode, UiMode::PatchBrowser { .. }));
    assert_eq!(ui.project.pool.get(5).unwrap().name_str(), "My Bass");
}

#[test]
fn browser_init_resets_to_track_chain_type() {
    let mut ui = UiState::new();

    // Set track 0 to Modal chain
    ui.project.tracks[0].patch.chain_type = ChainType::Modal;
    ui.project.tracks[0].patch.name = *b"Custom Modal\0\0\0\0";

    // Open browser via double-tap, scroll to init entry (slot 32 = POOL_SIZE)
    ui.set_tick(100);
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));
    ui.set_tick(150);
    ui.handle_input(&MockControls::new().button(ButtonId::B1, ButtonState::Pressed));

    // Scroll to the init entry at the end
    ui.handle_input(&MockControls::new().encoder(EncoderId::Main, POOL_SIZE as i8));

    // Select init
    ui.handle_input(&MockControls::new().button(ButtonId::Edit, ButtonState::Pressed));

    assert!(matches!(ui.ui_mode, UiMode::Normal));
    // Should reset to Modal init, not PizzaPoly
    assert_eq!(ui.project.tracks[0].patch.chain_type, ChainType::Modal);
    assert!(ui.project.tracks[0].patch.name_str().starts_with("(init)"));
}
