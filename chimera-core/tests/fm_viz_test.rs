//! FM algorithm diagram in the viz band of the FM algorithm and operator
//! pages (UI refresh spec § Page types: edited operator lit).

mod screen;

use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::viz::{ALG_OP_R, alg_edges, alg_op_center};
use chimera_hal::EncoderId;
use screen::*;

#[test]
fn every_algorithm_fits_the_band_without_overlap() {
    for alg in 0..8 {
        let c: Vec<(i32, i32)> = (0..4).map(|op| alg_op_center(alg, op)).collect();
        for &(x, y) in &c {
            assert!(
                (theme::VIZ_BAND_TOP + 8..=theme::VIZ_BAND_BOTTOM - 8).contains(&y),
                "alg {alg} y {y}"
            );
            assert!((8..232).contains(&x), "alg {alg} x {x}");
        }
        for i in 0..4 {
            for j in i + 1..4 {
                let (dx, dy) = (c[i].0 - c[j].0, c[i].1 - c[j].1);
                assert!(
                    dx * dx + dy * dy >= 16 * 16,
                    "alg {alg}: ops {} and {} overlap",
                    i + 1,
                    j + 1
                );
            }
        }
    }
}

#[test]
fn the_selected_operator_is_lit() {
    let mut ui = ui_for("engine_fm_op"); // operator 3 selected
    let alg = ui.performance.parts[0].sound.params.fm.algorithm;
    let lit = |ui: &chimera_core::ui::UiState| {
        let mut fb = Fb::new();
        ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
        (0..4)
            .filter(|&op| {
                let (x, y) = alg_op_center(alg, op);
                fb.at(x - 4, y) == theme::ACCENT
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(lit(&ui), [2]);
    feed(&mut ui, Input::turn(EncoderId::A, -1));
    settle(&mut ui);
    assert_eq!(lit(&ui), [1]);
}

#[test]
fn algorithm_seven_is_one_row_of_carriers() {
    let ys: Vec<i32> = (0..4).map(|op| alg_op_center(7, op).1).collect();
    assert!(ys.iter().all(|&y| y == theme::VIZ_BAND_MID));
}

#[test]
fn fm_pages_dirty_render_equals_full_render() {
    for name in ["engine_fm_alg", "engine_fm_op"] {
        assert!(render(name).px == render_dirty(name).px, "{name}");
    }
}

/// Every algorithm's edges: direction (source above target — the row a
/// modulator sits in must be readable) and clearance (an edge must not pass
/// so close to an unrelated node that it reads as routed through it).
#[test]
fn algorithm_edges_point_downward_and_clear_other_nodes() {
    fn dist_to_segment(p: (i32, i32), a: (i32, i32), b: (i32, i32)) -> f64 {
        let (px, py) = (p.0 as f64, p.1 as f64);
        let (ax, ay) = (a.0 as f64, a.1 as f64);
        let (bx, by) = (b.0 as f64, b.1 as f64);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 == 0.0 {
            0.0
        } else {
            ((px - ax) * dx + (py - ay) * dy) / len2
        }
        .clamp(0.0, 1.0);
        let (cx, cy) = (ax + t * dx, ay + t * dy);
        ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
    }

    for alg in 0..8u8 {
        let centers: Vec<(i32, i32)> = (0..4).map(|op| alg_op_center(alg, op)).collect();
        for &(from, to) in alg_edges(alg) {
            let (a, b) = (centers[from as usize - 1], centers[to as usize - 1]);
            assert!(
                a.1 < b.1,
                "alg {alg}: edge {from}->{to} doesn't point downward ({a:?} -> {b:?})"
            );
            for (op, &c) in centers.iter().enumerate() {
                if op == from as usize - 1 || op == to as usize - 1 {
                    continue;
                }
                let d = dist_to_segment(c, a, b);
                assert!(
                    d >= (ALG_OP_R + 2) as f64,
                    "alg {alg}: edge {from}->{to} passes within {d:.1}px of operator {}",
                    op + 1
                );
            }
        }
    }
}

/// The FM algorithm page edits no single operator, so accent (the active
/// element) lights none of them — only the FM operator page does that.
#[test]
fn fm_alg_page_lights_no_operator() {
    let ui = ui_for("engine_fm_alg");
    let alg = ui.performance.parts[0].sound.params.fm.algorithm;
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    for op in 0..4 {
        let (x, y) = alg_op_center(alg, op);
        assert_ne!(
            fb.at(x - 4, y),
            theme::ACCENT,
            "operator {} lit on the FM algorithm page",
            op + 1
        );
    }
}

/// ALG reads 1–8, like OP 1–4 and the TX81Z's own algorithm numbers.
#[test]
fn the_algorithm_shows_one_based() {
    use chimera_core::ui::block_registry::FM_ALG;
    use chimera_core::ui::fmt::{FmtBuf, fmt_val};
    let fmt = FM_ALG.params[0].format();
    for (v, want) in [(0.0, "1"), (3.0 / 7.0, "4"), (1.0, "8")] {
        let mut buf = FmtBuf::new();
        fmt_val(&mut buf, v, fmt);
        assert_eq!(buf.as_str(), want);
    }
}
