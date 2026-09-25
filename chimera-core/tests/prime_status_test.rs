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

/// The UI's mapping from the registry's refusals to the shown status;
/// `priming_past_matrix_capacity_on_the_fm_chain_reports_full` below
/// reaches `Full` through real input.
#[test]
fn registry_full_maps_to_the_full_status() {
    assert_eq!(PrimeStatus::from(RegistryError::Full), PrimeStatus::Full);
    assert_eq!(
        PrimeStatus::from(RegistryError::NotModulatable),
        PrimeStatus::NotModulatable
    );
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
    assert!(
        w <= theme::SCREEN_W - theme::MARGIN_X * 2,
        "hint is {w}px wide"
    );
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
    assert!(
        dirty.px == full.px,
        "dirty render with a status message must equal a full render"
    );
}

// ── BigViz pages (final review I1) ──────────────────────────────────────
//
// BigViz layouts have no focus band, so the status draws as one line at the
// top of the viz band, above `viz::PLOT_TOP` — where no curve and no
// touched-value readout ever reach (the readout is clamped to the plot).

use chimera_core::preset::ChainType;
use chimera_core::ui::viz;

fn full(ui: &UiState) -> Fb {
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    fb
}

/// Pixels that differ between `a` and `b` in rows `top..bottom`.
fn diff_rows(a: &Fb, b: &Fb, top: i32, bottom: i32) -> usize {
    let w = theme::SCREEN_W as usize;
    (top as usize * w..bottom as usize * w)
        .filter(|&i| a.px[i] != b.px[i])
        .count()
}

/// Pizza → Drive → Filter, CUTOFF focused.
fn filter_page() -> UiState {
    let mut ui = UiState::new();
    feed(&mut ui, Input::press(ButtonId::Plus));
    feed(&mut ui, Input::press(ButtonId::Plus));
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    settle(&mut ui);
    ui
}

/// FM init Sound → MOD node → FM ENV1 sub-page, slot C focused.
fn fm_env_page() -> UiState {
    let mut ui = UiState::new();
    load_init(&mut ui, ChainType::Fm);
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    feed(&mut ui, Input::press(ButtonId::Edit));
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    settle(&mut ui);
    ui
}

/// The status shows on a BigViz page, confined to the strip between the
/// header and the plot: the viz (and its readout) below is untouched.
fn assert_status_line_shows(mut ui: UiState, name: &str) {
    let before = full(&ui);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    settle(&mut ui);
    let status = ui.prime_status();
    assert!(status.is_some(), "{name}: MIX+PLUS set no status");
    let after = full(&ui);
    after.dump(name);

    let strip = diff_rows(&before, &after, theme::HEADER_BOTTOM, viz::PLOT_TOP);
    assert!(
        strip > 40,
        "{name}: {status:?} not drawn ({strip} px changed)"
    );
    assert_eq!(
        diff_rows(&before, &after, 0, theme::HEADER_BOTTOM),
        0,
        "{name}: status leaked into the header"
    );
    assert_eq!(
        diff_rows(&before, &after, viz::PLOT_TOP, theme::BIGVIZ_BOTTOM),
        0,
        "{name}: status overlaps the plot / readout"
    );
}

#[test]
fn the_status_shows_on_the_filter_page() {
    assert_status_line_shows(filter_page(), "prime_status_filter");
}

#[test]
fn the_status_shows_on_an_fm_envelope_page() {
    assert_status_line_shows(fm_env_page(), "prime_status_fm_env");
}

/// Dirty render == full render when the status appears on a BigViz page
/// (region cache seeded first, so this can't pass on sentinels), and again
/// when the next input clears it — and the clear really restores the strip.
#[test]
fn bigviz_status_appears_and_clears_through_the_dirty_regions() {
    let mut ui = filter_page();
    let scope = scope_fixture();
    let mut dirty = Fb::new();
    ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope);
    let before = full(&ui);

    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    settle(&mut ui);
    assert!(ui.prime_status().is_some());
    ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope);
    assert!(dirty.px == full(&ui).px, "appear: dirty != full");
    assert!(diff_rows(&before, &dirty, theme::HEADER_BOTTOM, viz::PLOT_TOP) > 0);

    feed(&mut ui, Input::press(ButtonId::Mix)); // any input retires it
    settle(&mut ui);
    assert_eq!(ui.prime_status(), None);
    ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope);
    let cleared = full(&ui);
    assert!(dirty.px == cleared.px, "clear: dirty != full");
    assert_eq!(
        diff_rows(&before, &dirty, theme::HEADER_BOTTOM, viz::PLOT_TOP),
        0,
        "clear left the status on screen"
    );
}

/// Final review M8: on a Focus-band page, the status clearing on the next
/// input redraws the focus band — dirty == full, and the value is back.
#[test]
fn focus_band_status_clearing_redraws_through_the_dirty_regions() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    settle(&mut ui);
    let before = full(&ui);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    settle(&mut ui);
    let scope = scope_fixture();
    let mut dirty = Fb::new();
    ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope); // status shown

    feed(&mut ui, Input::press(ButtonId::Mix)); // retires it, changes no value
    settle(&mut ui);
    assert_eq!(ui.prime_status(), None);
    let flushed = ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope);
    assert!(
        flushed.iter().any(|&(a, b)| a != b),
        "clearing redrew nothing"
    );
    let cleared = full(&ui);
    assert!(dirty.px == cleared.px, "clear: dirty != full");
    assert_eq!(
        diff_rows(&before, &cleared, theme::HEADER_BOTTOM, theme::FOCUS_BOTTOM),
        0,
        "the focus band did not return to the value readout"
    );
}

// ── Matrix capacity (final review I2) ───────────────────────────────────

const ALL_SLOTS: [EncoderId; 6] = [
    EncoderId::A,
    EncoderId::B,
    EncoderId::C,
    EncoderId::D,
    EncoderId::E,
    EncoderId::F,
];

/// MIX+PLUS each of `slots` on the current page; collects each status set.
fn prime_every_slot(ui: &mut UiState, slots: &[EncoderId], out: &mut Vec<PrimeStatus>) {
    for &enc in slots {
        feed(ui, Input::turn(enc, 1));
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
        out.extend(ui.prime_status());
    }
}

/// The FM chain alone exposes more modulatable addresses (4 operators ×
/// LEVEL/FDBK, Drive, Filter, Folder) than the matrix holds. Priming them
/// all through real input: exactly `MAX_MOD_DESTS` report ADDED, the next
/// distinct one reports MATRIX FULL, and the matrix holds every added one.
#[test]
fn priming_past_matrix_capacity_on_the_fm_chain_reports_full() {
    use chimera_core::modulation::MAX_MOD_DESTS;
    let mut ui = UiState::new();
    load_init(&mut ui, ChainType::Fm);
    let mut seen = Vec::new();

    feed(&mut ui, Input::press(ButtonId::Edit)); // Operator sub-page
    feed(&mut ui, Input::turn(EncoderId::A, -8)); // operator 1
    for op in 0..4 {
        if op > 0 {
            feed(&mut ui, Input::turn(EncoderId::A, 1)); // next operator
        }
        prime_every_slot(&mut ui, &ALL_SLOTS[1..], &mut seen); // A selects the op
    }
    for _ in 0..3 {
        feed(&mut ui, Input::press(ButtonId::Plus)); // Drive, Filter, Folder
        prime_every_slot(&mut ui, &ALL_SLOTS, &mut seen);
    }

    let added = seen.iter().filter(|&&s| s == PrimeStatus::Added).count();
    assert_eq!(added, MAX_MOD_DESTS, "{seen:?}");
    let first_full = seen.iter().position(|&s| s == PrimeStatus::Full);
    let added_before = first_full.map(|i| {
        seen[..i]
            .iter()
            .filter(|&&s| s == PrimeStatus::Added)
            .count()
    });
    assert_eq!(
        added_before,
        Some(MAX_MOD_DESTS),
        "the 17th distinct address must report MATRIX FULL: {seen:?}"
    );
    assert_eq!(ui.matrix_state.num_dests, MAX_MOD_DESTS);
    assert_eq!(
        ui.performance.parts[0].sound.dest_registry.len(),
        MAX_MOD_DESTS
    );
}
