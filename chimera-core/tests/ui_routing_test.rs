//! Priming mod destinations from pages (spec §5, Review Focus 1 and 5).

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::params::{DriveParams, FilterParams};
use chimera_core::ui::UiState;
use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

/// Mock controls for driving `UiState::handle_input`.
struct MockControls {
    buttons: Vec<(ButtonId, ButtonState)>,
    encoders: Vec<(EncoderId, i8)>,
}

impl MockControls {
    fn new() -> Self {
        Self {
            buttons: Vec::new(),
            encoders: Vec::new(),
        }
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
        self.buttons
            .iter()
            .find(|b| b.0 == id)
            .map_or(ButtonState::Up, |b| b.1)
    }
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.encoders.iter().find(|e| e.0 == id).map_or(0, |e| e.1)
    }
}

fn press(ui: &mut UiState, id: ButtonId) {
    ui.handle_input(&MockControls::new().button(id, ButtonState::Pressed));
}

/// Plus ×2 from a Part's home reaches the Drive page (DRIVE, TONE, MIX).
fn to_drive(ui: &mut UiState) {
    for _ in 0..2 {
        press(ui, ButtonId::Plus);
    }
}

/// Plus ×3 from a Part's home reaches Filter, then Edit reaches its
/// FLT › MODE sub-page (MODE, DRIVE).
fn to_filter_mode(ui: &mut UiState) {
    for _ in 0..3 {
        press(ui, ButtonId::Plus);
    }
    press(ui, ButtonId::Edit);
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

/// Touch encoder `enc` (focus its slot), then MIX + Plus.
fn prime_slot(ui: &mut UiState, enc: EncoderId) {
    ui.handle_input(&MockControls::new().encoder(enc, 1));
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(ButtonId::Plus, ButtonState::Pressed),
    );
}

/// Touch encoder `enc` (focus its slot), then MIX + Minus (un-prime).
fn unprime_slot(ui: &mut UiState, enc: EncoderId) {
    ui.handle_input(&MockControls::new().encoder(enc, 1));
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(ButtonId::Minus, ButtonState::Pressed),
    );
}

/// Plus x3 from the Drive page reaches the MOD node; Minus x3 returns.
fn enter_matrix(ui: &mut UiState) {
    for _ in 0..3 {
        press(ui, ButtonId::Plus);
    }
}
fn leave_matrix(ui: &mut UiState) {
    for _ in 0..3 {
        press(ui, ButtonId::Minus);
    }
}

fn primed(ui: &UiState) -> Vec<ParamAddr> {
    let reg = &ui.performance.parts[0].sound.dest_registry;
    (0..reg.len())
        .filter_map(|i| reg.get(i))
        .map(|e| e.addr)
        .collect()
}

#[test]
fn priming_on_a_part_page_registers_its_address() {
    let mut ui = UiState::new(); // Part 1, Drive page
    to_drive(&mut ui);
    prime_slot_0(&mut ui);
    assert_eq!(
        primed(&ui),
        [ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE)]
    );
    let reg = &ui.performance.parts[0].sound.dest_registry;
    assert_eq!(reg.get(0).unwrap().label_str(), "DRVDRIVE");
    assert_eq!(ui.mod_state().num_dests(), 1);
}

/// Review fix round 1: DRIVE lives on FLT › MODE, whose own short is "MDE",
/// not the filter's tag. `mod_label` must key its prefix off the address's
/// block (`mod_grid::block_tag`), not the active page's short name, so
/// priming it from the sub-page still tags it "FLT".
#[test]
fn priming_the_filter_drive_from_flt_mode_tags_it_flt() {
    let mut ui = UiState::new();
    to_filter_mode(&mut ui);
    prime_slot(&mut ui, EncoderId::B); // DRIVE (slot 1: MODE, DRIVE, --, --, --, --)
    assert_eq!(
        primed(&ui),
        [ParamAddr::new(BlockRef::Filter, FilterParams::DRIVE)]
    );
    let reg = &ui.performance.parts[0].sound.dest_registry;
    assert_eq!(reg.get(0).unwrap().label_str(), "FLTDRIVE");
}

/// Review Focus 1: priming on the Mixer (bound, not modulatable) or System
/// (Legacy) chain must not register anything (it used to register
/// `Block{node,i}`, which the voice read as an engine/Drive/Filter/Folder param).
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
    for _ in 0..5 {
        press(&mut ui, ButtonId::Plus); // → MOD node
    }
    press(&mut ui, ButtonId::Edit); // Envelope sub-page
    press(&mut ui, ButtonId::Edit); // LFO sub-page
    prime_slot_0(&mut ui);
    assert!(primed(&ui).is_empty());
}

/// Spec §4: after loading the Algo init sound the matrix rows are the eight
/// `ModSource`s (they used to be "Op1 Env".."Op4 Env", of which only two
/// produced values).
#[test]
fn algo_matrix_rows_are_the_eight_sources() {
    use chimera_core::preset::POOL_SIZE;

    let mut ui = UiState::new();
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Edit, ButtonState::Held)
            .button(ButtonId::B1, ButtonState::Pressed),
    );
    ui.handle_input(&MockControls::new().encoder(EncoderId::A, POOL_SIZE as i8));
    press(&mut ui, ButtonId::Edit); // load "(init) Algo"
    let rows: Vec<&str> = (0..ui.matrix_state.num_sources)
        .map(|i| ui.matrix_state.sources[i].unwrap().name)
        .collect();
    assert_eq!(rows, chimera_core::ui::block_registry::PART_MOD_SOURCES);
    assert_eq!(ui.matrix_state.num_dests, 0);
}

/// From a Part's first page: Plus ×5 to the MOD node, whose first page is
/// the matrix; encoder E sets the amount at the cursor (ENV → first dest).
fn set_first_amount(ui: &mut UiState, delta: i8) {
    for _ in 0..5 {
        press(ui, ButtonId::Plus);
    }
    ui.handle_input(&MockControls::new().encoder(EncoderId::E, delta));
}

fn routes(ui: &UiState, part: usize) -> Vec<(ParamAddr, i8)> {
    let ms = &ui.performance.parts[part].sound.mod_state;
    (0..ms.num_dests())
        .map(|d| (ms.dest(d), ms.amount(0, d)))
        .collect()
}

/// Switching Part (B<n>, MIX + B<n>) rebuilds the matrix for that Part —
/// sources, destinations and amounts — so editing Part 2's matrix never
/// writes Part 1's routes into it, and Part 1's amounts come back with it.
#[test]
fn switching_part_rebuilds_the_matrix_for_that_part() {
    let drive = ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE);
    let mut ui = UiState::new();
    to_drive(&mut ui);
    prime_slot_0(&mut ui); // Part 1: DRIVE
    press(&mut ui, ButtonId::B1); // back home
    set_first_amount(&mut ui, 10);
    assert_eq!(routes(&ui, 0), [(drive, 10)]);

    press(&mut ui, ButtonId::B2); // Part 2: nothing primed
    assert_eq!(ui.active_part, 1);
    assert_eq!(ui.matrix_state.num_dests, 0, "Part 2's matrix is empty");
    to_drive(&mut ui);
    ui.handle_input(&MockControls::new().encoder(EncoderId::B, 1)); // TONE
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(ButtonId::Plus, ButtonState::Pressed),
    );
    let p2 = ui.performance.parts[1]
        .sound
        .dest_registry
        .get(0)
        .expect("Part 2 primed")
        .addr;
    assert_ne!(p2, drive);
    press(&mut ui, ButtonId::B2); // home
    set_first_amount(&mut ui, 20);
    assert_eq!(routes(&ui, 1), [(p2, 20)], "Part 2 keeps its own route");
    assert_eq!(routes(&ui, 0), [(drive, 10)], "Part 1 untouched");

    // MIX + B1 then B1: back on Part 1, its matrix shows its own amount.
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(ButtonId::B1, ButtonState::Pressed),
    );
    assert_eq!(ui.active_part, 0);
    assert_eq!(
        (ui.matrix_state.num_dests, ui.matrix_state.amounts[0][0]),
        (1, 10)
    );
    press(&mut ui, ButtonId::B1);
    set_first_amount(&mut ui, 1);
    assert_eq!(
        routes(&ui, 0),
        [(drive, 11)],
        "edited from Part 1's amount, not Part 2's"
    );
    assert_eq!(routes(&ui, 1), [(p2, 20)]);
}

/// Issue #11, regression 1: `sel_col`/`scroll_x` used to survive a Part
/// switch uncapped. With real button/encoder input only, the observable
/// bug was not the "phantom route" wording in the issue -- reaching a page
/// where MIX+Plus can prime always requires leaving the matrix, and that
/// navigation already reloads the matrix's amounts from the committed
/// `ModState` (a separate, pre-existing behaviour), which wipes an
/// out-of-range write before any later prime could land on it. What a user
/// actually hit: the cursor stayed drawn past the last column after a Part
/// switch, and turning the amount encoder there silently edited nothing --
/// the write went to a column with no destination and was discarded on the
/// next navigation instead of landing on the Part's one real route.
#[test]
fn priming_after_a_stale_cursor_does_not_inherit_a_phantom_amount() {
    let drive = ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE);
    let mix = ParamAddr::new(BlockRef::Drive, DriveParams::MIX);
    let mut ui = UiState::new();

    // Prime 3 destinations on Part 1 (DRIVE, TONE, MIX) and move the
    // cursor to column 2.
    to_drive(&mut ui);
    prime_slot(&mut ui, EncoderId::A);
    prime_slot(&mut ui, EncoderId::B);
    prime_slot(&mut ui, EncoderId::C);
    enter_matrix(&mut ui);
    ui.handle_input(&MockControls::new().encoder(EncoderId::B, 2));
    assert_eq!(ui.matrix_state.sel_col, 2);

    // Switch to Part 2, which has one destination of its own (DRIVE). Before
    // the fix the cursor is still 2 here, one past Part 2's single column.
    press(&mut ui, ButtonId::B2);
    to_drive(&mut ui);
    prime_slot(&mut ui, EncoderId::A);
    assert_eq!(ui.matrix_state.num_dests, 1);
    assert_eq!(
        ui.matrix_state.sel_col, 0,
        "load_matrix must clamp the cursor to the new Part's destination count"
    );

    // Turn the amount encoder, leave the matrix, then prime a second
    // destination (MIX) -- real navigation and encoder input throughout.
    enter_matrix(&mut ui);
    ui.handle_input(&MockControls::new().encoder(EncoderId::E, 50));
    leave_matrix(&mut ui);
    prime_slot(&mut ui, EncoderId::C);

    assert_eq!(ui.matrix_state.num_dests, 2);
    assert_eq!(
        routes(&ui, 1)[0],
        (drive, 50),
        "the E turn must edit Part 2's own route, not a discarded phantom column"
    );
    assert_eq!(routes(&ui, 1)[1], (mix, 0), "the new route starts at 0");
}

/// Issue #11, regression 2: un-priming rebuilt the destination list (shifted
/// left) but left the amount columns in place, so a surviving route could
/// shift onto a neighbour's old amount. Amounts must follow their
/// destination's `ParamAddr`, not its column position.
#[test]
fn un_priming_keeps_the_other_routes_own_amounts() {
    let drive = ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE);
    let mix = ParamAddr::new(BlockRef::Drive, DriveParams::MIX);
    let mut ui = UiState::new();

    // Prime DRIVE (A), TONE (B), MIX (C) -- columns 0, 1, 2.
    to_drive(&mut ui);
    prime_slot(&mut ui, EncoderId::A);
    prime_slot(&mut ui, EncoderId::B);
    prime_slot(&mut ui, EncoderId::C);
    assert_eq!(ui.matrix_state.num_dests, 3);

    // Give each destination its own, distinct amount.
    enter_matrix(&mut ui);
    ui.handle_input(&MockControls::new().encoder(EncoderId::E, 10)); // col 0: DRIVE +10
    ui.handle_input(&MockControls::new().encoder(EncoderId::B, 1)); // -> col 1
    ui.handle_input(&MockControls::new().encoder(EncoderId::E, 20)); // col 1: TONE +20
    ui.handle_input(&MockControls::new().encoder(EncoderId::B, 1)); // -> col 2
    ui.handle_input(&MockControls::new().encoder(EncoderId::E, 30)); // col 2: MIX +30
    assert_eq!(routes(&ui, 0).len(), 3);

    // Un-prime TONE (B).
    leave_matrix(&mut ui);
    unprime_slot(&mut ui, EncoderId::B);

    assert_eq!(ui.matrix_state.num_dests, 2);
    assert_eq!(
        routes(&ui, 0),
        [(drive, 10), (mix, 30)],
        "A and C keep their own amounts, keyed by destination, not by column"
    );
}
