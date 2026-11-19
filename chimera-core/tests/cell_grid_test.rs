//! CellGrid pages in Direction A (UI refresh spec § Page types, § Testing):
//! header · focus band · live output · cells · map.

mod screen;

use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
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
    for name in [
        "engine_algo",
        "algo_alg",
        "algo_level",
        "algo_d2r",
        "settings_personal_theme",
    ] {
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
    ui.update(UiTick::for_test());
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
    init_part(&mut ui, PartId::ALL[0]);
    feed(&mut ui, Input::turn(EncoderId::C, 5)); // MORPH
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    let slot = &ui.page_def().params[2];
    let v = ui.renderer.anim[2].current();
    let mut text = FmtBuf::new();
    fmt_val(&mut text, v, slot.format());
    let mut want = Fb::new();
    want.px.fill(fb.px[0]); // ground
    components::focus_band(
        &mut want,
        slot.label(),
        text.as_str(),
        // MORPH's glyph: a crossfader on its set value.
        chimera_core::ui::glyph::Gauge::Crossfader {
            value: ui.renderer.set[2].current(),
        },
        components::Look::Live,
        None,
        chimera_core::ui::theme::TextSize::Normal,
    );
    // MORPH maps: its hint offers SEQ + turn's fine step, then MIX + SEQ's
    // learn (ADR 0279); unmapped, the fine step alone. The fine step is
    // bank 2's: without TAPE's UI the door is closed, and learn alone.
    use chimera_core::ui::{feel, hint::Hint, view};
    let hint: Hint = match (cfg!(feature = "tape"), cfg!(feature = "midi-map")) {
        (true, true) => &feel::FINE_LEARN,
        (true, false) => &feel::FINE,
        (false, true) => view::LEARN_HINT,
        (false, false) => &[],
    };
    chimera_core::ui::hint::focus(
        &mut want,
        hint,
        chimera_core::ui::theme::TextSize::Normal,
        228,
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
    ui.update(UiTick::for_test());
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
    components::cell(
        &mut fb,
        0,
        200,
        None,
        chimera_core::ui::theme::TextSize::Normal,
    );
    assert_eq!(fb.at(theme::MARGIN_X + 3, 197), theme::FAINT);
    let c = Cell {
        label: "MODE",
        text: "POLY",
        value: 1.0,
        fmt: ValFmt::Names(&["MONO", "POLY"]),
        active: false,
        mod_amount: None,
        look: components::Look::Live,
        motion: None,
        mini: None,
    };
    let mut fb = Fb::new();
    components::cell(
        &mut fb,
        1,
        200,
        Some(&c),
        chimera_core::ui::theme::TextSize::Normal,
    );
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
    let s = chimera_core::preset::Sound::clone(&ui.project().part(PartId::ALL[0]).sound);
    **ui.project_mut().edit_part(PartId::ALL[1]).sound = s;
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

/// Issue #559: after a turn the readout is the set value at once; only the
/// gauges and bars ease.
#[test]
fn the_readout_shows_the_target_the_frame_after_a_turn() {
    let turned = |settled: bool| {
        let mut ui = ui_for("algo_level");
        feed(&mut ui, Input::turn(EncoderId::C, 40));
        if settled {
            settle(&mut ui);
        } else {
            ui.update(UiTick::for_test());
        }
        render_ui(&ui)
    };
    let (mid, rest) = (turned(false), turned(true));
    let rows = |fb: &Fb, y0: i32, y1: i32, x0: i32, x1: i32| {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .map(|(x, y)| fb.at(x, y))
            .collect::<Vec<_>>()
    };
    let focus = |fb: &Fb| rows(fb, theme::FOCUS_LABEL_Y + 6, theme::FOCUS_BOTTOM, 0, 90);
    let cell = |fb: &Fb| {
        let x = theme::MARGIN_X + 2 * theme::CELL_COL_W;
        let y = theme::CELL_LABEL_Y + theme::CELL_VALUE_DY - 6;
        rows(
            fb,
            y,
            theme::CELL_LABEL_Y + theme::CELL_BAR_DY - 1,
            x,
            x + 50,
        )
    };
    assert!(mid.px != rest.px, "the gauges are still easing");
    assert!(focus(&mid) == focus(&rest), "focus readout lags");
    assert!(cell(&mid) == cell(&rest), "cell readout lags");
}

/// Issue #559 through the viz readout and the matrix amount, full and dirty:
/// one tick after a turn, a filter slot that does not move the curve
/// draws the settled screen, pixel for pixel.
#[test]
fn readouts_show_the_target_through_full_and_dirty_renders() {
    // The matrix fill eases by design: only its keys are checked here.
    for (case, enc, by, exact) in [
        ("bigviz_filter", EncoderId::E, 40, true),
        ("mod_matrix", EncoderId::E, 40, false),
    ] {
        let scope = scope_fixture();
        let mut rest = ui_for(case);
        feed(&mut rest, Input::turn(enc, by));
        settle(&mut rest);
        let mut want = Fb::new();
        rest.render_with_scope(&mut want, &PerfStats::zero(), &scope);

        let mut ui = ui_for(case);
        let mut dirty = Fb::new();
        ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope);
        feed(&mut ui, Input::turn(enc, by));
        ui.update(UiTick::for_test());
        ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope);
        let mut full = Fb::new();
        ui.render_with_scope(&mut full, &PerfStats::zero(), &scope);
        assert!(dirty.px == full.px, "{case}: dirty == full");
        assert!(!exact || full.px == want.px, "{case}: readout lags");
    }
}

/// The bands a dirty render flushes now, on a persistent `fb`.
fn flushed(ui: &mut UiState, fb: &mut Fb) -> Vec<(u16, u16)> {
    let out = ui.render_dirty_with_scope(fb, &PerfStats::zero(), &scope_fixture());
    out.into_iter().filter(|&(a, b)| a != b).collect()
}

/// The text keys follow what is printed (#559): a target move below the
/// display resolution dirties nothing, a change in the number dirties the
/// focus band and the cells.
#[test]
fn a_cell_page_redraws_on_the_printed_number_only() {
    let mut ui = ui_for("algo_level");
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    settle(&mut ui);
    let mut fb = Fb::new();
    flushed(&mut ui, &mut fb);
    assert!(flushed(&mut ui, &mut fb).is_empty());
    // The number reads the set value (#575); the animator follows it.
    let v = ui.renderer.set[2].target();
    let aim = |ui: &mut UiState, t: f32| {
        ui.renderer.set[2].set_target(t);
        ui.renderer.anim[2].set_target(t);
    };
    aim(&mut ui, v + 0.0002);
    assert_eq!(
        flushed(&mut ui, &mut fb),
        [],
        "below the display resolution"
    );
    // Modulation moves the animator alone: the printed number is the set one.
    let m = ui.renderer.anim[2].target();
    ui.renderer.anim[2].set_target(m + 0.1);
    assert_eq!(flushed(&mut ui, &mut fb), [], "modulation moves no number");
    ui.renderer.anim[2].set_target(m);
    aim(&mut ui, if v > 0.5 { v - 0.1 } else { v + 0.1 });
    assert_eq!(flushed(&mut ui, &mut fb), [(28, 118), (186, 266)]);
}

/// The same on the matrix: its grid and route readout key on the amount as
/// printed, and the readout's words follow the target at once.
#[test]
fn the_matrix_redraws_on_the_printed_amount_only() {
    use chimera_core::ui::renderer::{MATRIX_AMOUNT_SLOT as SLOT, amount_of, amount_value};
    let mut ui = ui_for("mod_matrix");
    let mut fb = Fb::new();
    flushed(&mut ui, &mut fb);
    assert!(flushed(&mut ui, &mut fb).is_empty());
    let now = amount_of(ui.renderer.anim[SLOT].target());
    let v = amount_value(now);
    assert_eq!(amount_of(v + 0.0005), now);
    ui.renderer.anim[SLOT].set_target(v + 0.0005);
    assert_eq!(
        flushed(&mut ui, &mut fb),
        [],
        "below the display resolution"
    );
    let rest = {
        let mut r = ui_for("mod_matrix");
        feed(&mut r, Input::turn(EncoderId::E, 1));
        settle(&mut r);
        r
    };
    ui.renderer.anim[SLOT].set_target(amount_value(now + 1));
    let bands = flushed(&mut ui, &mut fb);
    assert!(bands.len() >= 2, "readout and grid redraw: {bands:?}");
    // The readout is the settled one's, with the amount still easing.
    let (mut want, y) = (Fb::new(), bands[0]);
    rest.render_with_scope(&mut want, &PerfStats::zero(), &scope_fixture());
    let rows = |f: &Fb| f.px[y.0 as usize * W..y.1 as usize * W].to_vec();
    assert!(rows(&fb) == rows(&want), "route readout lags");
}

/// A slow LFO on a page: the viz band redraws only on frames where what it
/// shows moved, never each frame by the bits of a target.
#[test]
fn a_slow_lfo_redraws_only_when_the_display_moves() {
    let mut ui = ui_for("mod_matrix");
    feed(&mut ui, Input::press(ButtonId::Minus));
    feed(&mut ui, Input::press(ButtonId::Minus));
    let (mut fb, mut redraws, mut moved) = (Fb::new(), 0, 0);
    flushed(&mut ui, &mut fb);
    let shown = |ui: &UiState| {
        let a = &ui.renderer.anim;
        (0..6)
            .map(|i| chimera_core::ui::region::quantize(a[i].current()))
            .collect::<Vec<_>>()
    };
    for _ in 0..40 {
        let before = shown(&ui);
        ui.update(UiTick::for_test());
        moved += (shown(&ui) != before) as u32;
        redraws += !flushed(&mut ui, &mut fb).is_empty() as u32;
    }
    assert!(moved > 0, "the LFO moves the page");
    assert!(redraws <= moved, "{redraws} redraws for {moved} moves");
}
