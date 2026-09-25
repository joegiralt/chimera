//! BigViz pages in Direction A (UI refresh spec § Page types): header ·
//! large viz with the touched value riding on it · cells · map.

mod screen;

use chimera_core::ui::fmt::{FmtBuf, fmt_val};
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
    assert_eq!(
        viz::filter_y(0.5, 0.5, 1.0),
        PLOT_TOP,
        "full resonance peaks at the top"
    );
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
    viz::readout(&mut fb, 220, |_| 100, "CUTOFF", "127");
    for y in 28..186 {
        for x in theme::VIZ_RIGHT + 1..240 {
            assert_eq!(fb.px[y as usize * W + x as usize], 0, "({x},{y})");
        }
    }
}

/// At full resonance the peak reaches `PLOT_TOP`, leaving no room above it;
/// the readout must clear the curve at any cutoff (low, mid, high) rather
/// than let the peak pass through the digits.
#[test]
fn filter_readout_clears_the_peak_at_full_resonance() {
    for &cutoff in &[0.1_f32, 0.5, 0.9] {
        let mut without = Fb::new();
        viz::filter(&mut without, cutoff, 1.0, None);
        let mut with = Fb::new();
        viz::filter(&mut with, cutoff, 1.0, Some(("CUTOFF", "127")));
        assert_eq!(
            with.oob, 0,
            "cutoff={cutoff}: nothing drawn outside 240x320"
        );
        for y in 0..H {
            for x in 0..W {
                let i = y * W + x;
                let curve_px = without.at(x as i32, y as i32) == theme::ACCENT;
                let readout_drew_here = with.px[i] != without.px[i];
                assert!(
                    !(curve_px && readout_drew_here),
                    "cutoff={cutoff}: readout overlaps curve at ({x},{y})"
                );
            }
        }
    }
}

fn accent_pixels(ui: &mut chimera_core::ui::UiState) -> usize {
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    (28..186)
        .flat_map(|y| (0..240).map(move |x| (x, y)))
        .filter(|&(x, y)| fb.at(x, y) == theme::ACCENT)
        .count()
}

fn accent_in_viz(name_setup: impl FnOnce(&mut chimera_core::ui::UiState)) -> usize {
    let mut ui = ui_for("bigviz_env");
    name_setup(&mut ui);
    settle(&mut ui);
    accent_pixels(&mut ui)
}

/// Envelope: the segment the focused slot edits is lit; LEVEL/VEL light none.
#[test]
fn envelope_lights_the_edited_segment() {
    assert!(accent_in_viz(|_| {}) > 0, "DEC lit");
    assert_eq!(
        accent_in_viz(|ui| feed(ui, Input::turn(EncoderId::E, -1))),
        0,
        "DEPTH lights no segment"
    );
}

#[test]
fn fm_envelope_is_reachable_and_lit() {
    let mut ui = ui_for("bigviz_fm_op_env");
    feed(&mut ui, Input::press(ButtonId::Seq)); // back up to MOD
    feed(&mut ui, Input::press(ButtonId::Edit)); // E1 again
    assert_eq!(ui.focused_slot(), 2, "focus survives leaving the page");
    settle(&mut ui);
    assert!(accent_pixels(&mut ui) > 0, "D1R segment lit");
}

/// Columns in the envelope's stage-label row (below the base line and the
/// breakpoint dots) that hold `color`.
fn label_columns(fb: &Fb, color: embedded_graphics::pixelcolor::Rgb565) -> Vec<i32> {
    let base = PLOT_BASE - 8;
    (0..W as i32)
        .filter(|&x| (base + 4..base + 16).any(|y| fb.at(x, y) == color))
        .collect()
}

/// A short DEC focused next to a short ATK: the lit DEC label is drawn and
/// the ATK label, which would run into it, is left out.
#[test]
fn envelope_labels_never_collide_with_the_lit_one() {
    let mut fb = Fb::new();
    let w = (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32;
    viz::envelope(
        &mut fb,
        &[30.0 / w, 6.0 / w, 102.0 / w, 78.0 / w],
        &[0.0, 1.0, 0.6, 0.6, 0.0],
        &["ATK", "DEC", "SUS", "REL"],
        Some(1),
    );
    let lit = label_columns(&fb, theme::ACCENT);
    assert!(!lit.is_empty(), "the lit label is drawn");
    let (l, r) = (lit[0], *lit.last().unwrap());
    let rest = label_columns(&fb, theme::MID);
    assert!(
        rest.iter().all(|&x| x < l - 2 || x > r + 2),
        "lit {l}..={r}, others at {rest:?}"
    );
    assert!(!rest.is_empty(), "labels that fit are still drawn");
}

/// Across many shapes and every lit stage, the drawn label spans never
/// overlap or touch, and the lit stage's label is always among them.
#[test]
fn envelope_label_spans_never_overlap() {
    let labels = ["ATK", "DEC", "SUS", "REL"];
    for a in (0..=120).step_by(3) {
        for b in (0..=120).step_by(3) {
            let xs = [12, 12 + a, 12 + a + b, 12 + a + b + 40, 228];
            for lit in [None, Some(0), Some(1), Some(2), Some(3)] {
                let spans = viz::stage_label_spans(&xs, &labels, lit);
                if let Some(s) = lit {
                    assert!(spans[s].is_some(), "lit {s} drawn at {xs:?}");
                }
                let drawn: Vec<(i32, i32)> = spans.iter().flatten().copied().collect();
                for (i, p) in drawn.iter().enumerate() {
                    for q in &drawn[i + 1..] {
                        assert!(
                            p.1 < q.0 || q.1 < p.0,
                            "{p:?} vs {q:?} at {xs:?} lit {lit:?}"
                        );
                    }
                }
            }
        }
    }
}

/// `stage_label_spans` keeps its documented `STAGE_LABEL_GAP` (2 px) of
/// background between any two drawn spans -- not merely "not touching"
/// (`envelope_label_spans_never_overlap` above), which a 1-px gap would
/// also satisfy.
#[test]
fn envelope_label_spans_keep_the_two_pixel_gap() {
    const STAGE_LABEL_GAP: i32 = 2; // mirrors the private constant in ui::viz
    let labels = ["ATK", "DEC", "SUS", "REL"];
    for a in (0..=120).step_by(3) {
        for b in (0..=120).step_by(3) {
            let xs = [12, 12 + a, 12 + a + b, 12 + a + b + 40, 228];
            for lit in [None, Some(0), Some(1), Some(2), Some(3)] {
                let spans = viz::stage_label_spans(&xs, &labels, lit);
                let drawn: Vec<(i32, i32)> = spans.iter().flatten().copied().collect();
                for (i, p) in drawn.iter().enumerate() {
                    for q in &drawn[i + 1..] {
                        let gap = if p.1 < q.0 {
                            q.0 - p.1 - 1
                        } else {
                            p.0 - q.1 - 1
                        };
                        assert!(
                            gap >= STAGE_LABEL_GAP,
                            "{p:?} vs {q:?} at {xs:?} lit {lit:?}: gap {gap} < {STAGE_LABEL_GAP}"
                        );
                    }
                }
            }
        }
    }
}
