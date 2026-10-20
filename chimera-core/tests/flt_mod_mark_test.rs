//! FLT shows the set cutoff and RES, and modulation as a separate ghost
//! (#575): the CUTOFF number and the solid curve never follow it.

mod screen;

use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::viz::{self, ModMark, PLOT_BASE, PLOT_TOP};
use chimera_hal::EncoderId;
use screen::*;

/// Part 1 on FLT with a turned FREQ (CUTOFF is slot 4), nothing routed.
fn flt() -> UiState {
    let mut ui = stack_ui();
    init_part(&mut ui, PartId::ALL[0]);
    plus(&mut ui, 2);
    feed(&mut ui, Input::turn(EncoderId::E, -30));
    settle(&mut ui);
    ui
}

fn full(ui: &UiState) -> Fb {
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    fb
}

/// The rows (`y0..y1`) of `fb`, whole.
fn rows(fb: &Fb, y0: i32, y1: i32) -> Vec<u16> {
    fb.px[y0 as usize * W..y1 as usize * W].to_vec()
}

fn mark_pixels(fb: &Fb) -> Vec<u16> {
    ModMark::BOXES
        .iter()
        .flat_map(|&(x, y, w, h)| (y..y + h).flat_map(move |y| (x..x + w).map(move |x| (x, y))))
        .map(|(x, y)| fb.px[y as usize * W + x as usize])
        .collect()
}

fn ground() -> u16 {
    full(&flt()).px[0]
}

#[test]
fn an_lfo_moves_the_mark_not_the_number_or_the_curve() {
    let mut ui = flt();
    lfo_on_freq(&mut ui);
    let set = ui.renderer.set[4].target();
    let (mut curves, mut marks, mut numbers) = (vec![], vec![], vec![]);
    for _ in 0..40 {
        ui.update(UiTick::for_test());
        let fb = full(&ui);
        // The solid curve and its readout sit above the base; the number
        // is the CUTOFF cell's text line.
        curves.push(rows(&fb, PLOT_TOP, PLOT_BASE + 1));
        marks.push(mark_pixels(&fb));
        let y = theme::CELL_LABEL_Y + theme::CELL_ROW_H + theme::CELL_VALUE_DY;
        numbers.push(
            rows(&fb, y - 12, y + 2)
                .chunks(W)
                .map(|r| {
                    r[(theme::MARGIN_X + theme::CELL_COL_W) as usize
                        ..(theme::MARGIN_X + 2 * theme::CELL_COL_W - 4) as usize]
                        .to_vec()
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(ui.renderer.set[4].target(), set);
    }
    assert!(curves.windows(2).all(|w| w[0] == w[1]), "curve moved");
    assert!(numbers.windows(2).all(|w| w[0] == w[1]), "number moved");
    assert!(marks.windows(2).any(|w| w[0] != w[1]), "mark never moved");
    // The animator the cell's bar reads does move; the number reads the set.
    assert_ne!(ui.renderer.anim[4].target(), ui.renderer.set[4].target());
}

#[test]
fn unmodulated_shows_no_mark() {
    let ui = flt();
    let g = ground();
    assert!(mark_pixels(&full(&ui)).iter().all(|&p| p == g));
    // Turning a knob eases the set and animator alike: still none.
    let mut ui = ui;
    feed(&mut ui, Input::turn(EncoderId::E, 25));
    for _ in 0..10 {
        ui.update(UiTick::for_test());
        assert!(mark_pixels(&full(&ui)).iter().all(|&p| p == g));
    }
}

#[test]
fn a_modulated_res_marks_the_resonance_box_only() {
    let mut ui = flt();
    ui.mod_state_mut()
        .set_fixed(ModSource::Lfo1.index(), FixedCol::Q, 90);
    let l = &mut ui.params_mut().lfos[0];
    (l.depth, l.rate) = (1.0, 3.0);
    let g = ground();
    let mut lit = false;
    for _ in 0..40 {
        ui.update(UiTick::for_test());
        let fb = full(&ui);
        let (x, y, w, h) = ModMark::BOXES[0];
        assert!((y..y + h).all(|y| (x..x + w).all(|x| fb.px[y as usize * W + x as usize] == g)));
        let (x, y, w, h) = ModMark::BOXES[1];
        lit |= (y..y + h).any(|y| (x..x + w).any(|x| fb.px[y as usize * W + x as usize] != g));
    }
    assert!(lit);
}

/// A moving mark flushes its two boxes alone (never the viz band), and the
/// incremental screen equals a full render.
#[test]
fn a_moving_mark_redraws_only_its_boxes() {
    let mut ui = flt();
    lfo_on_freq(&mut ui);
    let mut fb = Fb::new();
    let scope = scope_fixture();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    let boxes = ModMark::BOXES.map(|(_, y, _, h)| (y as u16, (y + h) as u16));
    let mut moved = 0;
    for _ in 0..40 {
        ui.update(UiTick::for_test());
        let flushed = ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
        for (a, b) in flushed.into_iter().filter(|&(a, b)| a != b) {
            // Past the viz: the cells (their bar follows the animator).
            assert!(
                boxes.contains(&(a, b)) || a >= viz::PLOT_BASE as u16 + 16,
                "({a}, {b})"
            );
            moved += boxes.contains(&(a, b)) as u32;
        }
        assert!(fb.px == full(&ui).px, "dirty != full");
    }
    assert!(moved > 0);
}

#[test]
fn the_mark_is_whole_pixels() {
    assert_eq!(ModMark::of(0.5, 0.5, 0.3, 0.3), ModMark::default());
    assert_eq!(ModMark::of(0.5, 0.5001, 0.3, 0.3), ModMark::default());
    let m = ModMark::of(0.2, 0.6, 0.3, 0.3);
    assert!(m.cutoff.is_some() && m.res.is_none());
    assert_ne!(m.key(), 0);
    assert_eq!(ModMark::default().key(), 0);
}

#[test]
fn the_env_amt_case_shows_a_mark() {
    let g = ground();
    for name in ["flt_2040_env_amt", "flt_clean_lfo"] {
        let fb = render(name);
        assert!(mark_pixels(&fb).iter().any(|&p| p != g), "{name}");
    }
}
