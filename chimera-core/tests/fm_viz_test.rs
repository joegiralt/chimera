//! FM algorithm diagram in the viz band of the FM algorithm and operator
//! pages (UI refresh spec § Page types: edited operator lit).

mod screen;

use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::viz::alg_op_center;
use chimera_hal::EncoderId;
use screen::*;

#[test]
fn every_algorithm_fits_the_band_without_overlap() {
    for alg in 0..8 {
        let c: Vec<(i32, i32)> = (0..4).map(|op| alg_op_center(alg, op)).collect();
        for &(x, y) in &c {
            assert!((theme::VIZ_BAND_TOP + 8..=theme::VIZ_BAND_BOTTOM - 8).contains(&y), "alg {alg} y {y}");
            assert!((8..232).contains(&x), "alg {alg} x {x}");
        }
        for i in 0..4 {
            for j in i + 1..4 {
                let (dx, dy) = (c[i].0 - c[j].0, c[i].1 - c[j].1);
                assert!(dx * dx + dy * dy >= 16 * 16, "alg {alg}: ops {} and {} overlap", i + 1, j + 1);
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
        (0..4).filter(|&op| {
            let (x, y) = alg_op_center(alg, op);
            fb.at(x - 4, y) == theme::ACCENT
        }).collect::<Vec<_>>()
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
