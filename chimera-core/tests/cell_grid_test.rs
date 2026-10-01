//! CellGrid pages in Direction A (UI refresh spec § Page types, § Testing):
//! header · focus band · live output · cells · map.

mod screen;

use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::components::{self, Cell};
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::page::ValFmt;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::viz;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

fn band(fb: &Fb, y0: i32, y1: i32) -> Vec<u16> {
    fb.px[y0 as usize * W..y1 as usize * W].to_vec()
}

#[test]
fn dirty_render_from_scratch_equals_full_render() {
    for name in ["engine_algo", "algo_alg", "algo_level", "system"] {
        assert!(render(name).px == render_dirty(name).px, "{name}");
    }
}

#[test]
fn a_settled_silent_or_frozen_screen_flushes_nothing() {
    let mut ui = ui_for("engine_algo");
    let mut fb = Fb::new();
    let scope = scope_fixture();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    let second = ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    assert!(second.iter().all(|&(a, b)| a == b), "{second:?}");
}

#[test]
fn a_turn_redraws_focus_and_cells_only() {
    let mut ui = ui_for("algo_level");
    let mut fb = Fb::new();
    let scope = scope_fixture();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    feed(&mut ui, Input::turn(EncoderId::C, 3));
    ui.update();
    let flushed = ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    let bands: Vec<(u16, u16)> = flushed.into_iter().filter(|&(a, b)| a != b).collect();
    assert_eq!(bands, [(28, 118), (186, 266)]);
}

/// The ALGO page's diagram follows the lerped MORPH, so a MORPH turn also
/// redraws the viz band.
#[test]
fn a_morph_turn_redraws_the_diagram() {
    let mut ui = ui_for("algo_alg");
    let mut fb = Fb::new();
    let scope = scope_fixture();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    feed(&mut ui, Input::turn(EncoderId::C, 30));
    settle(&mut ui);
    let flushed = ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    let bands: Vec<(u16, u16)> = flushed.into_iter().filter(|&(a, b)| a != b).collect();
    assert_eq!(bands, [(28, 118), (118, 186), (186, 266)]);
    let mut full = Fb::new();
    ui.render_with_scope(&mut full, &PerfStats::zero(), &scope);
    assert!(fb.px == full.px, "dirty != full");
}

#[test]
fn new_live_output_redraws_only_the_viz_band() {
    let mut ui = ui_for("algo_level");
    let mut fb = Fb::new();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    let quieter = scope_fixture().map(|s| if s > 0.0 { s * 0.5 } else { s });
    let flushed = ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &quieter);
    let bands: Vec<(u16, u16)> = flushed.into_iter().filter(|&(a, b)| a != b).collect();
    assert_eq!(bands, [(118, 186)]);
}

#[test]
fn focus_band_shows_the_last_touched_slot() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::turn(EncoderId::C, 5)); // MORPH
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    let slot = &ui.nav.active_block_def().params[2];
    let v = ui.renderer.anim[2].current();
    let mut text = FmtBuf::new();
    fmt_val(&mut text, v, slot.format());
    let mut want = Fb::new();
    want.px.fill(fb.px[0]); // ground
    components::focus_band(
        &mut want,
        slot.label(),
        text.as_str(),
        v,
        slot.format().is_bipolar(),
        chimera_core::ui::glyph::Drawn::Arc,
        components::Look::Live,
        None,
    );
    assert!(
        band(&fb, 28, 118) == band(&want, 28, 118),
        "focus band is {} {}",
        slot.label(),
        text.as_str()
    );
}

/// The focus value lerps toward the new value (CLAUDE.md: never snap).
#[test]
fn the_focus_value_animates_toward_its_target() {
    let mut ui = ui_for("engine_algo");
    let before = ui.renderer.anim[0].current();
    feed(&mut ui, Input::turn(EncoderId::A, 40));
    ui.update();
    let (now, target) = (ui.renderer.anim[0].current(), ui.renderer.anim[0].target());
    assert!(before < now && now < target, "{before} < {now} < {target}");
}

#[test]
fn only_the_focused_cell_label_uses_the_accent() {
    let fb = render("engine_algo"); // focus OP1 (slot a)
    let accent_in = |x0: i32| {
        (theme::CELL_LABEL_Y - 8..=theme::CELL_LABEL_Y)
            .any(|y| (x0..x0 + 60).any(|x| fb.at(x, y) == theme::ACCENT))
    };
    assert!(accent_in(theme::MARGIN_X));
    assert!(!accent_in(theme::MARGIN_X + theme::CELL_COL_W));
    assert!(!accent_in(theme::MARGIN_X + 2 * theme::CELL_COL_W));
}

#[test]
fn empty_slots_are_a_dim_dash_and_choices_have_no_bar() {
    let mut fb = Fb::new();
    components::cell(&mut fb, 0, 200, None);
    assert_eq!(fb.at(theme::MARGIN_X + 3, 197), theme::FAINT);
    let c = Cell {
        label: "MODE",
        text: "POLY",
        value: 1.0,
        fmt: ValFmt::Names(&["MONO", "POLY"]),
        active: false,
        mod_amount: None,
        look: components::Look::Live,
    };
    let mut fb = Fb::new();
    components::cell(&mut fb, 1, 200, Some(&c));
    let bar_y = 200 + theme::CELL_BAR_DY;
    let x = theme::MARGIN_X + theme::CELL_COL_W;
    assert!(
        (x..x + theme::CELL_BAR_W).all(|x| fb.at(x, bar_y) != theme::FAINT),
        "no track under a choice"
    );
}

#[test]
fn live_output_is_flat_when_silent_and_scaled_to_the_band() {
    let silent = [0.0; chimera_core::scope::SCOPE_LEN];
    assert!(viz::live_columns(&silent).iter().all(|&c| c == 0));
    let cols = viz::live_columns(&scope_fixture());
    assert_eq!(cols.iter().max(), Some(&(theme::VIZ_BAND_AMP as i8)));
    let mut fb = Fb::new();
    viz::live_output(&mut fb, &scope_fixture());
    for (i, row) in fb.px.chunks(W).enumerate() {
        let y = i as i32;
        if !(theme::VIZ_BAND_TOP..theme::VIZ_BAND_BOTTOM).contains(&y) {
            assert!(row.iter().all(|&p| p == 0), "row {y} outside the band");
        }
    }
}

/// A page whose slots are all empty (System UPDATES) shows no focus band.
#[test]
fn an_all_empty_page_has_an_empty_focus_band() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::press(chimera_hal::ButtonId::Menu));
    for _ in 0..2 {
        feed(&mut ui, Input::press(chimera_hal::ButtonId::Plus)); // → UPDATES
    }
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    let ground = fb.px[0];
    assert!(band(&fb, 28, 118).iter().all(|&p| p == ground));
    assert_eq!(fb.oob, 0);
}

/// Garbage in the scope buffer (NaN, ±inf) draws a flat line, in the band.
#[test]
fn non_finite_live_output_is_flat() {
    let mut buf = scope_fixture();
    buf[3] = f32::NAN;
    buf[9] = f32::INFINITY;
    buf[20] = f32::NEG_INFINITY;
    let cols = viz::live_columns(&buf);
    assert!(cols.iter().all(|&c| c == 0), "{cols:?}");
    let mut fb = Fb::new();
    viz::live_output(&mut fb, &buf);
    assert_eq!(fb.oob, 0);
}

/// A Part switch redraws the cells when only their mod bars differ (#74):
/// both Parts prime MORPH at 0 with different cuts, so the shown values
/// both clamp to 0.
#[test]
fn part_switch_redraws_mod_bars_that_differ() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::algo::params::AlgoParams;
    use chimera_core::modulation::ModState;
    let addr = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    let mut ui = chimera_core::ui::UiState::new();
    let s = ui.project().part(PartId::ALL[0]).sound.clone();
    *ui.project_mut().edit_part(PartId::ALL[1]).sound = s;
    for (part, amount) in [(0, -64), (1, -40)] {
        let s = ui.project_mut().edit_part(PartId::ALL[part]).sound;
        s.dest_registry.add(addr, *b"MORPH\0\0\0").unwrap();
        s.mod_state = ModState::from_registry(&s.dest_registry, 2);
        s.mod_state.set_amount(0, 0, amount);
    }
    let (perf, scope) = (PerfStats::zero(), scope_fixture());
    feed(&mut ui, Input::press(ButtonId::B2));
    feed(&mut ui, Input::press(ButtonId::B1));
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    feed(&mut ui, Input::press(ButtonId::B2));
    settle(&mut ui);
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    let mut full = Fb::new();
    ui.render_with_scope(&mut full, &perf, &scope);
    assert!(fb.px == full.px);
}
