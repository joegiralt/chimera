//! Mod-priming feedback (issue #21): MIX+PLUS reports its outcome in the
//! focus band — `ADDED`, `ALREADY ROUTED`, `NOT MODULATABLE`, `MATRIX
//! FULL` — until the next encoder, button or page change, and the matrix
//! page's hint says where priming actually works.

mod screen;

use chimera_core::mod_path::RegistryError;
use chimera_core::ui::draw;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::{PrimeStatus, UiState};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

/// Focus a slot, then MIX + Plus.
fn prime(ui: &mut UiState, enc: EncoderId) {
    feed(ui, Input::turn(enc, 1));
    feed(ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
}

#[test]
fn mix_plus_on_a_fresh_modulatable_param_reports_added() {
    let mut ui = UiState::new(); // Part 1, Pizza page: slot A is SHAPE
    prime(&mut ui, EncoderId::A);
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Added));
}

#[test]
fn mix_plus_on_an_already_routed_param_reports_already_routed() {
    let mut ui = UiState::new();
    prime(&mut ui, EncoderId::A);
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Added));

    // Same slot, primed again: still focused (no other input in between).
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    assert_eq!(ui.prime_status(), Some(PrimeStatus::AlreadyRouted));
}

/// Mirrors `ui_routing_test::priming_a_non_modulatable_param_is_refused`
/// (LFO RATE, on the LFO sub-page under the matrix node), now also checking
/// the reported status.
#[test]
fn mix_plus_on_a_non_modulatable_param_reports_not_modulatable() {
    let mut ui = UiState::new();
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus)); // -> MOD node
    }
    feed(&mut ui, Input::press(ButtonId::Edit)); // Envelope sub-page
    feed(&mut ui, Input::press(ButtonId::Edit)); // LFO sub-page
    prime(&mut ui, EncoderId::A);
    assert_eq!(ui.prime_status(), Some(PrimeStatus::NotModulatable));
}

/// `MAX_REGISTRY_DESTS` (32) exceeds the number of distinct modulatable
/// addresses the current param specs define in total (25 —
/// `mod_registry_test::registry_accepts_exactly_the_modulatable_addresses`),
/// so a real matrix can never actually fill up through UI navigation alone;
/// `mod_path::tests::add_refuses_once_physically_full` covers the registry
/// itself. This covers the other half: the UI's mapping from that refusal
/// to the shown status.
#[test]
fn registry_full_maps_to_the_full_status() {
    assert_eq!(PrimeStatus::from(RegistryError::Full), PrimeStatus::Full);
    assert_eq!(PrimeStatus::from(RegistryError::NotModulatable), PrimeStatus::NotModulatable);
}

#[test]
fn an_encoder_turn_clears_the_status() {
    let mut ui = UiState::new();
    prime(&mut ui, EncoderId::A);
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Added));

    feed(&mut ui, Input::turn(EncoderId::B, 1)); // touches a different slot
    assert_eq!(ui.prime_status(), None);
}

#[test]
fn a_page_change_clears_the_status() {
    let mut ui = UiState::new();
    prime(&mut ui, EncoderId::A);
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Added));

    feed(&mut ui, Input::press(ButtonId::Plus)); // bare Plus: next node
    assert_eq!(ui.prime_status(), None);
}

#[test]
fn un_priming_also_clears_the_status() {
    let mut ui = UiState::new();
    prime(&mut ui, EncoderId::A);
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Added));

    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert_eq!(ui.prime_status(), None);
}

/// Direction A (ADR 0016): the hint must fit its row (spec: no literals —
/// widths come from `theme::SCREEN_W`/`MARGIN_X`, the same budget every
/// other row is checked against).
#[test]
fn matrix_hint_fits_its_row() {
    let w = draw::text_width(&theme::FONT_LABEL, "PRIME: MIX+PLUS ON A PARAM", 0);
    assert!(w <= theme::SCREEN_W - theme::MARGIN_X * 2, "hint is {w}px wide");
}

/// Review Focus (#21): the message is redrawn through the dirty-region
/// system, not painted over the old frame — a dirty render with a status
/// shown must equal a full render (the `all_pages_walk_test` pattern). The
/// first `render_dirty_with_scope` call on a fresh `UiState` always redraws
/// every region (its cache starts at sentinel values), which would make
/// this pass trivially even if `RegionData::Focus` never carried the status
/// at all; priming only *after* an initial settled render exercises the
/// actual region-diff path that `RegionData::Focus`'s new `status` field
/// has to feed.
#[test]
fn dirty_render_with_a_status_message_equals_full_render() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::turn(EncoderId::A, 1)); // focus SHAPE, no prime yet
    settle(&mut ui);
    let scope = scope_fixture();
    let mut dirty = Fb::new();
    ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope); // seeds the region cache, no status

    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus)); // prime -> ADDED
    settle(&mut ui);
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Added));

    let flushed = ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope);
    assert!(
        flushed.iter().any(|&(a, b)| a != b),
        "priming must dirty at least the focus band, not silently no-op: {flushed:?}"
    );
    let mut full = Fb::new();
    ui.render_with_scope(&mut full, &PerfStats::zero(), &scope);
    assert!(dirty.px == full.px, "dirty render with a status message must equal a full render");
}
