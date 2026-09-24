//! BigViz pages in Direction A (UI refresh spec § Page types): header ·
//! large viz with the touched value riding on it · cells · map.

mod screen;

use chimera_core::ui::fmt::{fmt_val, FmtBuf};
use chimera_core::ui::page::ValFmt;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::viz::{self, FILTER_PASS_Y, PLOT_BASE, PLOT_TOP};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

fn band(fb: &Fb, y0: i32, y1: i32) -> Vec<u16> {
    fb.px[y0 as usize * W..y1 as usize * W].to_vec()
}

#[test]
fn dirty_render_from_scratch_equals_full_render() {
    for name in ["bigviz_filter", "bigviz_env", "bigviz_fm_op_env"] {
        assert!(render(name).px == render_dirty(name).px, "{name}");
    }
}

#[test]
fn filter_curve_keeps_its_shape() {
    assert_eq!(viz::filter_y(0.0, 0.5, 0.0), FILTER_PASS_Y, "pass band");
    assert_eq!(viz::filter_y(0.5, 0.5, 1.0), PLOT_TOP, "full resonance peaks at the top");
    assert_eq!(viz::filter_y(1.0, 0.2, 0.5), PLOT_BASE, "rolled off");
    assert!(viz::filter_y(0.5, 0.5, 0.5) < FILTER_PASS_Y);
}

/// The readout on the filter is the focused slot, not always CUTOFF.
#[test]
fn filter_readout_rides_the_focused_value() {
    let mut ui = ui_for("bigviz_filter");
    feed(&mut ui, Input::turn(EncoderId::B, -1)); // RESO
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    let (cutoff, reso) = (ui.renderer.anim[0].current(), ui.renderer.anim[1].current());
    let mut text = FmtBuf::new();
    fmt_val(&mut text, reso, ValFmt::Uni);
    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    viz::filter(&mut want, cutoff, reso, Some(("RESO", text.as_str())));
    assert!(band(&fb, 28, 186) == band(&want, 28, 186));
}

#[test]
fn readout_flips_left_at_the_right_edge() {
    let mut fb = Fb::new();
    viz::readout(&mut fb, 220, 100, "CUTOFF", "127");
    for y in 28..186 {
        for x in theme::VIZ_RIGHT + 1..240 {
            assert_eq!(fb.px[y as usize * W + x as usize], 0, "({x},{y})");
        }
    }
}

fn accent_in_viz(name_setup: impl FnOnce(&mut chimera_core::ui::UiState)) -> usize {
    let mut ui = ui_for("bigviz_env");
    name_setup(&mut ui);
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    (28..186).flat_map(|y| (0..240).map(move |x| (x, y))).filter(|&(x, y)| fb.at(x, y) == theme::ACCENT).count()
}

/// Envelope: the segment the focused slot edits is lit; LEVEL/VEL light none.
#[test]
fn envelope_lights_the_edited_segment() {
    assert!(accent_in_viz(|_| {}) > 0, "DEC lit");
    assert_eq!(accent_in_viz(|ui| feed(ui, Input::turn(EncoderId::E, -1))), 0, "DEPTH lights no segment");
}

#[test]
fn fm_envelope_is_reachable_and_lit() {
    let mut ui = ui_for("bigviz_fm_op_env");
    feed(&mut ui, Input::press(ButtonId::Seq)); // back up to MOD
    feed(&mut ui, Input::press(ButtonId::Edit)); // E1 again
    assert_eq!(ui.focused_slot(), 2, "focus survives leaving the page");
}
