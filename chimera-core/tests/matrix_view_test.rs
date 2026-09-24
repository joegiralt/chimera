//! Mod matrix page in Direction A (UI refresh spec § Page types): the
//! selected route in the focus band, then a dot grid.

mod screen;

use chimera_core::ui::components;
use chimera_core::ui::mod_grid::cell_center;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::renderer::{amount_of, amount_value, MATRIX_AMOUNT_SLOT};
use chimera_core::ui::theme;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

#[test]
fn amount_display_value_round_trips() {
    for a in -127..=127i8 {
        assert_eq!(amount_of(amount_value(a)), a);
    }
    assert_eq!(amount_value(0), 0.5);
}

/// The fixture: ENV→CUTOFF +20, ENV→FOLD −30, LFO→CUTOFF +42 (selected).
#[test]
fn focus_band_names_the_selected_route() {
    let fb = render("mod_matrix");
    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    components::focus_route(&mut want, "LFO", "CUTOFF", "+42", amount_value(42));
    assert!(fb.px[28 * W..118 * W] == want.px[28 * W..118 * W]);
}

#[test]
fn dots_show_sign_and_size_and_the_cursor_is_outlined() {
    let fb = render("mod_matrix");
    let (x, y) = cell_center(0, 0); // ENV → CUTOFF, +20
    assert_eq!(fb.at(x, y), theme::INK2, "positive: filled");
    let (x, y) = cell_center(1, 0); // ENV → FOLD, −30
    assert_eq!(fb.at(x, y), theme::BG, "negative: a ring");
    assert!((1..6).any(|r| fb.at(x + r, y) == theme::INK2));
    let (x, y) = cell_center(1, 1); // LFO → FOLD, none
    assert_eq!(fb.at(x, y), theme::FAINT, "no route: a tiny dim dot");
    assert_ne!(fb.at(x + 2, y), theme::FAINT);
    let (x, y) = cell_center(0, 1); // LFO → CUTOFF, selected
    assert_eq!(fb.at(x, y), theme::ACCENT, "selected route lit");
    assert_eq!(fb.at(x - 14, y), theme::ACCENT, "cursor outline");
}

#[test]
fn the_amount_lerps() {
    let mut ui = ui_for("mod_matrix");
    let before = ui.renderer.anim[MATRIX_AMOUNT_SLOT].current();
    feed(&mut ui, Input::turn(EncoderId::E, 40));
    ui.update();
    let (now, target) = (ui.renderer.anim[MATRIX_AMOUNT_SLOT].current(), ui.renderer.anim[MATRIX_AMOUNT_SLOT].target());
    assert!(before < now && now < target, "{before} < {now} < {target}");
    assert_eq!(amount_of(target), 82);
}

#[test]
fn an_empty_matrix_says_so() {
    let mut ui = chimera_core::ui::UiState::new();
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    assert_eq!(fb.oob, 0);
    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    chimera_core::ui::draw::text_tracked(&mut want, &theme::FONT_VALUE, "NO DESTINATIONS", theme::MARGIN_X, theme::FOCUS_LABEL_Y, theme::MID, theme::LABEL_TRACKING);
    assert!(fb.px[28 * W..118 * W] == want.px[28 * W..118 * W]);
}

#[test]
fn matrix_dirty_render_equals_full_render() {
    assert!(render("mod_matrix").px == render_dirty("mod_matrix").px);
}

/// More destinations than columns: the grid scrolls, shows `<`, and the
/// cursor stays on screen.
#[test]
fn a_wide_matrix_scrolls_with_the_cursor() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::block::ParamId;
    use chimera_core::ui::mod_grid::{draw_grid, ModDest, MatrixState, MAX_DESTS};
    let mut m = MatrixState::new();
    m.rebuild_sources(&["ENV", "LFO"]);
    for i in 0..MAX_DESTS {
        m.dests[i] = Some(ModDest { addr: ParamAddr::new(BlockRef::Filter, ParamId(i as u8 % 6)), label: [b'X'; 8] });
    }
    m.num_dests = MAX_DESTS;
    m.move_col(MAX_DESTS as i8 - 1);
    assert_eq!(m.scroll_x, MAX_DESTS - m.visible_cols());
    let mut fb = Fb::new();
    draw_grid(&mut fb, &m);
    assert_eq!(fb.oob, 0);
    let (x, y) = cell_center(m.visible_cols() - 1, 0);
    assert_eq!(fb.at(x - 14, y), theme::ACCENT, "cursor in the last visible column");
}

/// PRE-FLIGHT ruling: the stats line must not overflow the 32-byte `FmtBuf`
/// at the worst case — `MAX_MOD_SOURCES` × `MAX_DESTS` routes, `MAX_DESTS`
/// of `MAX_DESTS` destinations. The original `"{} OF {} DESTINATIONS"`
/// wording produced `"128 ROUTES   16 OF 16 DESTINATIONS"` (34 bytes),
/// silently truncated by `FmtBuf`. The shortened wording must fit and must
/// not be truncated.
#[test]
fn stats_line_fits_untruncated_at_max_counts() {
    use chimera_core::modulation::MAX_MOD_SOURCES;
    use chimera_core::ui::fmt::FmtBuf;
    use chimera_core::ui::mod_grid::{fmt_stats, MAX_DESTS};

    let max_routes = MAX_MOD_SOURCES * MAX_DESTS;
    let mut buf = FmtBuf::new();
    fmt_stats(&mut buf, max_routes, MAX_DESTS);

    let want = format!("{} ROUTES   {} OF {} DEST", max_routes, MAX_DESTS, MAX_DESTS);
    assert!(want.len() <= 32, "test's own expectation must fit FmtBuf: {} bytes", want.len());
    assert_eq!(buf.as_str(), want, "stats line must be produced in full, not truncated");
}

/// The same worst case, rendered: `draw_grid` must not panic or draw outside
/// the screen when every cell of a full matrix is routed.
#[test]
fn stats_line_renders_at_max_counts_without_overflow() {
    use chimera_core::modulation::MAX_MOD_SOURCES;
    use chimera_core::ui::mod_grid::{draw_grid, ModDest, MatrixState, MAX_DESTS};
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::block::ParamId;

    let mut m = MatrixState::new();
    let names: [&'static str; MAX_MOD_SOURCES] = ["S0", "S1", "S2", "S3", "S4", "S5", "S6", "S7"];
    m.rebuild_sources(&names);
    for i in 0..MAX_DESTS {
        m.dests[i] = Some(ModDest { addr: ParamAddr::new(BlockRef::Filter, ParamId(i as u8 % 6)), label: [b'X'; 8] });
    }
    m.num_dests = MAX_DESTS;
    for r in 0..m.num_sources {
        for c in 0..m.num_dests {
            m.amounts[r][c] = 1;
        }
    }
    let mut fb = Fb::new();
    draw_grid(&mut fb, &m);
    assert_eq!(fb.oob, 0);
}
